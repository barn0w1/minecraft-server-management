use crate::{
    agent::AgentRegistry,
    domain::{ComputeInstance, Server, ServerId, ServerInstance},
    infrastructure::{ComputeInstanceRepository, ServerInstanceRepository, ServerRepository},
};

use super::ApplicationError;

#[derive(Debug, Clone)]
pub struct ServerStatusService {
    server_repository: ServerRepository,
    instance_repository: ServerInstanceRepository,
    compute_repository: ComputeInstanceRepository,
    agents: AgentRegistry,
}

impl ServerStatusService {
    #[must_use]
    pub fn new(
        server_repository: ServerRepository,
        instance_repository: ServerInstanceRepository,
        compute_repository: ComputeInstanceRepository,
        agents: AgentRegistry,
    ) -> Self {
        Self {
            server_repository,
            instance_repository,
            compute_repository,
            agents,
        }
    }

    pub async fn get(&self, server_id: ServerId) -> Result<ServerStatus, ApplicationError> {
        let server = self
            .server_repository
            .get(server_id)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        let active_instance = self
            .instance_repository
            .get_active_for_server(server_id)
            .await?;
        let active_compute = match active_instance.as_ref() {
            Some(instance) => {
                self.compute_repository
                    .get_active_for_instance(instance.id)
                    .await?
            }
            None => None,
        };
        let agent_connected = match active_compute.as_ref() {
            Some(compute) => self.agents.is_connected(compute.id).await,
            None => false,
        };

        Ok(ServerStatus {
            server,
            active_instance,
            active_compute,
            agent_connected,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ServerStatus {
    pub server: Server,
    pub active_instance: Option<ServerInstance>,
    pub active_compute: Option<ComputeInstance>,
    pub agent_connected: bool,
}
