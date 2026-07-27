use std::{io, net::SocketAddr, path::Path, sync::Arc, time::Duration};

use mcserver_protocol::{
    json_rpc::{self, ErrorObject, Request, Response},
    node_agent::{PROTOCOL_VERSION, RegisterParams, RegisterResult, method},
};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{Semaphore, mpsc},
    task::JoinSet,
};
use tokio_rustls::{TlsAcceptor, rustls};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
    domain::{Clock, ComputeInstanceId, ComputeProvider, SystemClock},
    infrastructure::{AgentAuthentication, ComputeInstanceRepository},
    reconciliation::ReconcileScheduler,
    shutdown::CancellationToken,
};

use super::registry::{AgentCallError, AgentCommand, AgentRegistry};

const COMMAND_CHANNEL_CAPACITY: usize = 32;
const MAX_CONCURRENT_AGENT_CONNECTIONS: usize = 256;
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(15);

type BoxedAgentStream = Box<dyn AgentStream>;

trait AgentStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> AgentStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

#[derive(Clone)]
struct ConnectionContext {
    registry: AgentRegistry,
    compute_repository: ComputeInstanceRepository,
    reconcile_scheduler: ReconcileScheduler,
    max_frame_bytes: usize,
    command_timeout: Duration,
    expected_provider: ComputeProvider,
    clock: SystemClock,
}

pub struct AgentServer {
    listener: TcpListener,
    context: ConnectionContext,
}

impl AgentServer {
    pub async fn bind(
        address: SocketAddr,
        registry: AgentRegistry,
        compute_repository: ComputeInstanceRepository,
        reconcile_scheduler: ReconcileScheduler,
        max_frame_bytes: usize,
        command_timeout: Duration,
    ) -> Result<Self, AgentServerError> {
        let listener = TcpListener::bind(address).await?;
        info!(address = %listener.local_addr()?, "local node-agent JSON-RPC listener is ready");
        Ok(Self {
            listener,
            context: ConnectionContext {
                registry,
                compute_repository,
                reconcile_scheduler,
                max_frame_bytes,
                command_timeout,
                expected_provider: ComputeProvider::LocalProcess,
                clock: SystemClock,
            },
        })
    }

    pub async fn run(self, cancellation: CancellationToken) -> Result<(), AgentServerError> {
        run_listener(self.listener, self.context, None, cancellation).await?;
        info!("local node-agent JSON-RPC listener stopped");
        Ok(())
    }
}

pub struct TlsAgentServer {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    context: ConnectionContext,
}

impl TlsAgentServer {
    #[allow(clippy::too_many_arguments)]
    pub async fn bind(
        address: SocketAddr,
        certificate_path: &Path,
        private_key_path: &Path,
        registry: AgentRegistry,
        compute_repository: ComputeInstanceRepository,
        reconcile_scheduler: ReconcileScheduler,
        max_frame_bytes: usize,
        command_timeout: Duration,
    ) -> Result<Self, AgentServerError> {
        let certificate = tokio::fs::read(certificate_path).await?;
        let private_key = tokio::fs::read(private_key_path).await?;
        let mut certificate_reader = std::io::BufReader::new(certificate.as_slice());
        let certificates = rustls_pemfile::certs(&mut certificate_reader)
            .collect::<Result<Vec<_>, _>>()?;
        if certificates.is_empty() {
            return Err(AgentServerError::TlsConfiguration(
                "TLS certificate file contains no certificates".to_owned(),
            ));
        }
        let mut private_key_reader = std::io::BufReader::new(private_key.as_slice());
        let private_key = rustls_pemfile::private_key(&mut private_key_reader)?.ok_or_else(|| {
            AgentServerError::TlsConfiguration(
                "TLS private key file contains no supported private key".to_owned(),
            )
        })?;
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certificates, private_key)
            .map_err(|error| AgentServerError::TlsConfiguration(error.to_string()))?;
        let listener = TcpListener::bind(address).await?;
        info!(address = %listener.local_addr()?, "remote TLS node-agent listener is ready");
        Ok(Self {
            listener,
            acceptor: TlsAcceptor::from(Arc::new(server_config)),
            context: ConnectionContext {
                registry,
                compute_repository,
                reconcile_scheduler,
                max_frame_bytes,
                command_timeout,
                expected_provider: ComputeProvider::Akamai,
                clock: SystemClock,
            },
        })
    }

    pub async fn run(self, cancellation: CancellationToken) -> Result<(), AgentServerError> {
        run_listener(
            self.listener,
            self.context,
            Some(self.acceptor),
            cancellation,
        )
        .await?;
        info!("remote TLS node-agent listener stopped");
        Ok(())
    }
}

async fn run_listener(
    listener: TcpListener,
    context: ConnectionContext,
    tls_acceptor: Option<TlsAcceptor>,
    cancellation: CancellationToken,
) -> Result<(), AgentServerError> {
    let mut connections = JoinSet::new();
    let connection_permits = Arc::new(Semaphore::new(MAX_CONCURRENT_AGENT_CONNECTIONS));
    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let Ok(connection_permit) = Arc::clone(&connection_permits).try_acquire_owned() else {
                    warn!(%peer, "node-agent connection limit reached; dropping connection");
                    continue;
                };
                debug!(%peer, tls = tls_acceptor.is_some(), "accepted node-agent connection");
                let context = context.clone();
                let child_cancellation = cancellation.child_token();
                let acceptor = tls_acceptor.clone();
                connections.spawn(async move {
                    let _connection_permit = connection_permit;
                    let stream: Result<BoxedAgentStream, AgentServerError> = match acceptor {
                        Some(acceptor) => match tokio::time::timeout(
                            TLS_HANDSHAKE_TIMEOUT,
                            acceptor.accept(stream),
                        )
                        .await
                        {
                            Ok(result) => result
                                .map(|stream| Box::new(stream) as BoxedAgentStream)
                                .map_err(AgentServerError::TlsHandshake),
                            Err(_) => Err(AgentServerError::TlsHandshakeTimeout),
                        },
                        None => Ok(Box::new(stream) as BoxedAgentStream),
                    };
                    match stream {
                        Ok(stream) => {
                            if let Err(error) = handle_connection(
                                stream,
                                context,
                                child_cancellation,
                            )
                            .await
                            {
                                warn!(%peer, %error, "node-agent connection ended with an error");
                            }
                        }
                        Err(error) => warn!(%peer, %error, "node-agent TLS handshake failed"),
                    }
                });
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = completed {
                    warn!(%error, "node-agent connection task failed");
                }
            }
        }
    }

    while let Some(result) = connections.join_next().await {
        if let Err(error) = result {
            warn!(%error, "node-agent connection task failed during shutdown");
        }
    }
    Ok(())
}

async fn handle_connection(
    stream: BoxedAgentStream,
    context: ConnectionContext,
    cancellation: CancellationToken,
) -> Result<(), AgentServerError> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let request = tokio::time::timeout(
        REGISTRATION_TIMEOUT,
        read_request(&mut reader, context.max_frame_bytes),
    )
    .await
    .map_err(|_| AgentServerError::RegistrationTimeout)??;
    if !request.id.is_valid() {
        write_response(
            &mut writer,
            &Response::error(
                Value::Null,
                ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, "Invalid registration"),
            ),
        )
        .await?;
        return Err(AgentServerError::Protocol(
            "registration request id is invalid".to_owned(),
        ));
    }
    let response_id = request
        .id
        .response_id()
        .ok_or_else(|| AgentServerError::Protocol("registration must be a request".to_owned()))?;

    if request.jsonrpc != json_rpc::VERSION || request.method != method::AGENT_REGISTER {
        write_response(
            &mut writer,
            &Response::error(
                response_id,
                ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, "Invalid registration"),
            ),
        )
        .await?;
        return Err(AgentServerError::Protocol(
            "first node-agent request must be agent.register".to_owned(),
        ));
    }

    let params = match serde_json::from_value::<RegisterParams>(request.params) {
        Ok(params) => params,
        Err(error) => {
            write_response(
                &mut writer,
                &Response::error(
                    response_id,
                    ErrorObject::new(
                        json_rpc::error_code::INVALID_PARAMS,
                        "Invalid registration params",
                    )
                    .with_data(json!({ "detail": error.to_string() })),
                ),
            )
            .await?;
            return Ok(());
        }
    };
    if params.protocol_version != PROTOCOL_VERSION {
        write_response(
            &mut writer,
            &Response::error(
                response_id,
                ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, "Protocol mismatch")
                    .with_data(json!({
                        "expected": PROTOCOL_VERSION,
                        "actual": params.protocol_version,
                    })),
            ),
        )
        .await?;
        return Err(AgentServerError::Protocol(
            "node-agent protocol version mismatch".to_owned(),
        ));
    }

    let compute_id = ComputeInstanceId::from_uuid(params.compute_instance_id);
    let Some(compute) = context.compute_repository.get(compute_id).await? else {
        reject_registration(&mut writer, response_id, "Unknown compute instance").await?;
        return Ok(());
    };
    if !compute.is_active() || compute.provider != context.expected_provider {
        reject_registration(&mut writer, response_id, "Registration rejected").await?;
        return Ok(());
    }

    let connected_at = context.clock.now()?;
    let replacement_connection_token = match context
        .compute_repository
        .authenticate_agent(
            compute_id,
            context.expected_provider,
            &params.connection_token,
            connected_at,
        )
        .await?
    {
        AgentAuthentication::Accepted => None,
        AgentAuthentication::ReplaceToken(token) => Some(token),
        AgentAuthentication::Rejected => {
            reject_registration(&mut writer, response_id, "Registration rejected").await?;
            return Ok(());
        }
    };

    write_response(
        &mut writer,
        &Response::success(
            response_id,
            serde_json::to_value(RegisterResult {
                accepted: true,
                replacement_connection_token: replacement_connection_token.clone(),
            })?,
        ),
    )
    .await?;

    if replacement_connection_token.is_some() {
        info!(
            compute_instance_id = %compute_id,
            "remote node agent enrollment accepted; reconnect token issued"
        );
        return Ok(());
    }

    let session_id = Uuid::new_v4();
    let (sender, receiver) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
    context.registry.register(compute_id, session_id, sender).await;
    context
        .reconcile_scheduler
        .enqueue_best_effort_for_instance(compute.server_instance_id);
    info!(compute_instance_id = %compute_id, "node agent registered");

    let result = run_session(
        &mut reader,
        &mut writer,
        receiver,
        context.max_frame_bytes,
        context.command_timeout,
        cancellation,
    )
    .await;
    context.registry.unregister(compute_id, session_id).await;
    context
        .reconcile_scheduler
        .enqueue_best_effort_for_instance(compute.server_instance_id);
    info!(compute_instance_id = %compute_id, "node agent disconnected");
    result
}

async fn reject_registration<W>(
    writer: &mut W,
    id: Value,
    message: &str,
) -> Result<(), AgentServerError>
where
    W: AsyncWrite + Unpin,
{
    write_response(
        writer,
        &Response::error(
            id,
            ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, message),
        ),
    )
    .await
}

async fn run_session<R, W>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    mut commands: mpsc::Receiver<AgentCommand>,
    max_frame_bytes: usize,
    command_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<(), AgentServerError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut next_id = 1_u64;
    loop {
        tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            command = commands.recv() => {
                let Some(command) = command else {
                    return Ok(());
                };
                let id = next_id;
                next_id = next_id.checked_add(1).unwrap_or(1);
                let request = json!({
                    "jsonrpc": json_rpc::VERSION,
                    "method": command.method,
                    "params": command.params,
                    "id": id,
                });
                let result = async {
                    write_command_request(writer, &request).await?;
                    let response = read_wire_response(reader, max_frame_bytes).await?;
                    if response.jsonrpc != json_rpc::VERSION {
                        return Err(AgentCallError::Protocol(
                            "node-agent response JSON-RPC version mismatch".to_owned(),
                        ));
                    }
                    if response.id != json!(id) {
                        return Err(AgentCallError::Protocol(
                            "node-agent response id does not match request".to_owned(),
                        ));
                    }
                    match (response.result, response.error) {
                        (Some(result), None) => Ok(result),
                        (None, Some(error)) => {
                            let detail = error
                                .data
                                .as_ref()
                                .and_then(|data| data.get("detail"))
                                .and_then(Value::as_str);
                            let message = match detail {
                                Some(detail) if !detail.trim().is_empty() => {
                                    format!("{}: {detail}", error.message)
                                }
                                _ => error.message,
                            };
                            Err(AgentCallError::Remote {
                                code: error.code,
                                message,
                            })
                        }
                        _ => Err(AgentCallError::Protocol(
                            "node-agent response must contain exactly one of result or error"
                                .to_owned(),
                        )),
                    }
                };
                let result = tokio::select! {
                    () = cancellation.cancelled() => return Ok(()),
                    result = tokio::time::timeout(command_timeout, result) => {
                        result
                            .map_err(|_| AgentCallError::Timeout)
                            .and_then(|result| result)
                    }
                };
                let session_unusable = matches!(
                    &result,
                    Err(
                        AgentCallError::Disconnected
                            | AgentCallError::Io(_)
                            | AgentCallError::Timeout
                            | AgentCallError::Protocol(_)
                    )
                );
                let _ = command.response.send(result);
                if session_unusable {
                    return Ok(());
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct WireResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<ErrorObjectWire>,
    id: Value,
}

#[derive(Debug, Deserialize)]
struct ErrorObjectWire {
    code: i64,
    message: String,
    data: Option<Value>,
}

async fn read_request<R>(
    reader: &mut BufReader<R>,
    max_frame_bytes: usize,
) -> Result<Request, AgentServerError>
where
    R: AsyncRead + Unpin,
{
    let value = read_value(reader, max_frame_bytes).await?;
    serde_json::from_value(value).map_err(AgentServerError::Serialization)
}

async fn read_wire_response<R>(
    reader: &mut BufReader<R>,
    max_frame_bytes: usize,
) -> Result<WireResponse, AgentCallError>
where
    R: AsyncRead + Unpin,
{
    let value = read_value(reader, max_frame_bytes)
        .await
        .map_err(|error| match error {
            AgentServerError::Io(error) => AgentCallError::Io(error),
            AgentServerError::Disconnected => AgentCallError::Disconnected,
            other => AgentCallError::Protocol(other.to_string()),
        })?;
    serde_json::from_value(value).map_err(AgentCallError::Serialization)
}

async fn read_value<R>(
    reader: &mut BufReader<R>,
    max_frame_bytes: usize,
) -> Result<Value, AgentServerError>
where
    R: AsyncRead + Unpin,
{
    let mut frame = Vec::new();
    let read = read_frame(reader, &mut frame, max_frame_bytes).await?;
    if read == 0 {
        return Err(AgentServerError::Disconnected);
    }
    let input = trim_line_ending(&frame);
    serde_json::from_slice(input).map_err(AgentServerError::Serialization)
}

async fn read_frame<R>(
    reader: &mut R,
    frame: &mut Vec<u8>,
    maximum: usize,
) -> Result<usize, AgentServerError>
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
                return Err(AgentServerError::FrameTooLarge { actual, maximum });
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

async fn write_response<W>(writer: &mut W, response: &Response) -> Result<(), AgentServerError>
where
    W: AsyncWrite + Unpin,
{
    write_value(writer, &serde_json::to_value(response)?).await
}

async fn write_command_request<W>(
    writer: &mut W,
    request: &Value,
) -> Result<(), AgentCallError>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&serde_json::to_vec(request)?).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn write_value<W>(writer: &mut W, value: &Value) -> Result<(), AgentServerError>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&serde_json::to_vec(value)?).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum AgentServerError {
    #[error("node-agent listener I/O failed")]
    Io(#[from] io::Error),
    #[error("node-agent disconnected")]
    Disconnected,
    #[error("node-agent frame is too large: at least {actual} bytes, maximum {maximum} bytes")]
    FrameTooLarge { actual: usize, maximum: usize },
    #[error("node-agent protocol serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("node-agent protocol violation: {0}")]
    Protocol(String),
    #[error("node-agent TLS configuration is invalid: {0}")]
    TlsConfiguration(String),
    #[error("node-agent TLS handshake failed")]
    TlsHandshake(#[source] io::Error),
    #[error("node-agent TLS handshake timed out")]
    TlsHandshakeTimeout,
    #[error("node-agent registration timed out")]
    RegistrationTimeout,
    #[error("node-agent registration persistence failed")]
    Repository(#[from] crate::infrastructure::RepositoryError),
    #[error("node-agent registration timestamp failed")]
    Timestamp(#[from] crate::domain::TimestampError),
}
