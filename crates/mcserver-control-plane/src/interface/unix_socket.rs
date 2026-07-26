use std::{
    io,
    os::unix::{fs::FileTypeExt, fs::PermissionsExt},
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::watch,
};
use tracing::{debug, error, info, warn};

use super::ClientRpcHandler;

pub struct UnixSocketServer {
    listener: UnixListener,
    socket_path: PathBuf,
    handler: ClientRpcHandler,
    max_frame_bytes: usize,
}

impl UnixSocketServer {
    pub async fn bind(
        socket_path: PathBuf,
        socket_mode: u32,
        max_frame_bytes: usize,
        handler: ClientRpcHandler,
    ) -> Result<Self, UnixSocketError> {
        prepare_socket_path(&socket_path).await?;
        let listener = UnixListener::bind(&socket_path)?;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(socket_mode))?;

        info!(path = %socket_path.display(), mode = format_args!("{socket_mode:o}"), "client JSON-RPC socket is listening");

        Ok(Self {
            listener,
            socket_path,
            handler,
            max_frame_bytes,
        })
    }

    pub async fn run(
        self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), UnixSocketError> {
        let socket_path = self.socket_path.clone();
        let _guard = SocketPathGuard::new(socket_path);

        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                accepted = self.listener.accept() => {
                    let (stream, address) = accepted?;
                    debug!(?address, "accepted client JSON-RPC connection");
                    let handler = self.handler.clone();
                    let max_frame_bytes = self.max_frame_bytes;
                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(stream, handler, max_frame_bytes).await {
                            warn!(%error, "client JSON-RPC connection closed with an error");
                        }
                    });
                }
            }
        }

        Ok(())
    }
}

async fn handle_connection(
    stream: UnixStream,
    handler: ClientRpcHandler,
    max_frame_bytes: usize,
) -> Result<(), UnixSocketError> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut frame = Vec::new();

    loop {
        let read = read_frame(&mut reader, &mut frame, max_frame_bytes).await?;
        if read == 0 {
            return Ok(());
        }

        let input = trim_line_ending(&frame);
        if input.is_empty() {
            continue;
        }
        let input = std::str::from_utf8(input)?;

        if let Some(response) = handler.handle_json(input).await {
            write_json_line(&mut writer, &response).await?;
        }
    }
}

async fn read_frame<R>(
    reader: &mut R,
    frame: &mut Vec<u8>,
    maximum: usize,
) -> Result<usize, UnixSocketError>
where
    R: AsyncBufRead + Unpin,
{
    frame.clear();

    loop {
        let (consumed, terminated) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                return Ok(frame.len());
            }

            let newline = available.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(available.len(), |index| index + 1);
            let actual = frame.len().saturating_add(consumed);
            if actual > maximum {
                return Err(UnixSocketError::FrameTooLarge { actual, maximum });
            }

            frame.extend_from_slice(&available[..consumed]);
            (consumed, newline.is_some())
        };

        reader.consume(consumed);
        if terminated {
            return Ok(frame.len());
        }
    }
}

fn trim_line_ending(frame: &[u8]) -> &[u8] {
    let frame = frame.strip_suffix(b"\n").unwrap_or(frame);
    frame.strip_suffix(b"\r").unwrap_or(frame)
}

async fn write_json_line<W>(writer: &mut W, value: &Value) -> Result<(), UnixSocketError>
where
    W: AsyncWrite + Unpin,
{
    let encoded = serde_json::to_vec(value)?;
    writer.write_all(&encoded).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn prepare_socket_path(path: &Path) -> Result<(), UnixSocketError> {
    let parent = path.parent().ok_or_else(|| UnixSocketError::MissingParent {
        path: path.to_path_buf(),
    })?;
    tokio::fs::create_dir_all(parent).await?;

    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_socket() => {
            tokio::fs::remove_file(path).await?;
            Ok(())
        }
        Ok(_) => Err(UnixSocketError::PathOccupied {
            path: path.to_path_buf(),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

struct SocketPathGuard {
    path: PathBuf,
}

impl SocketPathGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for SocketPathGuard {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.path) {
            Ok(()) => info!(path = %self.path.display(), "removed client JSON-RPC socket"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => error!(path = %self.path.display(), %error, "failed to remove client JSON-RPC socket"),
        }
    }
}

#[derive(Debug, Error)]
pub enum UnixSocketError {
    #[error("Unix socket I/O failed")]
    Io(#[from] io::Error),
    #[error("JSON serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("request frame is not valid UTF-8")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("Unix socket path has no parent: {path}")]
    MissingParent { path: PathBuf },
    #[error("Unix socket path is occupied by a non-socket file: {path}")]
    PathOccupied { path: PathBuf },
    #[error("request frame is too large: at least {actual} bytes, maximum {maximum} bytes")]
    FrameTooLarge { actual: usize, maximum: usize },
}
