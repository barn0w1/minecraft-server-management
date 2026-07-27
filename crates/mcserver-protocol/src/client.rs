use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod method {
    pub const SYSTEM_PING: &str = "system.ping";
    pub const SERVER_CREATE: &str = "server.create";
    pub const SERVER_GET: &str = "server.get";
    pub const SERVER_LIST: &str = "server.list";
    pub const SERVER_STATUS: &str = "server.status";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeTerminalResult {
    Deleted,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeProvider {
    LocalProcess,
    Akamai,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ComputeSpec {
    Local,
    Akamai {
        region: String,
        instance_type: String,
        image: String,
        #[serde(default)]
        firewall_id: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub container_image: String,
    pub server_type: String,
    pub version: String,
    pub host_port: u16,
    #[serde(default = "default_stop_timeout_seconds")]
    pub stop_timeout_seconds: u64,
    pub accept_eula: bool,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

const fn default_stop_timeout_seconds() -> u64 {
    30
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResult {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerResource {
    pub id: Uuid,
    pub name: String,
    pub generation: u64,
    pub desired_state: DesiredState,
    pub spec: ServerSpec,
    pub created_at_ms: i64,
    pub current_snapshot_id: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListServersResult {
    pub servers: Vec<ServerResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInstanceResource {
    pub id: Uuid,
    pub server_id: Uuid,
    pub server_generation: u64,
    pub resolved_spec: ServerSpec,
    pub fencing_token: u64,
    pub source_snapshot_id: Option<String>,
    pub data_prepared_at_ms: Option<i64>,
    pub process_running: bool,
    pub process_observed_at_ms: Option<i64>,
    pub result_snapshot_id: Option<String>,
    pub last_error: Option<String>,
    pub stop_requested_at_ms: Option<i64>,
    pub terminated_at_ms: Option<i64>,
    pub terminal_result: Option<TerminalResult>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListServerInstancesResult {
    pub server_instances: Vec<ServerInstanceResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeInstanceResource {
    pub id: Uuid,
    pub server_instance_id: Uuid,
    pub provider: ComputeProvider,
    pub provider_instance_id: Option<String>,
    pub public_ipv4: Option<String>,
    pub process_id: Option<u32>,
    pub agent_connected_at_ms: Option<i64>,
    pub shutdown_requested_at_ms: Option<i64>,
    pub terminated_at_ms: Option<i64>,
    pub terminal_result: Option<ComputeTerminalResult>,
    pub failure_message: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerStatusResource {
    pub server: ServerResource,
    pub active_instance: Option<ServerInstanceResource>,
    pub active_compute: Option<ComputeInstanceResource>,
    pub agent_connected: bool,
}
