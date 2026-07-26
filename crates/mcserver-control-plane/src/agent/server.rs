use std::{io, net::SocketAddr, time::Duration};

use mcserver_protocol::{
    json_rpc::{self, ErrorObject, Request, Response},
    node_agent::{PROTOCOL_VERSION, RegisterParams, RegisterResult, method},
};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinSet,
};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
    domain::{Clock, ComputeInstanceId, SystemClock},
    infrastructure::ComputeInstanceRepository,
    reconciliation::ReconcileScheduler,
    shutdown::CancellationToken,
};

use super::registry::{AgentCallError, AgentCommand, AgentRegistry};

const COMMAND_CHANNEL_CAPACITY: usize = 32;

pub struct AgentServer {
    listener: TcpListener,
    registry: AgentRegistry,
    compute_repository: ComputeInstanceRepository,
    reconcile_scheduler: ReconcileScheduler,
    max_frame_bytes: usize,
    command_timeout: Duration,
    clock: SystemClock,
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
        info!(address = %listener.local_addr()?, "node-agent JSON-RPC listener is ready");
        Ok(Self {
            listener,
            registry,
            compute_repository,
            reconcile_scheduler,
            max_frame_bytes,
            command_timeout,
            clock: SystemClock,
        })
    }

    pub async fn run(self, cancellation: CancellationToken) -> Result<(), AgentServerError> {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                accepted = self.listener.accept() => {
                    let (stream, peer) = accepted?;
                    debug!(%peer, "accepted node-agent connection");
                    let registry = self.registry.clone();
                    let compute_repository = self.compute_repository.clone();
                    let reconcile_scheduler = self.reconcile_scheduler.clone();
                    let child_cancellation = cancellation.child_token();
                    let maximum = self.max_frame_bytes;
                    let command_timeout = self.command_timeout;
                    let clock = self.clock;
                    connections.spawn(async move {
                        if let Err(error) = handle_connection(
                            stream,
                            registry,
                            compute_repository,
                            reconcile_scheduler,
                            maximum,
                            command_timeout,
                            clock,
                            child_cancellation,
                        )
                        .await
                        {
                            warn!(%peer, %error, "node-agent connection ended with an error");
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
        info!("node-agent JSON-RPC listener stopped");
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    stream: TcpStream,
    registry: AgentRegistry,
    compute_repository: ComputeInstanceRepository,
    reconcile_scheduler: ReconcileScheduler,
    max_frame_bytes: usize,
    command_timeout: Duration,
    clock: SystemClock,
    cancellation: CancellationToken,
) -> Result<(), AgentServerError> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let request = read_request(&mut reader, max_frame_bytes).await?;
    if !request.id.is_valid() {
        write_response(
            &mut writer,
            &Response::error(
                Value::Null,
                ErrorObject::new(
                    json_rpc::error_code::INVALID_REQUEST,
                    "Invalid registration",
                ),
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
                ErrorObject::new(
                    json_rpc::error_code::INVALID_REQUEST,
                    "Invalid registration",
                ),
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
    let Some(compute) = compute_repository.get(compute_id).await? else {
        reject_registration(&mut writer, response_id, "Unknown compute instance").await?;
        return Ok(());
    };
    if !compute.is_active() || compute.connection_token != params.connection_token {
        reject_registration(&mut writer, response_id, "Registration rejected").await?;
        return Ok(());
    }

    let connected_at = clock.now()?;
    if !compute_repository
        .mark_agent_connected(compute_id, connected_at)
        .await?
    {
        reject_registration(&mut writer, response_id, "Registration became stale").await?;
        return Ok(());
    }

    write_response(
        &mut writer,
        &Response::success(
            response_id,
            serde_json::to_value(RegisterResult { accepted: true })?,
        ),
    )
    .await?;

    let session_id = Uuid::new_v4();
    let (sender, receiver) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
    registry.register(compute_id, session_id, sender).await;
    reconcile_scheduler.enqueue_best_effort_for_instance(compute.server_instance_id);
    info!(compute_instance_id = %compute_id, "node agent registered");

    let result = run_session(
        &mut reader,
        &mut writer,
        receiver,
        max_frame_bytes,
        command_timeout,
        cancellation,
    )
    .await;
    registry.unregister(compute_id, session_id).await;
    reconcile_scheduler.enqueue_best_effort_for_instance(compute.server_instance_id);
    info!(compute_instance_id = %compute_id, "node agent disconnected");
    result
}

async fn reject_registration(
    writer: &mut WriteHalf<TcpStream>,
    id: Value,
    message: &str,
) -> Result<(), AgentServerError> {
    write_response(
        writer,
        &Response::error(
            id,
            ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, message),
        ),
    )
    .await
}

async fn run_session(
    reader: &mut BufReader<ReadHalf<TcpStream>>,
    writer: &mut WriteHalf<TcpStream>,
    mut commands: mpsc::Receiver<AgentCommand>,
    max_frame_bytes: usize,
    command_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<(), AgentServerError> {
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
                    write_value(writer, &request).await?;
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
                        (None, Some(error)) => Err(AgentCallError::Remote {
                            code: error.code,
                            message: error.message,
                        }),
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
}

async fn read_request(
    reader: &mut BufReader<ReadHalf<TcpStream>>,
    max_frame_bytes: usize,
) -> Result<Request, AgentServerError> {
    let value = read_value(reader, max_frame_bytes).await?;
    serde_json::from_value(value).map_err(AgentServerError::Serialization)
}

async fn read_wire_response(
    reader: &mut BufReader<ReadHalf<TcpStream>>,
    max_frame_bytes: usize,
) -> Result<WireResponse, AgentCallError> {
    let value = read_value(reader, max_frame_bytes)
        .await
        .map_err(|error| match error {
            AgentServerError::Io(error) => AgentCallError::Io(error),
            AgentServerError::Disconnected => AgentCallError::Disconnected,
            other => AgentCallError::Protocol(other.to_string()),
        })?;
    serde_json::from_value(value).map_err(AgentCallError::Serialization)
}

async fn read_value(
    reader: &mut BufReader<ReadHalf<TcpStream>>,
    max_frame_bytes: usize,
) -> Result<Value, AgentServerError> {
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

async fn write_response(
    writer: &mut WriteHalf<TcpStream>,
    response: &Response,
) -> Result<(), AgentServerError> {
    write_value(writer, &serde_json::to_value(response)?).await
}

async fn write_value(
    writer: &mut WriteHalf<TcpStream>,
    value: &Value,
) -> Result<(), AgentServerError> {
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
    #[error("node-agent registration persistence failed")]
    Repository(#[from] crate::infrastructure::RepositoryError),
    #[error("node-agent registration timestamp failed")]
    Timestamp(#[from] crate::domain::TimestampError),
}
