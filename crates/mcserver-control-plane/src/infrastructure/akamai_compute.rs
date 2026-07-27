use std::{net::Ipv4Addr, time::Duration};

use reqwest::{Client, Response, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    agent::AgentRegistry,
    config::{AkamaiConfig, RemoteAgentConfig},
    domain::{
        Clock, ComputeInstance, ComputeProvider, ComputeTerminalResult, ServerInstance,
        SystemClock, UnixTimestampMillis,
    },
};

use super::{
    ComputeInstanceRepository, RepositoryError,
    akamai_bootstrap::{BootstrapError, build_bootstrap},
};

const MANAGED_TAG: &str = "mcserver-managed";
const MAX_API_ERROR_BODY_BYTES: usize = 64 * 1024;
const CLOUD_INIT_CAPABILITY: &str = "cloud-init";
const METADATA_CAPABILITY: &str = "Metadata";

struct InstanceProvisioningRequest<'a> {
    label: &'a str,
    region: &'a str,
    instance_type: &'a str,
    image: &'a str,
    instance: &'a ServerInstance,
    compute: &'a ComputeInstance,
    allow_create: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AkamaiOrphanReapSummary {
    pub instances_adopted: usize,
    pub instances_deleted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AkamaiPreflightSummary {
    pub managed_instances: usize,
}

#[derive(Clone)]
pub struct AkamaiComputeManager {
    repository: ComputeInstanceRepository,
    agents: AgentRegistry,
    client: AkamaiClient,
    config: AkamaiConfig,
    remote: RemoteAgentConfig,
    agent_command_timeout: Duration,
    clock: SystemClock,
}

impl AkamaiComputeManager {
    pub fn new(
        repository: ComputeInstanceRepository,
        agents: AgentRegistry,
        config: AkamaiConfig,
        remote: RemoteAgentConfig,
        agent_command_timeout: Duration,
    ) -> Result<Self, AkamaiComputeError> {
        let client = AkamaiClient::new(
            &config.api_token,
            &config.api_base_url,
            config.request_timeout,
        )?;
        Ok(Self {
            repository,
            agents,
            client,
            config,
            remote,
            agent_command_timeout,
            clock: SystemClock,
        })
    }

    pub async fn preflight(&self) -> Result<AkamaiPreflightSummary, AkamaiComputeError> {
        self.client
            .verify_bootstrap_compatibility(&self.config.image, &self.config.region)
            .await?;
        for instance_type in &self.config.allowed_instance_types {
            self.client.verify_instance_type(instance_type).await?;
        }
        self.client
            .verify_firewall(self.config.firewall_id)
            .await?;
        let managed_instances = self
            .client
            .list_managed_instances(&self.config.scope)
            .await?
            .len();
        if managed_instances > self.config.max_active_instances {
            return Err(AkamaiComputeError::ActiveInstanceLimitExceeded {
                active: managed_instances,
                maximum: self.config.max_active_instances,
            });
        }
        Ok(AkamaiPreflightSummary { managed_instances })
    }

    #[must_use]
    pub fn lifetime_exceeded(
        &self,
        compute: &ComputeInstance,
        now: UnixTimestampMillis,
    ) -> bool {
        if compute.provider != ComputeProvider::Akamai {
            return false;
        }
        let maximum_millis = i64::try_from(self.config.max_instance_lifetime.as_millis())
            .unwrap_or(i64::MAX);
        now.as_millis().saturating_sub(compute.created_at.as_millis()) >= maximum_millis
    }

    fn require_live_operations_enabled(&self) -> Result<(), AkamaiComputeError> {
        if self.config.mutations_enabled() {
            Ok(())
        } else {
            Err(AkamaiComputeError::LiveOperationsDisabled)
        }
    }

    fn validate_spec(
        &self,
        region: &str,
        instance_type: &str,
        image: &str,
        firewall_id: Option<u64>,
    ) -> Result<(), AkamaiComputeError> {
        if region != self.config.region {
            return Err(AkamaiComputeError::RegionNotAllowed {
                requested: region.to_owned(),
                allowed: self.config.region.clone(),
            });
        }
        if image != self.config.image {
            return Err(AkamaiComputeError::ImageNotAllowed {
                requested: image.to_owned(),
                allowed: self.config.image.clone(),
            });
        }
        if !self.config.allows_instance_type(instance_type) {
            return Err(AkamaiComputeError::InstanceTypeNotAllowed(
                instance_type.to_owned(),
            ));
        }
        if firewall_id != Some(self.config.firewall_id) {
            return Err(AkamaiComputeError::FirewallNotAllowed {
                requested: firewall_id,
                required: self.config.firewall_id,
            });
        }
        Ok(())
    }

    pub async fn reap_orphans(
        &self,
        active: &[ComputeInstance],
    ) -> Result<AkamaiOrphanReapSummary, AkamaiComputeError> {
        self.require_live_operations_enabled()?;
        let mut summary = AkamaiOrphanReapSummary::default();
        let managed = self
            .client
            .list_managed_instances(&self.config.scope)
            .await?;
        for provider in managed {
            let Some(compute_id) = provider
                .label()
                .strip_prefix("mcserver-")
                .and_then(|value| value.parse::<Uuid>().ok())
            else {
                warn!(
                    provider_instance_id = provider.id,
                    label = provider.label(),
                    "deleting managed Akamai instance with invalid ownership label"
                );
                self.client.delete_instance(provider.id).await?;
                summary.instances_deleted = summary.instances_deleted.saturating_add(1);
                continue;
            };
            let active_compute = active
                .iter()
                .find(|compute| compute.id.as_uuid() == compute_id);
            match active_compute {
                Some(compute)
                    if compute
                        .provider_instance_id
                        .as_deref()
                        .is_none_or(|value| value.parse::<u64>().ok() == Some(provider.id)) =>
                {
                    if compute.provider_instance_id.is_none() {
                        let now = self.clock.now()?;
                        self.repository
                            .record_provider_instance(
                                compute.id,
                                &provider.id.to_string(),
                                provider.public_ipv4().as_deref(),
                                now,
                            )
                            .await?;
                        summary.instances_adopted = summary.instances_adopted.saturating_add(1);
                        info!(compute_instance_id = %compute.id, provider_instance_id = provider.id, "adopted Akamai instance during startup recovery");
                    }
                }
                _ => {
                    self.client.delete_instance(provider.id).await?;
                    summary.instances_deleted = summary.instances_deleted.saturating_add(1);
                    info!(
                        provider_instance_id = provider.id,
                        label = provider.label(),
                        "deleted orphaned Akamai instance"
                    );
                }
            }
        }
        Ok(summary)
    }

    pub async fn ensure_for_instance(
        &self,
        instance: &ServerInstance,
        now: UnixTimestampMillis,
    ) -> Result<(ComputeInstance, bool), AkamaiComputeError> {
        let (region, instance_type, image, firewall_id) = match &instance.resolved_spec.compute {
            crate::domain::ComputeSpec::Akamai {
                region,
                instance_type,
                image,
                firewall_id,
            } => (region, instance_type, image, *firewall_id),
            crate::domain::ComputeSpec::Local => return Err(AkamaiComputeError::WrongProvider),
        };
        self.validate_spec(region, instance_type, image, firewall_id)?;
        let (compute, mut changed) =
            match self.repository.get_active_for_instance(instance.id).await? {
                Some(compute) if compute.provider == ComputeProvider::Akamai => (compute, false),
                Some(_) => return Err(AkamaiComputeError::WrongProvider),
                None => {
                    self.require_live_operations_enabled()?;
                    let connection_token = super::compute::new_connection_token();
                    let enrollment_token = super::compute::new_connection_token();
                    let compute = self
                        .repository
                        .create_for_instance(
                            instance.id,
                            ComputeProvider::Akamai,
                            &connection_token,
                            Some(&enrollment_token),
                            now,
                        )
                        .await?
                        .ok_or(AkamaiComputeError::CreateConflict)?;
                    (compute, true)
                }
            };

        let label = provider_label(compute.id.as_uuid());
        let provider = match compute.provider_instance_id.as_deref() {
            Some(provider_id) => {
                let provider_id = provider_id
                    .parse::<u64>()
                    .map_err(AkamaiComputeError::InvalidProviderId)?;
                match self.client.get_instance(provider_id).await? {
                    Some(provider) if provider.is_owned_by(&label, &self.scope_tag()) => {
                        self.verify_provider_configuration(&provider, region, instance_type, image)
                            .await?;
                        provider
                    }
                    Some(provider) => {
                        return Err(AkamaiComputeError::ProviderOwnershipMismatch {
                            provider_instance_id: provider.id,
                            expected_label: label,
                        });
                    }
                    None => {
                        warn!(
                            compute_instance_id = %compute.id,
                            provider_instance_id = provider_id,
                            "persisted Akamai instance no longer exists; recreating"
                        );
                        self.find_or_create_instance(InstanceProvisioningRequest {
                            label: &label,
                            region,
                            instance_type,
                            image,
                            instance,
                            compute: &compute,
                            allow_create: instance.data_prepared_at.is_none(),
                        })
                        .await?
                    }
                }
            }
            None => {
                self.find_or_create_instance(InstanceProvisioningRequest {
                    label: &label,
                    region,
                    instance_type,
                    image,
                    instance,
                    compute: &compute,
                    allow_create: instance.data_prepared_at.is_none(),
                })
                .await?
            }
        };

        let provider_id = provider.id.to_string();
        let public_ipv4 = provider.public_ipv4();
        if compute.provider_instance_id.as_deref() != Some(provider_id.as_str())
            || compute.public_ipv4.as_deref() != public_ipv4.as_deref()
        {
            let observed_at = self.clock.now()?;
            if !self
                .repository
                .record_provider_instance(
                    compute.id,
                    &provider_id,
                    public_ipv4.as_deref(),
                    observed_at,
                )
                .await?
            {
                return Err(AkamaiComputeError::MissingAfterUpdate);
            }
            changed = true;
        }
        let compute = self
            .repository
            .get(compute.id)
            .await?
            .ok_or(AkamaiComputeError::MissingAfterUpdate)?;
        Ok((compute, changed))
    }

    async fn find_or_create_instance(
        &self,
        request: InstanceProvisioningRequest<'_>,
    ) -> Result<ProviderInstance, AkamaiComputeError> {
        let InstanceProvisioningRequest {
            label,
            region,
            instance_type,
            image,
            instance,
            compute,
            allow_create,
        } = request;
        if let Some(existing) = self.client.find_instance_by_label(label).await? {
            if !existing.is_owned_by(label, &self.scope_tag()) {
                return Err(AkamaiComputeError::ProviderOwnershipMismatch {
                    provider_instance_id: existing.id,
                    expected_label: label.to_owned(),
                });
            }
            self.verify_provider_configuration(&existing, region, instance_type, image)
                .await?;
            return Ok(existing);
        }
        if !allow_create {
            return Err(AkamaiComputeError::ProviderInstanceLost(instance.id));
        }
        self.require_live_operations_enabled()?;

        self.client
            .verify_bootstrap_compatibility(image, region)
            .await?;
        self.client.verify_instance_type(instance_type).await?;
        self.client.verify_firewall(self.config.firewall_id).await?;
        let active = self
            .client
            .list_managed_instances(&self.config.scope)
            .await?
            .len();
        if active >= self.config.max_active_instances {
            return Err(AkamaiComputeError::ActiveInstanceLimitReached {
                active,
                maximum: self.config.max_active_instances,
            });
        }

        let bootstrap = build_bootstrap(
            &self.remote,
            &self.config.authorized_keys_file,
            &self.config.scope,
            instance,
            compute,
        )
        .await?;
        let request = CreateInstanceRequest {
            region: region.to_owned(),
            instance_type: instance_type.to_owned(),
            image: image.to_owned(),
            label: label.to_owned(),
            booted: true,
            authorized_keys: bootstrap.authorized_keys,
            tags: vec![MANAGED_TAG.to_owned(), self.scope_tag()],
            firewall_id: Some(self.config.firewall_id),
            metadata: MetadataRequest {
                user_data: bootstrap.user_data_base64,
            },
        };
        let provider = self.client.create_instance(&request).await?;
        self.verify_provider_configuration(&provider, region, instance_type, image)
            .await?;
        Ok(provider)
    }

    async fn verify_provider_configuration(
        &self,
        provider: &ProviderInstance,
        region: &str,
        instance_type: &str,
        image: &str,
    ) -> Result<(), AkamaiComputeError> {
        provider.verify_configuration(region, instance_type, image)?;
        self.client
            .verify_instance_firewall(provider.id, self.config.firewall_id)
            .await
    }

    fn scope_tag(&self) -> String {
        format!("mcserver-scope-{}", self.config.scope)
    }

    pub async fn delete(&self, compute: &ComputeInstance) -> Result<bool, AkamaiComputeError> {
        if compute.provider != ComputeProvider::Akamai {
            return Err(AkamaiComputeError::WrongProvider);
        }
        let now = self.clock.now()?;
        self.repository.request_shutdown(compute.id, now).await?;

        if self.agents.is_connected(compute.id).await {
            let result = self
                .agents
                .call::<_, mcserver_protocol::node_agent::ShutdownResult>(
                    compute.id,
                    mcserver_protocol::node_agent::method::NODE_SHUTDOWN,
                    &json!({}),
                    self.agent_command_timeout,
                )
                .await;
            if let Err(error) = result {
                warn!(compute_instance_id = %compute.id, %error, "remote node-agent shutdown request failed");
            }
        }

        let label = provider_label(compute.id.as_uuid());
        let provider_id = match compute.provider_instance_id.as_deref() {
            Some(value) => {
                let provider_id = value
                    .parse::<u64>()
                    .map_err(AkamaiComputeError::InvalidProviderId)?;
                match self.client.get_instance(provider_id).await? {
                    Some(provider) if provider.is_owned_by(&label, &self.scope_tag()) => {
                        Some(provider_id)
                    }
                    Some(provider) => {
                        return Err(AkamaiComputeError::ProviderOwnershipMismatch {
                            provider_instance_id: provider.id,
                            expected_label: label,
                        });
                    }
                    None => None,
                }
            }
            None => match self.client.find_instance_by_label(&label).await? {
                Some(provider) if provider.is_owned_by(&label, &self.scope_tag()) => {
                    Some(provider.id)
                }
                Some(provider) => {
                    return Err(AkamaiComputeError::ProviderOwnershipMismatch {
                        provider_instance_id: provider.id,
                        expected_label: label,
                    });
                }
                None => None,
            },
        };
        if let Some(provider_id) = provider_id {
            self.client.delete_instance(provider_id).await?;
        }
        let deleted_at = self.clock.now()?;
        let changed = self
            .repository
            .terminate(compute.id, ComputeTerminalResult::Deleted, None, deleted_at)
            .await?;
        if changed {
            info!(compute_instance_id = %compute.id, provider_instance_id = ?provider_id, "Akamai compute instance deleted");
        }
        Ok(changed)
    }
}

#[derive(Clone)]
struct AkamaiClient {
    http: Client,
    base_url: String,
}

impl AkamaiClient {
    fn new(token: &str, base_url: &str, timeout: Duration) -> Result<Self, AkamaiComputeError> {
        let mut headers = header::HeaderMap::new();
        let authorization = header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(AkamaiComputeError::InvalidAuthorizationHeader)?;
        headers.insert(header::AUTHORIZATION, authorization);
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static(concat!(
                "mcserver-control-plane/",
                env!("CARGO_PKG_VERSION")
            )),
        );
        let http = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }

    async fn list_managed_instances(
        &self,
        scope: &str,
    ) -> Result<Vec<ProviderInstance>, AkamaiComputeError> {
        let filter = serde_json::to_string(&json!({
            "tags": { "+contains": MANAGED_TAG }
        }))?;
        let mut page = 1_u64;
        let mut result = Vec::new();
        loop {
            let response = self
                .http
                .get(format!(
                    "{}/linode/instances?page={page}&page_size=500",
                    self.base_url
                ))
                .header("X-Filter", &filter)
                .send()
                .await?;
            let body: ListInstancesResponse = checked_response(response).await?.json().await?;
            result.extend(body.data.into_iter().filter(|instance| {
                instance.tags.iter().any(|tag| tag == MANAGED_TAG)
                    && instance
                        .tags
                        .iter()
                        .any(|tag| tag == &format!("mcserver-scope-{scope}"))
            }));
            if page >= body.pages.max(1) {
                break;
            }
            page = page.saturating_add(1);
        }
        Ok(result)
    }

    async fn find_instance_by_label(
        &self,
        label: &str,
    ) -> Result<Option<ProviderInstance>, AkamaiComputeError> {
        let filter = serde_json::to_string(&json!({ "label": label }))?;
        let mut page = 1_u64;
        let mut instances = Vec::new();
        loop {
            let response = self
                .http
                .get(format!(
                    "{}/linode/instances?page={page}&page_size=500",
                    self.base_url
                ))
                .header("X-Filter", &filter)
                .send()
                .await?;
            let body: ListInstancesResponse = checked_response(response).await?.json().await?;
            instances.extend(
                body.data
                    .into_iter()
                    .filter(|instance| instance.label == label),
            );
            if page >= body.pages.max(1) {
                break;
            }
            page = page.saturating_add(1);
        }
        match instances.as_slice() {
            [] => Ok(None),
            [instance] => Ok(Some(instance.clone())),
            _ => Err(AkamaiComputeError::DuplicateProviderLabel(label.to_owned())),
        }
    }

    async fn create_instance(
        &self,
        request: &CreateInstanceRequest,
    ) -> Result<ProviderInstance, AkamaiComputeError> {
        let response = self
            .http
            .post(format!("{}/linode/instances", self.base_url))
            .json(request)
            .send()
            .await?;
        checked_response(response)
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    async fn verify_bootstrap_compatibility(
        &self,
        image_id: &str,
        region_id: &str,
    ) -> Result<(), AkamaiComputeError> {
        let image = self.get_image(image_id).await?;
        if image.id != image_id {
            return Err(AkamaiComputeError::ImageIdentityMismatch {
                expected: image_id.to_owned(),
                observed: image.id,
            });
        }
        if image.status != "available" {
            return Err(AkamaiComputeError::ImageUnavailable {
                image: image_id.to_owned(),
                status: image.status,
            });
        }
        if image.deprecated {
            return Err(AkamaiComputeError::ImageDeprecated(image_id.to_owned()));
        }
        if !image
            .capabilities
            .iter()
            .any(|capability| capability == CLOUD_INIT_CAPABILITY)
        {
            return Err(AkamaiComputeError::ImageMissingCloudInit(
                image_id.to_owned(),
            ));
        }

        let region = self.get_region(region_id).await?;
        if region.id != region_id {
            return Err(AkamaiComputeError::RegionIdentityMismatch {
                expected: region_id.to_owned(),
                observed: region.id,
            });
        }
        if !region
            .capabilities
            .iter()
            .any(|capability| capability == METADATA_CAPABILITY)
        {
            return Err(AkamaiComputeError::RegionMissingMetadata(
                region_id.to_owned(),
            ));
        }
        Ok(())
    }

    async fn verify_instance_type(&self, type_id: &str) -> Result<(), AkamaiComputeError> {
        let response = self
            .http
            .get(format!("{}/linode/types/{type_id}", self.base_url))
            .send()
            .await?;
        let instance_type: ProviderType = checked_response(response).await?.json().await?;
        if instance_type.id != type_id {
            return Err(AkamaiComputeError::InstanceTypeIdentityMismatch {
                expected: type_id.to_owned(),
                observed: instance_type.id,
            });
        }
        Ok(())
    }

    async fn verify_firewall(&self, firewall_id: u64) -> Result<(), AkamaiComputeError> {
        let response = self
            .http
            .get(format!(
                "{}/networking/firewalls/{firewall_id}",
                self.base_url
            ))
            .send()
            .await?;
        let firewall: ProviderFirewall = checked_response(response).await?.json().await?;
        if firewall.id != firewall_id {
            return Err(AkamaiComputeError::FirewallIdentityMismatch {
                expected: firewall_id,
                observed: firewall.id,
            });
        }
        if firewall.status != "enabled" {
            return Err(AkamaiComputeError::FirewallDisabled {
                firewall_id,
                status: firewall.status,
            });
        }
        Ok(())
    }

    async fn verify_instance_firewall(
        &self,
        provider_instance_id: u64,
        firewall_id: u64,
    ) -> Result<(), AkamaiComputeError> {
        let response = self
            .http
            .get(format!(
                "{}/linode/instances/{provider_instance_id}/firewalls?page=1&page_size=500",
                self.base_url
            ))
            .send()
            .await?;
        let firewalls: ListFirewallsResponse = checked_response(response).await?.json().await?;
        let Some(firewall) = firewalls.data.into_iter().find(|value| value.id == firewall_id) else {
            return Err(AkamaiComputeError::RequiredFirewallMissing {
                provider_instance_id,
                firewall_id,
            });
        };
        if firewall.status != "enabled" {
            return Err(AkamaiComputeError::FirewallDisabled {
                firewall_id,
                status: firewall.status,
            });
        }
        Ok(())
    }

    async fn get_image(&self, image_id: &str) -> Result<ProviderImage, AkamaiComputeError> {
        let response = self
            .http
            .get(format!("{}/images/{image_id}", self.base_url))
            .send()
            .await?;
        checked_response(response)
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    async fn get_region(&self, region_id: &str) -> Result<ProviderRegion, AkamaiComputeError> {
        let response = self
            .http
            .get(format!("{}/regions/{region_id}", self.base_url))
            .send()
            .await?;
        checked_response(response)
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    async fn get_instance(&self, id: u64) -> Result<Option<ProviderInstance>, AkamaiComputeError> {
        let response = self
            .http
            .get(format!("{}/linode/instances/{id}", self.base_url))
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        checked_response(response)
            .await?
            .json()
            .await
            .map(Some)
            .map_err(Into::into)
    }

    async fn delete_instance(&self, id: u64) -> Result<(), AkamaiComputeError> {
        let response = self
            .http
            .delete(format!("{}/linode/instances/{id}", self.base_url))
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        checked_response(response).await?;
        Ok(())
    }
}

async fn checked_response(mut response: Response) -> Result<Response, AkamaiComputeError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retry_after = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs);
    let mut bounded = Vec::new();
    while bounded.len() < MAX_API_ERROR_BODY_BYTES {
        let Some(chunk) = response.chunk().await? else {
            break;
        };
        let remaining = MAX_API_ERROR_BODY_BYTES.saturating_sub(bounded.len());
        bounded.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    let errors = serde_json::from_slice::<ApiErrors>(&bounded)
        .map(|body| {
            body.errors
                .into_iter()
                .map(|error| match error.field {
                    Some(field) => format!("{field}: {}", error.reason),
                    None => error.reason,
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_else(|_| String::from_utf8_lossy(&bounded).into_owned());
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(AkamaiComputeError::RateLimited {
            retry_after,
            message: errors,
        });
    }
    Err(AkamaiComputeError::Api {
        status: status.as_u16(),
        message: errors,
    })
}

fn provider_label(compute_id: Uuid) -> String {
    format!("mcserver-{compute_id}")
}

#[derive(Debug, Serialize)]
struct CreateInstanceRequest {
    region: String,
    #[serde(rename = "type")]
    instance_type: String,
    image: String,
    label: String,
    booted: bool,
    authorized_keys: Vec<String>,
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    firewall_id: Option<u64>,
    metadata: MetadataRequest,
}

#[derive(Debug, Serialize)]
struct MetadataRequest {
    user_data: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderInstance {
    id: u64,
    label: String,
    region: String,
    #[serde(rename = "type")]
    instance_type: String,
    image: String,
    #[serde(default)]
    ipv4: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProviderImage {
    id: String,
    status: String,
    #[serde(default)]
    deprecated: bool,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProviderRegion {
    id: String,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProviderType {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ProviderFirewall {
    id: u64,
    status: String,
}

#[derive(Debug, Deserialize)]
struct ListFirewallsResponse {
    data: Vec<ProviderFirewall>,
}

impl ProviderInstance {
    fn label(&self) -> &str {
        &self.label
    }

    fn is_owned_by(&self, expected_label: &str, scope_tag: &str) -> bool {
        self.label == expected_label
            && self.tags.iter().any(|tag| tag == MANAGED_TAG)
            && self.tags.iter().any(|tag| tag == scope_tag)
    }

    fn verify_configuration(
        &self,
        region: &str,
        instance_type: &str,
        image: &str,
    ) -> Result<(), AkamaiComputeError> {
        if self.region != region || self.instance_type != instance_type || self.image != image {
            return Err(AkamaiComputeError::ProviderConfigurationMismatch {
                provider_instance_id: self.id,
                expected_region: region.to_owned(),
                observed_region: self.region.clone(),
                expected_type: instance_type.to_owned(),
                observed_type: self.instance_type.clone(),
                expected_image: image.to_owned(),
                observed_image: self.image.clone(),
            });
        }
        Ok(())
    }

    fn public_ipv4(&self) -> Option<String> {
        self.ipv4.iter().find_map(|value| {
            value
                .parse::<Ipv4Addr>()
                .ok()
                .filter(|address| !address.is_private() && !address.is_loopback())
                .map(|address| address.to_string())
        })
    }
}

#[derive(Debug, Deserialize)]
struct ListInstancesResponse {
    data: Vec<ProviderInstance>,
    #[serde(default = "default_page_count")]
    pages: u64,
}

const fn default_page_count() -> u64 {
    1
}

#[derive(Debug, Deserialize)]
struct ApiErrors {
    errors: Vec<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    field: Option<String>,
    reason: String,
}

impl AkamaiComputeError {
    #[must_use]
    pub const fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum AkamaiComputeError {
    #[error("Akamai provider used for a non-Akamai compute specification")]
    WrongProvider,
    #[error("billable Akamai creation and startup orphan reaping are disabled; set MCSERVER_AKAMAI_LIVE_ENABLED=true after preflight")]
    LiveOperationsDisabled,
    #[error("Akamai region {requested} is not allowed; configured region is {allowed}")]
    RegionNotAllowed { requested: String, allowed: String },
    #[error("Akamai image {requested} is not allowed; configured image is {allowed}")]
    ImageNotAllowed { requested: String, allowed: String },
    #[error("Akamai instance type is not allowed: {0}")]
    InstanceTypeNotAllowed(String),
    #[error("Akamai firewall {requested:?} is not allowed; required firewall is {required}")]
    FirewallNotAllowed { requested: Option<u64>, required: u64 },
    #[error("Akamai managed instance limit is already reached: active={active}, maximum={maximum}")]
    ActiveInstanceLimitReached { active: usize, maximum: usize },
    #[error("Akamai managed instance count exceeds the configured limit: active={active}, maximum={maximum}")]
    ActiveInstanceLimitExceeded { active: usize, maximum: usize },
    #[error("active compute instance was created concurrently")]
    CreateConflict,
    #[error("compute instance disappeared after provider update")]
    MissingAfterUpdate,
    #[error("Akamai VM for server instance {0} disappeared after writable data was prepared")]
    ProviderInstanceLost(crate::domain::ServerInstanceId),
    #[error("persisted Akamai provider id is invalid")]
    InvalidProviderId(#[source] std::num::ParseIntError),
    #[error("multiple Akamai instances use provider label {0}")]
    DuplicateProviderLabel(String),
    #[error(
        "Akamai instance {provider_instance_id} does not have expected managed ownership label {expected_label}"
    )]
    ProviderOwnershipMismatch {
        provider_instance_id: u64,
        expected_label: String,
    },
    #[error(
        "Akamai instance {provider_instance_id} configuration differs from the resolved specification: region {observed_region} (expected {expected_region}), type {observed_type} (expected {expected_type}), image {observed_image} (expected {expected_image})"
    )]
    ProviderConfigurationMismatch {
        provider_instance_id: u64,
        expected_region: String,
        observed_region: String,
        expected_type: String,
        observed_type: String,
        expected_image: String,
        observed_image: String,
    },
    #[error("Akamai instance type response identity mismatch: expected {expected}, observed {observed}")]
    InstanceTypeIdentityMismatch { expected: String, observed: String },
    #[error("Akamai firewall response identity mismatch: expected {expected}, observed {observed}")]
    FirewallIdentityMismatch { expected: u64, observed: u64 },
    #[error("Akamai firewall {firewall_id} is not enabled; status={status}")]
    FirewallDisabled { firewall_id: u64, status: String },
    #[error("Akamai instance {provider_instance_id} is not attached to required firewall {firewall_id}")]
    RequiredFirewallMissing {
        provider_instance_id: u64,
        firewall_id: u64,
    },
    #[error("Akamai image response identity mismatch: expected {expected}, observed {observed}")]
    ImageIdentityMismatch { expected: String, observed: String },
    #[error("Akamai image {image} is not available; status={status}")]
    ImageUnavailable { image: String, status: String },
    #[error("Akamai image {0} is deprecated")]
    ImageDeprecated(String),
    #[error("Akamai image {0} does not advertise cloud-init support")]
    ImageMissingCloudInit(String),
    #[error("Akamai region response identity mismatch: expected {expected}, observed {observed}")]
    RegionIdentityMismatch { expected: String, observed: String },
    #[error("Akamai region {0} does not advertise Metadata support")]
    RegionMissingMetadata(String),
    #[error("Akamai API request failed")]
    Http(#[from] reqwest::Error),
    #[error("Akamai authorization header is invalid")]
    InvalidAuthorizationHeader(#[source] reqwest::header::InvalidHeaderValue),
    #[error("Akamai API returned HTTP {status}: {message}")]
    Api { status: u16, message: String },
    #[error("Akamai API rate limit reached; retry_after={retry_after:?}: {message}")]
    RateLimited {
        retry_after: Option<Duration>,
        message: String,
    },
    #[error("Akamai API payload serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("Akamai bootstrap generation failed")]
    Bootstrap(#[from] BootstrapError),
    #[error("Akamai compute persistence failed")]
    Repository(#[from] RepositoryError),
    #[error("Akamai compute timestamp failed")]
    Timestamp(#[from] crate::domain::TimestampError),
}
#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, time::Duration};

    use serde_json::json;
    use uuid::Uuid;

    use super::{
        AkamaiComputeError, CreateInstanceRequest, MetadataRequest, ProviderImage,
        ProviderInstance, ProviderRegion, provider_label,
    };

    #[test]
    fn provider_label_is_valid_and_deterministic() {
        let id = Uuid::from_u128(1);
        assert_eq!(
            provider_label(id),
            "mcserver-00000000-0000-0000-0000-000000000001"
        );
    }

    #[test]
    fn create_request_uses_linode_type_field() -> Result<(), serde_json::Error> {
        let request = CreateInstanceRequest {
            region: "jp-tyo-3".to_owned(),
            instance_type: "g6-nanode-1".to_owned(),
            image: "linode/debian13".to_owned(),
            label: "mcserver-00000000-0000-0000-0000-000000000001".to_owned(),
            booted: true,
            authorized_keys: vec!["ssh-ed25519 test".to_owned()],
            tags: vec!["mcserver-managed".to_owned()],
            firewall_id: Some(123),
            metadata: MetadataRequest {
                user_data: "dGVzdA==".to_owned(),
            },
        };
        let value = serde_json::to_value(request)?;

        assert_eq!(value["type"], json!("g6-nanode-1"));
        assert!(value.get("instance_type").is_none());
        assert_eq!(value["metadata"]["user_data"], json!("dGVzdA=="));
        Ok(())
    }

    #[test]
    fn public_ipv4_skips_private_addresses() {
        let instance = ProviderInstance {
            id: 1,
            label: "test".to_owned(),
            region: "jp-tyo-3".to_owned(),
            instance_type: "g6-nanode-1".to_owned(),
            image: "linode/debian13".to_owned(),
            ipv4: vec!["192.168.1.1".to_owned(), "203.0.113.10".to_owned()],
            tags: Vec::new(),
        };

        assert_eq!(instance.public_ipv4(), Some("203.0.113.10".to_owned()));
        assert_eq!(
            instance
                .public_ipv4()
                .and_then(|value| value.parse::<Ipv4Addr>().ok()),
            Some(Ipv4Addr::new(203, 0, 113, 10))
        );
    }

    #[test]
    fn provider_ownership_requires_label_and_both_tags() {
        let instance = ProviderInstance {
            id: 1,
            label: "mcserver-owned".to_owned(),
            region: "jp-tyo-3".to_owned(),
            instance_type: "g6-nanode-1".to_owned(),
            image: "linode/debian13".to_owned(),
            ipv4: Vec::new(),
            tags: vec![
                "mcserver-managed".to_owned(),
                "mcserver-scope-production".to_owned(),
            ],
        };
        assert!(instance.is_owned_by("mcserver-owned", "mcserver-scope-production"));
        assert!(!instance.is_owned_by("mcserver-other", "mcserver-scope-production"));
        assert!(!instance.is_owned_by("mcserver-owned", "mcserver-scope-staging"));
    }

    #[test]
    fn exposes_provider_retry_after() {
        let error = AkamaiComputeError::RateLimited {
            retry_after: Some(Duration::from_secs(17)),
            message: "rate limited".to_owned(),
        };
        assert_eq!(error.retry_after(), Some(Duration::from_secs(17)));
    }

    #[test]
    fn production_bootstrap_capabilities_are_spelled_exactly() {
        let image = ProviderImage {
            id: "linode/debian13".to_owned(),
            status: "available".to_owned(),
            deprecated: false,
            capabilities: vec!["cloud-init".to_owned()],
        };
        let region = ProviderRegion {
            id: "jp-tyo-3".to_owned(),
            capabilities: vec!["Metadata".to_owned()],
        };

        assert!(image.capabilities.iter().any(|value| value == "cloud-init"));
        assert!(region.capabilities.iter().any(|value| value == "Metadata"));
    }
}
