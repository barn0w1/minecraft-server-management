use std::{
    io,
    os::unix::{fs::FileTypeExt, fs::PermissionsExt},
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
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
    let mut frame = String::new();

    loop {
        frame.clear();
        let read = reader.read_line(&mut frame).await?;
        if read == 0 {
            return Ok(());
        }

        if frame.len() > max_frame_bytes {
            write_json_line(
                &mut writer,
                &json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32600,
                        "message": "Request frame is too large"
                    },
                    "id": null
                }),
            )
            .await?;
            return Err(UnixSocketError::FrameTooLarge {
                actual: frame.len(),
                maximum: max_frame_bytes,
            });
        }

        let input = frame.trim_end_matches(['\r', '\n']);
        if input.is_empty() {
            continue;
        }

        if let Some(response) = handler.handle_json(input).await {
            write_json_line(&mut writer, &response).await?;
        }
    }
}

async fn write_json_line<W>(writer: &mut W, value: &Value) -> Result<(), UnixSocketError>
where
    W: AsyncWriteExt + Unpin,
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
    const fn new(path: PathBuf) -> Self {
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
    #[error("Unix socket path has no parent: {path}")]
    MissingParent { path: PathBuf },
    #[error("Unix socket path is occupied by a non-socket file: {path}")]
    PathOccupied { path: PathBuf },
    #[error("request frame is too large: {actual} bytes, maximum {maximum} bytes")]
    FrameTooLarge { actual: usize, maximum: usize },
}
