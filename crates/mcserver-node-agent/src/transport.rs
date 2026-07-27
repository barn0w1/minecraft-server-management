use std::{io, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};

use mcserver_protocol::{
    json_rpc::{self, ErrorObject, Request, Response},
    node_agent::{
        self, AgentInspectParams, CleanupInstanceParams, RegisterParams, RegisterResult,
        RestoreDataParams, ShutdownResult, SnapshotDataParams, StartServerParams, StopServerParams,
        method,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use tokio_rustls::{TlsConnector, rustls};
use tracing::{info, warn};

use crate::{
    cancellation::CancellationToken,
    config::Config,
    executor::{AgentExecutor, ExecutorError},
};

const CONNECTION_TOKEN_FILE_NAME: &str = "connection-token";
const MAX_CONNECTION_TOKEN_CHARS: usize = 256;

pub async fn run(
    config: Config,
    executor: AgentExecutor,
    cancellation: CancellationToken,
) -> Result<(), TransportError> {
    let mut connection_token = load_connection_token(&config).await?;
    let mut backoff = config.reconnect_min;
    loop {
        if cancellation.is_cancelled() {
            return Ok(());
        }

        match run_session(
            &config,
            &connection_token,
            &executor,
            cancellation.child_token(),
        )
        .await
        {
            Ok(SessionOutcome::Shutdown) => return Ok(()),
            Ok(SessionOutcome::CredentialRotated(token)) => {
                persist_connection_token(&config, &token).await?;
                connection_token = token;
                backoff = config.reconnect_min;
                info!(
                    compute_instance_id = %config.compute_instance_id,
                    "persisted remote reconnect credential; reconnecting"
                );
                continue;
            }
            Ok(SessionOutcome::Disconnected) => {
                backoff = config.reconnect_min;
                warn!("control-plane connection closed; reconnecting");
            }
            Err(error) => {
                warn!(%error, "control-plane session failed; reconnecting");
            }
        }

        tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            () = tokio::time::sleep(backoff) => {}
        }
        backoff = std::cmp::min(
            backoff.checked_mul(2).unwrap_or(config.reconnect_max),
            config.reconnect_max,
        );
    }
}

async fn run_session(
    config: &Config,
    connection_token: &str,
    executor: &AgentExecutor,
    cancellation: CancellationToken,
) -> Result<SessionOutcome, TransportError> {
    let stream = tokio::select! {
        () = cancellation.cancelled() => return Ok(SessionOutcome::Disconnected),
        connected = connect(config) => connected?,
    };
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    let registration_id = json!(1);
    write_value(
        &mut writer,
        &json!({
            "jsonrpc": json_rpc::VERSION,
            "method": method::AGENT_REGISTER,
            "params": RegisterParams {
                protocol_version: node_agent::PROTOCOL_VERSION,
                compute_instance_id: config.compute_instance_id,
                connection_token: connection_token.to_owned(),
            },
            "id": registration_id,
        }),
    )
    .await?;
    let registration = read_response(&mut reader, config.max_frame_bytes).await?;
    if registration.id != registration_id {
        return Err(TransportError::Protocol(
            "registration response id mismatch".to_owned(),
        ));
    }
    let result = response_result::<RegisterResult>(registration)?;
    if !result.accepted {
        return Err(TransportError::RegistrationRejected);
    }
    if let Some(token) = result.replacement_connection_token {
        validate_connection_token(&token)?;
        return Ok(SessionOutcome::CredentialRotated(token));
    }
    info!(compute_instance_id = %config.compute_instance_id, "registered with control plane");

    loop {
        let request = tokio::select! {
            () = cancellation.cancelled() => return Ok(SessionOutcome::Disconnected),
            request = read_request(&mut reader, config.max_frame_bytes) => match request {
                Ok(request) => request,
                Err(TransportError::Disconnected) => return Ok(SessionOutcome::Disconnected),
                Err(error) => return Err(error),
            }
        };
        let Some(response_id) = request.id.response_id() else {
            continue;
        };
        let (response, shutdown) = dispatch(executor, request, response_id).await;
        write_value(&mut writer, &serde_json::to_value(response)?).await?;
        if shutdown {
            return Ok(SessionOutcome::Shutdown);
        }
    }
}

async fn load_connection_token(config: &Config) -> Result<String, TransportError> {
    let path = connection_token_path(config);
    match tokio::fs::read_to_string(&path).await {
        Ok(token) => {
            let token = token.trim_end_matches(['\r', '\n']).to_owned();
            validate_connection_token(&token)?;
            Ok(token)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            validate_connection_token(&config.connection_token)?;
            Ok(config.connection_token.clone())
        }
        Err(error) => Err(TransportError::CredentialIo {
            path,
            source: error,
        }),
    }
}

async fn persist_connection_token(config: &Config, token: &str) -> Result<(), TransportError> {
    validate_connection_token(token)?;
    tokio::fs::create_dir_all(&config.state_directory)
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: config.state_directory.clone(),
            source,
        })?;
    tokio::fs::set_permissions(
        &config.state_directory,
        std::fs::Permissions::from_mode(0o700),
    )
    .await
    .map_err(|source| TransportError::CredentialIo {
        path: config.state_directory.clone(),
        source,
    })?;
    let path = connection_token_path(config);
    let temporary = config.state_directory.join("connection-token.tmp");
    tokio::fs::write(&temporary, token)
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: temporary.clone(),
            source,
        })?;
    tokio::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: temporary.clone(),
            source,
        })?;
    let file =
        tokio::fs::File::open(&temporary)
            .await
            .map_err(|source| TransportError::CredentialIo {
                path: temporary.clone(),
                source,
            })?;
    file.sync_all()
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: temporary.clone(),
            source,
        })?;
    tokio::fs::rename(&temporary, &path)
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: path.clone(),
            source,
        })?;
    let directory = tokio::fs::File::open(&config.state_directory)
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: config.state_directory.clone(),
            source,
        })?;
    directory
        .sync_all()
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: config.state_directory.clone(),
            source,
        })?;
    Ok(())
}

fn connection_token_path(config: &Config) -> PathBuf {
    config.state_directory.join(CONNECTION_TOKEN_FILE_NAME)
}

fn validate_connection_token(token: &str) -> Result<(), TransportError> {
    if token.trim().is_empty()
        || token.contains('\0')
        || token.chars().count() > MAX_CONNECTION_TOKEN_CHARS
    {
        return Err(TransportError::InvalidConnectionToken);
    }
    Ok(())
}

type BoxedTransport = Box<dyn TransportStream>;

trait TransportStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> TransportStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

async fn connect(config: &Config) -> Result<BoxedTransport, TransportError> {
    let stream = TcpStream::connect(config.control_plane_address.as_str()).await?;
    stream.set_nodelay(true)?;
    let Some(tls) = config.tls.as_ref() else {
        return Ok(Box::new(stream));
    };

    let ca_pem = tokio::fs::read(&tls.ca_certificate).await?;
    let mut reader = std::io::BufReader::new(ca_pem.as_slice());
    let certificates = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certificates.is_empty() {
        return Err(TransportError::TlsConfiguration(
            "CA certificate file contains no certificates".to_owned(),
        ));
    }
    let mut roots = rustls::RootCertStore::empty();
    for certificate in certificates {
        roots
            .add(certificate)
            .map_err(|error| TransportError::TlsConfiguration(error.to_string()))?;
    }
    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name = rustls::pki_types::ServerName::try_from(tls.server_name.clone())
        .map_err(|error| TransportError::TlsConfiguration(error.to_string()))?;
    let connector = TlsConnector::from(Arc::new(client_config));
    let stream = connector.connect(server_name, stream).await?;
    Ok(Box::new(stream))
}

async fn dispatch(
    executor: &AgentExecutor,
    request: Request,
    response_id: Value,
) -> (Response, bool) {
    if request.jsonrpc != json_rpc::VERSION
        || request.method.is_empty()
        || !request.id.is_valid()
        || !matches!(
            &request.params,
            Value::Null | Value::Array(_) | Value::Object(_)
        )
    {
        return (
            Response::error(
                response_id,
                ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, "Invalid request"),
            ),
            false,
        );
    }

    let method_name = request.method;
    let shutdown_requested = method_name == method::NODE_SHUTDOWN;
    match dispatch_method(executor, &method_name, request.params).await {
        Ok(value) => (Response::success(response_id, value), shutdown_requested),
        Err(error) => (
            Response::error(response_id, error.into_error_object()),
            false,
        ),
    }
}

async fn dispatch_method(
    executor: &AgentExecutor,
    method_name: &str,
    params: Value,
) -> Result<Value, DispatchError> {
    match method_name {
        method::AGENT_INSPECT => {
            let params = parse_params::<AgentInspectParams>(params)?;
            to_value(executor.inspect(params.instance).await?)
        }
        method::DATA_RESTORE => {
            let params = parse_params::<RestoreDataParams>(params)?;
            to_value(executor.restore_data(params).await?)
        }
        method::SERVER_START => {
            let params = parse_params::<StartServerParams>(params)?;
            to_value(executor.start_server(params).await?)
        }
        method::SERVER_STOP => {
            let params = parse_params::<StopServerParams>(params)?;
            to_value(executor.stop_server(params).await?)
        }
        method::DATA_SNAPSHOT => {
            let params = parse_params::<SnapshotDataParams>(params)?;
            to_value(executor.snapshot_data(params).await?)
        }
        method::INSTANCE_CLEANUP => {
            let params = parse_params::<CleanupInstanceParams>(params)?;
            to_value(executor.cleanup_instance(params).await?)
        }
        method::NODE_SHUTDOWN => {
            require_no_params(&params)?;
            to_value(ShutdownResult { accepted: true })
        }
        _ => Err(DispatchError::MethodNotFound),
    }
}

fn parse_params<T>(value: Value) -> Result<T, DispatchError>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|error| DispatchError::InvalidParams(error.to_string()))
}

fn require_no_params(params: &Value) -> Result<(), DispatchError> {
    match params {
        Value::Null => Ok(()),
        Value::Array(values) if values.is_empty() => Ok(()),
        Value::Object(values) if values.is_empty() => Ok(()),
        _ => Err(DispatchError::InvalidParams(
            "this method does not accept params".to_owned(),
        )),
    }
}

fn to_value<T>(value: T) -> Result<Value, DispatchError>
where
    T: Serialize,
{
    serde_json::to_value(value).map_err(DispatchError::Serialization)
}

#[derive(Debug, Error)]
enum DispatchError {
    #[error("method not found")]
    MethodNotFound,
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("node operation failed")]
    Execution(#[from] ExecutorError),
    #[error("response serialization failed")]
    Serialization(serde_json::Error),
}

impl DispatchError {
    fn into_error_object(self) -> ErrorObject {
        match self {
            Self::MethodNotFound => {
                ErrorObject::new(json_rpc::error_code::METHOD_NOT_FOUND, "Method not found")
            }
            Self::InvalidParams(detail) => {
                ErrorObject::new(json_rpc::error_code::INVALID_PARAMS, "Invalid params")
                    .with_data(json!({ "detail": detail }))
            }
            Self::Execution(error) => {
                ErrorObject::new(json_rpc::error_code::INTERNAL_ERROR, "Execution failed")
                    .with_data(json!({ "detail": error.to_string() }))
            }
            Self::Serialization(error) => {
                ErrorObject::new(json_rpc::error_code::INTERNAL_ERROR, "Serialization failed")
                    .with_data(json!({ "detail": error.to_string() }))
            }
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct WireResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<WireError>,
    id: Value,
}

#[derive(Debug, serde::Deserialize)]
struct WireError {
    code: i64,
    message: String,
}

fn response_result<T>(response: WireResponse) -> Result<T, TransportError>
where
    T: DeserializeOwned,
{
    if response.jsonrpc != json_rpc::VERSION {
        return Err(TransportError::Protocol(
            "control-plane response JSON-RPC version mismatch".to_owned(),
        ));
    }
    match (response.result, response.error) {
        (Some(value), None) => serde_json::from_value(value).map_err(TransportError::Serialization),
        (None, Some(error)) => Err(TransportError::Remote {
            code: error.code,
            message: error.message,
        }),
        _ => Err(TransportError::Protocol(
            "response must contain exactly one of result or error".to_owned(),
        )),
    }
}

async fn read_request<R>(reader: &mut R, maximum: usize) -> Result<Request, TransportError>
where
    R: AsyncBufRead + Unpin,
{
    let value = read_value(reader, maximum).await?;
    serde_json::from_value(value).map_err(TransportError::Serialization)
}

async fn read_response<R>(reader: &mut R, maximum: usize) -> Result<WireResponse, TransportError>
where
    R: AsyncBufRead + Unpin,
{
    let value = read_value(reader, maximum).await?;
    serde_json::from_value(value).map_err(TransportError::Serialization)
}

async fn read_value<R>(reader: &mut R, maximum: usize) -> Result<Value, TransportError>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::new();
    let read = read_frame(reader, &mut frame, maximum).await?;
    if read == 0 {
        return Err(TransportError::Disconnected);
    }
    let input = trim_line_ending(&frame);
    serde_json::from_slice(input).map_err(TransportError::Serialization)
}

async fn read_frame<R>(
    reader: &mut R,
    frame: &mut Vec<u8>,
    maximum: usize,
) -> Result<usize, TransportError>
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
                return Err(TransportError::FrameTooLarge { actual, maximum });
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

async fn write_value<W>(writer: &mut W, value: &Value) -> Result<(), TransportError>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&serde_json::to_vec(value)?).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionOutcome {
    CredentialRotated(String),
    Disconnected,
    Shutdown,
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("node-agent transport I/O failed")]
    Io(#[from] io::Error),
    #[error("node-agent protocol serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("node-agent credential file {path:?} failed")]
    CredentialIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("node-agent connection token is invalid")]
    InvalidConnectionToken,
    #[error("control plane disconnected")]
    Disconnected,
    #[error("node-agent frame is too large: at least {actual} bytes, maximum {maximum} bytes")]
    FrameTooLarge { actual: usize, maximum: usize },
    #[error("node-agent protocol violation: {0}")]
    Protocol(String),
    #[error("node-agent registration was rejected")]
    RegistrationRejected,
    #[error("control plane returned an error: {code}: {message}")]
    Remote { code: i64, message: String },
    #[error("TLS configuration is invalid: {0}")]
    TlsConfiguration(String),
}
