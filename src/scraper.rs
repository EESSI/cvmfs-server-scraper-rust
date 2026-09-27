use crate::transport::ScrapeClient;
use crate::{
    ConfigurationError, Hostname, RedirectPolicy, RepositoryName, ScrapeError, ScrapeLimits,
    ScrapedServer, Server, ServerBackendType, DEFAULT_GEOAPI_SERVERS,
};
use futures::{stream, StreamExt};
use std::collections::BTreeSet;

/// Repository inclusion rules. Only mode has no exclusion state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositorySelection {
    Only {
        repositories: BTreeSet<RepositoryName>,
    },
    Discover {
        include: BTreeSet<RepositoryName>,
        exclude: BTreeSet<RepositoryName>,
    },
}
impl Default for RepositorySelection {
    fn default() -> Self {
        Self::discover([], [])
    }
}
impl RepositorySelection {
    pub fn only(repositories: impl IntoIterator<Item = RepositoryName>) -> Self {
        Self::Only {
            repositories: repositories.into_iter().collect(),
        }
    }
    pub fn discover(
        include: impl IntoIterator<Item = RepositoryName>,
        exclude: impl IntoIterator<Item = RepositoryName>,
    ) -> Self {
        Self::Discover {
            include: include.into_iter().collect(),
            exclude: exclude.into_iter().collect(),
        }
    }
    pub(crate) fn resolve(
        &self,
        discovered: impl IntoIterator<Item = RepositoryName>,
    ) -> BTreeSet<RepositoryName> {
        match self {
            Self::Only { repositories } => repositories.clone(),
            Self::Discover { include, exclude } => include
                .iter()
                .cloned()
                .chain(discovered)
                .filter(|name| !exclude.contains(name))
                .collect(),
        }
    }
    fn configured_count(&self) -> usize {
        match self {
            Self::Only { repositories } => repositories.len(),
            Self::Discover { include, exclude } => include.len().saturating_add(exclude.len()),
        }
    }
}

/// Nonempty, unique, bounded query hosts. Empty input never silently enables defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoapiHosts(Vec<Hostname>);
impl GeoapiHosts {
    pub fn new(hosts: Vec<Hostname>) -> Result<Self, ConfigurationError> {
        if hosts.is_empty() || hosts.len() > 128 {
            return Err(ConfigurationError {
                field: "GeoAPI hosts",
                reason: "expected 1..=128 hosts; use GeoapiProbe::Disabled to skip the probe"
                    .into(),
            });
        }
        if hosts.iter().collect::<BTreeSet<_>>().len() != hosts.len() {
            return Err(ConfigurationError {
                field: "GeoAPI hosts",
                reason: "duplicate hostname".into(),
            });
        }
        Ok(Self(hosts))
    }
    pub fn as_slice(&self) -> &[Hostname] {
        &self.0
    }
}
#[derive(Debug, Clone)]
pub enum GeoapiProbe {
    Disabled,
    Enabled(GeoapiHosts),
}
impl Default for GeoapiProbe {
    fn default() -> Self {
        Self::Enabled(
            GeoapiHosts::new(DEFAULT_GEOAPI_SERVERS.clone())
                .expect("default hosts are unique and nonempty"),
        )
    }
}

/// Required repository failures fail the server. Optional metadata and GeoAPI
/// failures are preserved in their own outcomes without discarding repository data.
#[derive(Debug, Clone, Default)]
pub struct ScrapeOptions {
    selection: RepositorySelection,
    geoapi: GeoapiProbe,
    limits: ScrapeLimits,
    redirects: RedirectPolicy,
}
impl ScrapeOptions {
    pub fn with_selection(mut self, selection: RepositorySelection) -> Self {
        self.selection = selection;
        self
    }
    pub fn with_geoapi(mut self, probe: GeoapiProbe) -> Self {
        self.geoapi = probe;
        self
    }
    pub fn with_limits(mut self, limits: ScrapeLimits) -> Self {
        self.limits = limits;
        self
    }
    pub fn with_redirects(mut self, policy: RedirectPolicy) -> Self {
        self.redirects = policy;
        self
    }
    pub fn selection(&self) -> &RepositorySelection {
        &self.selection
    }
    pub fn geoapi(&self) -> &GeoapiProbe {
        &self.geoapi
    }
    pub fn limits(&self) -> &ScrapeLimits {
        &self.limits
    }
}

pub struct WithoutServers {
    options: ScrapeOptions,
}
pub struct WithServers {
    servers: Vec<Server>,
    options: ScrapeOptions,
}
pub struct ValidatedAndReady {
    plan: ValidatedScrapePlan,
    client: ScrapeClient,
}

/// Immutable plan whose options have been checked against these exact servers.
/// No Deserialize implementation can manufacture this proof.
pub struct ValidatedScrapePlan {
    servers: Vec<Server>,
    options: ScrapeOptions,
}
impl ValidatedScrapePlan {
    fn new(servers: Vec<Server>, options: ScrapeOptions) -> Result<Self, ScrapeError> {
        if servers.is_empty() {
            return Err(ConfigurationError {
                field: "servers",
                reason: "at least one server is required".into(),
            }
            .into());
        }
        let limit = options.limits.repository_count().get();
        let configured = options.selection.configured_count();
        if configured > limit {
            return Err(ScrapeError::RepositoryLimit {
                actual: configured,
                limit,
            });
        }
        let effective = options.selection.resolve([]);
        if effective.is_empty() {
            if let Some(server) = servers
                .iter()
                .find(|s| s.backend_type() == ServerBackendType::S3)
            {
                return Err(ScrapeError::EmptyRepositoryList(
                    server.endpoint().to_string(),
                ));
            }
        }
        Ok(Self { servers, options })
    }
    pub fn servers(&self) -> &[Server] {
        &self.servers
    }
    pub fn options(&self) -> &ScrapeOptions {
        &self.options
    }
}

/// Builder with configuration methods available only before validation.
///
/// ```compile_fail
/// use cvmfs_server_scraper::*;
/// fn mutate_ready(ready: Scraper<ValidatedAndReady>) {
///     ready.repository_selection(RepositorySelection::default());
/// }
/// ```
///
/// ```no_run
/// use cvmfs_server_scraper::*;
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let server = Server::new(ServerType::Stratum1, ServerBackendType::AutoDetect,
///     "https://example.org".parse()?);
/// let results = Scraper::new()
///     .repository_selection(RepositorySelection::only(["software.eessi.io".parse()?]))
///     .with_servers(vec![server]).validate()?.scrape().await;
/// # Ok(()) }
/// ```
pub struct Scraper<State = WithoutServers> {
    state: State,
}
impl Default for Scraper<WithoutServers> {
    fn default() -> Self {
        Self::new()
    }
}
impl Scraper<WithoutServers> {
    pub fn new() -> Self {
        Self {
            state: WithoutServers {
                options: ScrapeOptions::default(),
            },
        }
    }
    pub fn with_servers(self, servers: Vec<Server>) -> Scraper<WithServers> {
        Scraper {
            state: WithServers {
                servers,
                options: self.state.options,
            },
        }
    }
}
pub trait ScraperCommon: Sized {
    fn options(self, options: ScrapeOptions) -> Self;
    fn repository_selection(self, selection: RepositorySelection) -> Self;
    fn geoapi(self, probe: GeoapiProbe) -> Self;
}
macro_rules! builder_options {
    ($state:ty) => {
        impl ScraperCommon for Scraper<$state> {
            fn options(mut self, options: ScrapeOptions) -> Self {
                self.state.options = options;
                self
            }
            fn repository_selection(mut self, selection: RepositorySelection) -> Self {
                self.state.options.selection = selection;
                self
            }
            fn geoapi(mut self, probe: GeoapiProbe) -> Self {
                self.state.options.geoapi = probe;
                self
            }
        }
    };
}
builder_options!(WithoutServers);
builder_options!(WithServers);
impl Scraper<WithServers> {
    pub fn validate(self) -> Result<Scraper<ValidatedAndReady>, ScrapeError> {
        let plan = ValidatedScrapePlan::new(self.state.servers, self.state.options)?;
        let client = ScrapeClient::new(plan.options.limits.clone(), plan.options.redirects)?;
        // Options are moved into the proof; the ready state exposes no setters.
        Ok(Scraper {
            state: ValidatedAndReady { plan, client },
        })
    }
}
impl Scraper<ValidatedAndReady> {
    pub fn plan(&self) -> &ValidatedScrapePlan {
        &self.state.plan
    }
    /// Results retain input server order. Each server has its own total deadline;
    /// request permits are shared across all servers and released on cancellation.
    pub async fn scrape(&self) -> Vec<ScrapedServer> {
        let options = &self.state.plan.options;
        let mut results: Vec<_> = stream::iter(0..self.state.plan.servers.len())
            .map(|index| async move {
                (
                    index,
                    self.state.plan.servers[index]
                        .scrape_with(&self.state.client, options)
                        .await,
                )
            })
            .buffer_unordered(options.limits.servers().get())
            .collect()
            .await;
        results.sort_unstable_by_key(|(index, _)| *index);
        results.into_iter().map(|(_, result)| result).collect()
    }
}
