use std::{
    collections::{BTreeMap, BTreeSet},
    env, fmt, fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};

use thiserror::Error;

const DEFAULT_SOCKET_PATH: &str = "/run/mcserver/control-plane.sock";
const DEFAULT_DATABASE_URL: &str = "sqlite:///var/lib/mcserver/control-plane.db?mode=rwc";
const DEFAULT_SOCKET_MODE: u32 = 0o660;
const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;
const DEFAULT_RECONCILE_INTERVAL_SECONDS: u64 = 30;
const DEFAULT_RECONCILE_RETRY_SECONDS: u64 = 5;
const DEFAULT_SHUTDOWN_TIMEOUT_SECONDS: u64 = 15;
const DEFAULT_AGENT_LISTEN_ADDRESS: &str = "127.0.0.1:39001";
const DEFAULT_AGENT_COMMAND_TIMEOUT_SECONDS: u64 = 900;
const DEFAULT_NODE_AGENT_BINARY: &str = "mcserver-node-agent";
const DEFAULT_NODE_AGENT_ROOT: &str = "/var/lib/mcserver/local-agents";
const DEFAULT_LOCAL_PROCESS_STOP_TIMEOUT_SECONDS: u64 = 10;
const DEFAULT_LOCAL_CONTROL_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_PODMAN_BINARY: &str = "podman";
const DEFAULT_LOCAL_SCOPE: &str = "default";
const DEFAULT_AKAMAI_SCOPE: &str = "default";
const DEFAULT_AKAMAI_API_BASE_URL: &str = "https://api.linode.com/v4";
const DEFAULT_AKAMAI_REQUEST_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_AKAMAI_REAP_ORPHANS_ON_START: bool = false;
const DEFAULT_AKAMAI_LIVE_ENABLED: bool = false;
const DEFAULT_AKAMAI_ALLOWED_REGIONS: &str = "jp-tyo-3";
const DEFAULT_AKAMAI_ALLOWED_IMAGES: &str = "linode/debian13";
const DEFAULT_AKAMAI_ALLOWED_INSTANCE_TYPES: &str = "g6-nanode-1";
const DEFAULT_AKAMAI_MAX_ACTIVE_INSTANCES: usize = 1;
const DEFAULT_AKAMAI_MAX_INSTANCE_LIFETIME_SECONDS: u64 = 12 * 60 * 60;
const DEFAULT_CLOUDFLARE_API_BASE_URL: &str = "https://api.cloudflare.com/client/v4";
const DEFAULT_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS: u64 = 13 * 60 * 60;
const MAX_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
const DEFAULT_AGENT_CERTIFICATE_WORK_DIRECTORY: &str = "/run/mcserver/agent-pki";
const DEFAULT_AGENT_CERTIFICATE_VALIDITY_SECONDS: u64 = 2 * 24 * 60 * 60;
const AGENT_CERTIFICATE_SHUTDOWN_BUFFER_SECONDS: u64 = 60 * 60;
const DEFAULT_OPENSSL_BINARY: &str = "openssl";
const MAX_SCOPE_CHARS: usize = 128;
const MAX_AKAMAI_SCOPE_CHARS: usize = 35;
const MAX_ADDRESS_CHARS: usize = 512;
const MAX_URL_CHARS: usize = 4096;
const MAX_SERVER_NAME_CHARS: usize = 253;
const MAX_SECRET_CHARS: usize = 4096;
const MAX_RUNTIME_ENVIRONMENT_BYTES: usize = 64 * 1024;
const MAX_RUNTIME_ENVIRONMENT_ENTRIES: usize = 64;
const MAX_RUNTIME_ENVIRONMENT_VALUE_CHARS: usize = 16 * 1024;
const REQUIRED_REMOTE_RUNTIME_ENVIRONMENT_KEYS: [&str; 1] = ["AWS_DEFAULT_REGION"];
const SHA256_HEX_CHARS: usize = 64;
const DEFAULT_REAP_ORPHANS_ON_START: bool = true;

#[derive(Debug, Clone)]
pub struct Config {
    pub socket_path: PathBuf,
    pub database_url: String,
    pub socket_mode: u32,
    pub max_frame_bytes: usize,
    pub reconcile_interval: Duration,
    pub reconcile_retry: Duration,
    pub shutdown_timeout: Duration,
    pub agent_listen_address: SocketAddr,
    pub agent_command_timeout: Duration,
    pub node_agent_binary: PathBuf,
    pub node_agent_root: PathBuf,
    pub podman_binary: PathBuf,
    pub local_scope: String,
    pub reap_orphans_on_start: bool,
    pub local_control_timeout: Duration,
    pub local_process_stop_timeout: Duration,
    pub remote_agent: Option<RemoteAgentConfig>,
    pub akamai: Option<AkamaiConfig>,
    pub r2: Option<R2Config>,
}

#[derive(Debug, Clone)]
pub struct RemoteAgentConfig {
    pub listen_address: SocketAddr,
    pub public_address: String,
    pub tls_server_name: String,
    pub tls_certificate: PathBuf,
    pub tls_private_key: PathBuf,
    pub tls_ca_certificate: PathBuf,
    pub client_ca_certificate: PathBuf,
    pub client_ca_private_key: PathBuf,
    pub certificate_work_directory: PathBuf,
    pub certificate_validity: Duration,
    pub openssl_binary: PathBuf,
    pub trust_domain: String,
    pub node_agent_download_url: String,
    pub node_agent_sha256: String,
    pub max_frame_bytes: usize,
    pub node_operation_timeout: Duration,
}

#[derive(Clone)]
pub struct AkamaiConfig {
    pub api_token: String,
    pub api_base_url: String,
    pub authorized_keys_file: PathBuf,
    pub scope: String,
    pub request_timeout: Duration,
    pub reap_orphans_on_start: bool,
    pub live_enabled: bool,
    pub allowed_regions: BTreeSet<String>,
    pub allowed_images: BTreeSet<String>,
    pub allowed_instance_types: BTreeSet<String>,
    pub allowed_firewall_ids: BTreeSet<u64>,
    pub max_active_instances: usize,
    pub max_instance_lifetime: Duration,
    pub loopback_api: bool,
}

#[derive(Clone)]
pub struct R2Config {
    pub api_token: String,
    pub api_base_url: String,
    pub account_id: String,
    pub parent_access_key_id: String,
    pub bucket: String,
    pub temporary_credential_ttl: Duration,
    pub runtime_environment_file: PathBuf,
    pub runtime_environment: BTreeMap<String, String>,
    pub request_timeout: Duration,
    pub loopback_api: bool,
}

impl fmt::Debug for AkamaiConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AkamaiConfig")
            .field("api_token", &"[redacted]")
            .field("api_base_url", &self.api_base_url)
            .field("authorized_keys_file", &self.authorized_keys_file)
            .field("scope", &self.scope)
            .field("request_timeout", &self.request_timeout)
            .field("reap_orphans_on_start", &self.reap_orphans_on_start)
            .field("live_enabled", &self.live_enabled)
            .field("allowed_regions", &self.allowed_regions)
            .field("allowed_images", &self.allowed_images)
            .field("allowed_instance_types", &self.allowed_instance_types)
            .field("allowed_firewall_ids", &self.allowed_firewall_ids)
            .field("max_active_instances", &self.max_active_instances)
            .field("max_instance_lifetime", &self.max_instance_lifetime)
            .field("loopback_api", &self.loopback_api)
            .finish()
    }
}

impl fmt::Debug for R2Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("R2Config")
            .field("api_token", &"[redacted]")
            .field("api_base_url", &self.api_base_url)
            .field("account_id", &self.account_id)
            .field("parent_access_key_id", &self.parent_access_key_id)
            .field("bucket", &self.bucket)
            .field("temporary_credential_ttl", &self.temporary_credential_ttl)
            .field("runtime_environment_file", &self.runtime_environment_file)
            .field(
                "runtime_environment",
                &format_args!("[{} redacted values]", self.runtime_environment.len()),
            )
            .field("request_timeout", &self.request_timeout)
            .field("loopback_api", &self.loopback_api)
            .finish()
    }
}

impl AkamaiConfig {
    #[must_use]
    pub const fn mutations_enabled(&self) -> bool {
        self.live_enabled || self.loopback_api
    }

    #[must_use]
    pub fn allows_instance_type(&self, value: &str) -> bool {
        self.allowed_instance_types.contains(value)
    }

    #[must_use]
    pub fn allows_region(&self, value: &str) -> bool {
        self.allowed_regions.contains(value)
    }

    #[must_use]
    pub fn allows_image(&self, value: &str) -> bool {
        self.allowed_images.contains(value)
    }

    #[must_use]
    pub fn allows_firewall(&self, value: u64) -> bool {
        self.allowed_firewall_ids.contains(&value)
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let socket_path = env::var_os("MCSERVER_CONTROL_PLANE_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH));
        let database_url =
            optional_string("MCSERVER_CONTROL_PLANE_DATABASE_URL", DEFAULT_DATABASE_URL)?;
        let socket_mode =
            parse_socket_mode(optional_string_value("MCSERVER_CONTROL_PLANE_SOCKET_MODE")?)?;
        let max_frame_bytes = parse_positive_usize(
            "MCSERVER_CONTROL_PLANE_MAX_FRAME_BYTES",
            DEFAULT_MAX_FRAME_BYTES,
        )?;
        let reconcile_interval = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_RECONCILE_INTERVAL_SECONDS",
            DEFAULT_RECONCILE_INTERVAL_SECONDS,
        )?;
        let reconcile_retry = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_RECONCILE_RETRY_SECONDS",
            DEFAULT_RECONCILE_RETRY_SECONDS,
        )?;
        let shutdown_timeout = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_SHUTDOWN_TIMEOUT_SECONDS",
            DEFAULT_SHUTDOWN_TIMEOUT_SECONDS,
        )?;
        let agent_listen_address: SocketAddr = optional_string(
            "MCSERVER_CONTROL_PLANE_AGENT_LISTEN_ADDRESS",
            DEFAULT_AGENT_LISTEN_ADDRESS,
        )?
        .parse()
        .map_err(ConfigError::InvalidSocketAddress)?;
        if !agent_listen_address.ip().is_loopback() {
            return Err(ConfigError::AgentAddressMustBeLoopback(
                agent_listen_address,
            ));
        }
        if agent_listen_address.port() == 0 {
            return Err(ConfigError::ZeroAgentPort);
        }
        let agent_command_timeout = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_AGENT_COMMAND_TIMEOUT_SECONDS",
            DEFAULT_AGENT_COMMAND_TIMEOUT_SECONDS,
        )?;
        let node_agent_binary = env::var_os("MCSERVER_CONTROL_PLANE_NODE_AGENT_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_NODE_AGENT_BINARY));
        let node_agent_root = env::var_os("MCSERVER_CONTROL_PLANE_NODE_AGENT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_NODE_AGENT_ROOT));
        let podman_binary = env::var_os("MCSERVER_CONTROL_PLANE_PODMAN_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PODMAN_BINARY));
        let local_scope =
            optional_scope("MCSERVER_CONTROL_PLANE_LOCAL_SCOPE", DEFAULT_LOCAL_SCOPE)?;
        let reap_orphans_on_start = parse_bool(
            "MCSERVER_CONTROL_PLANE_REAP_ORPHANS_ON_START",
            DEFAULT_REAP_ORPHANS_ON_START,
        )?;
        let local_control_timeout = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_LOCAL_CONTROL_TIMEOUT_SECONDS",
            DEFAULT_LOCAL_CONTROL_TIMEOUT_SECONDS,
        )?;
        let local_process_stop_timeout = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_LOCAL_PROCESS_STOP_TIMEOUT_SECONDS",
            DEFAULT_LOCAL_PROCESS_STOP_TIMEOUT_SECONDS,
        )?;
        let remote_agent = remote_agent_config(max_frame_bytes, agent_command_timeout)?;
        let akamai = akamai_config(remote_agent.as_ref())?;
        let r2 = r2_config(akamai.as_ref(), agent_command_timeout)?;

        Ok(Self {
            socket_path,
            database_url,
            socket_mode,
            max_frame_bytes,
            reconcile_interval,
            reconcile_retry,
            shutdown_timeout,
            agent_listen_address,
            agent_command_timeout,
            node_agent_binary,
            node_agent_root,
            podman_binary,
            local_scope,
            reap_orphans_on_start,
            local_control_timeout,
            local_process_stop_timeout,
            remote_agent,
            akamai,
            r2,
        })
    }
}

pub(crate) fn node_operation_timeout(agent_call_timeout: Duration) -> Duration {
    agent_call_timeout
        .checked_sub(Duration::from_secs(5))
        .filter(|duration| !duration.is_zero())
        .unwrap_or(Duration::from_secs(1))
}

fn remote_agent_config(
    max_frame_bytes: usize,
    agent_command_timeout: Duration,
) -> Result<Option<RemoteAgentConfig>, ConfigError> {
    let Some(listen_value) =
        optional_string_value("MCSERVER_CONTROL_PLANE_REMOTE_AGENT_LISTEN_ADDRESS")?
    else {
        reject_partial_remote_config()?;
        return Ok(None);
    };
    let listen_address = listen_value
        .parse::<SocketAddr>()
        .map_err(ConfigError::InvalidRemoteSocketAddress)?;
    if listen_address.port() == 0 {
        return Err(ConfigError::ZeroRemoteAgentPort);
    }

    let public_address = required_non_blank(
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_PUBLIC_ADDRESS",
        MAX_ADDRESS_CHARS,
    )?;
    validate_host_port(&public_address)?;
    let tls_server_name = required_non_blank(
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_SERVER_NAME",
        MAX_SERVER_NAME_CHARS,
    )?;
    validate_dns_name(&tls_server_name)?;
    let tls_certificate = required_path("MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CERTIFICATE")?;
    let tls_private_key = required_path("MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_PRIVATE_KEY")?;
    let tls_ca_certificate =
        required_path("MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CA_CERTIFICATE")?;
    let client_ca_certificate =
        required_path("MCSERVER_CONTROL_PLANE_AGENT_CLIENT_CA_CERTIFICATE")?;
    let client_ca_private_key =
        required_path("MCSERVER_CONTROL_PLANE_AGENT_CLIENT_CA_PRIVATE_KEY")?;
    let certificate_work_directory =
        env::var_os("MCSERVER_CONTROL_PLANE_AGENT_CERTIFICATE_WORK_DIRECTORY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_AGENT_CERTIFICATE_WORK_DIRECTORY));
    let certificate_validity = parse_positive_duration(
        "MCSERVER_CONTROL_PLANE_AGENT_CERTIFICATE_VALIDITY_SECONDS",
        DEFAULT_AGENT_CERTIFICATE_VALIDITY_SECONDS,
    )?;
    let openssl_binary = env::var_os("MCSERVER_CONTROL_PLANE_OPENSSL_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OPENSSL_BINARY));
    let trust_domain = required_non_blank(
        "MCSERVER_CONTROL_PLANE_AGENT_TRUST_DOMAIN",
        MAX_SERVER_NAME_CHARS,
    )?;
    validate_dns_name(&trust_domain)?;
    let node_agent_download_url = required_non_blank(
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_DOWNLOAD_URL",
        MAX_URL_CHARS,
    )?;
    validate_https_url(
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_DOWNLOAD_URL",
        &node_agent_download_url,
    )?;
    let node_agent_sha256 =
        required_non_blank("MCSERVER_CONTROL_PLANE_NODE_AGENT_SHA256", SHA256_HEX_CHARS)?;
    if node_agent_sha256.len() != SHA256_HEX_CHARS
        || !node_agent_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ConfigError::InvalidSha256(node_agent_sha256));
    }

    Ok(Some(RemoteAgentConfig {
        listen_address,
        public_address,
        tls_server_name,
        tls_certificate,
        tls_private_key,
        tls_ca_certificate,
        client_ca_certificate,
        client_ca_private_key,
        certificate_work_directory,
        certificate_validity,
        openssl_binary,
        trust_domain,
        node_agent_download_url,
        node_agent_sha256: node_agent_sha256.to_ascii_lowercase(),
        max_frame_bytes,
        node_operation_timeout: node_operation_timeout(agent_command_timeout),
    }))
}

fn reject_partial_remote_config() -> Result<(), ConfigError> {
    for name in [
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_PUBLIC_ADDRESS",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_SERVER_NAME",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CERTIFICATE",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_PRIVATE_KEY",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CA_CERTIFICATE",
        "MCSERVER_CONTROL_PLANE_AGENT_CLIENT_CA_CERTIFICATE",
        "MCSERVER_CONTROL_PLANE_AGENT_CLIENT_CA_PRIVATE_KEY",
        "MCSERVER_CONTROL_PLANE_AGENT_CERTIFICATE_WORK_DIRECTORY",
        "MCSERVER_CONTROL_PLANE_AGENT_CERTIFICATE_VALIDITY_SECONDS",
        "MCSERVER_CONTROL_PLANE_AGENT_TRUST_DOMAIN",
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_DOWNLOAD_URL",
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_SHA256",
    ] {
        if optional_string_value(name)?.is_some() {
            return Err(ConfigError::RemoteAgentListenAddressRequired);
        }
    }
    Ok(())
}

fn akamai_config(
    remote_agent: Option<&RemoteAgentConfig>,
) -> Result<Option<AkamaiConfig>, ConfigError> {
    let direct_token = optional_string_value("MCSERVER_AKAMAI_API_TOKEN")?;
    let token_file = optional_string_value("MCSERVER_AKAMAI_API_TOKEN_FILE")?;
    let api_token = match (direct_token, token_file) {
        (Some(_), Some(_)) => {
            return Err(ConfigError::ConflictingSecretSources {
                direct: "MCSERVER_AKAMAI_API_TOKEN",
                file: "MCSERVER_AKAMAI_API_TOKEN_FILE",
            });
        }
        (Some(value), None) => Some(value),
        (None, Some(path)) => Some(read_secret_file(
            "MCSERVER_AKAMAI_API_TOKEN_FILE",
            Path::new(&path),
        )?),
        (None, None) => None,
    };
    let Some(api_token) = api_token else {
        for name in [
            "MCSERVER_AKAMAI_API_BASE_URL",
            "MCSERVER_AKAMAI_AUTHORIZED_KEYS_FILE",
            "MCSERVER_AKAMAI_SCOPE",
            "MCSERVER_AKAMAI_REQUEST_TIMEOUT_SECONDS",
            "MCSERVER_AKAMAI_REAP_ORPHANS_ON_START",
            "MCSERVER_AKAMAI_LIVE_ENABLED",
            "MCSERVER_AKAMAI_ALLOWED_REGIONS",
            "MCSERVER_AKAMAI_ALLOWED_IMAGES",
            "MCSERVER_AKAMAI_ALLOWED_INSTANCE_TYPES",
            "MCSERVER_AKAMAI_ALLOWED_FIREWALL_IDS",
            "MCSERVER_AKAMAI_MAX_ACTIVE_INSTANCES",
            "MCSERVER_AKAMAI_MAX_INSTANCE_LIFETIME_SECONDS",
        ] {
            if optional_string_value(name)?.is_some() {
                return Err(ConfigError::AkamaiTokenRequired);
            }
        }
        return Ok(None);
    };
    validate_non_blank("MCSERVER_AKAMAI_API_TOKEN", &api_token, MAX_SECRET_CHARS)?;
    if remote_agent.is_none() {
        return Err(ConfigError::RemoteAgentRequiredForAkamai);
    }
    let api_base_url =
        optional_string("MCSERVER_AKAMAI_API_BASE_URL", DEFAULT_AKAMAI_API_BASE_URL)?;
    let loopback_api = validate_api_base_url(&api_base_url)?;
    let authorized_keys_file = required_path("MCSERVER_AKAMAI_AUTHORIZED_KEYS_FILE")?;
    let scope = optional_scope("MCSERVER_AKAMAI_SCOPE", DEFAULT_AKAMAI_SCOPE)?;
    if scope.chars().count() > MAX_AKAMAI_SCOPE_CHARS {
        return Err(ConfigError::AkamaiScopeTooLong {
            maximum: MAX_AKAMAI_SCOPE_CHARS,
        });
    }
    let request_timeout = parse_positive_duration(
        "MCSERVER_AKAMAI_REQUEST_TIMEOUT_SECONDS",
        DEFAULT_AKAMAI_REQUEST_TIMEOUT_SECONDS,
    )?;
    let reap_orphans_on_start = parse_bool(
        "MCSERVER_AKAMAI_REAP_ORPHANS_ON_START",
        DEFAULT_AKAMAI_REAP_ORPHANS_ON_START,
    )?;
    let live_enabled = parse_bool("MCSERVER_AKAMAI_LIVE_ENABLED", DEFAULT_AKAMAI_LIVE_ENABLED)?;
    let allowed_regions = parse_identifier_set(
        "MCSERVER_AKAMAI_ALLOWED_REGIONS",
        DEFAULT_AKAMAI_ALLOWED_REGIONS,
    )?;
    let allowed_images = parse_identifier_set(
        "MCSERVER_AKAMAI_ALLOWED_IMAGES",
        DEFAULT_AKAMAI_ALLOWED_IMAGES,
    )?;
    let allowed_instance_types = parse_identifier_set(
        "MCSERVER_AKAMAI_ALLOWED_INSTANCE_TYPES",
        DEFAULT_AKAMAI_ALLOWED_INSTANCE_TYPES,
    )?;
    let allowed_firewall_ids = parse_positive_u64_set("MCSERVER_AKAMAI_ALLOWED_FIREWALL_IDS")?;
    let max_active_instances = parse_positive_usize(
        "MCSERVER_AKAMAI_MAX_ACTIVE_INSTANCES",
        DEFAULT_AKAMAI_MAX_ACTIVE_INSTANCES,
    )?;
    let max_instance_lifetime = parse_positive_duration(
        "MCSERVER_AKAMAI_MAX_INSTANCE_LIFETIME_SECONDS",
        DEFAULT_AKAMAI_MAX_INSTANCE_LIFETIME_SECONDS,
    )?;
    let remote_agent = remote_agent.ok_or(ConfigError::RemoteAgentRequiredForAkamai)?;
    let minimum_certificate_validity = max_instance_lifetime
        .checked_add(Duration::from_secs(
            AGENT_CERTIFICATE_SHUTDOWN_BUFFER_SECONDS,
        ))
        .ok_or(ConfigError::DurationOutOfRange(
            "MCSERVER_AKAMAI_MAX_INSTANCE_LIFETIME_SECONDS",
        ))?;
    if remote_agent.certificate_validity < minimum_certificate_validity {
        return Err(ConfigError::AgentCertificateValidityTooShort {
            certificate_validity: remote_agent.certificate_validity,
            minimum: minimum_certificate_validity,
        });
    }

    Ok(Some(AkamaiConfig {
        api_token,
        api_base_url: api_base_url.trim_end_matches('/').to_owned(),
        authorized_keys_file,
        scope,
        request_timeout,
        reap_orphans_on_start,
        live_enabled,
        allowed_regions,
        allowed_images,
        allowed_instance_types,
        allowed_firewall_ids,
        max_active_instances,
        max_instance_lifetime,
        loopback_api,
    }))
}

fn r2_config(
    akamai: Option<&AkamaiConfig>,
    agent_command_timeout: Duration,
) -> Result<Option<R2Config>, ConfigError> {
    let direct_token = optional_string_value("MCSERVER_R2_API_TOKEN")?;
    let token_file = optional_string_value("MCSERVER_R2_API_TOKEN_FILE")?;
    let api_token = match (direct_token, token_file) {
        (Some(_), Some(_)) => {
            return Err(ConfigError::ConflictingSecretSources {
                direct: "MCSERVER_R2_API_TOKEN",
                file: "MCSERVER_R2_API_TOKEN_FILE",
            });
        }
        (Some(value), None) => Some(value),
        (None, Some(path)) => Some(read_secret_file(
            "MCSERVER_R2_API_TOKEN_FILE",
            Path::new(&path),
        )?),
        (None, None) => None,
    };

    let configured_names = [
        "MCSERVER_R2_API_BASE_URL",
        "MCSERVER_R2_ACCOUNT_ID",
        "MCSERVER_R2_PARENT_ACCESS_KEY_ID",
        "MCSERVER_R2_BUCKET",
        "MCSERVER_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS",
        "MCSERVER_R2_RUNTIME_ENVIRONMENT_FILE",
        "MCSERVER_R2_REQUEST_TIMEOUT_SECONDS",
    ];
    let Some(api_token) = api_token else {
        let mut partially_configured = false;
        for name in configured_names {
            partially_configured |= optional_string_value(name)?.is_some();
        }
        if partially_configured {
            return Err(ConfigError::R2TokenRequired);
        }
        if akamai.is_some() {
            return Err(ConfigError::R2RequiredForAkamai);
        }
        return Ok(None);
    };
    let akamai = akamai.ok_or(ConfigError::AkamaiRequiredForR2)?;

    validate_non_blank("MCSERVER_R2_API_TOKEN", &api_token, MAX_SECRET_CHARS)?;
    let api_base_url =
        optional_string("MCSERVER_R2_API_BASE_URL", DEFAULT_CLOUDFLARE_API_BASE_URL)?;
    let loopback_api = validate_r2_api_base_url(&api_base_url)?;
    let account_id = required_non_blank("MCSERVER_R2_ACCOUNT_ID", 32)?;
    if account_id.len() != 32 || !account_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ConfigError::InvalidR2AccountId(account_id));
    }
    let parent_access_key_id = required_non_blank("MCSERVER_R2_PARENT_ACCESS_KEY_ID", 256)?;
    let bucket = required_non_blank("MCSERVER_R2_BUCKET", 63)?;
    validate_r2_bucket_name(&bucket)?;
    let temporary_credential_ttl = parse_positive_duration(
        "MCSERVER_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS",
        DEFAULT_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS,
    )?;
    if temporary_credential_ttl.as_secs() > MAX_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS {
        return Err(ConfigError::R2CredentialTtlTooLong {
            maximum: Duration::from_secs(MAX_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS),
        });
    }
    let shutdown_buffer = agent_command_timeout.max(Duration::from_secs(
        AGENT_CERTIFICATE_SHUTDOWN_BUFFER_SECONDS,
    ));
    let minimum_ttl = akamai
        .max_instance_lifetime
        .checked_add(shutdown_buffer)
        .ok_or(ConfigError::DurationOutOfRange(
            "MCSERVER_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS",
        ))?;
    if temporary_credential_ttl < minimum_ttl {
        return Err(ConfigError::R2CredentialTtlTooShort {
            configured: temporary_credential_ttl,
            minimum: minimum_ttl,
        });
    }
    let runtime_environment_file = required_path("MCSERVER_R2_RUNTIME_ENVIRONMENT_FILE")?;
    let runtime_environment = read_runtime_environment(&runtime_environment_file)?;
    let request_timeout = parse_positive_duration(
        "MCSERVER_R2_REQUEST_TIMEOUT_SECONDS",
        DEFAULT_AKAMAI_REQUEST_TIMEOUT_SECONDS,
    )?;

    Ok(Some(R2Config {
        api_token,
        api_base_url: api_base_url.trim_end_matches('/').to_owned(),
        account_id,
        parent_access_key_id,
        bucket,
        temporary_credential_ttl,
        runtime_environment_file,
        runtime_environment,
        request_timeout,
        loopback_api,
    }))
}

fn read_secret_file(name: &'static str, path: &Path) -> Result<String, ConfigError> {
    let value = fs::read_to_string(path).map_err(|source| ConfigError::SecretFile {
        name,
        path: path.to_path_buf(),
        source,
    })?;
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    validate_non_blank(name, &value, MAX_SECRET_CHARS)?;
    Ok(value)
}

fn read_runtime_environment(path: &Path) -> Result<BTreeMap<String, String>, ConfigError> {
    let bytes = fs::read(path).map_err(|source| ConfigError::RuntimeEnvironmentFile {
        path: path.to_path_buf(),
        source,
    })?;
    if bytes.len() > MAX_RUNTIME_ENVIRONMENT_BYTES {
        return Err(ConfigError::RuntimeEnvironmentTooLarge {
            path: path.to_path_buf(),
            maximum: MAX_RUNTIME_ENVIRONMENT_BYTES,
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(ConfigError::RuntimeEnvironmentEncoding)?;
    let mut values = BTreeMap::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(ConfigError::InvalidRuntimeEnvironmentLine(index + 1));
        };
        if !is_runtime_environment_key(key) {
            return Err(ConfigError::InvalidRuntimeEnvironmentKey(key.to_owned()));
        }
        if value.contains('\0') || value.chars().count() > MAX_RUNTIME_ENVIRONMENT_VALUE_CHARS {
            return Err(ConfigError::InvalidRuntimeEnvironmentValue(key.to_owned()));
        }
        if values.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(ConfigError::DuplicateRuntimeEnvironmentKey(key.to_owned()));
        }
        if values.len() > MAX_RUNTIME_ENVIRONMENT_ENTRIES {
            return Err(ConfigError::TooManyRuntimeEnvironmentEntries {
                maximum: MAX_RUNTIME_ENVIRONMENT_ENTRIES,
            });
        }
    }
    if values.is_empty() {
        return Err(ConfigError::EmptyRuntimeEnvironment);
    }
    validate_required_runtime_environment(&values)?;
    Ok(values)
}

fn validate_required_runtime_environment(
    values: &BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    for key in REQUIRED_REMOTE_RUNTIME_ENVIRONMENT_KEYS {
        let Some(value) = values.get(key) else {
            return Err(ConfigError::MissingRequiredRuntimeEnvironmentKey(key));
        };
        if value.trim().is_empty() {
            return Err(ConfigError::BlankRequiredRuntimeEnvironmentValue(key));
        }
    }
    if values.get("AWS_DEFAULT_REGION").map(String::as_str) != Some("auto") {
        return Err(ConfigError::InvalidR2Region);
    }
    Ok(())
}

fn is_runtime_environment_key(key: &str) -> bool {
    key == "AWS_DEFAULT_REGION"
}

fn required_path(name: &'static str) -> Result<PathBuf, ConfigError> {
    Ok(PathBuf::from(required_non_blank(name, 4096)?))
}

fn required_non_blank(name: &'static str, maximum: usize) -> Result<String, ConfigError> {
    let value = optional_string_value(name)?.ok_or(ConfigError::MissingValue(name))?;
    validate_non_blank(name, &value, maximum)?;
    Ok(value)
}

fn validate_non_blank(name: &'static str, value: &str, maximum: usize) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::BlankValue(name));
    }
    if value.contains('\0') {
        return Err(ConfigError::NulByte(name));
    }
    if value.chars().count() > maximum {
        return Err(ConfigError::ValueTooLong { name, maximum });
    }
    Ok(())
}

fn optional_scope(name: &'static str, default: &str) -> Result<String, ConfigError> {
    let value = optional_string(name, default)?;
    validate_non_blank(name, &value, MAX_SCOPE_CHARS)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ConfigError::InvalidScope { name, value });
    }
    Ok(value)
}

fn parse_identifier_set(
    name: &'static str,
    default: &str,
) -> Result<BTreeSet<String>, ConfigError> {
    let raw = optional_string(name, default)?;
    let values = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if values.is_empty() || values.iter().any(|value| !is_provider_identifier(value)) {
        return Err(ConfigError::InvalidIdentifierSet { name, value: raw });
    }
    Ok(values)
}

fn is_provider_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/' | b'.'))
}

fn validate_host_port(value: &str) -> Result<(), ConfigError> {
    if let Ok(address) = value.parse::<SocketAddr>() {
        return if address.port() == 0 {
            Err(ConfigError::ZeroRemoteAgentPort)
        } else {
            Ok(())
        };
    }

    let Some((host, port)) = value.rsplit_once(':') else {
        return Err(ConfigError::InvalidRemotePublicAddress(value.to_owned()));
    };
    validate_dns_name(host)?;
    let port = port
        .parse::<u16>()
        .map_err(|_| ConfigError::InvalidRemotePublicAddress(value.to_owned()))?;
    if port == 0 {
        return Err(ConfigError::ZeroRemoteAgentPort);
    }
    Ok(())
}

fn validate_dns_name(value: &str) -> Result<(), ConfigError> {
    if value.len() > MAX_SERVER_NAME_CHARS
        || value.starts_with('.')
        || value.ends_with('.')
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(ConfigError::InvalidTlsServerName(value.to_owned()));
    }
    Ok(())
}

fn validate_https_url(name: &'static str, value: &str) -> Result<(), ConfigError> {
    let Ok(url) = reqwest::Url::parse(value) else {
        return Err(ConfigError::InvalidUrl { name });
    };
    if url.scheme() != "https" {
        return Err(ConfigError::HttpsRequired { name });
    }
    if !has_explicit_authority(value)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::InvalidUrl { name });
    }
    Ok(())
}

fn validate_api_base_url(value: &str) -> Result<bool, ConfigError> {
    let Ok(url) = reqwest::Url::parse(value) else {
        return Err(ConfigError::InvalidAkamaiApiBaseUrl(value.to_owned()));
    };
    if !has_explicit_authority(value)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::InvalidAkamaiApiBaseUrl(value.to_owned()));
    }
    let Some(host) = url.host_str() else {
        return Err(ConfigError::InvalidAkamaiApiBaseUrl(value.to_owned()));
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    match url.scheme() {
        "https" => Ok(loopback),
        "http" if loopback => Ok(true),
        _ => Err(ConfigError::InvalidAkamaiApiBaseUrl(value.to_owned())),
    }
}

fn validate_r2_api_base_url(value: &str) -> Result<bool, ConfigError> {
    let Ok(url) = reqwest::Url::parse(value) else {
        return Err(ConfigError::InvalidR2ApiBaseUrl(value.to_owned()));
    };
    if !has_explicit_authority(value)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::InvalidR2ApiBaseUrl(value.to_owned()));
    }
    let Some(host) = url.host_str() else {
        return Err(ConfigError::InvalidR2ApiBaseUrl(value.to_owned()));
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    match url.scheme() {
        "https" => Ok(loopback),
        "http" if loopback => Ok(true),
        _ => Err(ConfigError::InvalidR2ApiBaseUrl(value.to_owned())),
    }
}

fn validate_r2_bucket_name(value: &str) -> Result<(), ConfigError> {
    let bytes = value.as_bytes();
    let valid = (3..=63).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
        && !value.contains("..")
        && !value.contains(".-")
        && !value.contains("-.");
    if valid {
        Ok(())
    } else {
        Err(ConfigError::InvalidR2Bucket(value.to_owned()))
    }
}

fn has_explicit_authority(value: &str) -> bool {
    value
        .split_once("://")
        .and_then(|(_, remainder)| remainder.split(['/', '?', '#']).next())
        .is_some_and(|authority| !authority.is_empty())
}

fn parse_bool(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    match env::var(name) {
        Ok(value) => match value.as_str() {
            "1" | "true" | "TRUE" | "yes" | "YES" => Ok(true),
            "0" | "false" | "FALSE" | "no" | "NO" => Ok(false),
            _ => Err(ConfigError::InvalidBoolean { name, value }),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(source) => Err(ConfigError::Environment { name, source }),
    }
}

fn optional_string(name: &'static str, default: &str) -> Result<String, ConfigError> {
    match optional_string_value(name)? {
        Some(value) => Ok(value),
        None => Ok(default.to_owned()),
    }
}

fn optional_string_value(name: &'static str) -> Result<Option<String>, ConfigError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(source) => Err(ConfigError::Environment { name, source }),
    }
}

fn parse_socket_mode(value: Option<String>) -> Result<u32, ConfigError> {
    let mode = match value {
        Some(value) => u32::from_str_radix(value.trim_start_matches("0o"), 8)
            .map_err(|source| ConfigError::InvalidSocketMode { value, source })?,
        None => DEFAULT_SOCKET_MODE,
    };
    if mode > 0o777 {
        return Err(ConfigError::SocketModeOutOfRange(mode));
    }
    Ok(mode)
}

fn parse_positive_usize(name: &'static str, default: usize) -> Result<usize, ConfigError> {
    let value = match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|source| ConfigError::InvalidInteger {
                name,
                value,
                source,
            })?,
        Err(env::VarError::NotPresent) => default,
        Err(source) => return Err(ConfigError::Environment { name, source }),
    };
    if value == 0 {
        return Err(ConfigError::ZeroValue(name));
    }
    Ok(value)
}

fn parse_positive_u64_set(name: &'static str) -> Result<BTreeSet<u64>, ConfigError> {
    let raw = required_non_blank(name, 4096)?;
    let values = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<u64>)
        .collect::<Result<BTreeSet<_>, _>>();
    match values {
        Ok(values) if !values.is_empty() && !values.contains(&0) => Ok(values),
        _ => Err(ConfigError::InvalidPositiveIntegerSet { name, value: raw }),
    }
}

fn parse_positive_duration(name: &'static str, default: u64) -> Result<Duration, ConfigError> {
    let seconds = match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|source| ConfigError::InvalidInteger {
                name,
                value,
                source,
            })?,
        Err(env::VarError::NotPresent) => default,
        Err(source) => return Err(ConfigError::Environment { name, source }),
    };
    if seconds == 0 {
        return Err(ConfigError::ZeroValue(name));
    }
    Ok(Duration::from_secs(seconds))
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("environment variable {name} is invalid")]
    Environment {
        name: &'static str,
        #[source]
        source: env::VarError,
    },
    #[error("{name} must be an integer, got {value}")]
    InvalidInteger {
        name: &'static str,
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("socket mode must be an octal integer, got {value}")]
    InvalidSocketMode {
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("socket mode must be between 0000 and 0777, got {0:o}")]
    SocketModeOutOfRange(u32),
    #[error("{0} must be greater than zero")]
    ZeroValue(&'static str),
    #[error("{0} is outside the supported duration range")]
    DurationOutOfRange(&'static str),
    #[error("required environment variable {0} is missing")]
    MissingValue(&'static str),
    #[error("{0} must not be blank")]
    BlankValue(&'static str),
    #[error("{0} must not contain a NUL byte")]
    NulByte(&'static str),
    #[error("{name} must be no longer than {maximum} characters")]
    ValueTooLong { name: &'static str, maximum: usize },
    #[error("{name} contains invalid scope characters: {value}")]
    InvalidScope { name: &'static str, value: String },
    #[error("{name} contains an invalid provider identifier: {value}")]
    InvalidProviderIdentifier { name: &'static str, value: String },
    #[error("{name} must be a non-empty comma-separated identifier list: {value}")]
    InvalidIdentifierSet { name: &'static str, value: String },
    #[error("{name} must be a comma-separated set of positive integers: {value}")]
    InvalidPositiveIntegerSet { name: &'static str, value: String },
    #[error("{name} must be a boolean, got {value}")]
    InvalidBoolean { name: &'static str, value: String },
    #[error("local agent listen address is invalid")]
    InvalidSocketAddress(#[source] std::net::AddrParseError),
    #[error("remote agent listen address is invalid")]
    InvalidRemoteSocketAddress(#[source] std::net::AddrParseError),
    #[error("local agent listen address must be loopback, got {0}")]
    AgentAddressMustBeLoopback(SocketAddr),
    #[error("local agent listen port must be greater than zero")]
    ZeroAgentPort,
    #[error("remote agent listen port must be greater than zero")]
    ZeroRemoteAgentPort,
    #[error("remote agent public address must be HOST:PORT or a socket address: {0}")]
    InvalidRemotePublicAddress(String),
    #[error("remote agent settings require MCSERVER_CONTROL_PLANE_REMOTE_AGENT_LISTEN_ADDRESS")]
    RemoteAgentListenAddressRequired,
    #[error("Akamai settings require MCSERVER_AKAMAI_API_TOKEN or MCSERVER_AKAMAI_API_TOKEN_FILE")]
    AkamaiTokenRequired,
    #[error("Akamai provider requires remote agent TLS configuration")]
    RemoteAgentRequiredForAkamai,
    #[error("Akamai provider requires Cloudflare R2 temporary credential configuration")]
    R2RequiredForAkamai,
    #[error("R2 temporary credential configuration requires the Akamai provider")]
    AkamaiRequiredForR2,
    #[error("R2 settings require MCSERVER_R2_API_TOKEN or MCSERVER_R2_API_TOKEN_FILE")]
    R2TokenRequired,
    #[error("both {direct} and {file} were configured; choose one secret source")]
    ConflictingSecretSources {
        direct: &'static str,
        file: &'static str,
    },
    #[error("failed to read secret file {path} configured by {name}")]
    SecretFile {
        name: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read node runtime environment file {path}")]
    RuntimeEnvironmentFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("node runtime environment file {path} exceeds {maximum} bytes")]
    RuntimeEnvironmentTooLarge { path: PathBuf, maximum: usize },
    #[error("node runtime environment file is not UTF-8")]
    RuntimeEnvironmentEncoding(#[source] std::str::Utf8Error),
    #[error("node runtime environment line {0} must be KEY=VALUE")]
    InvalidRuntimeEnvironmentLine(usize),
    #[error("R2 runtime environment key is not an allowed RESTIC_ key or AWS_DEFAULT_REGION: {0}")]
    InvalidRuntimeEnvironmentKey(String),
    #[error("node runtime environment value for {0} is invalid or too long")]
    InvalidRuntimeEnvironmentValue(String),
    #[error("node runtime environment key is duplicated: {0}")]
    DuplicateRuntimeEnvironmentKey(String),
    #[error("R2 runtime environment must contain the required restic settings")]
    EmptyRuntimeEnvironment,
    #[error("node runtime environment contains more than {maximum} entries")]
    TooManyRuntimeEnvironmentEntries { maximum: usize },
    #[error("node runtime environment is missing required key {0}")]
    MissingRequiredRuntimeEnvironmentKey(&'static str),
    #[error("node runtime environment value for required key {0} must not be blank")]
    BlankRequiredRuntimeEnvironmentValue(&'static str),
    #[error("AWS_DEFAULT_REGION must be auto for Cloudflare R2")]
    InvalidR2Region,
    #[error("invalid TLS server name: {0}")]
    InvalidTlsServerName(String),
    #[error("{name} is not a valid URL")]
    InvalidUrl { name: &'static str },
    #[error("{name} must use https")]
    HttpsRequired { name: &'static str },
    #[error("invalid Akamai API base URL: {0}; use https, or loopback http for tests")]
    InvalidAkamaiApiBaseUrl(String),
    #[error("invalid Cloudflare API base URL: {0}; use https, or loopback http for tests")]
    InvalidR2ApiBaseUrl(String),
    #[error("Cloudflare account ID must be exactly 32 hexadecimal characters: {0}")]
    InvalidR2AccountId(String),
    #[error("invalid R2 bucket name: {0}")]
    InvalidR2Bucket(String),
    #[error("R2 temporary credential TTL exceeds the Cloudflare maximum of {maximum:?}")]
    R2CredentialTtlTooLong { maximum: Duration },
    #[error(
        "R2 temporary credential TTL {configured:?} must be at least {minimum:?} so one VM session can stop and snapshot safely"
    )]
    R2CredentialTtlTooShort {
        configured: Duration,
        minimum: Duration,
    },
    #[error("Akamai scope must be no longer than {maximum} characters")]
    AkamaiScopeTooLong { maximum: usize },
    #[error(
        "agent certificate validity {certificate_validity:?} must be at least {minimum:?} so an expiring VM can stop and publish a snapshot"
    )]
    AgentCertificateValidityTooShort {
        certificate_validity: Duration,
        minimum: Duration,
    },
    #[error("node-agent SHA-256 must be 64 hexadecimal characters: {0}")]
    InvalidSha256(String),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        ConfigError, is_runtime_environment_key, validate_api_base_url, validate_https_url,
        validate_r2_bucket_name, validate_required_runtime_environment,
    };

    #[test]
    fn remote_download_requires_structurally_valid_https() {
        assert!(
            validate_https_url("TEST", "https://downloads.example.com/agent?version=1").is_ok()
        );
        assert!(validate_https_url("TEST", "http://downloads.example.com/agent").is_err());
        assert!(validate_https_url("TEST", "https:///missing-host").is_err());
        assert!(validate_https_url("TEST", "https://user:secret@example.com/agent").is_err());
    }

    #[test]
    fn akamai_api_allows_http_only_for_loopback_tests() {
        assert!(validate_api_base_url("https://api.linode.com/v4").is_ok_and(|loopback| !loopback));
        assert!(validate_api_base_url("http://127.0.0.1:3000/v4").is_ok_and(|loopback| loopback));
        assert!(validate_api_base_url("http://[::1]:3000/v4").is_ok_and(|loopback| loopback));
        assert!(validate_api_base_url("http://localhost.evil.example/v4").is_err());
        assert!(validate_api_base_url("https://api.linode.com/v4?token=secret").is_err());
    }

    #[test]
    fn runtime_environment_requires_r2_region() {
        let values = BTreeMap::new();
        assert!(matches!(
            validate_required_runtime_environment(&values),
            Err(ConfigError::MissingRequiredRuntimeEnvironmentKey(
                "AWS_DEFAULT_REGION"
            ))
        ));
    }

    #[test]
    fn runtime_environment_rejects_blank_required_values() {
        let values = BTreeMap::from([("AWS_DEFAULT_REGION".to_owned(), "   ".to_owned())]);
        assert!(matches!(
            validate_required_runtime_environment(&values),
            Err(ConfigError::BlankRequiredRuntimeEnvironmentValue(
                "AWS_DEFAULT_REGION"
            ))
        ));
    }

    #[test]
    fn runtime_environment_only_accepts_r2_region() {
        assert!(!is_runtime_environment_key("RESTIC_PASSWORD"));
        assert!(is_runtime_environment_key("AWS_DEFAULT_REGION"));
        assert!(!is_runtime_environment_key("AWS_ACCESS_KEY_ID"));
        assert!(!is_runtime_environment_key("AWS_SECRET_ACCESS_KEY"));
        assert!(!is_runtime_environment_key("AWS_SESSION_TOKEN"));
        assert!(!is_runtime_environment_key("RESTIC_REPOSITORY"));
        assert!(!is_runtime_environment_key(
            "MCSERVER_NODE_AGENT_CONTROL_PLANE_ADDRESS"
        ));
        assert!(!is_runtime_environment_key("LD_PRELOAD"));
    }

    #[test]
    fn validates_r2_bucket_names() {
        assert!(validate_r2_bucket_name("mcserver-snapshots").is_ok());
        assert!(validate_r2_bucket_name("a.b-c").is_ok());
        assert!(validate_r2_bucket_name("UPPERCASE").is_err());
        assert!(validate_r2_bucket_name("-leading").is_err());
        assert!(validate_r2_bucket_name("double..dot").is_err());
    }
}
