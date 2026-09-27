use crate::models::{cvmfs_status_json::StatusJSON, repositories_json::RepositoriesJSON};
use crate::transport::ScrapeClient;
use crate::utilities::generate_random_string;
use crate::{
    CVMFSScraperError, ContactMetadata, GenericError, GeoapiOrdering, GeoapiOutcome, GeoapiProbe,
    GeoapiServerQuery, GeoapiSkipReason, Manifest, ReportedTimestamp, RepositoryManifest,
    RepositoryName, Revision, ScrapeError, ScrapeOptions, Scraper, ScraperCommon, ServerEndpoint,
};
use futures::{stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Copy)]
pub enum ServerType {
    Stratum0,
    Stratum1,
    SyncServer,
}

/// Requested backend policy. AutoDetect falls back only on index HTTP 404.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Copy, Default)]
pub enum ServerBackendType {
    S3,
    CVMFS,
    #[default]
    AutoDetect,
}

/// Evidence for the backend choice. HTTP 404 is an assumption, not proof of S3.
#[derive(Debug, Serialize, Clone, PartialEq, Eq, Copy)]
pub enum BackendResolution {
    ConfiguredS3,
    ConfiguredCvmfs,
    DiscoveredCvmfs,
    AssumedS3IndexNotFound,
}
impl BackendResolution {
    pub fn is_s3(self) -> bool {
        matches!(self, Self::ConfiguredS3 | Self::AssumedS3IndexNotFound)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Server {
    server_type: ServerType,
    #[serde(default)]
    backend_type: ServerBackendType,
    endpoint: ServerEndpoint,
}
impl Server {
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
    pub fn server_type(&self) -> ServerType {
        self.server_type
    }
    pub fn backend_type(&self) -> ServerBackendType {
        self.backend_type
    }
    pub fn endpoint(&self) -> &ServerEndpoint {
        &self.endpoint
    }
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
        match tokio::time::timeout(
            options.limits().server_timeout().get(),
            self.try_scrape(client, options),
        )
        .await
        {
            Ok(Ok(server)) => ScrapedServer::Populated(Box::new(server)),
            Ok(Err(error)) => self.failed(error),
            Err(_) => self.failed(ScrapeError::Timeout(self.endpoint.to_string()).into()),
        }
    }
    async fn try_scrape(
        &self,
        client: &ScrapeClient,
        options: &ScrapeOptions,
    ) -> Result<PopulatedServer, CVMFSScraperError> {
        let (backend, index) = match self.backend_type {
            ServerBackendType::S3 => (BackendResolution::ConfiguredS3, None),
            ServerBackendType::CVMFS | ServerBackendType::AutoDetect => {
                match client
                    .json::<RepositoriesJSON>(&self.endpoint.index())
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
        let mut repositories = stream::iter(names)
            .map(|name| async move { self.scrape_repository(client, &name).await })
            .buffer_unordered(options.limits().repositories().get())
            .try_collect::<Vec<_>>()
            .await?;
        repositories.sort_unstable_by(|a, b| a.name().cmp(b.name()));
        let contact_future = async {
            match client
                .json::<ContactMetadata>(&self.endpoint.metadata())
                .await
            {
                Ok(value) => OptionalFetch::Available(value),
                Err(error) if error.is_not_found() => OptionalFetch::Absent,
                Err(error) => OptionalFetch::Failed(error),
            }
        };
        let (contact, geoapi) = futures::join!(
            contact_future,
            self.fetch_geoapi(client, options, backend, repositories.first())
        );
        Ok(PopulatedServer {
            server: self.clone(),
            backend,
            repositories,
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
        let result = async {
            let bytes = client.bytes(&endpoint).await?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|e| ScrapeError::GeoAPIFailure(format!("{}: {e}", endpoint.url())))?;
            let ordering = GeoapiOrdering::from_response(hosts.as_slice().to_vec(), text)
                .map_err(|e| ScrapeError::GeoAPIFailure(format!("{}: {e}", endpoint.url())))?;
            Ok(GeoapiServerQuery::new(self.endpoint.clone(), ordering))
        }
        .await;
        match result {
            Ok(query) => GeoapiOutcome::Available(query),
            Err(error) => GeoapiOutcome::Failed(error),
        }
    }
}

#[derive(Debug, Clone)]
pub enum OptionalFetch<T> {
    Available(T),
    Absent,
    Failed(ScrapeError),
}

/// Read-only result of the required fetches. Inspect ancillary contact/GeoAPI
/// outcomes before treating the whole server as healthy.
#[derive(Debug, Clone)]
pub struct PopulatedServer {
    server: Server,
    backend: BackendResolution,
    repositories: Vec<PopulatedRepositoryOrReplica>,
    metadata: ServerMetadata,
    contact: OptionalFetch<ContactMetadata>,
    geoapi: GeoapiOutcome,
}
impl PopulatedServer {
    pub fn server(&self) -> &Server {
        &self.server
    }
    pub fn hostname(&self) -> &str {
        self.server.hostname()
    }
    pub fn backend(&self) -> BackendResolution {
        self.backend
    }
    pub fn repositories(&self) -> &[PopulatedRepositoryOrReplica] {
        &self.repositories
    }
    pub fn metadata(&self) -> &ServerMetadata {
        &self.metadata
    }
    pub fn contact(&self) -> &OptionalFetch<ContactMetadata> {
        &self.contact
    }
    pub fn geoapi(&self) -> &GeoapiOutcome {
        &self.geoapi
    }
    pub fn has_repository(&self, name: &RepositoryName) -> bool {
        self.repositories.iter().any(|r| r.name() == name)
    }
    pub fn output(&self) {
        println!("{self:#?}");
    }
}
impl std::fmt::Display for PopulatedServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({:?}, {:?})",
            self.server.endpoint, self.server.server_type, self.backend
        )
    }
}

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
#[derive(Debug, Clone)]
pub enum ScrapedServer {
    Populated(Box<PopulatedServer>),
    Failed(FailedServer),
}
impl ScrapedServer {
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
    pub fn is_populated(&self) -> bool {
        matches!(self, Self::Populated(_))
    }
    pub fn is_ok(&self) -> bool {
        self.is_populated()
    }
    pub fn as_populated_server(&self) -> Option<&PopulatedServer> {
        match self {
            Self::Populated(value) => Some(value),
            _ => None,
        }
    }
    pub fn as_failed_server(&self) -> Option<&FailedServer> {
        match self {
            Self::Failed(value) => Some(value),
            _ => None,
        }
    }
    pub fn into_populated_server(self) -> Result<PopulatedServer, GenericError> {
        match self {
            Self::Populated(value) => Ok(*value),
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
            Self::Populated(value) => Err(GenericError::TypeError(format!(
                "{} is a populated server",
                value.hostname()
            ))),
        }
    }
    pub fn get_populated_server(self) -> Result<PopulatedServer, GenericError> {
        self.into_populated_server()
    }
    pub fn get_failed_server(self) -> Result<FailedServer, GenericError> {
        self.into_failed_server()
    }
}

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
    pub fn schema_version(&self) -> Option<u32> {
        self.schema_version
    }
    pub fn cvmfs_version(&self) -> Option<&semver::Version> {
        self.cvmfs_version.as_ref()
    }
    pub fn last_geodb_update(&self) -> Option<&ReportedTimestamp> {
        self.last_geodb_update.as_ref()
    }
    pub fn os_version_id(&self) -> Option<&str> {
        self.os_version_id.as_deref()
    }
    pub fn os_pretty_name(&self) -> Option<&str> {
        self.os_pretty_name.as_deref()
    }
    pub fn os_id(&self) -> Option<&str> {
        self.os_id.as_deref()
    }
    pub fn output(&self) {
        println!("{self:#?}");
    }
}

/// Bound repository result; only a completed scrape can construct this value.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct PopulatedRepositoryOrReplica {
    manifest: RepositoryManifest,
    last_snapshot: Option<ReportedTimestamp>,
    last_gc: Option<ReportedTimestamp>,
}
impl PopulatedRepositoryOrReplica {
    pub fn name(&self) -> &RepositoryName {
        self.manifest.repository_name()
    }
    pub fn manifest(&self) -> &RepositoryManifest {
        &self.manifest
    }
    pub fn last_snapshot(&self) -> Option<&ReportedTimestamp> {
        self.last_snapshot.as_ref()
    }
    pub fn last_gc(&self) -> Option<&ReportedTimestamp> {
        self.last_gc.as_ref()
    }
    pub fn revision(&self) -> Revision {
        self.manifest.manifest().revision()
    }
    pub fn output(&self) {
        println!("{self:#?}");
    }
}
