use std::{collections::BTreeMap, fmt, time::Duration};

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::R2Config;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_CREDENTIAL_CHARS: usize = 16 * 1024;
const MAX_REPOSITORY_PREFIX_CHARS: usize = 1024;
const PREFLIGHT_PREFIX: &str = "mcserver-preflight/";
const PREFLIGHT_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub struct R2TemporaryCredentialManager {
    http: Client,
    config: R2Config,
}

impl fmt::Debug for R2TemporaryCredentialManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("R2TemporaryCredentialManager")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl R2TemporaryCredentialManager {
    pub fn new(config: R2Config) -> Result<Self, R2TemporaryCredentialError> {
        let http = Client::builder()
            .timeout(config.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { http, config })
    }

    pub async fn preflight(&self) -> Result<(), R2TemporaryCredentialError> {
        self.issue(PREFLIGHT_PREFIX, PREFLIGHT_TTL).await?;
        Ok(())
    }

    pub async fn runtime_environment_for_repository(
        &self,
        repository: &str,
    ) -> Result<BTreeMap<String, String>, R2TemporaryCredentialError> {
        let prefix = repository_prefix(repository, &self.config.account_id, &self.config.bucket)?;
        let credentials = self
            .issue(&prefix, self.config.temporary_credential_ttl)
            .await?;
        let mut environment = self.config.runtime_environment.clone();
        for (key, value) in [
            ("AWS_ACCESS_KEY_ID", credentials.access_key_id),
            ("AWS_SECRET_ACCESS_KEY", credentials.secret_access_key),
            ("AWS_SESSION_TOKEN", credentials.session_token),
        ] {
            if environment.insert(key.to_owned(), value).is_some() {
                return Err(R2TemporaryCredentialError::ReservedRuntimeKey(
                    key.to_owned(),
                ));
            }
        }
        Ok(environment)
    }

    async fn issue(
        &self,
        prefix: &str,
        ttl: Duration,
    ) -> Result<R2TemporaryCredentials, R2TemporaryCredentialError> {
        validate_prefix(prefix)?;
        let endpoint = format!(
            "{}/accounts/{}/r2/temp-access-credentials",
            self.config.api_base_url, self.config.account_id
        );
        let request = CreateTemporaryCredentialRequest {
            bucket: &self.config.bucket,
            parent_access_key_id: &self.config.parent_access_key_id,
            permission: "object-read-write",
            ttl_seconds: ttl.as_secs(),
            prefixes: [prefix],
        };
        let response = self
            .http
            .post(endpoint)
            .bearer_auth(&self.config.api_token)
            .json(&request)
            .send()
            .await?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = response.bytes().await?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(R2TemporaryCredentialError::ResponseTooLarge {
                actual: body.len(),
                maximum: MAX_RESPONSE_BYTES,
            });
        }
        let envelope = serde_json::from_slice::<CloudflareEnvelope<CreateCredentialResult>>(&body)
            .map_err(|source| R2TemporaryCredentialError::InvalidResponse { status, source })?;
        if !status.is_success() || !envelope.success {
            return Err(R2TemporaryCredentialError::Api {
                status,
                errors: envelope.errors,
                retry_after,
            });
        }
        let result = envelope
            .result
            .ok_or(R2TemporaryCredentialError::MissingResult)?;
        let credentials = R2TemporaryCredentials {
            access_key_id: required_credential("accessKeyId", result.access_key_id)?,
            secret_access_key: required_credential("secretAccessKey", result.secret_access_key)?,
            session_token: required_credential("sessionToken", result.session_token)?,
        };
        Ok(credentials)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateTemporaryCredentialRequest<'a> {
    bucket: &'a str,
    parent_access_key_id: &'a str,
    permission: &'static str,
    ttl_seconds: u64,
    prefixes: [&'a str; 1],
}

#[derive(Deserialize)]
struct CloudflareEnvelope<T> {
    result: Option<T>,
    #[serde(default)]
    errors: Vec<CloudflareApiError>,
    success: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloudflareApiError {
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default, alias = "reason")]
    pub message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCredentialResult {
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    session_token: Option<String>,
}

struct R2TemporaryCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: String,
}

fn required_credential(
    field: &'static str,
    value: Option<String>,
) -> Result<String, R2TemporaryCredentialError> {
    let value = value.ok_or(R2TemporaryCredentialError::MissingCredentialField(field))?;
    if value.trim().is_empty()
        || value.contains('\0')
        || value.chars().count() > MAX_CREDENTIAL_CHARS
    {
        return Err(R2TemporaryCredentialError::InvalidCredentialField(field));
    }
    Ok(value)
}

fn repository_prefix(
    repository: &str,
    account_id: &str,
    bucket: &str,
) -> Result<String, R2TemporaryCredentialError> {
    let Some(url_value) = repository.strip_prefix("s3:") else {
        return Err(R2TemporaryCredentialError::UnsupportedRepository(
            repository.to_owned(),
        ));
    };
    let url = reqwest::Url::parse(url_value)
        .map_err(|_| R2TemporaryCredentialError::UnsupportedRepository(repository.to_owned()))?;
    let expected_host = format!("{account_id}.r2.cloudflarestorage.com");
    if url.scheme() != "https"
        || url.host_str() != Some(expected_host.as_str())
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(R2TemporaryCredentialError::UnsupportedRepository(
            repository.to_owned(),
        ));
    }
    let path = url.path().strip_prefix('/').unwrap_or(url.path());
    if path.contains('%') {
        return Err(R2TemporaryCredentialError::UnsupportedRepository(
            repository.to_owned(),
        ));
    }
    let mut segments = path.split('/');
    if segments.next() != Some(bucket) {
        return Err(R2TemporaryCredentialError::RepositoryBucketMismatch {
            expected: bucket.to_owned(),
        });
    }
    let mut prefix_segments = segments.collect::<Vec<_>>();
    if prefix_segments.last() == Some(&"") {
        prefix_segments.pop();
    }
    if prefix_segments.is_empty()
        || prefix_segments
            .iter()
            .any(|segment| !is_safe_prefix_segment(segment))
    {
        return Err(R2TemporaryCredentialError::UnsafeRepositoryPrefix(
            repository.to_owned(),
        ));
    }
    let prefix = format!("{}/", prefix_segments.join("/"));
    validate_prefix(&prefix)?;
    Ok(prefix)
}

fn is_safe_prefix_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_prefix(value: &str) -> Result<(), R2TemporaryCredentialError> {
    if value.is_empty()
        || !value.ends_with('/')
        || value.starts_with('/')
        || value.chars().count() > MAX_REPOSITORY_PREFIX_CHARS
        || value.contains('\0')
    {
        return Err(R2TemporaryCredentialError::InvalidPrefix(value.to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum R2TemporaryCredentialError {
    #[error("failed to build or call the Cloudflare API")]
    Http(#[from] reqwest::Error),
    #[error("Cloudflare API response exceeded {maximum} bytes: {actual}")]
    ResponseTooLarge { actual: usize, maximum: usize },
    #[error("Cloudflare API returned invalid JSON with status {status}")]
    InvalidResponse {
        status: StatusCode,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "Cloudflare API rejected the temporary credential request with status {status}: {errors:?}"
    )]
    Api {
        status: StatusCode,
        errors: Vec<CloudflareApiError>,
        retry_after: Option<Duration>,
    },
    #[error("Cloudflare API success response contained no result")]
    MissingResult,
    #[error("Cloudflare API success response omitted {0}")]
    MissingCredentialField(&'static str),
    #[error("Cloudflare API returned an invalid {0}")]
    InvalidCredentialField(&'static str),
    #[error("runtime environment attempted to override temporary credential key {0}")]
    ReservedRuntimeKey(String),
    #[error("repository must use the configured Cloudflare R2 S3 endpoint: {0}")]
    UnsupportedRepository(String),
    #[error("repository does not use configured R2 bucket {expected}")]
    RepositoryBucketMismatch { expected: String },
    #[error("repository must contain a non-empty safe prefix below the configured R2 bucket: {0}")]
    UnsafeRepositoryPrefix(String),
    #[error("invalid R2 temporary credential prefix: {0}")]
    InvalidPrefix(String),
}

#[cfg(test)]
mod tests {
    use super::repository_prefix;

    #[test]
    fn extracts_scoped_prefix_from_restic_repository() {
        let result = repository_prefix(
            "s3:https://0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com/mcserver/server-1/restic",
            "0123456789abcdef0123456789abcdef",
            "mcserver",
        );
        assert_eq!(result.as_deref().ok(), Some("server-1/restic/"));
    }

    #[test]
    fn rejects_cross_bucket_and_bucket_root_repositories() {
        assert!(
            repository_prefix(
                "s3:https://0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com/other/server-1",
                "0123456789abcdef0123456789abcdef",
                "mcserver",
            )
            .is_err()
        );
        assert!(
            repository_prefix(
                "s3:https://0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com/mcserver",
                "0123456789abcdef0123456789abcdef",
                "mcserver",
            )
            .is_err()
        );
    }
}
