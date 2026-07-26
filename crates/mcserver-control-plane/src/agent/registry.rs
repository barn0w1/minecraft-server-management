use std::{collections::HashMap, sync::Arc, time::Duration};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{RwLock, mpsc, oneshot};
use uuid::Uuid;

use crate::domain::ComputeInstanceId;

#[derive(Debug)]
pub(super) struct AgentCommand {
    pub method: &'static str,
    pub params: Value,
    pub response: oneshot::Sender<Result<Value, AgentCallError>>,
}

#[derive(Debug, Clone)]
struct AgentHandle {
    session_id: Uuid,
    sender: mpsc::Sender<AgentCommand>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentRegistry {
    sessions: Arc<RwLock<HashMap<ComputeInstanceId, AgentHandle>>>,
}

impl AgentRegistry {
    pub(super) async fn register(
        &self,
        compute_instance_id: ComputeInstanceId,
        session_id: Uuid,
        sender: mpsc::Sender<AgentCommand>,
    ) {
        self.sessions
            .write()
            .await
            .insert(compute_instance_id, AgentHandle { session_id, sender });
    }

    pub(super) async fn unregister(
        &self,
        compute_instance_id: ComputeInstanceId,
        session_id: Uuid,
    ) {
        let mut sessions = self.sessions.write().await;
        if sessions
            .get(&compute_instance_id)
            .is_some_and(|handle| handle.session_id == session_id)
        {
            sessions.remove(&compute_instance_id);
        }
    }

    pub async fn is_connected(&self, compute_instance_id: ComputeInstanceId) -> bool {
        self.sessions
            .read()
            .await
            .contains_key(&compute_instance_id)
    }

    pub async fn call<P, R>(
        &self,
        compute_instance_id: ComputeInstanceId,
        method: &'static str,
        params: &P,
        timeout: Duration,
    ) -> Result<R, AgentCallError>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let handle = self
            .sessions
            .read()
            .await
            .get(&compute_instance_id)
            .cloned()
            .ok_or(AgentCallError::NotConnected)?;
        let params = serde_json::to_value(params)?;
        let (response_sender, response_receiver) = oneshot::channel();
        handle
            .sender
            .send(AgentCommand {
                method,
                params,
                response: response_sender,
            })
            .await
            .map_err(|_| AgentCallError::Disconnected)?;

        let watchdog_timeout = timeout
            .checked_add(Duration::from_secs(5))
            .unwrap_or(timeout);
        let value = tokio::time::timeout(watchdog_timeout, response_receiver)
            .await
            .map_err(|_| AgentCallError::Timeout)?
            .map_err(|_| AgentCallError::Disconnected)??;
        serde_json::from_value(value).map_err(AgentCallError::Serialization)
    }
}

#[derive(Debug, Error)]
pub enum AgentCallError {
    #[error("node agent is not connected")]
    NotConnected,
    #[error("node agent disconnected")]
    Disconnected,
    #[error("node agent command timed out")]
    Timeout,
    #[error("node agent rejected the command: {code}: {message}")]
    Remote { code: i64, message: String },
    #[error("node agent protocol serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("node agent protocol violation: {0}")]
    Protocol(String),
    #[error("node agent transport failed")]
    Io(#[from] std::io::Error),
}
