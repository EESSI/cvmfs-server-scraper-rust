//! Bounded, concurrent scraping of public CVMFS metadata.
//!
//! Configuration uses validated domain types. Scraped metadata is an observation:
//! manifest signatures are preserved byte-for-byte but are not authenticated.
//! HTTP endpoints may be public or private; endpoint syntax is not an egress policy.
//!
//! ```no_run
//! use cvmfs_server_scraper::*;
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let servers = vec![Server::new(ServerType::Stratum1, ServerBackendType::AutoDetect,
//!     "http://aws-eu-central-s1.eessi.science".parse()?)];
//! let selection = RepositorySelection::discover(["software.eessi.io".parse()?], []);
//! let results = Scraper::new().repository_selection(selection)
//!     .with_servers(servers).validate()?.scrape().await;
//! for result in results {
//!     match result {
//!         ScrapedServer::Populated(server) => server.output(),
//!         ScrapedServer::Failed(server) => eprintln!("{}: {}", server.hostname(), server.error()),
//!     }
//! }
//! # Ok(()) }
//! ```
mod constants;
mod errors;
mod models;
mod scraper;
mod transport;
mod utilities;

pub use constants::DEFAULT_GEOAPI_SERVERS;
pub use errors::{
    CVMFSScraperError, ConfigurationError, GenericError, HostnameError, ManifestError,
    RepositoryNameError, ScrapeError,
};
pub use models::*;
pub use scraper::{
    GeoapiHosts, GeoapiProbe, RepositorySelection, ScrapeOptions, Scraper, ScraperCommon,
    ValidatedAndReady, ValidatedScrapePlan, WithServers, WithoutServers,
};
pub use transport::{
    ConcurrencyLimit, RedirectPolicy, RepositoryLimit, RequestTimeout, ResponseByteLimit,
    ScrapeLimits, ServerEndpoint,
};
