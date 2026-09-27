use crate::transport::ScrapeClient;
use crate::{
    ConfigurationError, Hostname, RedirectPolicy, RepositoryName, ScrapeError, ScrapeLimits,
    ScrapedServer, Server, ServerBackendType, DEFAULT_GEOAPI_SERVERS,
};
use futures::{stream, StreamExt};
use std::collections::BTreeSet;

/// Repository inclusion rules. Only mode has no exclusion state.
///
/// The default discovers all repositories without additions or exclusions.
/// Names are deduplicated and ordered lexically. Explicit S3 backends require a
/// nonempty effective selection because they cannot discover repository names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositorySelection {
    /// Scrape exactly these names, regardless of the index contents.
    Only {
        /// Repository names to fetch; duplicates are collapsed.
        repositories: BTreeSet<RepositoryName>,
    },
    /// Combine index entries and inclusions, then remove exclusions.
    Discover {
        /// Names to fetch even when absent from the index.
        include: BTreeSet<RepositoryName>,
        /// Names to omit, including any also present in `include`.
        exclude: BTreeSet<RepositoryName>,
    },
}
impl Default for RepositorySelection {
    fn default() -> Self {
        Self::discover([], [])
    }
}
impl RepositorySelection {
    /// Select only the given repositories. An empty selection is allowed for
    /// CVMFS backends, but fails preflight for an explicit S3 backend.
    pub fn only(repositories: impl IntoIterator<Item = RepositoryName>) -> Self {
        Self::Only {
            repositories: repositories.into_iter().collect(),
        }
    }
    /// Include discovered and explicitly named repositories, except exclusions.
    /// Excluded names need not exist; nonexistent exclusions have no effect.
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
    /// Preserve query order while checking that there are 1..=128 unique hosts.
    ///
    /// # Errors
    ///
    /// Returns a configuration error for empty, oversized, or duplicate lists.
    /// Use [`GeoapiProbe::Disabled`] to disable queries instead of an empty list.
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
    /// Hosts in the query order used to interpret one-based GeoAPI response IDs.
    pub fn as_slice(&self) -> &[Hostname] {
        &self.0
    }
}
/// Whether to probe the server's GeoAPI, and which hosts to ask it to order.
/// Defaults to an enabled probe using [`DEFAULT_GEOAPI_SERVERS`].
#[derive(Debug, Clone)]
pub enum GeoapiProbe {
    /// Skip the optional GeoAPI request.
    Disabled,
    /// Query with this validated, ordered host list when the backend supports it.
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
///
/// Defaults discover all repositories, enable the default GeoAPI hosts, apply
/// [`ScrapeLimits::default`], and disable redirects. Use these options with either
/// [`Server::scrape`] or [`ScraperCommon::options`]. Each setter replaces its setting.
#[derive(Debug, Clone, Default)]
pub struct ScrapeOptions {
    selection: RepositorySelection,
    geoapi: GeoapiProbe,
    limits: ScrapeLimits,
    redirects: RedirectPolicy,
}
impl ScrapeOptions {
    /// Replace the repository inclusion and exclusion policy.
    pub fn with_selection(mut self, selection: RepositorySelection) -> Self {
        self.selection = selection;
        self
    }
    /// Enable a particular GeoAPI host list or explicitly disable the probe.
    pub fn with_geoapi(mut self, probe: GeoapiProbe) -> Self {
        self.geoapi = probe;
        self
    }
    /// Replace the resource budgets used by the transport and scheduler.
    pub fn with_limits(mut self, limits: ScrapeLimits) -> Self {
        self.limits = limits;
        self
    }
    /// Replace the redirect policy; the default rejects redirects.
    pub fn with_redirects(mut self, policy: RedirectPolicy) -> Self {
        self.redirects = policy;
        self
    }
    /// Repository selection policy applied independently to each server.
    pub fn selection(&self) -> &RepositorySelection {
        &self.selection
    }
    /// Configured GeoAPI probe policy and host order.
    pub fn geoapi(&self) -> &GeoapiProbe {
        &self.geoapi
    }
    /// Configured HTTP and scheduling resource budgets.
    pub fn limits(&self) -> &ScrapeLimits {
        &self.limits
    }
}

/// Initial builder state: options can be set and the server list can be supplied.
pub struct WithoutServers {
    options: ScrapeOptions,
}
/// Builder state with a fixed server list; options can still change before validation.
pub struct WithServers {
    servers: Vec<Server>,
    options: ScrapeOptions,
}
/// Validated, immutable state with a reusable HTTP client and shared request budget.
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
    /// Servers in configuration order, which is also the result order.
    pub fn servers(&self) -> &[Server] {
        &self.servers
    }
    /// Options checked against this exact server list.
    pub fn options(&self) -> &ScrapeOptions {
        &self.options
    }
}

/// Builder with configuration methods available only before validation.
///
/// The three states make the lifecycle explicit:
///
/// 1. [`Scraper::new`] creates a [`WithoutServers`] builder with default options.
/// 2. [`Scraper::with_servers`] supplies the server list and enters [`WithServers`].
/// 3. [`Scraper::validate`] checks the plan and enters [`ValidatedAndReady`].
/// 4. [`Scraper::scrape`] fetches results; the same ready scraper can be reused.
///
/// Import [`ScraperCommon`] to configure options in either builder state. The
/// server list is supplied once, and validation consumes the configurable builder.
/// Only the ready state can scrape; it exposes no configuration setters:
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
    /// Create a builder with default options and no servers.
    /// Supply the server list with [`Self::with_servers`] before validation.
    pub fn new() -> Self {
        Self {
            state: WithoutServers {
                options: ScrapeOptions::default(),
            },
        }
    }
    /// Supply the complete server list and enter the [`WithServers`] state.
    /// Options remain configurable, but another server list cannot be supplied.
    /// An empty list is rejected by [`Scraper::validate`].
    pub fn with_servers(self, servers: Vec<Server>) -> Scraper<WithServers> {
        Scraper {
            state: WithServers {
                servers,
                options: self.state.options,
            },
        }
    }
}
/// Configuration methods shared by the two builder states.
/// Import this trait to use these methods; they are unavailable after validation.
pub trait ScraperCommon: Sized {
    /// Replace all options, including any previously configured selection or GeoAPI hosts.
    fn options(self, options: ScrapeOptions) -> Self;
    /// Replace repository selection. Use [`RepositorySelection::only`] for an
    /// exact list or [`RepositorySelection::discover`] for additions and exclusions.
    fn repository_selection(self, selection: RepositorySelection) -> Self;
    /// Replace the GeoAPI probe policy. The default uses [`DEFAULT_GEOAPI_SERVERS`].
    ///
    /// ```
    /// use cvmfs_server_scraper::{GeoapiHosts, GeoapiProbe, Scraper, ScraperCommon};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let hosts = GeoapiHosts::new(vec![
    ///     "cvmfs-stratum-one.cern.ch".parse()?,
    ///     "cvmfs-stratum-one.ihep.ac.cn".parse()?,
    /// ])?;
    /// let scraper = Scraper::new().geoapi(GeoapiProbe::Enabled(hosts));
    /// # let _ = scraper;
    /// # Ok(()) }
    /// ```
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
    /// Check the configuration and create the immutable, ready scraper.
    ///
    /// No requests are made. Validation requires at least one server, checks the
    /// configured repository count, and requires a nonempty effective selection
    /// for every explicit S3 backend. The effective selection accounts for exclusions.
    /// Discovered counts and remote metadata are checked later during the scrape.
    ///
    /// # Errors
    ///
    /// Returns a [`ScrapeError`] for an invalid configuration or failure to
    /// initialize the shared HTTP client.
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
    /// Inspect the validated plan without changing its servers or options.
    pub fn plan(&self) -> &ValidatedScrapePlan {
        &self.state.plan
    }
    /// Results retain input server order. Each server has its own total deadline;
    /// request permits are shared across all servers and released on cancellation.
    ///
    /// Returns one [`ScrapedServer`] per configured server. A required-resource
    /// failure affects that server; other servers continue. Optional contact and
    /// GeoAPI failures remain available on successful results. The deadline starts
    /// when a server is admitted by the concurrency limit, not while it waits for
    /// admission. There is no separate whole-run deadline.
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
