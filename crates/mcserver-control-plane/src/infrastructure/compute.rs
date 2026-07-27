use thiserror::Error;
use uuid::Uuid;

use crate::domain::{ComputeInstance, ComputeProvider, ServerInstance, UnixTimestampMillis};

use super::{AkamaiComputeError, AkamaiComputeManager, LocalComputeError, LocalComputeManager};

#[derive(Clone)]
pub struct ComputeManager {
    local: LocalComputeManager,
    akamai: Option<AkamaiComputeManager>,
}

impl ComputeManager {
    #[must_use]
    pub fn new(local: LocalComputeManager, akamai: Option<AkamaiComputeManager>) -> Self {
        Self { local, akamai }
    }

    pub async fn ensure_for_instance(
        &self,
        instance: &ServerInstance,
        now: UnixTimestampMillis,
    ) -> Result<(ComputeInstance, bool), ComputeError> {
        match &instance.resolved_spec.compute {
            crate::domain::ComputeSpec::Local => self
                .local
                .ensure_for_instance(instance, now)
                .await
                .map_err(Into::into),
            crate::domain::ComputeSpec::Akamai { .. } => self
                .akamai
                .as_ref()
                .ok_or(ComputeError::AkamaiNotConfigured)?
                .ensure_for_instance(instance, now)
                .await
                .map_err(Into::into),
        }
    }

    #[must_use]
    pub fn lifetime_exceeded(&self, compute: &ComputeInstance, now: UnixTimestampMillis) -> bool {
        match compute.provider {
            ComputeProvider::LocalProcess => false,
            ComputeProvider::Akamai => self
                .akamai
                .as_ref()
                .is_some_and(|manager| manager.lifetime_exceeded(compute, now)),
        }
    }

    pub async fn delete(&self, compute: &ComputeInstance) -> Result<bool, ComputeError> {
        match compute.provider {
            ComputeProvider::LocalProcess => self.local.delete(compute).await.map_err(Into::into),
            ComputeProvider::Akamai => self
                .akamai
                .as_ref()
                .ok_or(ComputeError::AkamaiNotConfigured)?
                .delete(compute)
                .await
                .map_err(Into::into),
        }
    }
}

pub(super) fn new_connection_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

impl ComputeError {
    #[must_use]
    pub const fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Self::Akamai(error) => error.retry_after(),
            Self::Local(_) | Self::AkamaiNotConfigured => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum ComputeError {
    #[error("local compute operation failed: {0}")]
    Local(#[from] LocalComputeError),
    #[error("Akamai compute operation failed: {0}")]
    Akamai(#[from] AkamaiComputeError),
    #[error("Akamai compute was requested but the provider is not configured")]
    AkamaiNotConfigured,
}

#[cfg(test)]
mod tests {
    use super::new_connection_token;

    #[test]
    fn connection_tokens_are_high_entropy_hex_values() {
        let first = new_connection_token();
        let second = new_connection_token();
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }
}
