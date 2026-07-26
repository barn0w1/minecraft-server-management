use mcserver_protocol::{
    client::{
        self, ComputeInstanceResource, CreateServerParams, GetServerInstanceParams,
        GetServerParams, ListServerInstancesParams, ListServerInstancesResult, ListServersResult,
        PingResult, ServerInstanceResource, ServerResource, ServerStatusResource,
        SetServerDesiredStateParams,
    },
    json_rpc::{self, ErrorObject, Request, Response},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tracing::error;

use crate::{
    application::{
        ApplicationError, ServerInstanceService, ServerService, ServerStatus, ServerStatusService,
    },
    domain::{
        ComputeInstance, ComputeSpec, ComputeTerminalResult, DataSpec, DesiredState, ProcessSpec,
        Server, ServerId, ServerInstance, ServerInstanceId, ServerSpec, TerminalResult,
    },
};

#[derive(Debug, Clone)]
pub struct ClientRpcHandler {
    server_service: ServerService,
    server_instance_service: ServerInstanceService,
    server_status_service: ServerStatusService,
}

impl ClientRpcHandler {
    #[must_use]
    pub fn new(
        server_service: ServerService,
        server_instance_service: ServerInstanceService,
        server_status_service: ServerStatusService,
    ) -> Self {
        Self {
            server_service,
            server_instance_service,
            server_status_service,
        }
    }

    pub async fn handle_json(&self, input: &str) -> Option<Value> {
        match serde_json::from_str::<Value>(input) {
            Ok(value) => self.handle_value(value).await,
            Err(error) => Some(response_value(Response::error(
                Value::Null,
                ErrorObject::new(json_rpc::error_code::PARSE_ERROR, "Parse error")
                    .with_data(json!({ "detail": error.to_string() })),
            ))),
        }
    }

    async fn handle_value(&self, value: Value) -> Option<Value> {
        match value {
            Value::Array(requests) if requests.is_empty() => Some(response_value(Response::error(
                Value::Null,
                ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, "Invalid Request"),
            ))),
            Value::Array(requests) => {
                let mut responses = Vec::new();
                for request in requests {
                    if let Some(response) = self.handle_single(request).await {
                        responses.push(response);
                    }
                }

                if responses.is_empty() {
                    None
                } else {
                    Some(Value::Array(responses))
                }
            }
            request => self.handle_single(request).await,
        }
    }

    async fn handle_single(&self, value: Value) -> Option<Value> {
        let request = match serde_json::from_value::<Request>(value) {
            Ok(request) => request,
            Err(error) => {
                return Some(response_value(Response::error(
                    Value::Null,
                    ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, "Invalid Request")
                        .with_data(json!({ "detail": error.to_string() })),
                )));
            }
        };

        if request.jsonrpc != json_rpc::VERSION
            || request.method.is_empty()
            || !request.id.is_valid()
        {
            return Some(response_value(Response::error(
                request.id.response_id().unwrap_or(Value::Null),
                ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, "Invalid Request"),
            )));
        }

        if !matches!(
            &request.params,
            Value::Null | Value::Array(_) | Value::Object(_)
        ) {
            return request.id.response_id().map(|id| {
                response_value(Response::error(
                    id,
                    ErrorObject::new(json_rpc::error_code::INVALID_PARAMS, "Invalid params"),
                ))
            });
        }

        let is_notification = request.id.is_notification();
        let response_id = request.id.response_id().unwrap_or(Value::Null);
        let result = self.dispatch(&request.method, request.params).await;

        if is_notification {
            if let Err(error) = result {
                error!(method = request.method, %error, "JSON-RPC notification failed");
            }
            return None;
        }

        Some(match result {
            Ok(result) => response_value(Response::success(response_id, result)),
            Err(error) => response_value(Response::error(response_id, error.into_error_object())),
        })
    }

    async fn dispatch(&self, method: &str, params: Value) -> Result<Value, RpcDispatchError> {
        match method {
            client::method::SYSTEM_PING => {
                require_no_params(&params)?;
                to_value(PingResult {
                    status: "ok".to_owned(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                })
            }
            client::method::SERVER_CREATE => {
                let params = parse_params::<CreateServerParams>(params)?;
                let server = self
                    .server_service
                    .create(params.name, domain_spec(params.spec))
                    .await?;
                to_value(protocol_server(server))
            }
            client::method::SERVER_GET => {
                let params = parse_params::<GetServerParams>(params)?;
                let server = self
                    .server_service
                    .get(ServerId::from_uuid(params.server_id))
                    .await?;
                to_value(protocol_server(server))
            }
            client::method::SERVER_LIST => {
                require_no_params(&params)?;
                let servers = self
                    .server_service
                    .list()
                    .await?
                    .into_iter()
                    .map(protocol_server)
                    .collect();
                to_value(ListServersResult { servers })
            }
            client::method::SERVER_STATUS => {
                let params = parse_params::<GetServerParams>(params)?;
                let status = self
                    .server_status_service
                    .get(ServerId::from_uuid(params.server_id))
                    .await?;
                to_value(protocol_server_status(status))
            }
            client::method::SERVER_SET_DESIRED_STATE => {
                let params = parse_params::<SetServerDesiredStateParams>(params)?;
                let server = self
                    .server_service
                    .set_desired_state(
                        ServerId::from_uuid(params.server_id),
                        domain_desired_state(params.desired_state),
                        params.expected_generation,
                    )
                    .await?;
                to_value(protocol_server(server))
            }
            client::method::SERVER_INSTANCE_GET => {
                let params = parse_params::<GetServerInstanceParams>(params)?;
                let instance = self
                    .server_instance_service
                    .get(ServerInstanceId::from_uuid(params.server_instance_id))
                    .await?;
                to_value(protocol_server_instance(instance))
            }
            client::method::SERVER_INSTANCE_LIST => {
                let params = parse_params::<ListServerInstancesParams>(params)?;
                let server_instances = self
                    .server_instance_service
                    .list_for_server(ServerId::from_uuid(params.server_id))
                    .await?
                    .into_iter()
                    .map(protocol_server_instance)
                    .collect();
                to_value(ListServerInstancesResult { server_instances })
            }
            _ => Err(RpcDispatchError::MethodNotFound),
        }
    }
}

fn parse_params<T: DeserializeOwned>(params: Value) -> Result<T, RpcDispatchError> {
    serde_json::from_value(params)
        .map_err(|error| RpcDispatchError::InvalidParams(error.to_string()))
}

fn require_no_params(params: &Value) -> Result<(), RpcDispatchError> {
    match params {
        Value::Null => Ok(()),
        Value::Array(values) if values.is_empty() => Ok(()),
        Value::Object(values) if values.is_empty() => Ok(()),
        _ => Err(RpcDispatchError::InvalidParams(
            "this method does not accept params".to_owned(),
        )),
    }
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, RpcDispatchError> {
    serde_json::to_value(value).map_err(RpcDispatchError::Serialization)
}

fn response_value(response: Response) -> Value {
    match serde_json::to_value(response) {
        Ok(value) => value,
        Err(error) => {
            error!(%error, "failed to serialize JSON-RPC response");
            json!({
                "jsonrpc": json_rpc::VERSION,
                "error": {
                    "code": json_rpc::error_code::INTERNAL_ERROR,
                    "message": "Internal error"
                },
                "id": null
            })
        }
    }
}

fn domain_spec(spec: client::ServerSpec) -> ServerSpec {
    ServerSpec {
        compute: match spec.compute {
            client::ComputeSpec::Local => ComputeSpec::Local,
        },
        process: ProcessSpec {
            container_image: spec.process.container_image,
            server_type: spec.process.server_type,
            version: spec.process.version,
            host_port: spec.process.host_port,
            stop_timeout_seconds: spec.process.stop_timeout_seconds,
            accept_eula: spec.process.accept_eula,
            environment: spec.process.environment,
        },
        data: DataSpec {
            repository: spec.data.repository,
        },
    }
}

fn protocol_spec(spec: ServerSpec) -> client::ServerSpec {
    client::ServerSpec {
        compute: match spec.compute {
            ComputeSpec::Local => client::ComputeSpec::Local,
        },
        process: client::ProcessSpec {
            container_image: spec.process.container_image,
            server_type: spec.process.server_type,
            version: spec.process.version,
            host_port: spec.process.host_port,
            stop_timeout_seconds: spec.process.stop_timeout_seconds,
            accept_eula: spec.process.accept_eula,
            environment: spec.process.environment,
        },
        data: client::DataSpec {
            repository: spec.data.repository,
        },
    }
}

const fn domain_desired_state(state: client::DesiredState) -> DesiredState {
    match state {
        client::DesiredState::Running => DesiredState::Running,
        client::DesiredState::Stopped => DesiredState::Stopped,
    }
}

const fn protocol_desired_state(state: DesiredState) -> client::DesiredState {
    match state {
        DesiredState::Running => client::DesiredState::Running,
        DesiredState::Stopped => client::DesiredState::Stopped,
    }
}

fn protocol_server(server: Server) -> ServerResource {
    ServerResource {
        id: server.id.as_uuid(),
        name: server.name.as_str().to_owned(),
        generation: server.generation,
        desired_state: protocol_desired_state(server.desired_state),
        spec: protocol_spec(server.spec),
        created_at_ms: server.created_at.as_millis(),
        current_snapshot_id: server.current_snapshot_id,
        updated_at_ms: server.updated_at.as_millis(),
    }
}

fn protocol_server_instance(instance: ServerInstance) -> ServerInstanceResource {
    ServerInstanceResource {
        id: instance.id.as_uuid(),
        server_id: instance.server_id.as_uuid(),
        server_generation: instance.server_generation,
        resolved_spec: protocol_spec(instance.resolved_spec),
        fencing_token: instance.fencing_token,
        source_snapshot_id: instance.source_snapshot_id,
        data_prepared_at_ms: instance.data_prepared_at.map(|value| value.as_millis()),
        process_running: instance.process_running,
        process_observed_at_ms: instance.process_observed_at.map(|value| value.as_millis()),
        result_snapshot_id: instance.result_snapshot_id,
        last_error: instance.last_error,
        stop_requested_at_ms: instance.stop_requested_at.map(|value| value.as_millis()),
        terminated_at_ms: instance.terminated_at.map(|value| value.as_millis()),
        terminal_result: instance.terminal_result.map(protocol_terminal_result),
        created_at_ms: instance.created_at.as_millis(),
        updated_at_ms: instance.updated_at.as_millis(),
    }
}

fn protocol_compute_instance(compute: ComputeInstance) -> ComputeInstanceResource {
    ComputeInstanceResource {
        id: compute.id.as_uuid(),
        server_instance_id: compute.server_instance_id.as_uuid(),
        process_id: compute.process_id,
        agent_connected_at_ms: compute.agent_connected_at.map(|value| value.as_millis()),
        shutdown_requested_at_ms: compute
            .shutdown_requested_at
            .map(|value| value.as_millis()),
        terminated_at_ms: compute.terminated_at.map(|value| value.as_millis()),
        terminal_result: compute
            .terminal_result
            .map(protocol_compute_terminal_result),
        failure_message: compute.failure_message,
        created_at_ms: compute.created_at.as_millis(),
        updated_at_ms: compute.updated_at.as_millis(),
    }
}

fn protocol_server_status(status: ServerStatus) -> ServerStatusResource {
    ServerStatusResource {
        server: protocol_server(status.server),
        active_instance: status.active_instance.map(protocol_server_instance),
        active_compute: status.active_compute.map(protocol_compute_instance),
        agent_connected: status.agent_connected,
    }
}

const fn protocol_compute_terminal_result(
    result: ComputeTerminalResult,
) -> client::ComputeTerminalResult {
    match result {
        ComputeTerminalResult::Deleted => client::ComputeTerminalResult::Deleted,
        ComputeTerminalResult::Failed => client::ComputeTerminalResult::Failed,
    }
}

const fn protocol_terminal_result(result: TerminalResult) -> client::TerminalResult {
    match result {
        TerminalResult::Completed => client::TerminalResult::Completed,
        TerminalResult::Failed => client::TerminalResult::Failed,
    }
}

#[derive(Debug, thiserror::Error)]
enum RpcDispatchError {
    #[error("method not found")]
    MethodNotFound,
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("application error: {0}")]
    Application(#[from] ApplicationError),
    #[error("response serialization failed")]
    Serialization(serde_json::Error),
}

impl RpcDispatchError {
    fn into_error_object(self) -> ErrorObject {
        match self {
            Self::MethodNotFound => {
                ErrorObject::new(json_rpc::error_code::METHOD_NOT_FOUND, "Method not found")
            }
            Self::InvalidParams(detail) => {
                ErrorObject::new(json_rpc::error_code::INVALID_PARAMS, "Invalid params")
                    .with_data(json!({ "detail": detail }))
            }
            Self::Application(ApplicationError::Validation(error)) => {
                ErrorObject::new(json_rpc::error_code::INVALID_PARAMS, "Invalid params")
                    .with_data(json!({ "detail": error.to_string() }))
            }
            Self::Application(ApplicationError::NotFound) => {
                ErrorObject::new(json_rpc::error_code::NOT_FOUND, "Server not found")
            }
            Self::Application(ApplicationError::ServerInstanceNotFound) => {
                ErrorObject::new(json_rpc::error_code::NOT_FOUND, "Server instance not found")
            }
            Self::Application(
                error @ (ApplicationError::GenerationConflict { .. }
                | ApplicationError::ConcurrentUpdate
                | ApplicationError::Repository(
                    crate::infrastructure::RepositoryError::Conflict(_),
                )),
            ) => ErrorObject::new(json_rpc::error_code::CONFLICT, "Resource conflict")
                .with_data(json!({ "detail": error.to_string() })),
            Self::Application(error) => {
                error!(%error, "control-plane application operation failed");
                ErrorObject::new(json_rpc::error_code::INTERNAL_ERROR, "Internal error")
            }
            Self::Serialization(error) => {
                error!(%error, "JSON-RPC result serialization failed");
                ErrorObject::new(json_rpc::error_code::INTERNAL_ERROR, "Internal error")
            }
        }
    }
}
