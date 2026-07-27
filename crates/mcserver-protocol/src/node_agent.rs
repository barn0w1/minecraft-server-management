use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 3;

pub mod method {
    pub const AGENT_ENROLL: &str = "agent.enroll";
    pub const AGENT_REGISTER: &str = "agent.register";
    pub const AGENT_INSPECT: &str = "agent.inspect";
    pub const DATA_RESTORE: &str = "data.restore";
    pub const SERVER_START: &str = "server.start";
    pub const SERVER_STOP: &str = "server.stop";
    pub const DATA_SNAPSHOT: &str = "data.snapshot";
    pub const INSTANCE_CLEANUP: &str = "instance.cleanup";
    pub const NODE_SHUTDOWN: &str = "node.shutdown";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollParams {
    pub protocol_version: u32,
    pub compute_instance_id: Uuid,
    pub enrollment_token: String,
    pub certificate_signing_request_pem: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollResult {
    pub client_certificate_chain_pem: String,
    pub connection_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterParams {
    pub protocol_version: u32,
    pub compute_instance_id: Uuid,
    pub connection_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterResult {
    pub accepted: bool,
    #[serde(default)]
    pub runtime_environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceIdentity {
    pub server_instance_id: Uuid,
    pub fencing_token: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInspectParams {
    pub instance: InstanceIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInspectResult {
    pub data_prepared: bool,
    pub process_running: bool,
    pub last_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreDataParams {
    pub instance: InstanceIdentity,
    pub server_id: Uuid,
    pub repository: String,
    pub source_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub container_image: String,
    pub server_type: String,
    pub version: String,
    pub host_port: u16,
    pub stop_timeout_seconds: u64,
    pub accept_eula: bool,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartServerParams {
    pub instance: InstanceIdentity,
    pub process: ProcessSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopServerParams {
    pub instance: InstanceIdentity,
    pub stop_timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDataParams {
    pub instance: InstanceIdentity,
    pub server_id: Uuid,
    pub repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupInstanceParams {
    pub instance: InstanceIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedResult {
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDataResult {
    pub snapshot_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownResult {
    pub accepted: bool,
}
