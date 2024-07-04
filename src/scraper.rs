use log::{debug, info, trace, warn};
use std::convert::TryFrom;
use std::{fmt::Debug, time::Instant};

use futures::future::join_all;
use std::marker::PhantomData;

use crate::constants::DEFAULT_GEOAPI_SERVERS;
use crate::errors::{HostnameError, ScrapeError};
use crate::models::{Hostname, ScrapedServer, Server, ServerBackendType};

pub struct WithoutServers;
pub struct WithServers;
pub struct ValidatedAndReady;

/// A scraper for CVMFS servers.
///
/// This struct provides a builder interface for scraping CVMFS servers, and it has three
/// states: WithoutServers, WithServers, and ValidatedAndReady. The scraper is created
/// with the new() method, and then servers can be added with the with_servers() method.
///
/// Transitions:
/// - new(): creates a Scraper in the WithoutServers state.
/// - with_servers(): WithoutServers -> WithServers.
/// - validate(): WithServers -> ValidatedAndReady
///
/// Notes:
/// - You may only add servers in the WithoutServers state.
/// - You may only validate the scraper in the WithServers state.
/// - You may only scrape the servers in the ValidatedAndReady state.
/// - Once the scraper is in the ValidatedAndReady state, it is no longer mutable.
///
/// ### Example
///
/// ```rust
/// use cvmfs_server_scraper::{Scraper, ScraperCommon, Hostname, Server, ServerType, ServerBackendType};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let servers = vec![
///         Server::new(
///             ServerType::Stratum1,
///             ServerBackendType::CVMFS,
///             Hostname::try_from("azure-us-east-s1.eessi.science").unwrap(),
///         ),
///         Server::new(
///             ServerType::Stratum1,
///             ServerBackendType::AutoDetect,
///             Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
///         ),
///     ];
///    
///     let scraper = Scraper::new()
///        .forced_repositories(vec!["repo1", "repo2"])
///        .with_servers(servers)
///        .ignored_repositories(vec!["repo3", "repo4"])
///        .geoapi_servers(vec!["cvmfs-stratum-one.cern.ch", "cvmfs-stratum-one.ihep.ac.cn"])?;
///    
///     let server_results = scraper.validate()?.scrape().await;
///     Ok(())
/// }
/// ```
pub struct Scraper<State = WithoutServers> {
    servers: Option<Vec<Server>>,
    forced_repos: Vec<String>,
    ignored_repos: Vec<String>,
    geoapi_servers: Vec<Hostname>,
    _state: PhantomData<State>,
}

// Implementation for WithoutServers state
impl Default for Scraper<WithoutServers> {
    fn default() -> Self {
        Self::new()
    }
}

impl Scraper<WithoutServers> {
    /// Create a new Scraper.
    ///
    /// This method creates a new Scraper with no servers added and in the
    /// WithoutServers state. To add servers, use the with_servers() method.
    pub fn new() -> Self {
        Scraper {
            servers: None,
            forced_repos: Vec::new(),
            ignored_repos: Vec::new(),
            geoapi_servers: DEFAULT_GEOAPI_SERVERS.clone(),
            _state: PhantomData,
        }
    }

    /// Add a list of servers to the scraper.
    ///
    /// This method transitions the scraper to the WithServers state, and you may
    /// no longer add servers after calling this method.
    pub fn with_servers(self, servers: Vec<Server>) -> Scraper<WithServers> {
        Scraper {
            servers: Some(servers),
            forced_repos: self.forced_repos,
            ignored_repos: self.ignored_repos,
            geoapi_servers: self.geoapi_servers,
            _state: PhantomData,
        }
    }
}

// Trait for common functionality across the WithoutServers and WithServers states.
pub trait ScraperCommon {
    /// Add a list of forced repositories to the scraper.
    ///
    /// Forced repositories are repositories that will be scraped even if they are not listed in
    /// repositories.json. Using this is required if the backend type of any server is S3 as S3
    /// servers do not have a repositories.json file.
    fn forced_repositories<I, S>(self, repos: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        Self: Sized;

    /// Add a list of ignored repositories to the scraper.
    ///
    /// Ignored repositories are repositories that will not be scraped even if they are listed in
    /// repositories.json or are given via the forced_repositories() method. This is useful if you
    /// want to exclude certain repositories from the scrape (e.g. dev/test repositories).
    ///
    /// If a repository is listed in both the forced and ignored lists, it will NOT be scraped.
    ///
    /// There is no attempt to validate the existence of any of the repositories in the ignored list.
    /// If a repository is listed in the ignored list but does not exist, it will be silently ignored.
    fn ignored_repositories<I, S>(self, repos: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        Self: Sized;

    /// Add a list of geoapi servers to the scraper.
    ///
    /// Geoapi servers are used to resolve the location of a server. This list contains the servers
    /// that will be used for a GeoAPI query and they will be returned in the order of distance from
    /// the querier.
    ///
    /// Defaults to `cvmfs_server_scraper::constants::DEFAULT_GEOAPI_SERVERS`.
    ///
    /// You may pass either something that can be converted into a Hostname (str/string) or a Hostname
    /// directly. If you pass a Hostname, the conversion is infallible so it is safe to unwrap().
    ///
    /// ### Example
    ///
    /// ```rust
    /// use std::convert::TryFrom;
    /// use cvmfs_server_scraper::{Scraper, ScraperCommon, Hostname};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     /// Using strings
    ///     let scraper = Scraper::new()
    ///        .geoapi_servers(vec!["cvmfs-stratum-one.cern.ch", "cvmfs-stratum-one.ihep.ac.cn"])?;
    ///
    ///     /// Using Hostname
    ///     let hostnames: Vec<Hostname> = vec!["cvmfs-stratum-one.cern.ch".parse()?, "cvmfs-stratum-one.ihep.ac.cn".parse()?];
    ///     let scraper = Scraper::new()
    ///        .geoapi_servers(hostnames).unwrap();
    ///     Ok(())
    /// }
    /// ```
    fn geoapi_servers<I, S>(self, servers: I) -> Result<Self, HostnameError>
    where
        I: IntoIterator<Item = S>,
        Hostname: TryFrom<S>,
        <Hostname as TryFrom<S>>::Error: Into<HostnameError>,
        Self: Sized;
}

// Implement common functionality for WithoutServers state
impl ScraperCommon for Scraper<WithoutServers> {
    fn forced_repositories<I, S>(mut self, repos: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.forced_repos = repos.into_iter().map(Into::into).collect();
        self
    }

    fn ignored_repositories<I, S>(mut self, repos: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ignored_repos = repos.into_iter().map(Into::into).collect();
        self
    }

    fn geoapi_servers<I, S>(mut self, servers: I) -> Result<Self, HostnameError>
    where
        I: IntoIterator<Item = S>,
        Hostname: TryFrom<S>,
        <Hostname as TryFrom<S>>::Error: Into<HostnameError>,
    {
        self.geoapi_servers = servers
            .into_iter()
            .map(|s| Hostname::try_from(s).map_err(Into::into))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self)
    }
}

// Implement common functionality for WithServers state
impl ScraperCommon for Scraper<WithServers> {
    fn forced_repositories<I, S>(mut self, repos: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.forced_repos = repos.into_iter().map(Into::into).collect();
        self
    }

    fn ignored_repositories<I, S>(mut self, repos: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ignored_repos = repos.into_iter().map(Into::into).collect();
        self
    }

    fn geoapi_servers<I, S>(mut self, servers: I) -> Result<Self, HostnameError>
    where
        I: IntoIterator<Item = S>,
        Hostname: TryFrom<S>,
        <Hostname as TryFrom<S>>::Error: Into<HostnameError>,
    {
        self.geoapi_servers = servers
            .into_iter()
            .map(|s| Hostname::try_from(s).map_err(Into::into))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self)
    }
}

// Implementation for WithServers state
impl Scraper<WithServers> {
    /// Validate the scraper and transition to the ValidatedAndReady state.
    ///
    /// This method performs some basic pre-flight checks to ensure that the scraper is
    /// correctly configured. If the checks pass, the scraper transitions to the
    /// ValidatedAndReady state, and you may no longer add servers or repositories.
    ///
    /// The checks performed are:
    /// - If any servers use the S3 backend, the forced repositories list cannot be empty.
    pub fn validate(self) -> Result<Scraper<ValidatedAndReady>, ScrapeError> {
        if self
            .servers
            .as_ref()
            .unwrap()
            .iter()
            .any(|s| s.backend_type == ServerBackendType::S3)
            && self.forced_repos.is_empty()
        {
            return Err(ScrapeError::EmptyRepositoryList(
                "Forced repositories list cannot be empty if any servers use the S3 backend"
                    .to_string(),
            ));
        }
        Ok(Scraper {
            servers: self.servers,
            forced_repos: self.forced_repos,
            ignored_repos: self.ignored_repos,
            geoapi_servers: self.geoapi_servers,
            _state: PhantomData,
        })
    }
}

// Implementation for ValidatedAndReady state
impl Scraper<ValidatedAndReady> {
    /// Scrape the servers.
    ///
    /// This method scrapes the servers and returns a list of ScrapedServer objects,
    /// which contain the results of the scrape. This list will contain either
    /// PopulatedServer objects or FailedServer objects, depending on whether the
    /// scrape was successful or not for that specific server.
    pub async fn scrape(&self) -> Vec<ScrapedServer> {
        let servers = self.servers.as_ref().unwrap();
        scrape_servers(
            servers.clone(),
            self.forced_repos.clone(),
            self.ignored_repos.clone(),
            self.geoapi_servers.clone(),
        )
        .await
    }
}

/// Scrape a list of servers in parallel.
///
/// This function scrapes a list of servers in parallel and returns a list of ScrapedServer objects,
async fn scrape_servers<R>(
    servers: Vec<Server>,
    scrape_repos: Vec<R>,
    ignored_repos: Vec<R>,
    geoapi_hosts: Vec<Hostname>,
) -> Vec<ScrapedServer>
where
    R: AsRef<str> + Debug + std::fmt::Display + Clone,
{
    let geoapi_servers = if geoapi_hosts.is_empty() {
        debug!("No geoapi servers provided to scrape_server, using default servers");
        DEFAULT_GEOAPI_SERVERS.clone()
    } else {
        geoapi_hosts
    };

    let start = Instant::now();
    let scrapes_attempted = servers.len();
    trace!(
        "Start of scraping run. Servers: {:?}, repositories: {:?} (ignored: {:?}), geoapi_servers: {:?}",
        servers,
        scrape_repos,
        ignored_repos,
        geoapi_servers
    );
    let futures = servers.iter().map(|server| {
        let repolist = scrape_repos.clone();
        let ignore = ignored_repos.clone();
        let geoapi_servers = geoapi_servers.clone();
        async move {
            server
                .scrape(
                    repolist.clone(),
                    ignore.clone(),
                    Some(geoapi_servers.clone()),
                )
                .await
        }
    });

    let scraped_servers = join_all(futures).await;

    for server in scraped_servers.iter() {
        match server {
            ScrapedServer::Populated(popserver) => {
                info!(
                    "Scraped server: {} with {} repositories",
                    popserver.hostname,
                    popserver.repositories.len()
                );
            }
            ScrapedServer::Failed(failedserver) => {
                warn!(
                    "Scraping failed for server: {} with error: {}",
                    failedserver.hostname, failedserver.error
                );
            }
        }
    }

    info!(
        "Scraped {} servers ({} succeeded), run duration: {:?}",
        scrapes_attempted,
        scraped_servers.iter().filter(|s| s.is_ok()).count(),
        start.elapsed()
    );
    trace!(
        "Scraping servers completed with results: {:?}",
        scraped_servers
    );
    scraped_servers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Hostname, Server, ServerBackendType, ServerType};

    #[tokio::test]
    async fn test_online_cvmfs_servers_using_scan_servers() {
        let servers = vec![
            Server::new(
                ServerType::Stratum1,
                ServerBackendType::CVMFS,
                Hostname::try_from("azure-us-east-s1.eessi.science").unwrap(),
            ),
            Server::new(
                ServerType::Stratum1,
                ServerBackendType::CVMFS,
                Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
            ),
            Server::new(
                ServerType::SyncServer,
                ServerBackendType::S3,
                Hostname::try_from("aws-eu-west-s1-sync.eessi.science").unwrap(),
            ),
        ];

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let results = scrape_servers(servers, repolist.clone(), vec![], vec![]).await;

        for result in results {
            match result {
                ScrapedServer::Populated(popserver) => {
                    for repo in repolist.clone() {
                        assert!(popserver.has_repository(repo));
                    }
                }
                ScrapedServer::Failed(failedserver) => {
                    panic!("Error: {:?}", failedserver.error);
                }
            }
        }
    }
}
