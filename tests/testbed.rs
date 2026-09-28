//! Real CVMFS integration tests. See docs/testing.md for setup and invocation.
//! These tests mutate a disposable deployment and never contact public EESSI servers.
use cvmfs_server_scraper::*;
use rstest::rstest;
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf, process::Command, time::Duration};

const DEV: &str = "dev.testbed.test";
const SOFTWARE: &str = "software.testbed.test";
// Also serialize when someone runs the ignored suite without --test-threads=1.
static DEPLOYMENT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Deserialize)]
struct Endpoints {
    host: BTreeMap<String, ServerEndpoint>,
    repositories: Vec<RepositoryName>,
}

struct Testbed {
    state: PathBuf,
    endpoints: Endpoints,
}

impl Testbed {
    fn load() -> Self {
        let state = PathBuf::from(std::env::var_os("CVMFS_TESTBED_STATE").expect(
            "set CVMFS_TESTBED_STATE to a running disposable testbed; see docs/testing.md",
        ));
        // Read and mutate the same deployment, regardless of the working directory.
        let endpoints: Endpoints = serde_json::from_slice(
            &std::fs::read(state.join("endpoints.json")).expect("read testbed endpoints.json"),
        )
        .expect("parse testbed endpoints.json");
        let mut repositories = endpoints.repositories.clone();
        repositories.sort();
        assert_eq!(repositories, names(&[DEV, SOFTWARE]));
        Self { state, endpoints }
    }

    async fn run(&self, args: &[&str]) {
        eprintln!("cvmfs-testbed {}", args.join(" "));
        let state = self.state.clone();
        let arguments: Vec<_> = args.iter().map(|arg| (*arg).to_owned()).collect();
        // Keep the HTTP connection drivers running while the CLI is busy.
        // Blocking this test's Tokio thread can leave closed sockets in the pool.
        let output = tokio::task::spawn_blocking(move || {
            Command::new("cvmfs-testbed")
                .arg("--state-dir")
                .arg(state)
                .args(arguments)
                .output()
                .expect("run cvmfs-testbed (its bin directory must be on PATH)")
        })
        .await
        .expect("testbed command task panicked");
        assert!(
            output.status.success(),
            "cvmfs-testbed {args:?} failed: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn server(&self, service: &str, role: ServerType, backend: ServerBackendType) -> Server {
        Server::new(role, backend, self.endpoints.host[service].clone())
    }

    fn scraper(&self) -> Scraper<ValidatedAndReady> {
        Scraper::new()
            .options(options(RepositorySelection::discover(
                self.endpoints.repositories.clone(),
                [],
            )))
            .with_servers(vec![
                self.server("s0", ServerType::Stratum0, ServerBackendType::AutoDetect),
                self.server("s1", ServerType::Stratum1, ServerBackendType::AutoDetect),
                self.server(
                    "s1-s3",
                    ServerType::SyncServer,
                    ServerBackendType::AutoDetect,
                ),
            ])
            .validate()
            .unwrap()
    }
}

// Restore a stopped/frozen service even if an assertion unwinds. Successful
// scenarios restore explicitly, so a restoration failure fails the test too.
struct RestoreService<'a> {
    bed: &'a Testbed,
    service: &'a str,
    command: &'a str,
    armed: bool,
}

impl RestoreService<'_> {
    async fn restore(mut self) {
        self.bed.run(&[self.command, self.service]).await;
        self.armed = false;
        // A running object-store container can still be initializing S3. Wait
        // for native manifests/status files before asserting scraper recovery.
        self.bed.run(&["wait", "--timeout", "60"]).await;
    }
}

impl Drop for RestoreService<'_> {
    fn drop(&mut self) {
        if self.armed {
            // Avoid a second panic while an assertion is already unwinding.
            for args in [
                vec![self.command, self.service],
                vec!["wait", "--timeout", "60"],
            ] {
                let result = Command::new("cvmfs-testbed")
                    .arg("--state-dir")
                    .arg(&self.bed.state)
                    .args(args)
                    .status();
                if !result.is_ok_and(|status| status.success()) {
                    eprintln!("could not restore testbed service {}", self.service);
                    break;
                }
            }
        }
    }
}

fn names(values: &[&str]) -> Vec<RepositoryName> {
    values.iter().map(|name| name.parse().unwrap()).collect()
}

fn options(selection: RepositorySelection) -> ScrapeOptions {
    ScrapeOptions::default()
        .with_selection(selection)
        .with_geoapi(GeoapiProbe::Disabled)
        .with_limits(
            ScrapeLimits::default()
                .with_servers(ConcurrencyLimit::new(3).unwrap())
                .with_repositories(ConcurrencyLimit::new(2).unwrap())
                .with_requests(ConcurrencyLimit::new(2).unwrap())
                .with_repository_count(RepositoryLimit::new(10).unwrap())
                .with_connect_timeout(RequestTimeout::new(Duration::from_secs(3)).unwrap())
                .with_read_timeout(RequestTimeout::new(Duration::from_secs(5)).unwrap())
                .with_request_timeout(RequestTimeout::new(Duration::from_secs(8)).unwrap())
                .with_server_timeout(RequestTimeout::new(Duration::from_secs(20)).unwrap()),
        )
}

fn populated(result: ScrapedServer) -> PopulatedServer {
    match result {
        ScrapedServer::Populated(server) => *server,
        ScrapedServer::Failed(server) => panic!(
            "scrape of {} failed: {:#?}",
            server.server().endpoint(),
            server.error()
        ),
    }
}

fn assert_repositories(server: &PopulatedServer, expected: &[RepositoryName]) {
    assert_eq!(
        server
            .repositories()
            .iter()
            .map(|repo| repo.name())
            .collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
    for repo in server.repositories() {
        let bound = repo.manifest();
        let manifest = bound.manifest();
        assert_eq!(bound.identity(), RepositoryIdentity::Matched);
        assert_eq!(manifest.repository_name(), Some(repo.name()));
        assert!(repo.revision().get() > 0);
        assert!(!manifest
            .signature()
            .expect("native signed manifest")
            .as_bytes()
            .is_empty());
        assert!(manifest
            .published_at()
            .expect("publication timestamp")
            .datetime()
            .is_ok());
        // Native status timestamps can be absent or contain an unknown timezone.
        for timestamp in [repo.last_snapshot(), repo.last_gc()].into_iter().flatten() {
            assert_eq!(serde_json::to_value(timestamp).unwrap(), timestamp.as_str());
        }
        let json = serde_json::to_value(repo).unwrap();
        assert_eq!(json["manifest"]["requested"], repo.name().as_str());
        assert_eq!(json["manifest"]["manifest"]["s"], repo.revision().get());
    }
    assert!(matches!(
        server.geoapi(),
        GeoapiOutcome::Skipped(GeoapiSkipReason::Disabled)
    ));
}

async fn scrape_all(scraper: &Scraper<ValidatedAndReady>) -> Vec<ScrapedServer> {
    let results = tokio::time::timeout(Duration::from_secs(30), scraper.scrape())
        .await
        .expect("scrape exceeded the integration-test deadline");
    assert_eq!(results.len(), scraper.plan().servers().len());
    for (result, expected) in results.iter().zip(scraper.plan().servers()) {
        let actual = match result {
            ScrapedServer::Populated(server) => server.server(),
            ScrapedServer::Failed(server) => server.server(),
        };
        assert_eq!(actual, expected, "server result order");
    }
    results
}

async fn healthy(scraper: &Scraper<ValidatedAndReady>) -> Vec<PopulatedServer> {
    let servers: Vec<_> = scrape_all(scraper)
        .await
        .into_iter()
        .map(populated)
        .collect();
    for (server, backend) in servers.iter().zip([
        BackendResolution::DiscoveredCvmfs,
        BackendResolution::DiscoveredCvmfs,
        BackendResolution::AssumedS3IndexNotFound,
    ]) {
        assert_eq!(server.backend(), backend);
        assert_repositories(server, &names(&[DEV, SOFTWARE]));
    }
    servers
}

fn manifest<'a>(server: &'a PopulatedServer, name: &str) -> &'a Manifest {
    server
        .repositories()
        .iter()
        .find(|repo| repo.name().as_str() == name)
        .expect("expected repository")
        .manifest()
        .manifest()
}

fn assert_same_publication(left: &PopulatedServer, right: &PopulatedServer, name: &str) {
    let left = manifest(left, name);
    let right = manifest(right, name);
    assert_eq!(left.revision(), right.revision(), "{name}: revision");
    assert_eq!(
        left.catalog_hash(),
        right.catalog_hash(),
        "{name}: root catalog"
    );
    assert_eq!(
        left.signature(),
        right.signature(),
        "{name}: signature bytes"
    );
}

#[rstest]
#[case::s0_explicit(
    "s0",
    ServerType::Stratum0,
    ServerBackendType::CVMFS,
    BackendResolution::ConfiguredCvmfs
)]
#[case::s0_detected(
    "s0",
    ServerType::Stratum0,
    ServerBackendType::AutoDetect,
    BackendResolution::DiscoveredCvmfs
)]
#[case::s1_explicit(
    "s1",
    ServerType::Stratum1,
    ServerBackendType::CVMFS,
    BackendResolution::ConfiguredCvmfs
)]
#[case::s1_detected(
    "s1",
    ServerType::Stratum1,
    ServerBackendType::AutoDetect,
    BackendResolution::DiscoveredCvmfs
)]
#[case::s3_explicit(
    "s1-s3",
    ServerType::SyncServer,
    ServerBackendType::S3,
    BackendResolution::ConfiguredS3
)]
#[case::s3_detected(
    "s1-s3",
    ServerType::SyncServer,
    ServerBackendType::AutoDetect,
    BackendResolution::AssumedS3IndexNotFound
)]
#[tokio::test]
#[ignore = "requires a disposable cvmfs-testbed deployment; see docs/testing.md"]
async fn testbed_discovery_and_native_metadata(
    #[case] service: &str,
    #[case] role: ServerType,
    #[case] backend: ServerBackendType,
    #[case] resolution: BackendResolution,
) {
    let _guard = DEPLOYMENT.lock().await;
    let bed = Testbed::load();
    let selection = if service == "s1-s3" {
        RepositorySelection::only(bed.endpoints.repositories.clone())
    } else {
        RepositorySelection::default()
    };
    let result = populated(
        bed.server(service, role, backend)
            .scrape(options(selection))
            .await,
    );
    assert_eq!(result.backend(), resolution);
    assert_repositories(&result, &names(&[DEV, SOFTWARE]));
    if service == "s1-s3" {
        assert_eq!(result.metadata(), &ServerMetadata::default());
        assert!(matches!(result.contact(), OptionalFetch::Absent));
    } else {
        assert_eq!(result.metadata().schema_version(), Some(1));
        assert!(result.metadata().cvmfs_version().is_some());
    }
}

#[rstest]
#[case::only(RepositorySelection::only(names(&[DEV])))]
#[case::exclude(RepositorySelection::discover([], names(&[SOFTWARE])))]
#[tokio::test]
#[ignore = "requires a disposable cvmfs-testbed deployment; see docs/testing.md"]
async fn testbed_repository_selection(#[case] selection: RepositorySelection) {
    let _guard = DEPLOYMENT.lock().await;
    let bed = Testbed::load();
    for (service, role) in [("s0", ServerType::Stratum0), ("s1", ServerType::Stratum1)] {
        let result = populated(
            bed.server(service, role, ServerBackendType::CVMFS)
                .scrape(options(selection.clone()))
                .await,
        );
        assert_repositories(&result, &names(&[DEV]));
    }
}

#[rstest]
#[case::publisher_as_replica("s0", ServerType::Stratum1)]
#[case::replica_as_publisher("s1", ServerType::Stratum0)]
#[tokio::test]
#[ignore = "requires a disposable cvmfs-testbed deployment; see docs/testing.md"]
async fn testbed_rejects_incorrect_role(#[case] service: &str, #[case] role: ServerType) {
    let _guard = DEPLOYMENT.lock().await;
    let bed = Testbed::load();
    let result = bed
        .server(service, role, ServerBackendType::AutoDetect)
        .scrape(options(RepositorySelection::default()))
        .await;
    let failed = result.as_failed_server().expect("incorrect role must fail");
    assert!(matches!(
        failed.error(),
        CVMFSScraperError::Scrape(ScrapeError::ServerTypeMismatch(_))
    ));
}

#[tokio::test]
#[ignore = "requires a disposable cvmfs-testbed deployment; see docs/testing.md"]
async fn testbed_publication_selective_replication_and_repeated_scraping() {
    let _guard = DEPLOYMENT.lock().await;
    let bed = Testbed::load();
    bed.run(&["replicate", "all"]).await;
    let scraper = bed.scraper();
    // Exercise lag in both directions using the same client/pool throughout.
    for (name, unchanged, replica, caught_up, stale_replica, stale) in [
        (SOFTWARE, DEV, "s1", 1, "s1-s3", 2),
        (DEV, SOFTWARE, "s1-s3", 2, "s1", 1),
    ] {
        let before = healthy(&scraper).await;
        for server in &before[1..] {
            assert_same_publication(&before[0], server, name);
        }
        bed.run(&["publish", name, "--message", "scraper integration test"])
            .await;
        let published = healthy(&scraper).await;
        assert!(manifest(&published[0], name).revision() > manifest(&before[0], name).revision());
        assert_ne!(
            manifest(&published[0], name).catalog_hash(),
            manifest(&before[0], name).catalog_hash()
        );
        for i in 1..3 {
            assert_same_publication(&before[i], &published[i], name);
        }
        for i in 0..3 {
            assert_same_publication(&before[i], &published[i], unchanged);
        }

        bed.run(&["replicate", replica, name]).await;
        let lag = healthy(&scraper).await;
        assert_same_publication(&lag[0], &lag[caught_up], name);
        assert_same_publication(&before[stale], &lag[stale], name);
        assert!(manifest(&lag[stale], name).revision() < manifest(&lag[0], name).revision());

        bed.run(&["replicate", stale_replica, name]).await;
        let recovered = healthy(&scraper).await;
        for server in &recovered[1..] {
            assert_same_publication(&recovered[0], server, name);
        }
    }
}

#[tokio::test]
#[ignore = "requires a disposable cvmfs-testbed deployment; see docs/testing.md"]
async fn testbed_scraper_can_be_reused_after_an_idle_interval() {
    let _guard = DEPLOYMENT.lock().await;
    let bed = Testbed::load();
    let scraper = bed.scraper();
    let before = healthy(&scraper).await;
    // The native Apache fixture closes idle keep-alive connections. Unused
    // speculative connections from the first scrape also need to expire.
    tokio::time::sleep(Duration::from_secs(20)).await;
    let after = healthy(&scraper).await;
    for (before, after) in before.iter().zip(&after) {
        for name in [DEV, SOFTWARE] {
            assert_same_publication(before, after, name);
        }
    }
}

#[rstest]
#[case::publisher_outage("s0", 0, false)]
#[case::replica_outage("s1", 1, false)]
#[case::s3_outage("s1-s3", 2, false)]
#[case::object_store_outage("object-store", 2, false)]
#[case::frozen_replica("s1", 1, true)]
#[tokio::test]
#[ignore = "requires a disposable cvmfs-testbed deployment; see docs/testing.md"]
async fn testbed_failure_isolation_and_recovery(
    #[case] service: &str,
    #[case] affected: usize,
    #[case] freeze: bool,
) {
    let _guard = DEPLOYMENT.lock().await;
    let bed = Testbed::load();
    let scraper = bed.scraper();
    eprintln!("{service}: scrape before outage");
    let before = healthy(&scraper).await;
    let restore = RestoreService {
        bed: &bed,
        service,
        command: if freeze { "resume" } else { "start" },
        armed: true,
    };
    bed.run(&[if freeze { "pause" } else { "stop" }, service])
        .await;
    eprintln!("{service}: scrape during outage");
    let results = scrape_all(&scraper).await;
    for (i, result) in results.into_iter().enumerate() {
        if i == affected {
            let failed = result
                .as_failed_server()
                .expect("outage must fail this server");
            if freeze {
                assert!(
                    match failed.error() {
                        CVMFSScraperError::Scrape(ScrapeError::Timeout(_)) => true,
                        CVMFSScraperError::Scrape(ScrapeError::Fetch { source, .. }) =>
                            source.is_timeout(),
                        _ => false,
                    },
                    "expected timeout, got {:?}",
                    failed.error()
                );
            } else {
                assert!(
                    matches!(
                        failed.error(),
                        CVMFSScraperError::Scrape(
                            ScrapeError::Fetch { .. }
                                | ScrapeError::HttpStatus {
                                    status: 500..=599,
                                    ..
                                }
                                | ScrapeError::Timeout(_)
                        )
                    ),
                    "expected transport/upstream failure, got {:?}",
                    failed.error()
                );
            }
        } else {
            let server = populated(result);
            assert_repositories(&server, &names(&[DEV, SOFTWARE]));
            for name in [DEV, SOFTWARE] {
                assert_same_publication(&before[i], &server, name);
            }
        }
    }
    restore.restore().await;
    eprintln!("{service}: scrape after recovery");
    let recovered = healthy(&scraper).await;
    for (before, after) in before.iter().zip(&recovered) {
        for name in [DEV, SOFTWARE] {
            assert_same_publication(before, after, name);
        }
    }
}
