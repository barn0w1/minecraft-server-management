use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod method {
    pub const SYSTEM_PING: &str = "system.ping";
    pub const SERVER_CREATE: &str = "server.create";
    pub const SERVER_GET: &str = "server.get";
    pub const SERVER_LIST: &str = "server.list";
    pub const SERVER_SET_DESIRED_STATE: &str = "server.set_desired_state";
    pub const SERVER_INSTANCE_GET: &str = "server_instance.get";
    pub const SERVER_INSTANCE_LIST: &str = "server_instance.list";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredState {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalResult {
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeSpec {
    pub region: String,
    pub instance_type: String,
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub container_image: String,
    pub server_type: String,
    pub version: String,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSpec {
    pub repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSpec {
    pub compute: ComputeSpec,
    pub process: ProcessSpec,
    pub data: DataSpec,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateServerParams {
    pub name: String,
    pub spec: ServerSpec,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetServerParams {
    pub server_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetServerDesiredStateParams {
    pub server_id: Uuid,
    pub desired_state: DesiredState,
    #[serde(default)]
    pub expected_generation: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetServerInstanceParams {
    pub server_instance_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListServerInstancesParams {
    pub server_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
pub struct PingResult {
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerResource {
    pub id: Uuid,
    pub name: String,
    pub generation: u64,
    pub desired_state: DesiredState,
    pub spec: ServerSpec,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListServersResult {
    pub servers: Vec<ServerResource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerInstanceResource {
    pub id: Uuid,
    pub server_id: Uuid,
    pub server_generation: u64,
    pub resolved_spec: ServerSpec,
    pub fencing_token: u64,
    pub stop_requested_at_ms: Option<i64>,
    pub terminated_at_ms: Option<i64>,
    pub terminal_result: Option<TerminalResult>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListServerInstancesResult {
    pub server_instances: Vec<ServerInstanceResource>,
}
