//! Opt-in checks of the public EESSI deployment used by this crate's examples.
//! Failures can indicate a server outage or access policy as well as a regression.
//! No revision, timestamp, version, or GeoAPI distance ordering is pinned.
use cvmfs_server_scraper::*;
use rstest::rstest;
use std::time::Duration;

const AWS_STRATUM1: &str = "http://aws-eu-central-s1.eessi.science";
const AZURE_STRATUM1: &str = "http://azure-us-east-s1.eessi.science";
const S3_SYNC: &str = "http://aws-eu-west-s1-sync.eessi.science";
const EESSI_REPOSITORIES: [&str; 3] = ["dev.eessi.io", "riscv.eessi.io", "software.eessi.io"];

fn names(values: &[&str]) -> Vec<RepositoryName> {
    values.iter().map(|value| value.parse().unwrap()).collect()
}

fn server(origin: &str, role: ServerType, backend: ServerBackendType) -> Server {
    Server::new(role, backend, origin.parse().unwrap())
}

fn options(selection: RepositorySelection) -> ScrapeOptions {
    // Keep public-server load bounded even when a test uses multiple servers.
    ScrapeOptions::default()
        .with_selection(selection)
        .with_limits(
            ScrapeLimits::default()
                .with_servers(ConcurrencyLimit::new(3).unwrap())
                .with_repositories(ConcurrencyLimit::new(2).unwrap())
                .with_requests(ConcurrencyLimit::new(4).unwrap())
                .with_repository_count(RepositoryLimit::new(100).unwrap())
                .with_server_timeout(RequestTimeout::new(Duration::from_secs(60)).unwrap()),
        )
}

fn populated(result: ScrapedServer) -> PopulatedServer {
    match result {
        ScrapedServer::Populated(server) => *server,
        ScrapedServer::Failed(server) => panic!(
            "online scrape of {} failed: {}\n{:#?}",
            server.server().endpoint(),
            server.error(),
            server.error()
        ),
    }
}

fn assert_repository_data(server: &PopulatedServer, expected: &[RepositoryName]) {
    for name in expected {
        let repository = server
            .repositories()
            .iter()
            .find(|repository| repository.name() == name)
            .unwrap_or_else(|| panic!("{} did not return {name}", server.server().endpoint()));
        let bound = repository.manifest();
        let manifest = bound.manifest();
        assert_eq!(bound.identity(), RepositoryIdentity::Matched, "{name}");
        assert_eq!(manifest.repository_name(), Some(name));
        assert!(
            repository.revision().get() > 0,
            "{name}: expected a published revision"
        );
        assert!(!manifest
            .signature()
            .expect("EESSI manifests should be signed")
            .as_bytes()
            .is_empty());
        assert!(manifest
            .published_at()
            .expect("EESSI manifests should report publication time")
            .datetime()
            .is_ok());
        // Snapshot/GC fields remain optional; present date text must survive serde.
        for timestamp in [repository.last_snapshot(), repository.last_gc()]
            .into_iter()
            .flatten()
        {
            assert_eq!(serde_json::to_value(timestamp).unwrap(), timestamp.as_str());
        }
        // Exercise the public result schema on actual remote manifest/status data.
        let serialized = serde_json::to_value(repository).unwrap();
        assert_eq!(serialized["manifest"]["requested"], name.as_str());
        assert_eq!(
            serialized["manifest"]["manifest"]["s"],
            repository.revision().get()
        );
    }
    assert!(server
        .repositories()
        .windows(2)
        .all(|pair| pair[0].name() < pair[1].name()));
}

fn assert_geoapi(server: &PopulatedServer, hosts: &[Hostname]) {
    let GeoapiOutcome::Available(query) = server.geoapi() else {
        panic!(
            "{}: expected GeoAPI ordering, got {:?}",
            server.server().endpoint(),
            server.geoapi()
        );
    };
    assert_eq!(query.endpoint(), server.server().endpoint());
    assert_eq!(query.ordering().hosts(), hosts);
    let mut ordered = query.map_response_order_to_geoapi_hostnames();
    let mut expected = hosts.to_vec();
    ordered.sort();
    expected.sort();
    assert_eq!(ordered, expected);
    // Distance order varies with the runner's location; assert the permutation only.
}

#[rstest]
#[case::aws_explicit(
    AWS_STRATUM1,
    ServerBackendType::CVMFS,
    BackendResolution::ConfiguredCvmfs
)]
#[case::aws_detected(
    AWS_STRATUM1,
    ServerBackendType::AutoDetect,
    BackendResolution::DiscoveredCvmfs
)]
#[case::azure_explicit(
    AZURE_STRATUM1,
    ServerBackendType::CVMFS,
    BackendResolution::ConfiguredCvmfs
)]
#[tokio::test]
#[ignore = "contacts public EESSI Stratum1 servers; enable with the online-tests PR label"]
async fn online_stratum1_discovery_metadata_and_geoapi(
    #[case] origin: &str,
    #[case] backend: ServerBackendType,
    #[case] expected_backend: BackendResolution,
) {
    let server = server(origin, ServerType::Stratum1, backend);
    let result = populated(server.scrape(options(RepositorySelection::default())).await);
    assert_eq!(result.backend(), expected_backend);
    assert_repository_data(&result, &names(&EESSI_REPOSITORIES));
    let metadata = result.metadata();
    assert_eq!(metadata.schema_version(), Some(1));
    assert!(metadata.cvmfs_version().is_some());
    for value in [
        metadata.os_id(),
        metadata.os_version_id(),
        metadata.os_pretty_name(),
    ] {
        assert!(value.is_some_and(|value| !value.is_empty()));
    }
    assert!(metadata
        .last_geodb_update()
        .expect("expected GeoIP update time")
        .datetime()
        .is_some());
    let OptionalFetch::Available(contact) = result.contact() else {
        panic!(
            "{origin}: expected EESSI contact metadata, got {:?}",
            result.contact()
        );
    };
    assert_eq!(contact.organisation.as_deref(), Some("EESSI"));
    assert!(contact
        .administrator
        .as_ref()
        .is_some_and(|value| !value.is_empty()));
    assert!(contact
        .email
        .as_ref()
        .is_some_and(|value| !value.is_empty()));
    assert_geoapi(&result, &DEFAULT_GEOAPI_SERVERS);
}

#[rstest]
#[case::only(RepositorySelection::only(names(&["dev.eessi.io", "software.eessi.io"])))]
#[case::discovery_with_exclusion(RepositorySelection::discover(
    names(&EESSI_REPOSITORIES), names(&["riscv.eessi.io"])
))]
#[tokio::test]
#[ignore = "contacts a public EESSI Stratum1 server; enable with the online-tests PR label"]
async fn online_repository_selection_and_custom_geoapi(#[case] selection: RepositorySelection) {
    let exact = matches!(&selection, RepositorySelection::Only { .. });
    let hosts = vec![DEFAULT_GEOAPI_SERVERS[0].clone()];
    let server = server(AWS_STRATUM1, ServerType::Stratum1, ServerBackendType::CVMFS);
    let options = options(selection).with_geoapi(GeoapiProbe::Enabled(
        GeoapiHosts::new(hosts.clone()).unwrap(),
    ));
    let result = populated(server.scrape(options).await);
    assert_repository_data(&result, &names(&["dev.eessi.io", "software.eessi.io"]));
    assert!(!result.has_repository(&"riscv.eessi.io".parse().unwrap()));
    if exact {
        assert_eq!(result.repositories().len(), 2);
    }
    assert_geoapi(&result, &hosts);
}

#[rstest]
#[case::configured(ServerBackendType::S3, BackendResolution::ConfiguredS3)]
#[case::detected(
    ServerBackendType::AutoDetect,
    BackendResolution::AssumedS3IndexNotFound
)]
#[tokio::test]
#[ignore = "contacts the public EESSI S3 sync server; enable with the online-tests PR label"]
async fn online_s3_repositories_and_backend_resolution(
    #[case] backend: ServerBackendType,
    #[case] expected_backend: BackendResolution,
) {
    let server = server(S3_SYNC, ServerType::SyncServer, backend);
    let result = populated(
        server
            .scrape(options(RepositorySelection::only(names(
                &EESSI_REPOSITORIES,
            ))))
            .await,
    );
    assert_eq!(result.backend(), expected_backend);
    assert_eq!(result.repositories().len(), EESSI_REPOSITORIES.len());
    assert_repository_data(&result, &names(&EESSI_REPOSITORIES));
    assert!(result.metadata().schema_version().is_none());
    assert!(result.metadata().cvmfs_version().is_none());
    assert!(result.metadata().last_geodb_update().is_none());
    assert!(result.metadata().os_id().is_none());
    assert!(result.metadata().os_version_id().is_none());
    assert!(result.metadata().os_pretty_name().is_none());
    assert!(matches!(result.geoapi(), GeoapiOutcome::Unsupported));
}

#[tokio::test]
#[ignore = "contacts public EESSI CVMFS and S3 servers; enable with the online-tests PR label"]
async fn online_mixed_backends_preserve_configuration_order() {
    let servers = vec![
        server(
            AZURE_STRATUM1,
            ServerType::Stratum1,
            ServerBackendType::CVMFS,
        ),
        server(
            AWS_STRATUM1,
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
        ),
        server(S3_SYNC, ServerType::SyncServer, ServerBackendType::S3),
    ];
    let scraper = Scraper::new()
        .options(
            options(RepositorySelection::only(names(&EESSI_REPOSITORIES)))
                .with_geoapi(GeoapiProbe::Disabled),
        )
        .with_servers(servers.clone())
        .validate()
        .unwrap();
    let results = scraper.scrape().await;
    assert_eq!(results.len(), servers.len());
    for ((result, server), backend) in results.into_iter().zip(servers).zip([
        BackendResolution::ConfiguredCvmfs,
        BackendResolution::DiscoveredCvmfs,
        BackendResolution::ConfiguredS3,
    ]) {
        let result = populated(result);
        assert_eq!(result.server(), &server);
        assert_eq!(result.backend(), backend);
        assert_eq!(result.repositories().len(), EESSI_REPOSITORIES.len());
        assert_repository_data(&result, &names(&EESSI_REPOSITORIES));
        assert!(matches!(
            result.geoapi(),
            GeoapiOutcome::Skipped(GeoapiSkipReason::Disabled)
        ));
    }
}

#[tokio::test]
#[ignore = "contacts public EESSI CVMFS and S3 servers; enable with the online-tests PR label"]
async fn online_validated_scraper_can_be_reused_across_cycles() {
    let expected_names = names(&["dev.eessi.io", "software.eessi.io"]);
    let targets = [
        (
            server(
                AWS_STRATUM1,
                ServerType::Stratum1,
                ServerBackendType::AutoDetect,
            ),
            BackendResolution::DiscoveredCvmfs,
        ),
        (
            server(S3_SYNC, ServerType::SyncServer, ServerBackendType::S3),
            BackendResolution::ConfiguredS3,
        ),
    ];
    let options = options(RepositorySelection::only(expected_names.clone()))
        .with_geoapi(GeoapiProbe::Disabled);
    // A single shared permit forces queued requests to make progress in both runs.
    let limits = options
        .limits()
        .clone()
        .with_requests(ConcurrencyLimit::new(1).unwrap());
    let scraper = Scraper::new()
        .options(options.with_limits(limits))
        .with_servers(targets.iter().map(|(server, _)| server.clone()).collect())
        .validate()
        .unwrap();

    for cycle in 1..=2 {
        let results = scraper.scrape().await;
        assert_eq!(results.len(), targets.len(), "cycle {cycle}");
        for (result, (server, backend)) in results.into_iter().zip(&targets) {
            let result = populated(result);
            assert_eq!(result.server(), server, "cycle {cycle}: server order");
            assert_eq!(result.backend(), *backend, "cycle {cycle}: backend");
            assert!(
                result
                    .repositories()
                    .iter()
                    .map(|repository| repository.name())
                    .eq(expected_names.iter()),
                "cycle {cycle}: repository selection/order for {}",
                server.endpoint()
            );
            assert!(matches!(
                result.geoapi(),
                GeoapiOutcome::Skipped(GeoapiSkipReason::Disabled)
            ));
            // Each cycle must be valid independently: revisions and timestamps may change.
            assert_repository_data(&result, &expected_names);
        }
    }
}

#[tokio::test]
#[ignore = "contacts a public EESSI Stratum1 server; enable with the online-tests PR label"]
async fn online_stratum1_cannot_be_claimed_as_stratum0() {
    let server = server(AWS_STRATUM1, ServerType::Stratum0, ServerBackendType::CVMFS);
    let result = server.scrape(options(RepositorySelection::default())).await;
    let failure = result
        .as_failed_server()
        .expect("Stratum0 role must reject a replica index");
    assert!(matches!(
        failure.error(),
        CVMFSScraperError::Scrape(ScrapeError::ServerTypeMismatch(_))
    ));
}
