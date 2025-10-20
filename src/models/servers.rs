use log::{debug, error, trace, warn};
use serde::{Deserialize, Serialize};

use crate::constants::DEFAULT_GEOAPI_SERVERS;
use crate::errors::{CVMFSScraperError, GenericError, ManifestError, ScrapeError};
use crate::models::cvmfs_status_json::StatusJSON;
use crate::models::geoapi::GeoapiServerQuery;
use crate::models::meta_json::MetaJSON;
use crate::models::repositories_json::RepositoriesJSON;
use crate::models::{Hostname, Manifest, MaybeRfc2822DateTime};
use crate::utilities::{fetch_json, fetch_text, generate_random_string};

/// The type of server we're dealing with.
///
/// Stratum0: The main server that holds the master copy of the data.
/// Stratum1: A server that holds a copy of the data from the Stratum0 server.
/// SyncServer: A server that holds a copy of the data from the Stratum0 server, but is not a Stratum1 server.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Copy)]
pub enum ServerType {
    Stratum0,
    Stratum1,
    SyncServer,
}

/// The type of backend a given server is using.
///
/// S3: The server is using S3 as the backend.
/// CVMFS: The server is using a standard CVMFS web server as the backend.
/// AutoDetect: The server will try to detect the backend type.
///
/// The AutoDetect backend type will try to fetch the repositories.json file from the server. If it
/// fails, it will assume the server is using S3 as the backend. If it succeeds, it will assume the
/// server is using CVMFS as the backend.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Copy)]
pub enum ServerBackendType {
    S3,
    CVMFS,
    AutoDetect,
}

/// A server object.
///
/// This object represents a CVMFS server. It contains the server type, the backend type, and the
/// hostname of the server.
///
/// The server object can be used to scrape the server for information about the repositories it
/// hosts. The scrape method will return a populated server object that contains information about
/// the server and the repositories it hosts.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Server {
    pub server_type: ServerType,
    #[serde(default = "default_backend_type")]
    pub backend_type: ServerBackendType,
    pub hostname: Hostname,
}

fn default_backend_type() -> ServerBackendType {
    ServerBackendType::AutoDetect
}

/// A populated server object.
///
/// This type is not to be manually created, but is the result of scraping a server object.
///
/// This object represents a CVMFS server that has been scraped for information about the repositories
/// it hosts. Note that replicas and repositories are consolidated into the attribute "repositories" as
/// they are functionally the same and no server will have both.
///
/// Fields:
///
/// - server_type: The server type (Stratum0, Stratum1, or SyncServer)
/// - backend_type: The backend type (S3, CVMFS, or AutoDetect)
/// - backend_detected: The detected backend type (S3 or CVMFS), will never be AutoDetect.
/// - hostname: The hostname of the server
/// - repositories: A list of populated repositories (or replicas)
/// - metadata: Metadata about the server (merged from repositories.json and meta.json, if found).
///
/// Metadata is not available servers using S3 as the backend as they do not provide repositories.json
#[derive(Debug, Clone, PartialEq)]
pub struct PopulatedServer {
    pub server_type: ServerType,
    pub backend_type: ServerBackendType,
    pub backend_detected: ServerBackendType,
    pub hostname: Hostname,
    pub repositories: Vec<PopulatedRepositoryOrReplica>,
    pub metadata: ServerMetadata,
    pub geoapi: GeoapiServerQuery,
}

/// A server that failed to scrape.
///
/// This struct is used to store information about a server that failed to scrape. It contains the
/// hostname of the server and the error that occurred.
#[derive(Debug, Clone)]
pub struct FailedServer {
    pub hostname: Hostname,
    pub server_type: ServerType,
    pub backend_type: ServerBackendType,
    pub error: CVMFSScraperError,
}

#[derive(Debug, Clone)]
pub enum ScrapedServer {
    Populated(PopulatedServer),
    Failed(FailedServer),
}

impl ScrapedServer {
    pub fn is_failed(&self) -> bool {
        matches!(self, ScrapedServer::Failed(_))
    }
    pub fn is_ok(&self) -> bool {
        matches!(self, ScrapedServer::Populated(_))
    }
    pub fn get_populated_server(self) -> Result<PopulatedServer, GenericError> {
        match self {
            ScrapedServer::Populated(server) => Ok(server),
            ScrapedServer::Failed(failed) => Err(GenericError::TypeError(format!(
                "{} is a failed server",
                failed.hostname
            ))),
        }
    }
    pub fn get_failed_server(self) -> Result<FailedServer, GenericError> {
        match self {
            ScrapedServer::Failed(failed) => Ok(failed),
            ScrapedServer::Populated(server) => Err(GenericError::TypeError(format!(
                "{} is a populated server",
                server.hostname
            ))),
        }
    }
}

impl Server {
    pub fn new(
        server_type: ServerType,
        backend_type: ServerBackendType,
        hostname: Hostname,
    ) -> Self {
        trace!("Creating server object for {}", hostname);
        Server {
            server_type,
            backend_type,
            hostname,
        }
    }

    pub fn to_failed_server(&self, error: CVMFSScraperError) -> FailedServer {
        FailedServer {
            hostname: self.hostname.clone(),
            server_type: self.server_type,
            backend_type: self.backend_type,
            error,
        }
    }

    /// Scrape the server for information about itself and its repos.
    ///
    /// This method will scrape the server for information about the repositories it hosts. It will
    /// also fetch metadata about the server from the repositories.json and meta.json files, if they
    /// are available.
    ///
    /// ## Arguments
    ///
    /// - `repositories`: A list of repositories to scrape. This may be empty unless the backend is S3.
    /// - `ignored_repositories`: A list of repositories to ignore. This may be empty.
    /// - `only_scrape_forced_repos`: If true, only the repositories provided in the `repositories` argument will be scraped
    ///    which overrides ignored_repositories. If false, the repositories from repositories.json will be merged with
    ///    the provided list and then filtered by ignored_repositories.
    ///
    /// ## Returns
    ///
    /// A ScrapedServer enum containing either a PopulatedServer or a FailedServer.
    pub async fn scrape<R>(
        &self,
        repositories: Vec<R>,
        ignored_repositories: Vec<R>,
        only_scrape_forced_repos: bool,
        geoapi_servers: Option<Vec<Hostname>>,
    ) -> ScrapedServer
    where
        R: AsRef<str> + std::fmt::Display + Clone,
    {
        debug!("Scraping server {}", self.hostname);

        let geoapi_servers = match geoapi_servers {
            Some(servers) => servers,
            None => DEFAULT_GEOAPI_SERVERS.clone(),
        };

        let ignore = ignored_repositories
            .iter()
            .map(|r| r.to_string())
            .collect::<std::collections::BTreeSet<_>>();

        let client = reqwest::Client::new();
        let mut all_repos = repositories
            .iter()
            .map(|repo| repo.to_string())
            .filter(|repo| !ignore.contains(repo))
            .collect::<std::collections::BTreeSet<_>>();
        let mut populated_repos = vec![];
        let mut backend_detected = self.backend_type;

        let mut metadata = MetadataFromRepoJSON {
            schema_version: None,
            cvmfs_version: None,
            last_geodb_update: MaybeRfc2822DateTime(None),
            os_version_id: None,
            os_pretty_name: None,
            os_id: None,
        };

        // Backend type behavior when dealing with repos from http://servername/info/v1/repositories.json
        // AutoDetect: Try to fetch the repositories.json, if it fails, assume we're on S3 and
        //             scrape the repositories provided. Accept fetch failures, and accept an empty list.
        // S3: Scrape the repositories provided. Raise an error if the list is empty.
        // CMVFS: Fetch the repositories.json and merge it with the repositories provided. Raise an error
        //        if the fetch fails.

        match self.backend_type {
            ServerBackendType::AutoDetect => match self.fetch_repos_json(&client).await {
                Ok(repo_json) => {
                    debug!("Detected CVMFS backend for {}", self.hostname);
                    match self.validate_repo_json_and_server_type(&repo_json) {
                        Ok(_) => {}
                        Err(error) => return ScrapedServer::Failed(self.to_failed_server(error)),
                    }
                    metadata = match MetadataFromRepoJSON::try_from(repo_json.clone()) {
                        Ok(meta) => meta,
                        Err(error) => {
                            return ScrapedServer::Failed(self.to_failed_server(error.into()))
                        }
                    };
                    backend_detected = ServerBackendType::CVMFS;

                    if !only_scrape_forced_repos {
                        all_repos.extend(
                            repo_json
                                .repositories_and_replicas()
                                .into_iter()
                                .filter(|r| !ignore.contains(&r.name))
                                .map(|r| r.name),
                        );
                    };
                }
                Err(error) => match error {
                    ScrapeError::FetchError(_) => {
                        debug!("Detected S3 backend for {}", self.hostname);
                        backend_detected = ServerBackendType::S3;
                    }
                    _ => return ScrapedServer::Failed(self.to_failed_server(error.into())),
                },
            },
            ServerBackendType::S3 => {
                if all_repos.is_empty() {
                    error!(
                        "Empty repository list with explicit S3 backend: {}",
                        self.hostname
                    );
                    return ScrapedServer::Failed(self.to_failed_server(
                        ScrapeError::EmptyRepositoryList(self.hostname.to_string()).into(),
                    ));
                }
            }
            ServerBackendType::CVMFS => {
                let repo_json = match self.fetch_repos_json(&client).await {
                    Ok(repo_json) => repo_json,
                    Err(error) => {
                        return ScrapedServer::Failed(self.to_failed_server(error.into()))
                    }
                };
                metadata = match MetadataFromRepoJSON::try_from(repo_json.clone()) {
                    Ok(meta) => meta,
                    Err(error) => {
                        return ScrapedServer::Failed(self.to_failed_server(error.into()))
                    }
                };
                match self.validate_repo_json_and_server_type(&repo_json) {
                    Ok(_) => {}
                    Err(error) => {
                        return ScrapedServer::Failed(self.to_failed_server(error));
                    }
                }
                if !only_scrape_forced_repos {
                    all_repos.extend(
                        repo_json
                            .repositories_and_replicas()
                            .into_iter()
                            .filter(|r| !ignore.contains(&r.name))
                            .map(|r| r.name),
                    )
                };
            }
        }

        for repo in all_repos {
            let repo = RepositoryOrReplica::new(&repo, self);
            let populated_repo = match repo.scrape(&client).await {
                Ok(repo) => repo,
                Err(error) => {
                    return ScrapedServer::Failed(self.to_failed_server(error));
                }
            };
            populated_repos.push(populated_repo);
        }

        let meta_json: Option<MetaJSON> = match self.fetch_meta_json(&client).await {
            Ok(meta) => Some(meta),
            Err(_) => None,
        };

        let metadata = self.merge_metadata(metadata, meta_json);
        let geoapi = if populated_repos.len() > 0 && self.server_type != ServerType::Stratum0 {
            match self
                .fetch_geoapi(
                    &client,
                    &populated_repos[0].name,
                    &backend_detected,
                    geoapi_servers,
                )
                .await
            {
                Ok(geoapi) => geoapi,
                Err(error) => {
                    return ScrapedServer::Failed(self.to_failed_server(error.into()));
                }
            }
        } else {
            GeoapiServerQuery {
                hostname: self.hostname.clone(),
                geoapi_hosts: geoapi_servers,
                response: Vec::new(),
            }
        };

        ScrapedServer::Populated(PopulatedServer {
            server_type: self.server_type,
            backend_type: self.backend_type,
            backend_detected,
            hostname: self.hostname.clone(),
            repositories: populated_repos,
            metadata,
            geoapi,
        })
    }

    async fn fetch_repos_json(
        &self,
        client: &reqwest::Client,
    ) -> Result<RepositoriesJSON, ScrapeError> {
        fetch_json(
            client,
            format!("http://{}/cvmfs/info/v1/repositories.json", self.hostname),
        )
        .await
    }

    async fn fetch_meta_json(&self, client: &reqwest::Client) -> Result<MetaJSON, ScrapeError> {
        fetch_json(
            client,
            format!("http://{}/cvmfs/info/v1/meta.json", self.hostname),
        )
        .await
    }

    async fn fetch_geoapi(
        &self,
        client: &reqwest::Client,
        repository_name: &String,
        backend_type: &ServerBackendType,
        geoapi_hosts: Vec<Hostname>,
    ) -> Result<GeoapiServerQuery, ScrapeError> {
        // S3 servers do not have GeoAPI support. S3 _is_ the GeoAPI.
        if *backend_type == ServerBackendType::S3 {
            debug!("Skipping GeoAPI for S3 server {}", self.hostname);
            return Ok(GeoapiServerQuery {
                hostname: self.hostname.clone(),
                geoapi_hosts,
                response: Vec::new(),
            });
        }

        let random_string = generate_random_string(12);
        trace!(
            "Fetching geoapi for {} (using {} as the random string)",
            self.hostname,
            random_string
        );
        let url = format!(
            "http://{}/cvmfs/{}/api/v1.0/geo/{}/{}",
            self.hostname,
            repository_name,
            random_string,
            geoapi_hosts
                .iter()
                .map(|hostname| hostname.to_str())
                .collect::<Vec<&str>>()
                .join(",")
        );
        let response = match fetch_text(client, &url).await {
            Ok(response) => {
                debug!("Fetched geoapi: {} -> {}", url, response);
                response
                    .trim()
                    .split(',')
                    .map(|x| {
                        x.parse::<u32>()
                            .map_err(|e| ScrapeError::GeoAPIFailure(e.to_string()))
                    })
                    .collect::<Result<Vec<u32>, ScrapeError>>()?
            }
            Err(_) => {
                let error_string = format!(
                    "Failed to fetch geoapi for {} on {:?} (with {})",
                    self.hostname, self.backend_type, random_string
                );
                warn!("{}", error_string);
                return Err(ScrapeError::GeoAPIFailure(error_string));
            }
        };

        Ok(GeoapiServerQuery {
            hostname: self.hostname.clone(),
            geoapi_hosts,
            response,
        })
    }

    fn validate_repo_json_and_server_type(
        &self,
        repo_json: &RepositoriesJSON,
    ) -> Result<(), CVMFSScraperError> {
        trace!("Validating {}", self.hostname);
        match (self.server_type, repo_json.replicas.is_empty()) {
            (ServerType::Stratum0, false) => Err(CVMFSScraperError::ScrapeError(
                ScrapeError::ServerTypeMismatch(format!(
                    "{} is a Stratum0 server, but replicas were found in the repositories.json",
                    self.hostname
                )),
            )),
            (ServerType::Stratum1, true) => Err(CVMFSScraperError::ScrapeError(
                ScrapeError::ServerTypeMismatch(format!(
                    "{} is a Stratum1 server, but no replicas were found in the repositories.json",
                    self.hostname
                )),
            )),
            (ServerType::SyncServer, true) => Err(CVMFSScraperError::ScrapeError(
                ScrapeError::ServerTypeMismatch(format!(
                    "{} is a SyncServer, but no replicas were found in the repositories.json",
                    self.hostname
                )),
            )),
            _ => Ok(()),
        }
    }

    fn merge_metadata(
        &self,
        repo_meta: MetadataFromRepoJSON,
        meta_json: Option<MetaJSON>,
    ) -> ServerMetadata {
        let mut server_metadata = if let Some(meta) = meta_json {
            ServerMetadata::from(meta)
        } else {
            ServerMetadata {
                schema_version: None,
                cvmfs_version: None,
                last_geodb_update: MaybeRfc2822DateTime(None),
                os_version_id: None,
                os_pretty_name: None,
                os_id: None,
                administrator: None,
                email: None,
                organisation: None,
                custom: None,
            }
        };

        server_metadata.merge_repo_metadata(repo_meta);
        server_metadata
    }
}

impl std::fmt::Display for PopulatedServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({:?}, {:?})",
            self.hostname, self.server_type, self.backend_type
        )
    }
}

impl PopulatedServer {
    pub fn output(&self) {
        println!("Server: {}", self.hostname);
        println!("Type: {:?}", self.server_type);
        println!("Backend: {:?}", self.backend_type);
        if self.backend_type == ServerBackendType::AutoDetect {
            println!("Detected Backend: {:?}", self.backend_detected);
        }
        if self.backend_detected != ServerBackendType::S3 {
            self.metadata.output();
        } else {
            println!("Metadata: Not vailable for S3 servers.");
        }
        if self.backend_detected != ServerBackendType::S3 {
            println!("GeoAPI:");
            self.geoapi.output();
        } else {
            println!("GeoAPI: Not available for S3 servers.");
        }

        println!("Repositories:");
        for repo in &self.repositories {
            println!("\n Name: {}", repo.name);
            repo.output();
        }
    }

    pub fn has_repository(&self, repository: &str) -> bool {
        self.repositories.iter().any(|r| r.name == *repository)
    }
}

/// Metadata about the server from the repositories.json file.
///
/// Note that all the fields are optional. They are not set if the backend is S3, and a CVMFS server
/// may opt not to provide some of the fields for privacy reasons.
///
/// - schema_version: The schema version, typically 1
/// - cvmfs_version: The version of CVMFS running on the server
/// - last_geodb_update: The last time the GeoDB was updated
/// - os_version_id: The version of the operating system
/// - os_pretty_name: The pretty name of the operating system
/// - os_id: The ID of the operating system (e.g. rhel)
#[derive(Debug, Clone, PartialEq)]
pub struct MetadataFromRepoJSON {
    pub schema_version: Option<u32>,
    pub cvmfs_version: Option<semver::Version>,
    pub last_geodb_update: MaybeRfc2822DateTime,
    pub os_version_id: Option<String>,
    pub os_pretty_name: Option<String>,
    pub os_id: Option<String>,
}

impl TryFrom<RepositoriesJSON> for MetadataFromRepoJSON {
    type Error = ScrapeError;

    fn try_from(repo_json: RepositoriesJSON) -> Result<Self, Self::Error> {
        let cvmfs_version = repo_json
            .cvmfs_version
            .clone()
            .map(|v| {
                v.parse::<semver::Version>()
                    .map_err(|e| ScrapeError::ConversionError(e.to_string()))
            })
            .transpose()?;

        Ok(MetadataFromRepoJSON {
            schema_version: Some(repo_json.schema),
            cvmfs_version,
            last_geodb_update: repo_json.last_geodb_update.clone(),
            os_version_id: repo_json.os_version_id.clone(),
            os_pretty_name: repo_json.os_pretty_name.clone(),
            os_id: repo_json.os_id.clone(),
        })
    }
}

// Custom serializer function as semver::Version does not implement Serialize
fn serialize_version_as_string<S>(
    version: &Option<semver::Version>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::ser::Serializer,
{
    match version {
        Some(v) => serializer.serialize_some(&v.to_string()),
        None => serializer.serialize_none(),
    }
}

/// Merged metadata about the server from the repositories.json and meta.json files.
///
/// This struct contains metadata about the server. It is a combination of the metadata from the
/// repositories.json file and the meta.json file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServerMetadata {
    pub schema_version: Option<u32>,
    #[serde(serialize_with = "serialize_version_as_string")]
    pub cvmfs_version: Option<semver::Version>,
    pub last_geodb_update: MaybeRfc2822DateTime,
    pub os_version_id: Option<String>,
    pub os_pretty_name: Option<String>,
    pub os_id: Option<String>,
    pub administrator: Option<String>,
    pub email: Option<String>,
    pub organisation: Option<String>,
    pub custom: Option<serde_json::Value>,
}

impl From<MetaJSON> for ServerMetadata {
    fn from(meta: MetaJSON) -> Self {
        ServerMetadata {
            schema_version: None,
            cvmfs_version: None,
            last_geodb_update: MaybeRfc2822DateTime(None),
            os_version_id: None,
            os_pretty_name: None,
            os_id: None,
            administrator: Some(meta.administrator),
            email: Some(meta.email),
            organisation: Some(meta.organisation),
            custom: Some(meta.custom),
        }
    }
}

impl ServerMetadata {
    pub fn merge_repo_metadata(&mut self, repo_meta: MetadataFromRepoJSON) {
        self.schema_version = repo_meta.schema_version;
        self.cvmfs_version = repo_meta.cvmfs_version;
        self.last_geodb_update = repo_meta.last_geodb_update;
        self.os_version_id = repo_meta.os_version_id;
        self.os_pretty_name = repo_meta.os_pretty_name;
        self.os_id = repo_meta.os_id;
    }

    pub fn output(&self) {
        println!("Metadata:");
        if let Some(schema_version) = self.schema_version {
            println!("  Schema Version: {}", schema_version);
        }
        if let Some(cvmfs_version) = &self.cvmfs_version {
            println!("  CVMFS Version: {}", cvmfs_version);
        }
        if let MaybeRfc2822DateTime(Some(last_geodb_update)) = &self.last_geodb_update {
            println!("  Last GeoDB Update: {}", last_geodb_update);
        }
        if let Some(os_version_id) = &self.os_version_id {
            println!("  OS Version ID: {}", os_version_id);
        }
        if let Some(os_pretty_name) = &self.os_pretty_name {
            println!("  OS Pretty Name: {}", os_pretty_name);
        }
        if let Some(os_id) = &self.os_id {
            println!("  OS ID: {}", os_id);
        }
        if let Some(administrator) = &self.administrator {
            println!("  Administrator: {}", administrator);
        }
        if let Some(email) = &self.email {
            println!("  Email: {}", email);
        }
        if let Some(organisation) = &self.organisation {
            println!("  Organisation: {}", organisation);
        }
        if let Some(custom) = &self.custom {
            println!("  Custom: {}", custom);
        }
    }
}

pub struct RepositoryOrReplica {
    pub server: Server,
    pub name: String,
}

impl RepositoryOrReplica {
    pub fn new(name: &str, server: &Server) -> Self {
        RepositoryOrReplica {
            server: server.clone(),
            name: name.to_string(),
        }
    }

    pub async fn scrape(
        &self,
        client: &reqwest::Client,
    ) -> Result<PopulatedRepositoryOrReplica, CVMFSScraperError> {
        let repo_status = self.fetch_repository_status_json(client).await?;
        Ok(PopulatedRepositoryOrReplica {
            name: self.name.clone(),
            manifest: self.fetch_repository_manifest(client).await?,
            last_snapshot: repo_status.last_snapshot,
            last_gc: repo_status.last_gc,
        })
    }

    async fn fetch_repository_manifest(
        &self,
        client: &reqwest::Client,
    ) -> Result<Manifest, ManifestError> {
        let url = format!(
            "http://{}/cvmfs/{}/.cvmfspublished",
            self.server.hostname, self.name
        );
        let response = client.get(url).send().await?;
        response.error_for_status()?.text().await?.parse()
    }

    async fn fetch_repository_status_json(
        &self,
        client: &reqwest::Client,
    ) -> Result<StatusJSON, ScrapeError> {
        fetch_json(
            client,
            format!(
                "http://{}/cvmfs/{}/.cvmfs_status.json",
                self.server.hostname, self.name
            ),
        )
        .await
    }
}

/// A populated repository or replica object.
///
/// This object represents a CVMFS repository or replica that has been scraped for information about
/// the repository. For fetching the revision of the repository, one can use the `revision` method
/// as a shortcut to get the revision from the manifest.
///
/// Fields:
///
/// - name: The name of the repository
/// - manifest: The manifest of the repository
/// - last_snapshot: The last time a snapshot was taken (optional)
/// - last_gc: The last time garbage collection was run (optional)
///
/// The MaybeRfc2822DateTime type is used to represent a date and time that may or may not be present,
/// and may or may not be in the RFC 2822 format. See the documentation for the MaybeRfc2822DateTime
/// type for more information.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PopulatedRepositoryOrReplica {
    pub name: String,
    pub manifest: Manifest,
    pub last_snapshot: Option<MaybeRfc2822DateTime>,
    pub last_gc: Option<MaybeRfc2822DateTime>,
}

impl PopulatedRepositoryOrReplica {
    pub fn output(&self) {
        if self.last_gc.is_some() {
            println!("  Last Snapshot: {}", self.last_snapshot.as_ref().unwrap());
        }
        if self.last_gc.is_some() {
            println!("  Last GC: {}", self.last_gc.as_ref().unwrap());
        }
        self.manifest.output();
    }
    pub fn revision(&self) -> i32 {
        self.manifest.s
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use serde_json::{json, Value};
    use yare::parameterized;

    #[parameterized(
        test_full_data = {
            Some(1),
            Some("2.8.4"),
            Some("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some("rhel7"),
            Some("Red Hat Enterprise Linux 7"),
            Some("rhel"),
            Some("admin"),
            Some("admin@host.com"),
            Some("host.com"),
            None // custom field
        },
        test_minimal_data = {
            None, None, None, None, None, None, None, None, None, None
        },
        test_custom_data = {
            None, None, None, None, None, None, None, None, None, Some(json!({"key": "value"}))
        }
    )]
    fn test_serialization_of_metadata(
        schema_version: Option<u32>,
        cvmfs_version: Option<&str>,
        last_geodb_update: Option<&str>,
        os_version_id: Option<&str>,
        os_pretty_name: Option<&str>,
        os_id: Option<&str>,
        administrator: Option<&str>,
        email: Option<&str>,
        organisation: Option<&str>,
        custom: Option<Value>,
    ) {
        // Construct the ServerMetadata instance
        let metadata = ServerMetadata {
            schema_version,
            cvmfs_version: cvmfs_version.map(|v| semver::Version::parse(v).unwrap()),
            last_geodb_update: MaybeRfc2822DateTime(last_geodb_update.map(|s| s.to_string())),
            os_version_id: os_version_id.map(|s| s.to_string()),
            os_pretty_name: os_pretty_name.map(|s| s.to_string()),
            os_id: os_id.map(|s| s.to_string()),
            administrator: administrator.map(|s| s.to_string()),
            email: email.map(|s| s.to_string()),
            organisation: organisation.map(|s| s.to_string()),
            custom: custom.clone(),
        };

        // Build the expected JSON
        let expected = json!({
            "schema_version": schema_version,
            "cvmfs_version": cvmfs_version,
            "last_geodb_update": last_geodb_update,
            "os_version_id": os_version_id,
            "os_pretty_name": os_pretty_name,
            "os_id": os_id,
            "administrator": administrator,
            "email": email,
            "organisation": organisation,
            "custom": custom.unwrap_or(Value::Null),
        });

        // Serialize the metadata to JSON
        let json = serde_json::to_value(&metadata).unwrap();

        // Compare the actual JSON with the expected JSON
        assert_eq!(json, expected);
    }
}
