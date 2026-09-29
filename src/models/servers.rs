use crate::models::{cvmfs_status_json::StatusJSON, repositories_json::RepositoriesJSON};
use crate::transport::ScrapeClient;
use crate::utilities::generate_random_string;
use crate::{
    CVMFSScraperError, ConfigurationError, ContactMetadata, GenericError, GeoapiOrdering,
    GeoapiOutcome, GeoapiProbe, GeoapiServerQuery, GeoapiSkipReason, Hostname, Manifest,
    ReportedTimestamp, RepositoryManifest, RepositoryName, Revision, ScrapeError, ScrapeOptions,
    Scraper, ScraperCommon, ServerEndpoint,
};
use futures::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::future::Future;
use tokio::time::{timeout_at, Instant};

/// Server role used to check whether a CVMFS index contains primaries or replicas.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Copy)]
pub enum ServerType {
    /// Origin server holding the primary repository data; its index must not list replicas.
    Stratum0,
    /// Replica server holding copies of Stratum0 repositories.
    Stratum1,
    /// Synchronization server holding replicated data without being a Stratum1 server.
    SyncServer,
}

/// Requested backend policy. AutoDetect falls back only on index HTTP 404.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Copy, Default)]
pub enum ServerBackendType {
    /// Skip index discovery and fetch explicitly selected repositories from an S3 backend.
    S3,
    /// Require a valid `repositories.json` index consistent with the configured role.
    CVMFS,
    /// Use a valid index when available; only an index HTTP 404 permits an S3 assumption.
    /// Other HTTP, transport, and parsing failures remain errors.
    #[default]
    AutoDetect,
}

/// Evidence for the backend choice. HTTP 404 is an assumption, not proof of S3.
#[derive(Debug, Serialize, Clone, PartialEq, Eq, Copy)]
pub enum BackendResolution {
    /// The caller selected S3 explicitly.
    ConfiguredS3,
    /// The caller selected CVMFS and its index passed validation.
    ConfiguredCvmfs,
    /// Autodetection obtained and validated a CVMFS index.
    DiscoveredCvmfs,
    /// Autodetection received an index HTTP 404 and used configured repository names.
    AssumedS3IndexNotFound,
}
impl BackendResolution {
    pub fn is_s3(self) -> bool {
        matches!(self, Self::ConfiguredS3 | Self::AssumedS3IndexNotFound)
    }
}

/// Configuration of a CVMFS server: its role, requested backend, and HTTP(S) origin.
///
/// Construction does not fetch metadata. [`Self::scrape`] returns a
/// [`ScrapedServer`] containing either read-only results or the original
/// configuration together with an error. Use [`Scraper`] to share a connection
/// pool and request budget across multiple servers or repeated runs.
/// JSON accepts either `endpoint` or legacy `hostname` (converted to HTTP), but
/// not both. Serialization always emits the canonical `endpoint` form.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(try_from = "ServerConfig")]
pub struct Server {
    server_type: ServerType,
    backend_type: ServerBackendType,
    endpoint: ServerEndpoint,
}

#[derive(Deserialize)]
struct ServerConfig {
    server_type: ServerType,
    #[serde(default)]
    backend_type: ServerBackendType,
    endpoint: Option<ServerEndpoint>,
    hostname: Option<Hostname>,
}
impl TryFrom<ServerConfig> for Server {
    type Error = ConfigurationError;

    fn try_from(config: ServerConfig) -> Result<Self, Self::Error> {
        let endpoint = match (config.endpoint, config.hostname) {
            (Some(endpoint), None) => endpoint,
            (None, Some(hostname)) => format!("http://{hostname}").parse()?,
            _ => {
                return Err(ConfigurationError {
                    field: "server address",
                    reason: "specify exactly one of endpoint or legacy hostname".into(),
                })
            }
        };
        Ok(Self::new(config.server_type, config.backend_type, endpoint))
    }
}

// Every stage shares the server's absolute deadline. Check before polling so
// queued repositories and probes cannot start requests after it has expired.
async fn before_deadline<T, E: From<ScrapeError>>(
    deadline: Instant,
    context: &str,
    future: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    if Instant::now() >= deadline {
        return Err(ScrapeError::Timeout(context.into()).into());
    }
    timeout_at(deadline, future)
        .await
        .unwrap_or_else(|_| Err(ScrapeError::Timeout(context.into()).into()))
}

impl Server {
    /// Combine a server role and backend policy with a validated endpoint.
    pub fn new(
        server_type: ServerType,
        backend_type: ServerBackendType,
        endpoint: ServerEndpoint,
    ) -> Self {
        Self {
            server_type,
            backend_type,
            endpoint,
        }
    }
    /// Configured server role, checked against discovery data when available.
    pub fn server_type(&self) -> ServerType {
        self.server_type
    }
    /// Requested backend policy; see [`ServerReport::backend`] for the outcome.
    pub fn backend_type(&self) -> ServerBackendType {
        self.backend_type
    }
    /// Configured HTTP(S) origin, including any explicit port.
    pub fn endpoint(&self) -> &ServerEndpoint {
        &self.endpoint
    }
    /// Host component of the origin, without its scheme or port; may be an IP address.
    pub fn hostname(&self) -> &str {
        self.endpoint.host()
    }
    fn failed(&self, error: CVMFSScraperError) -> ScrapedServer {
        ScrapedServer::Failed(FailedServer {
            server: self.clone(),
            error,
        })
    }

    /// Convenience entry point using exactly the builder's validation and transport.
    ///
    /// Fetches the selected repositories' status files and manifests, plus optional
    /// contact metadata and GeoAPI results. Options control selection, probe hosts,
    /// deadlines, response sizes, and concurrency. Configuration and discovery
    /// errors produce [`ScrapedServer::Failed`]; repository and optional-probe
    /// failures are retained in [`ServerReport`]. Each call creates a new client; use a
    /// validated [`Scraper`] when the connection pool should be reused across calls.
    ///
    /// ```no_run
    /// use cvmfs_server_scraper::{RepositorySelection, ScrapeOptions, Server, ServerBackendType, ServerType};
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let server = Server::new(ServerType::SyncServer, ServerBackendType::S3,
    ///     "http://aws-eu-west-s1-sync.eessi.science".parse()?);
    /// let options = ScrapeOptions::default().with_selection(
    ///     RepositorySelection::only(["software.eessi.io".parse()?]));
    /// let result = server.scrape(options).await;
    /// # let _ = result;
    /// # Ok(()) }
    /// ```
    pub async fn scrape(&self, options: ScrapeOptions) -> ScrapedServer {
        match Scraper::new()
            .options(options)
            .with_servers(vec![self.clone()])
            .validate()
        {
            Ok(scraper) => scraper
                .scrape()
                .await
                .pop()
                .expect("one configured server yields one result"),
            Err(error) => self.failed(error.into()),
        }
    }
    pub(crate) async fn scrape_with(
        &self,
        client: &ScrapeClient,
        options: &ScrapeOptions,
    ) -> ScrapedServer {
        log::debug!("Scraping {}", self.endpoint);
        let deadline = Instant::now() + options.limits().server_timeout().get();
        match self.try_scrape(client, options, deadline).await {
            Ok(server) => ScrapedServer::Collected(Box::new(server)),
            Err(error) => self.failed(error),
        }
    }
    async fn try_scrape(
        &self,
        client: &ScrapeClient,
        options: &ScrapeOptions,
        deadline: Instant,
    ) -> Result<ServerReport, CVMFSScraperError> {
        let (backend, index) = match self.backend_type {
            ServerBackendType::S3 => (BackendResolution::ConfiguredS3, None),
            ServerBackendType::CVMFS | ServerBackendType::AutoDetect => {
                match before_deadline(
                    deadline,
                    &self.endpoint.to_string(),
                    client.json::<RepositoriesJSON>(&self.endpoint.index()),
                )
                .await
                {
                    Ok(index) => {
                        self.validate_index(&index, options)?;
                        let resolution = if self.backend_type == ServerBackendType::CVMFS {
                            BackendResolution::ConfiguredCvmfs
                        } else {
                            BackendResolution::DiscoveredCvmfs
                        };
                        (resolution, Some(index))
                    }
                    Err(error)
                        if self.backend_type == ServerBackendType::AutoDetect
                            && error.is_not_found() =>
                    {
                        (BackendResolution::AssumedS3IndexNotFound, None)
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        };
        let (metadata, discovered) = match index {
            Some(index) => {
                let metadata = ServerMetadata {
                    schema_version: Some(index.schema),
                    cvmfs_version: index.cvmfs_version,
                    last_geodb_update: index.last_geodb_update,
                    os_id: index.os_id,
                    os_version_id: index.os_version_id,
                    os_pretty_name: index.os_pretty_name,
                };
                let discovered = index
                    .repositories
                    .into_iter()
                    .chain(index.replicas)
                    .map(|r| r.name)
                    .collect::<Vec<_>>();
                (metadata, discovered)
            }
            None => (ServerMetadata::default(), Vec::new()),
        };
        let names = options.selection().resolve(discovered);
        let limit = options.limits().repository_count().get();
        if names.len() > limit {
            return Err(ScrapeError::RepositoryLimit {
                actual: names.len(),
                limit,
            }
            .into());
        }
        if backend.is_s3() && names.is_empty() {
            return Err(ScrapeError::EmptyRepositoryList(self.endpoint.to_string()).into());
        }
        let mut jobs = stream::iter(names)
            .map(|name| async move {
                let result = before_deadline(
                    deadline,
                    &self.endpoint.to_string(),
                    self.scrape_repository(client, &name),
                )
                .await;
                result.map_err(|error| FailedRepository { name, error })
            })
            .buffer_unordered(options.limits().repositories().get());
        let mut repositories = Vec::new();
        let mut failed_repositories = Vec::new();
        while let Some(result) = jobs.next().await {
            match result {
                Ok(repository) => repositories.push(repository),
                Err(failure) => failed_repositories.push(failure),
            }
        }
        repositories.sort_unstable_by(|a, b| a.name().cmp(b.name()));
        failed_repositories.sort_unstable_by(|a, b| a.name().cmp(b.name()));
        let contact_future = async {
            let endpoint = self.endpoint.metadata();
            match before_deadline(
                deadline,
                endpoint.url().as_str(),
                client.json::<ContactMetadata>(&endpoint),
            )
            .await
            {
                Ok(value) => OptionalFetch::Available(value),
                Err(error) if error.is_not_found() => OptionalFetch::Absent,
                Err(error) => OptionalFetch::Failed(error),
            }
        };
        let (contact, geoapi) = futures::join!(
            contact_future,
            self.fetch_geoapi(client, options, backend, repositories.first(), deadline)
        );
        Ok(ServerReport {
            server: self.clone(),
            backend,
            repositories,
            failed_repositories,
            metadata,
            contact,
            geoapi,
        })
    }
    fn validate_index(
        &self,
        index: &RepositoriesJSON,
        options: &ScrapeOptions,
    ) -> Result<(), ScrapeError> {
        if index.schema != 1 {
            return Err(ScrapeError::UnsupportedSchema(index.schema));
        }
        let count = index
            .repositories
            .len()
            .saturating_add(index.replicas.len());
        let limit = options.limits().repository_count().get();
        if count > limit {
            return Err(ScrapeError::RepositoryLimit {
                actual: count,
                limit,
            });
        }
        let mismatch = match self.server_type {
            ServerType::Stratum0 if !index.replicas.is_empty() => {
                Some("Stratum0 index contains replicas")
            }
            ServerType::Stratum1 | ServerType::SyncServer if index.replicas.is_empty() => {
                Some("replica server index contains no replicas")
            }
            ServerType::Stratum1 | ServerType::SyncServer if !index.repositories.is_empty() => {
                Some("replica server index also contains primary repositories")
            }
            _ => None,
        };
        if let Some(reason) = mismatch {
            return Err(ScrapeError::ServerTypeMismatch(format!(
                "{}: {reason}",
                self.endpoint
            )));
        }
        Ok(())
    }
    async fn scrape_repository(
        &self,
        client: &ScrapeClient,
        name: &RepositoryName,
    ) -> Result<PopulatedRepositoryOrReplica, CVMFSScraperError> {
        let repository = self.endpoint.repository(name);
        let status_endpoint = repository.status();
        let manifest_endpoint = repository.manifest();
        let (status, bytes) = futures::try_join!(
            client.json::<StatusJSON>(&status_endpoint),
            client.bytes(&manifest_endpoint)
        )?;
        let manifest = Manifest::from_bytes(&bytes)
            .and_then(|m| m.bind_to_repository(name.clone()))
            .map_err(|source| CVMFSScraperError::Manifest {
                url: manifest_endpoint.url().to_string(),
                source,
            })?;
        Ok(PopulatedRepositoryOrReplica {
            manifest,
            last_snapshot: status.last_snapshot,
            last_gc: status.last_gc,
        })
    }
    async fn fetch_geoapi(
        &self,
        client: &ScrapeClient,
        options: &ScrapeOptions,
        backend: BackendResolution,
        repository: Option<&PopulatedRepositoryOrReplica>,
        deadline: Instant,
    ) -> GeoapiOutcome {
        let hosts = match options.geoapi() {
            GeoapiProbe::Disabled => return GeoapiOutcome::Skipped(GeoapiSkipReason::Disabled),
            GeoapiProbe::Enabled(hosts) => hosts,
        };
        if backend.is_s3() {
            return GeoapiOutcome::Unsupported;
        }
        if self.server_type == ServerType::Stratum0 {
            return GeoapiOutcome::Skipped(GeoapiSkipReason::Stratum0);
        }
        let Some(repository) = repository else {
            return GeoapiOutcome::Skipped(GeoapiSkipReason::NoRepositories);
        };
        let endpoint = self
            .endpoint
            .repository(repository.name())
            .geoapi(&generate_random_string(12), hosts.as_slice());
        let result = before_deadline(deadline, endpoint.url().as_str(), async {
            let bytes = client.bytes(&endpoint).await?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|e| ScrapeError::GeoAPIFailure(format!("{}: {e}", endpoint.url())))?;
            let ordering = GeoapiOrdering::from_response(hosts.as_slice().to_vec(), text)
                .map_err(|e| ScrapeError::GeoAPIFailure(format!("{}: {e}", endpoint.url())))?;
            Ok(GeoapiServerQuery::new(self.endpoint.clone(), ordering))
        })
        .await;
        match result {
            Ok(query) => GeoapiOutcome::Available(query),
            Err(error) => GeoapiOutcome::Failed(error),
        }
    }
}

/// Outcome of an optional resource fetch, preserving absence separately from failure.
#[derive(Debug, Clone)]
pub enum OptionalFetch<T> {
    /// The resource was fetched and parsed successfully.
    Available(T),
    /// The endpoint returned HTTP 404.
    Absent,
    /// Fetching or parsing failed for another reason.
    Failed(ScrapeError),
}

/// Repository collection outcome, independent of contact metadata and GeoAPI.
///
/// Obtained from [`ServerReport::repository_outcome`]. Configuration and
/// discovery failures instead produce [`ScrapedServer::Failed`] without a report.
/// This enum does not describe the overall health of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryOutcome {
    /// At least one repository was selected, and every selected repository succeeded.
    Complete,
    /// Some selected repositories succeeded and some failed.
    Partial,
    /// At least one repository was selected, and every selected repository failed.
    AllFailed,
    /// The effective selection contained no repositories; none were fetched.
    Empty,
}

/// Read-only results after successful discovery/selection, including repository
/// failures. Inspect [`Self::repository_outcome`] and ancillary contact/GeoAPI
/// outcomes before treating the whole server as healthy. A report can contain
/// zero successful repositories; [`RepositoryOutcome::Empty`] distinguishes an
/// empty selection from [`RepositoryOutcome::AllFailed`].
///
/// Created only by a completed scrape. Primary repositories and replicas share
/// [`Self::repositories`], sorted by name. The configured role determines which
/// index entries are accepted. [`Self::metadata`] comes from the repository index;
/// [`Self::contact`] independently describes the optional `meta.json` fetch.
/// Index metadata is absent on S3, but contact metadata is still requested.
#[derive(Debug, Clone)]
pub struct ServerReport {
    server: Server,
    backend: BackendResolution,
    repositories: Vec<PopulatedRepositoryOrReplica>,
    failed_repositories: Vec<FailedRepository>,
    metadata: ServerMetadata,
    contact: OptionalFetch<ContactMetadata>,
    geoapi: GeoapiOutcome,
}
impl ServerReport {
    /// Summarize the selected repositories, including failures and timeouts.
    /// Computed from the immutable results so it cannot disagree with them.
    /// Optional-probe outcomes do not affect this value.
    ///
    /// ```
    /// use cvmfs_server_scraper::{RepositoryOutcome, ServerReport};
    ///
    /// fn describe(report: &ServerReport) -> &'static str {
    ///     match report.repository_outcome() {
    ///         RepositoryOutcome::Complete => "Every selected repository succeeded",
    ///         RepositoryOutcome::Partial => "Some repositories failed; successes remain available",
    ///         RepositoryOutcome::AllFailed => "Every selected repository failed",
    ///         RepositoryOutcome::Empty => "No repositories were selected",
    ///     }
    /// }
    /// ```
    pub fn repository_outcome(&self) -> RepositoryOutcome {
        match (
            self.repositories.is_empty(),
            self.failed_repositories.is_empty(),
        ) {
            (false, true) => RepositoryOutcome::Complete,
            (false, false) => RepositoryOutcome::Partial,
            (true, false) => RepositoryOutcome::AllFailed,
            (true, true) => RepositoryOutcome::Empty,
        }
    }
    /// Original server configuration, including the requested backend policy.
    pub fn server(&self) -> &Server {
        &self.server
    }
    /// Host component of the original server endpoint.
    pub fn hostname(&self) -> &str {
        self.server.hostname()
    }
    /// Resolved backend and the evidence or configuration behind that choice.
    pub fn backend(&self) -> BackendResolution {
        self.backend
    }
    /// Successful repository/replica results in lexical name order.
    pub fn repositories(&self) -> &[PopulatedRepositoryOrReplica] {
        &self.repositories
    }
    /// Failed repository/replica results in lexical name order. Together with
    /// [`Self::repositories`], accounts for every selected name exactly once.
    /// Includes queued and active repositories that reached the server deadline.
    pub fn failed_repositories(&self) -> &[FailedRepository] {
        &self.failed_repositories
    }
    /// Metadata obtained from the repository index, if this backend supplied one.
    pub fn metadata(&self) -> &ServerMetadata {
        &self.metadata
    }
    /// Independent outcome of fetching optional server contact metadata.
    pub fn contact(&self) -> &OptionalFetch<ContactMetadata> {
        &self.contact
    }
    /// Available ordering, skip/unsupported reason, or optional probe error.
    pub fn geoapi(&self) -> &GeoapiOutcome {
        &self.geoapi
    }
    /// Whether the completed results contain the given repository or replica.
    pub fn has_repository(&self, name: &RepositoryName) -> bool {
        self.repositories.iter().any(|r| r.name() == name)
    }
    pub fn output(&self) {
        println!("{self:#?}");
    }
}
impl std::fmt::Display for ServerReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({:?}, {:?})",
            self.server.endpoint, self.server.server_type, self.backend
        )
    }
}

/// A failed configuration or discovery/selection step with the original server
/// configuration and its error. Once repositories are selected, their failures
/// and timeouts stay on [`ServerReport`], alongside any successful results.
#[derive(Debug, Clone)]
pub struct FailedServer {
    server: Server,
    error: CVMFSScraperError,
}
impl FailedServer {
    pub fn server(&self) -> &Server {
        &self.server
    }
    pub fn hostname(&self) -> &str {
        self.server.hostname()
    }
    pub fn error(&self) -> &CVMFSScraperError {
        &self.error
    }
}
/// Result of scraping one configured server.
///
/// `Collected` means a report is available, including when every selected
/// repository failed. Match this enum, then inspect
/// [`ServerReport::repository_outcome`] and the optional-probe outcomes separately.
#[derive(Debug, Clone)]
pub enum ScrapedServer {
    /// Discovery/selection succeeded. Repository and ancillary failures are
    /// retained alongside successes, even when every selected repository failed.
    Collected(Box<ServerReport>),
    /// Configuration or discovery/selection failed before repository collection.
    Failed(FailedServer),
}
impl ScrapedServer {
    /// Borrow the collection report, regardless of its repository/probe outcomes.
    pub fn as_report(&self) -> Option<&ServerReport> {
        match self {
            Self::Collected(value) => Some(value),
            _ => None,
        }
    }
    /// Borrow a failure that prevented repository collection.
    pub fn as_failed_server(&self) -> Option<&FailedServer> {
        match self {
            Self::Failed(value) => Some(value),
            _ => None,
        }
    }
    /// Extract the collection report. `Ok` means a report exists, not that its
    /// repositories or optional probes all succeeded; inspect their outcomes.
    pub fn into_report(self) -> Result<ServerReport, GenericError> {
        match self {
            Self::Collected(value) => Ok(*value),
            Self::Failed(value) => Err(GenericError::TypeError(format!(
                "{}: {}",
                value.hostname(),
                value.error()
            ))),
        }
    }
    pub fn into_failed_server(self) -> Result<FailedServer, GenericError> {
        match self {
            Self::Failed(value) => Ok(value),
            Self::Collected(value) => Err(GenericError::TypeError(format!(
                "{} has a collection report",
                value.hostname()
            ))),
        }
    }
    pub fn get_failed_server(self) -> Result<FailedServer, GenericError> {
        self.into_failed_server()
    }
}

/// Failure of one selected repository. Other repositories continue independently.
#[derive(Debug, Clone)]
pub struct FailedRepository {
    name: RepositoryName,
    error: CVMFSScraperError,
}
impl FailedRepository {
    pub fn name(&self) -> &RepositoryName {
        &self.name
    }
    pub fn error(&self) -> &CVMFSScraperError {
        &self.error
    }
}

/// Server metadata from `cvmfs/info/v1/repositories.json`.
///
/// Fields are optional because an S3 backend has no index and CVMFS servers may
/// omit version or OS details, including for privacy reasons. The schema is present
/// for a successfully validated CVMFS index. Contact information from `meta.json`
/// is kept separately in [`ServerReport::contact`].
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ServerMetadata {
    schema_version: Option<u32>,
    cvmfs_version: Option<semver::Version>,
    last_geodb_update: Option<ReportedTimestamp>,
    os_version_id: Option<String>,
    os_pretty_name: Option<String>,
    os_id: Option<String>,
}
impl ServerMetadata {
    /// Schema of the validated index, currently 1; absent without an index.
    pub fn schema_version(&self) -> Option<u32> {
        self.schema_version
    }
    /// CVMFS server version reported by the index.
    pub fn cvmfs_version(&self) -> Option<&semver::Version> {
        self.cvmfs_version.as_ref()
    }
    /// Reported GeoIP database update time; raw text remains available if unparseable.
    pub fn last_geodb_update(&self) -> Option<&ReportedTimestamp> {
        self.last_geodb_update.as_ref()
    }
    /// Reported operating-system version, such as `9.4`.
    pub fn os_version_id(&self) -> Option<&str> {
        self.os_version_id.as_deref()
    }
    /// Human-readable operating-system name.
    pub fn os_pretty_name(&self) -> Option<&str> {
        self.os_pretty_name.as_deref()
    }
    /// Operating-system identifier, such as `rhel`.
    pub fn os_id(&self) -> Option<&str> {
        self.os_id.as_deref()
    }
    pub fn output(&self) {
        println!("{self:#?}");
    }
}

/// Bound repository result; only a completed scrape can construct this value.
///
/// Combines `.cvmfspublished` with optional snapshot and garbage-collection times
/// from `.cvmfs_status.json`. Repositories and replicas use the same representation.
/// [`Self::revision`] is a shortcut to the manifest's revision. Timestamp absence
/// is distinct from a present but unparseable [`ReportedTimestamp`].
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct PopulatedRepositoryOrReplica {
    manifest: RepositoryManifest,
    last_snapshot: Option<ReportedTimestamp>,
    last_gc: Option<ReportedTimestamp>,
}
impl PopulatedRepositoryOrReplica {
    /// Validated repository name used for the request and manifest binding.
    pub fn name(&self) -> &RepositoryName {
        self.manifest.repository_name()
    }
    /// Parsed manifest bound to the requested repository, without signature verification.
    pub fn manifest(&self) -> &RepositoryManifest {
        &self.manifest
    }
    /// Last snapshot time reported by the status file, if supplied.
    pub fn last_snapshot(&self) -> Option<&ReportedTimestamp> {
        self.last_snapshot.as_ref()
    }
    /// Last garbage-collection time reported by the status file, if supplied.
    pub fn last_gc(&self) -> Option<&ReportedTimestamp> {
        self.last_gc.as_ref()
    }
    /// Revision number from the bound manifest.
    pub fn revision(&self) -> Revision {
        self.manifest.manifest().revision()
    }
    pub fn output(&self) {
        println!("{self:#?}");
    }
}
