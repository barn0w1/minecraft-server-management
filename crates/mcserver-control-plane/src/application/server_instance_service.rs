use crate::{
    domain::{ServerId, ServerInstance, ServerInstanceId},
    infrastructure::ServerInstanceRepository,
};

use super::ApplicationError;

#[derive(Debug, Clone)]
pub struct ServerInstanceService {
    repository: ServerInstanceRepository,
}

impl ServerInstanceService {
    #[must_use]
    pub fn new(repository: ServerInstanceRepository) -> Self {
        Self { repository }
    }

    pub async fn get(&self, id: ServerInstanceId) -> Result<ServerInstance, ApplicationError> {
        self.repository
            .get(id)
            .await?
            .ok_or(ApplicationError::ServerInstanceNotFound)
    }

    pub async fn list_for_server(
        &self,
        server_id: ServerId,
    ) -> Result<Vec<ServerInstance>, ApplicationError> {
        self.repository
            .list_for_server(server_id)
            .await
            .map_err(Into::into)
    }
}
