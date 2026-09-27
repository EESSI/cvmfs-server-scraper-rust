use crate::{ConfigurationError, Hostname, RepositoryName, ScrapeError};
use reqwest::{redirect, Client, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fmt, str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

/// A validated HTTP(S) origin, including an optional port. Contains no credentials,
/// query, fragment, or base path. This is syntax validation, not egress authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ServerEndpoint(Url);
impl FromStr for ServerEndpoint {
    type Err = ConfigurationError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let invalid = |reason: &str| ConfigurationError {
            field: "server endpoint",
            reason: reason.into(),
        };
        if input.trim() != input || input.bytes().any(|b| b.is_ascii_control()) {
            return Err(invalid("whitespace and control characters are not allowed"));
        }
        let url = Url::parse(input).map_err(|e| invalid(&e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(invalid("expected an absolute http:// or https:// origin"));
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(invalid("credentials, paths, queries and fragments are not allowed; specify only scheme, host and optional port"));
        }
        // URL parsing canonicalizes IDNA and IP addresses. DNS names still pass
        // the same hostname constructor as configuration and GeoAPI inputs.
        if let Some(reqwest_host) = url.host_str() {
            if reqwest_host.parse::<std::net::IpAddr>().is_err()
                && !(reqwest_host.starts_with('[') && reqwest_host.ends_with(']'))
            {
                reqwest_host
                    .parse::<Hostname>()
                    .map_err(|e| invalid(&e.to_string()))?;
            }
        }
        Ok(Self(url))
    }
}
impl TryFrom<String> for ServerEndpoint {
    type Error = ConfigurationError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}
impl From<ServerEndpoint> for String {
    fn from(value: ServerEndpoint) -> Self {
        value.0.to_string()
    }
}
impl fmt::Display for ServerEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl ServerEndpoint {
    pub fn as_url(&self) -> &Url {
        &self.0
    }
    pub fn host(&self) -> &str {
        self.0.host_str().expect("validated HTTP origin has a host")
    }
    pub(crate) fn index(&self) -> ResourceEndpoint {
        self.resource(
            &["cvmfs", "info", "v1", "repositories.json"],
            ResourceKind::Index,
        )
    }
    pub(crate) fn metadata(&self) -> ResourceEndpoint {
        self.resource(
            &["cvmfs", "info", "v1", "meta.json"],
            ResourceKind::Metadata,
        )
    }
    pub(crate) fn repository(&self, name: &RepositoryName) -> RepositoryEndpoint {
        RepositoryEndpoint {
            server: self.clone(),
            name: name.clone(),
        }
    }
    fn resource(&self, segments: &[&str], kind: ResourceKind) -> ResourceEndpoint {
        let mut url = self.0.clone();
        url.path_segments_mut()
            .expect("validated HTTP origin is a base URL")
            .clear()
            .extend(segments);
        ResourceEndpoint { url, kind }
    }
}
#[derive(Clone)]
pub(crate) struct RepositoryEndpoint {
    server: ServerEndpoint,
    name: RepositoryName,
}
impl RepositoryEndpoint {
    pub fn manifest(&self) -> ResourceEndpoint {
        self.server.resource(
            &["cvmfs", self.name.as_str(), ".cvmfspublished"],
            ResourceKind::Manifest,
        )
    }
    pub fn status(&self) -> ResourceEndpoint {
        self.server.resource(
            &["cvmfs", self.name.as_str(), ".cvmfs_status.json"],
            ResourceKind::Metadata,
        )
    }
    pub fn geoapi(&self, nonce: &str, hosts: &[Hostname]) -> ResourceEndpoint {
        let hosts = hosts
            .iter()
            .map(Hostname::as_str)
            .collect::<Vec<_>>()
            .join(",");
        self.server.resource(
            &[
                "cvmfs",
                self.name.as_str(),
                "api",
                "v1.0",
                "geo",
                nonce,
                &hosts,
            ],
            ResourceKind::Geoapi,
        )
    }
}
#[derive(Clone, Copy)]
enum ResourceKind {
    Index,
    Metadata,
    Manifest,
    Geoapi,
}
pub(crate) struct ResourceEndpoint {
    url: Url,
    kind: ResourceKind,
}
impl ResourceEndpoint {
    pub fn url(&self) -> &Url {
        &self.url
    }
}

macro_rules! bounded_limit {
    ($name:ident, $max:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(usize);
        impl $name {
            pub fn new(value: usize) -> Result<Self, ConfigurationError> {
                if value == 0 || value > $max {
                    return Err(ConfigurationError {
                        field: stringify!($name),
                        reason: format!("expected 1..={}, got {value}", $max),
                    });
                }
                Ok(Self(value))
            }
            pub fn get(self) -> usize {
                self.0
            }
        }
    };
}
bounded_limit!(ResponseByteLimit, 64 * 1024 * 1024);
bounded_limit!(ConcurrencyLimit, 1024);
bounded_limit!(RepositoryLimit, 100_000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestTimeout(Duration);
impl RequestTimeout {
    pub fn new(value: Duration) -> Result<Self, ConfigurationError> {
        if value.is_zero() || value > Duration::from_secs(3600) {
            return Err(ConfigurationError {
                field: "RequestTimeout",
                reason: "expected a positive duration of at most one hour".into(),
            });
        }
        Ok(Self(value))
    }
    pub fn get(self) -> Duration {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RedirectPolicy {
    /// Reject redirects, preserving the configured origin.
    #[default]
    None,
    /// At most five redirects within the exact original scheme/host/port.
    SameOrigin,
}

/// Bounded transport and scheduling settings. Changes require checked scalar types.
#[derive(Debug, Clone)]
pub struct ScrapeLimits {
    connect_timeout: RequestTimeout,
    read_timeout: RequestTimeout,
    request_timeout: RequestTimeout,
    server_timeout: RequestTimeout,
    index_bytes: ResponseByteLimit,
    metadata_bytes: ResponseByteLimit,
    manifest_bytes: ResponseByteLimit,
    geoapi_bytes: ResponseByteLimit,
    servers: ConcurrencyLimit,
    repositories: ConcurrencyLimit,
    requests: ConcurrencyLimit,
    repository_count: RepositoryLimit,
}
impl Default for ScrapeLimits {
    fn default() -> Self {
        Self {
            connect_timeout: RequestTimeout(Duration::from_secs(5)),
            read_timeout: RequestTimeout(Duration::from_secs(10)),
            request_timeout: RequestTimeout(Duration::from_secs(30)),
            server_timeout: RequestTimeout(Duration::from_secs(300)),
            index_bytes: ResponseByteLimit(2 * 1024 * 1024),
            metadata_bytes: ResponseByteLimit(256 * 1024),
            manifest_bytes: ResponseByteLimit(1024 * 1024),
            geoapi_bytes: ResponseByteLimit(16 * 1024),
            servers: ConcurrencyLimit(8),
            repositories: ConcurrencyLimit(4),
            requests: ConcurrencyLimit(32),
            repository_count: RepositoryLimit(10_000),
        }
    }
}
macro_rules! limit_methods {
    ($($name:ident, $with:ident, $ty:ty;)+) => { $(
        pub fn $name(&self) -> $ty { self.$name }
        pub fn $with(mut self, value: $ty) -> Self { self.$name = value; self }
    )+ };
}
impl ScrapeLimits {
    limit_methods! {
        connect_timeout, with_connect_timeout, RequestTimeout;
        read_timeout, with_read_timeout, RequestTimeout;
        request_timeout, with_request_timeout, RequestTimeout;
        server_timeout, with_server_timeout, RequestTimeout;
        index_bytes, with_index_bytes, ResponseByteLimit;
        metadata_bytes, with_metadata_bytes, ResponseByteLimit;
        manifest_bytes, with_manifest_bytes, ResponseByteLimit;
        geoapi_bytes, with_geoapi_bytes, ResponseByteLimit;
        servers, with_servers, ConcurrencyLimit;
        repositories, with_repositories, ConcurrencyLimit;
        requests, with_requests, ConcurrencyLimit;
        repository_count, with_repository_count, RepositoryLimit;
    }
    fn byte_limit(&self, kind: ResourceKind) -> usize {
        match kind {
            ResourceKind::Index => self.index_bytes.get(),
            ResourceKind::Metadata => self.metadata_bytes.get(),
            ResourceKind::Manifest => self
                .manifest_bytes
                .get()
                .min(crate::models::MAX_MANIFEST_BYTES),
            ResourceKind::Geoapi => self.geoapi_bytes.get(),
        }
    }
}

/// Shared connection pool and global request budget. Kept private so callers
/// cannot inject a client that bypasses the validated transport policy.
#[derive(Clone)]
pub(crate) struct ScrapeClient {
    client: Client,
    limits: ScrapeLimits,
    permits: Arc<Semaphore>,
}
impl ScrapeClient {
    pub fn new(limits: ScrapeLimits, redirects: RedirectPolicy) -> Result<Self, ScrapeError> {
        let policy = match redirects {
            RedirectPolicy::None => redirect::Policy::none(),
            RedirectPolicy::SameOrigin => redirect::Policy::custom(|attempt| {
                if attempt.previous().len() > 5 {
                    return attempt.error("redirect limit exceeded");
                }
                if attempt
                    .previous()
                    .first()
                    .is_some_and(|first| first.origin() != attempt.url().origin())
                {
                    attempt.error("redirect would leave the configured origin")
                } else {
                    attempt.follow()
                }
            }),
        };
        let client = Client::builder()
            .connect_timeout(limits.connect_timeout.get())
            .read_timeout(limits.read_timeout.get())
            .timeout(limits.request_timeout.get())
            .redirect(policy)
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|source| ScrapeError::Fetch {
                url: "HTTP client initialization".into(),
                source: Arc::new(source),
            })?;
        let permits = Arc::new(Semaphore::new(limits.requests.get()));
        Ok(Self {
            client,
            limits,
            permits,
        })
    }
    pub async fn bytes(&self, endpoint: &ResourceEndpoint) -> Result<Vec<u8>, ScrapeError> {
        let url = endpoint.url.to_string();
        let failure = |source| ScrapeError::Fetch {
            url: url.clone(),
            source: Arc::new(source),
        };
        let _permit = self
            .permits
            .acquire()
            .await
            .expect("private semaphore is never closed");
        let mut response = self
            .client
            .get(endpoint.url.clone())
            .send()
            .await
            .map_err(failure)?;
        if !response.status().is_success() {
            return Err(ScrapeError::HttpStatus {
                url,
                status: response.status().as_u16(),
            });
        }
        let limit = self.limits.byte_limit(endpoint.kind);
        let oversized = || ScrapeError::BodyTooLarge {
            url: url.clone(),
            limit,
        };
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(oversized());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(failure)? {
            if chunk.len() > limit - body.len() {
                return Err(oversized());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
    pub async fn json<T: DeserializeOwned>(
        &self,
        endpoint: &ResourceEndpoint,
    ) -> Result<T, ScrapeError> {
        let bytes = self.bytes(endpoint).await?;
        serde_json::from_slice(&bytes).map_err(|source| ScrapeError::Json {
            url: endpoint.url.to_string(),
            source: Arc::new(source),
        })
    }
}
