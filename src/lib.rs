//! A library for scraping CVMFS servers and extracting their metadata.
//!
//! CVMFS servers expose metadata about their repositories, replicas, and server
//! configuration. Availability depends on the backend and administrator, and many
//! fields are optional. This library fetches those observations concurrently and
//! exposes them through validated domain types and explicit result states.
//!
//! # Metadata sources
//!
//! All paths are relative to a configured [`ServerEndpoint`]:
//!
//! | Path | Contents and availability |
//! | --- | --- |
//! | `cvmfs/info/v1/repositories.json` | Repository/replica discovery and server version/OS metadata; required for the CVMFS backend and skipped for S3. |
//! | `cvmfs/info/v1/meta.json` | Optional administrator, contact, organisation, and custom metadata. |
//! | `cvmfs/<repo>/.cvmfs_status.json` | Required status file for each selected repository; snapshot and garbage-collection timestamps are optional. |
//! | `cvmfs/<repo>/.cvmfspublished` | Required repository manifest, including its revision and catalog hashes. |
//! | `cvmfs/<repo>/api/v1.0/geo/<nonce>/<hosts>` | Optional GeoAPI ordering of the configured host list. |
//!
//! A [`ServerType`] describes the server's role; [`ServerBackendType`] controls
//! discovery. Explicit S3 backends have no discovery step, so configure repository
//! names through [`RepositorySelection`]. Autodetection assumes S3 only when the
//! index returns HTTP 404, and reports that assumption through [`BackendResolution`].
//!
//! # Example: scrape several servers
//!
//! Configuration constructors and deserialization validate endpoints and repository
//! names. [`Scraper`] then checks the configuration as a whole before making any
//! requests. Applications using `#[tokio::main]` must enable Tokio's `macros` and
//! runtime features in their own Cargo manifest.
//!
//! ```no_run
//! use cvmfs_server_scraper::{
//!     GeoapiOutcome, OptionalFetch, RepositorySelection, ScrapedServer, Scraper,
//!     ScraperCommon, Server, ServerBackendType, ServerType,
//! };
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let servers = vec![
//!         Server::new(
//!             ServerType::Stratum1,
//!             ServerBackendType::CVMFS,
//!             "http://azure-us-east-s1.eessi.science".parse()?,
//!         ),
//!         Server::new(
//!             ServerType::Stratum1,
//!             ServerBackendType::AutoDetect,
//!             "http://aws-eu-central-s1.eessi.science".parse()?,
//!         ),
//!         Server::new(
//!             ServerType::SyncServer,
//!             ServerBackendType::S3,
//!             "http://aws-eu-west-s1-sync.eessi.science".parse()?,
//!         ),
//!     ];
//!     // Include these repositories even when an index does not list them.
//!     // Exclusions apply to both discovered and explicitly included names.
//!     let selection = RepositorySelection::discover(
//!         ["software.eessi.io".parse()?, "dev.eessi.io".parse()?],
//!         ["riscv.eessi.io".parse()?],
//!     );
//!     let scraper = Scraper::new()
//!         .repository_selection(selection)
//!         .with_servers(servers) // Configuration can still be changed here.
//!         .validate()?;         // The ready scraper is immutable and reusable.
//!
//!     for result in scraper.scrape().await {
//!         match result {
//!             ScrapedServer::Collected(server) => {
//!                 println!("{server}: {:?}", server.repository_outcome());
//!                 for repository in server.repositories() {
//!                     println!("{}: revision {}", repository.name(), repository.revision());
//!                 }
//!                 for failure in server.failed_repositories() {
//!                     eprintln!("{}: {}", failure.name(), failure.error());
//!                 }
//!                 if let OptionalFetch::Failed(error) = server.contact() {
//!                     eprintln!("Contact metadata: {error}");
//!                 }
//!                 if let GeoapiOutcome::Failed(error) = server.geoapi() {
//!                     eprintln!("GeoAPI: {error}");
//!                 }
//!             }
//!             ScrapedServer::Failed(server) => {
//!                 eprintln!("{}: {}", server.hostname(), server.error());
//!             }
//!         }
//!     }
//!     Ok(())
//! }
//! ```
//!
//! # Selection, outcomes, and optional fields
//!
//! [`RepositorySelection::only`] selects exactly the supplied names, without an
//! exclusion list. [`RepositorySelection::discover`] combines included and
//! discovered names, then removes exclusions. Results are deduplicated and sorted
//! by repository name; server results retain configuration order. See
//! [`Server::scrape`] for the single-server entry point using [`ScrapeOptions`].
//!
//! Configuration and discovery failures produce a [`FailedServer`]. Once
//! repositories are selected, [`ServerReport`] retains successful results and
//! named [`FailedRepository`] errors independently, including at the server deadline.
//! [`ScrapedServer::Collected`] means a report is available. Its
//! [`ServerReport::repository_outcome`] distinguishes [`RepositoryOutcome::Complete`],
//! [`RepositoryOutcome::Partial`], [`RepositoryOutcome::AllFailed`], and
//! [`RepositoryOutcome::Empty`]. Only a nonempty selection can be complete.
//! Inspect optional contact metadata and GeoAPI outcomes through [`OptionalFetch`]
//! and [`GeoapiOutcome`] separately. Missing timestamps remain `None`; present but
//! unrecognized date strings remain available through [`ReportedTimestamp`].
//!
//! # Resource limits and validation scope
//!
//! [`ScrapeLimits`] bounds concurrent work, repository counts, response sizes, and
//! timeouts. A validated scraper shares its connection pool and request budget
//! across runs. Redirects are disabled by default; [`RedirectPolicy::SameOrigin`]
//! permits only a bounded chain within the configured origin.
//!
//! Endpoint validation checks syntax, not network authorization. Applications that
//! accept untrusted destinations must enforce their own network access policy.
//! [`Manifest`] preserves binary signatures without authenticating them.
//! [`RepositoryManifest`] records agreement with the requested repository name,
//! not publisher authorization; an absent name remains explicitly unspecified.
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
