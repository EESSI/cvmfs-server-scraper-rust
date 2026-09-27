mod support;
use cvmfs_server_scraper::*;
use rstest::rstest;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use support::*;
use tokio::sync::Semaphore;

#[tokio::test]
async fn forced_only_and_discovery_have_distinct_selection_rules() {
    let fixture = Fixture::new(|path| {
        if path.ends_with("repositories.json") {
            Reply::ok(index(&["discovered.org", "excluded.org"]))
        } else {
            normal_reply(path)
        }
    })
    .await;
    let options = ScrapeOptions::default()
        .with_selection(RepositorySelection::discover(
            [
                "forced.org".parse().unwrap(),
                "excluded.org".parse().unwrap(),
            ],
            ["excluded.org".parse().unwrap()],
        ))
        .with_geoapi(GeoapiProbe::Disabled);
    let server = fixture.server(ServerBackendType::CVMFS);
    let populated = server
        .scrape(options)
        .await
        .into_populated_server()
        .unwrap();
    assert_eq!(
        populated
            .repositories()
            .iter()
            .map(|r| r.name().as_str())
            .collect::<Vec<_>>(),
        ["discovered.org", "forced.org"]
    );
    let populated = server
        .scrape(only(&["excluded.org"]))
        .await
        .into_populated_server()
        .unwrap();
    assert_eq!(populated.repositories().len(), 1);
    assert_eq!(populated.repositories()[0].name().as_str(), "excluded.org");
    assert!(populated.has_repository(&"excluded.org".parse().unwrap()));
}
#[tokio::test]
async fn preflight_rejects_empty_effective_s3_selection_and_empty_servers() {
    let fixture = Fixture::new(normal_reply).await;
    let options = ScrapeOptions::default().with_selection(RepositorySelection::discover(
        ["example.org".parse().unwrap()],
        ["example.org".parse().unwrap()],
    ));
    assert!(Scraper::new()
        .options(options.clone())
        .with_servers(vec![fixture.server(ServerBackendType::S3)])
        .validate()
        .is_err());
    assert!(fixture
        .server(ServerBackendType::S3)
        .scrape(options)
        .await
        .is_failed());
    assert!(Scraper::new().with_servers(vec![]).validate().is_err());
    assert!(fixture.requests.paths().is_empty());
    let populated = fixture
        .server(ServerBackendType::S3)
        .scrape(only(&["example.org"]))
        .await
        .into_populated_server()
        .unwrap();
    assert_eq!(populated.backend(), BackendResolution::ConfiguredS3);
    assert!(fixture
        .requests
        .paths()
        .iter()
        .all(|p| !p.ends_with("repositories.json")));
}
#[rstest]
#[case::unauthenticated(401)]
#[case::forbidden(403)]
#[case::rate_limited(429)]
#[case::server_error(500)]
#[case::unavailable(503)]
#[tokio::test]
async fn autodetection_preserves_http_failures(#[case] status: u16) {
    let fixture = Fixture::new(move |_| Reply::status(status, "error")).await;
    let result = fixture
        .server(ServerBackendType::AutoDetect)
        .scrape(ScrapeOptions::default())
        .await;
    assert!(
        matches!(result.as_failed_server().unwrap().error(), CVMFSScraperError::Scrape(ScrapeError::HttpStatus {status: actual, ..}) if *actual == status)
    );
    assert_eq!(fixture.requests.paths().len(), 1);
}

#[tokio::test]
async fn autodetection_only_assumes_s3_on_404() {
    let fixture = Fixture::new(|path| {
        if path.ends_with("repositories.json") {
            Reply::status(404, "")
        } else {
            normal_reply(path)
        }
    })
    .await;
    let populated = fixture
        .server(ServerBackendType::AutoDetect)
        .scrape(only(&["example.org"]))
        .await
        .into_populated_server()
        .unwrap();
    assert_eq!(
        populated.backend(),
        BackendResolution::AssumedS3IndexNotFound
    );
    assert!(fixture
        .server(ServerBackendType::AutoDetect)
        .scrape(ScrapeOptions::default())
        .await
        .is_failed());
    assert!(fixture
        .server(ServerBackendType::CVMFS)
        .scrape(only(&["example.org"]))
        .await
        .is_failed());
}
#[tokio::test]
async fn invalid_discovery_names_schemas_counts_and_roles_fail_before_repository_fetches() {
    for body in [
        serde_json::json!({"schema":1,"repositories":[],"replicas":[{"name":"../../admin?x="}]}),
        serde_json::json!({"schema":2,"repositories":[],"replicas":[{"name":"example.org"}]}),
        serde_json::json!({"schema":1,"repositories":[{"name":"example.org"}],"replicas":[]}),
        serde_json::json!({"schema":1,"repositories":[{"name":"primary.org"}],"replicas":[{"name":"replica.org"}]}),
    ] {
        let fixture = Fixture::new(move |_| Reply::ok(serde_json::to_vec(&body).unwrap())).await;
        assert!(fixture
            .server(ServerBackendType::CVMFS)
            .scrape(ScrapeOptions::default())
            .await
            .is_failed());
        assert_eq!(fixture.requests.paths().len(), 1);
    }
    let fixture = Fixture::new(|_| Reply::ok(index(&["a.org", "b.org"]))).await;
    let options = ScrapeOptions::default().with_limits(
        ScrapeLimits::default().with_repository_count(RepositoryLimit::new(1).unwrap()),
    );
    let result = fixture
        .server(ServerBackendType::CVMFS)
        .scrape(options)
        .await;
    assert!(matches!(
        result.as_failed_server().unwrap().error(),
        CVMFSScraperError::Scrape(ScrapeError::RepositoryLimit { .. })
    ));
    assert_eq!(fixture.requests.paths().len(), 1);
}
#[tokio::test]
async fn manifest_identity_mismatch_and_malformed_bytes_fail_without_panics() {
    for bytes in [
        manifest("other.org"),
        b"\n".to_vec(),
        "évalue\n".as_bytes().to_vec(),
    ] {
        let fixture = Fixture::new(move |path| {
            if path.ends_with(".cvmfspublished") {
                Reply::ok(&bytes)
            } else {
                normal_reply(path)
            }
        })
        .await;
        let result = fixture
            .server(ServerBackendType::S3)
            .scrape(only(&["example.org"]))
            .await;
        assert!(matches!(
            result.as_failed_server().unwrap().error(),
            CVMFSScraperError::Manifest { .. }
        ));
    }
}
#[tokio::test]
async fn scraper_preserves_binary_manifest_signature() {
    let tail = b"checksum\n\xff\x00\r\n--\n\xfe\n";
    let fixture = Fixture::new(move |path| {
        if path.ends_with(".cvmfspublished") {
            Reply::ok([manifest("example.org").as_slice(), b"--\n", tail].concat())
        } else {
            normal_reply(path)
        }
    })
    .await;
    let populated = fixture
        .server(ServerBackendType::S3)
        .scrape(only(&["example.org"]))
        .await
        .into_populated_server()
        .unwrap();
    assert_eq!(
        populated.repositories()[0]
            .manifest()
            .manifest()
            .signature()
            .unwrap()
            .as_bytes(),
        tail
    );
}
#[tokio::test]
async fn valid_geoapi_is_mapped_and_invalid_geoapi_preserves_repository_data() {
    let fixture = Fixture::new(normal_reply).await;
    let populated = fixture
        .server(ServerBackendType::AutoDetect)
        .scrape(ScrapeOptions::default())
        .await
        .into_populated_server()
        .unwrap();
    assert_eq!(populated.backend(), BackendResolution::DiscoveredCvmfs);
    let GeoapiOutcome::Available(query) = populated.geoapi() else {
        panic!("expected available GeoAPI")
    };
    assert_eq!(
        query.map_response_order_to_geoapi_hostnames(),
        *DEFAULT_GEOAPI_SERVERS
    );
    assert!(query.check_against_expected_order_by_hostname(&DEFAULT_GEOAPI_SERVERS));
    for bad in ["0,1,2", "1,1,2", "1,2,4", "1,2"] {
        let fixture = Fixture::new(move |path| {
            if path.contains("/geo/") {
                Reply::ok(bad)
            } else {
                normal_reply(path)
            }
        })
        .await;
        let populated = fixture
            .server(ServerBackendType::CVMFS)
            .scrape(ScrapeOptions::default())
            .await
            .into_populated_server()
            .unwrap();
        assert_eq!(populated.repositories().len(), 1);
        assert!(matches!(populated.geoapi(), GeoapiOutcome::Failed(_)));
    }
}
#[tokio::test]
async fn optional_metadata_has_distinct_available_absent_and_failed_states() {
    for status in [200, 404, 500] {
        let fixture = Fixture::new(move |path| {
            if path.ends_with("meta.json") {
                Reply::status(status, r#"{"administrator":"operator"}"#)
            } else {
                normal_reply(path)
            }
        })
        .await;
        let populated = fixture
            .server(ServerBackendType::S3)
            .scrape(only(&["example.org"]))
            .await
            .into_populated_server()
            .unwrap();
        match (status, populated.contact()) {
            (200, OptionalFetch::Available(metadata)) => {
                assert_eq!(metadata.administrator.as_deref(), Some("operator"))
            }
            (404, OptionalFetch::Absent) | (500, OptionalFetch::Failed(_)) => {}
            other => panic!("unexpected metadata outcome: {other:?}"),
        }
        assert_eq!(populated.repositories().len(), 1);
    }
    let fixture = Fixture::new(|path| {
        if path.ends_with("meta.json") {
            Reply::ok("not JSON")
        } else {
            normal_reply(path)
        }
    })
    .await;
    let populated = fixture
        .server(ServerBackendType::S3)
        .scrape(only(&["example.org"]))
        .await
        .into_populated_server()
        .unwrap();
    assert!(matches!(
        populated.contact(),
        OptionalFetch::Failed(ScrapeError::Json { .. })
    ));
}
#[tokio::test]
async fn stratum0_and_s3_do_not_probe_geoapi() {
    let fixture = Fixture::new(|path| {
        if path.ends_with("repositories.json") {
            Reply::ok(r#"{"schema":1,"repositories":[{"name":"example.org"}],"replicas":[]}"#)
        } else {
            normal_reply(path)
        }
    })
    .await;
    let server = Server::new(
        ServerType::Stratum0,
        ServerBackendType::CVMFS,
        fixture.endpoint.clone(),
    );
    let populated = server
        .scrape(ScrapeOptions::default())
        .await
        .into_populated_server()
        .unwrap();
    assert!(matches!(
        populated.geoapi(),
        GeoapiOutcome::Skipped(GeoapiSkipReason::Stratum0)
    ));
    let options = ScrapeOptions::default()
        .with_selection(RepositorySelection::only(["example.org".parse().unwrap()]));
    let populated = fixture
        .server(ServerBackendType::S3)
        .scrape(options)
        .await
        .into_populated_server()
        .unwrap();
    assert!(matches!(populated.geoapi(), GeoapiOutcome::Unsupported));
    assert!(!fixture.requests.paths().iter().any(|p| p.contains("/geo/")));
}
#[tokio::test]
async fn redirects_never_leave_the_configured_origin() {
    let destination = Fixture::new(normal_reply).await;
    for policy in [RedirectPolicy::None, RedirectPolicy::SameOrigin] {
        let target = destination.endpoint.to_string();
        let fixture = Fixture::new(move |_| Reply::redirect(&format!("{target}private"))).await;
        let result = fixture
            .server(ServerBackendType::S3)
            .scrape(only(&["example.org"]).with_redirects(policy))
            .await;
        assert!(result.is_failed());
    }
    assert!(destination.requests.paths().is_empty());
}
#[tokio::test]
async fn same_origin_redirects_are_opt_in_and_bounded() {
    let fixture = Fixture::new(|path| {
        if path.ends_with(".cvmfspublished") {
            Reply::redirect("/manifest")
        } else if path == "/manifest" {
            Reply::ok(manifest("example.org"))
        } else {
            normal_reply(path)
        }
    })
    .await;
    assert!(fixture
        .server(ServerBackendType::S3)
        .scrape(only(&["example.org"]))
        .await
        .is_failed());
    assert!(fixture
        .server(ServerBackendType::S3)
        .scrape(only(&["example.org"]).with_redirects(RedirectPolicy::SameOrigin))
        .await
        .is_populated());
    let looping = Fixture::new(|_| Reply::redirect("/loop")).await;
    assert!(looping
        .server(ServerBackendType::AutoDetect)
        .scrape(ScrapeOptions::default().with_redirects(RedirectPolicy::SameOrigin))
        .await
        .is_failed());
    assert!(looping.requests.paths().len() <= 6);
}
#[tokio::test]
async fn response_limits_cover_content_length_chunking_and_truncated_bodies() {
    for wire in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n".to_vec(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n20\r\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n20\r\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\r\n0\r\n\r\n".to_vec(),
    ] {
        let fixture = Fixture::new(move |path| if path.ends_with(".cvmfspublished") { Reply::raw(wire.clone()) } else { normal_reply(path) }).await;
        let options = only(&["example.org"]).with_limits(ScrapeLimits::default().with_manifest_bytes(ResponseByteLimit::new(40).unwrap()));
        let result = fixture.server(ServerBackendType::S3).scrape(options).await;
        assert!(matches!(result.as_failed_server().unwrap().error(), CVMFSScraperError::Scrape(ScrapeError::BodyTooLarge {limit:40,..})));
    }
    let fixture = Fixture::new(|path| {
        if path.ends_with(".cvmfspublished") {
            Reply::raw(
                b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort"
                    .to_vec(),
            )
        } else {
            normal_reply(path)
        }
    })
    .await;
    assert!(fixture
        .server(ServerBackendType::S3)
        .scrape(only(&["example.org"]))
        .await
        .is_failed());
}
#[tokio::test]
async fn stalled_requests_have_request_and_server_deadlines() {
    let fixture = Fixture::new(|_| Reply::ok("{}").delayed(Duration::from_secs(10))).await;
    let limits = ScrapeLimits::default()
        .with_request_timeout(RequestTimeout::new(Duration::from_millis(50)).unwrap());
    let result = fixture
        .server(ServerBackendType::AutoDetect)
        .scrape(ScrapeOptions::default().with_limits(limits))
        .await;
    assert!(
        matches!(result.as_failed_server().unwrap().error(), CVMFSScraperError::Scrape(ScrapeError::Fetch {source,..}) if source.is_timeout())
    );
    let limits = ScrapeLimits::default()
        .with_server_timeout(RequestTimeout::new(Duration::from_millis(50)).unwrap());
    let result = fixture
        .server(ServerBackendType::AutoDetect)
        .scrape(ScrapeOptions::default().with_limits(limits))
        .await;
    assert!(matches!(
        result.as_failed_server().unwrap().error(),
        CVMFSScraperError::Scrape(ScrapeError::Timeout(_))
    ));
}
#[tokio::test]
async fn repository_fetches_are_concurrent_but_respect_the_repository_limit() {
    let gate = Arc::new(Semaphore::new(0));
    let held = gate.clone();
    let fixture = Fixture::new(move |path| {
        let reply = normal_reply(path);
        if path.contains("/.cvmfs") {
            reply.gated(held.clone())
        } else {
            reply
        }
    })
    .await;
    let server = fixture.server(ServerBackendType::S3);
    let options = only(&["a.org", "b.org", "c.org"])
        .with_limits(ScrapeLimits::default().with_repositories(ConcurrencyLimit::new(2).unwrap()));
    let task = tokio::spawn(async move { server.scrape(options).await });
    fixture.requests.wait_for(4).await;
    assert_eq!(fixture.requests.paths().len(), 4);
    assert_eq!(fixture.requests.peak(), 4);
    assert!(fixture
        .requests
        .paths()
        .iter()
        .all(|p| !p.contains("c.org")));
    gate.add_permits(100);
    let result = task.await.unwrap().into_populated_server().unwrap();
    assert_eq!(
        result
            .repositories()
            .iter()
            .map(|r| r.name().as_str())
            .collect::<Vec<_>>(),
        ["a.org", "b.org", "c.org"]
    );
}
#[tokio::test]
async fn global_request_limit_applies_across_servers() {
    let gate = Arc::new(Semaphore::new(0));
    let held = gate.clone();
    let fixture = Fixture::new(move |path| normal_reply(path).gated(held.clone())).await;
    let servers = vec![fixture.server(ServerBackendType::S3); 3];
    let options = only(&["a.org", "b.org"])
        .with_limits(ScrapeLimits::default().with_requests(ConcurrencyLimit::new(3).unwrap()));
    let scraper = Scraper::new()
        .options(options)
        .with_servers(servers)
        .validate()
        .unwrap();
    let task = tokio::spawn(async move { scraper.scrape().await });
    fixture.requests.wait_for(3).await;
    assert_eq!(fixture.requests.paths().len(), 3);
    assert_eq!(fixture.requests.peak(), 3);
    gate.add_permits(100);
    assert!(task.await.unwrap().iter().all(ScrapedServer::is_populated));
    assert!(fixture.requests.peak() <= 3);
}
#[tokio::test]
async fn server_limit_preserves_input_order() {
    let gate = Arc::new(Semaphore::new(0));
    let held = gate.clone();
    let first = Fixture::new(move |path| normal_reply(path).gated(held.clone())).await;
    let second = Fixture::new(normal_reply).await;
    let options = only(&["example.org"])
        .with_limits(ScrapeLimits::default().with_servers(ConcurrencyLimit::new(1).unwrap()));
    let scraper = Scraper::new()
        .options(options)
        .with_servers(vec![
            first.server(ServerBackendType::S3),
            second.server(ServerBackendType::S3),
        ])
        .validate()
        .unwrap();
    let task = tokio::spawn(async move { scraper.scrape().await });
    first.requests.wait_for(2).await;
    assert!(second.requests.paths().is_empty());
    gate.add_permits(100);
    let results = task.await.unwrap();
    assert_eq!(
        results[0]
            .as_populated_server()
            .unwrap()
            .server()
            .endpoint(),
        &first.endpoint
    );
    assert_eq!(
        results[1]
            .as_populated_server()
            .unwrap()
            .server()
            .endpoint(),
        &second.endpoint
    );
}
#[tokio::test]
async fn cancellation_releases_shared_request_permits_for_the_next_run() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let fixture = Fixture::new(move |path| {
        let reply = normal_reply(path);
        if observed.fetch_add(1, Ordering::SeqCst) == 0 {
            reply.delayed(Duration::from_secs(10))
        } else {
            reply
        }
    })
    .await;
    let limits = ScrapeLimits::default()
        .with_requests(ConcurrencyLimit::new(1).unwrap())
        .with_server_timeout(RequestTimeout::new(Duration::from_millis(200)).unwrap());
    let scraper = Scraper::new()
        .options(only(&["example.org"]).with_limits(limits))
        .with_servers(vec![fixture.server(ServerBackendType::S3)])
        .validate()
        .unwrap();
    assert!(scraper.scrape().await[0].is_failed());
    assert!(scraper.scrape().await[0].is_populated());
}

#[tokio::test]
async fn response_limits_apply_to_each_resource_kind() {
    let fixture = Fixture::new(normal_reply).await;
    let options = ScrapeOptions::default()
        .with_limits(ScrapeLimits::default().with_index_bytes(ResponseByteLimit::new(16).unwrap()));
    let result = fixture
        .server(ServerBackendType::CVMFS)
        .scrape(options)
        .await;
    assert!(matches!(
        result.as_failed_server().unwrap().error(),
        CVMFSScraperError::Scrape(ScrapeError::BodyTooLarge { limit: 16, .. })
    ));

    let fixture = Fixture::new(|path| {
        if path.ends_with("meta.json") {
            Reply::ok(r#"{"administrator":"operator"}"#)
        } else {
            normal_reply(path)
        }
    })
    .await;
    let options = ScrapeOptions::default().with_limits(
        ScrapeLimits::default()
            .with_metadata_bytes(ResponseByteLimit::new(2).unwrap())
            .with_geoapi_bytes(ResponseByteLimit::new(1).unwrap()),
    );
    let populated = fixture
        .server(ServerBackendType::CVMFS)
        .scrape(options)
        .await
        .into_populated_server()
        .unwrap();
    assert!(matches!(
        populated.contact(),
        OptionalFetch::Failed(ScrapeError::BodyTooLarge { limit: 2, .. })
    ));
    assert!(matches!(
        populated.geoapi(),
        GeoapiOutcome::Failed(ScrapeError::BodyTooLarge { limit: 1, .. })
    ));
    assert_eq!(populated.repositories().len(), 1);
}

#[tokio::test]
async fn read_timeout_is_enforced_independently_of_total_deadlines() {
    let fixture = Fixture::new(|_| Reply::ok("{}").delayed(Duration::from_secs(10))).await;
    let options = ScrapeOptions::default().with_limits(
        ScrapeLimits::default()
            .with_read_timeout(RequestTimeout::new(Duration::from_millis(50)).unwrap()),
    );
    let result = fixture
        .server(ServerBackendType::CVMFS)
        .scrape(options)
        .await;
    assert!(
        matches!(result.as_failed_server().unwrap().error(), CVMFSScraperError::Scrape(ScrapeError::Fetch { source, .. }) if source.is_timeout())
    );
}

#[tokio::test]
async fn result_accessors_and_server_metadata_remain_consistent() {
    let fixture = Fixture::new(|path| if path.ends_with("repositories.json") {
        Reply::ok(r#"{"schema":1,"cvmfs_version":"2.11.3-1","last_geodb_update":"Fri Jun 21 17:40:02 UTC 2024","os_id":"rhel","os_version_id":"9","os_pretty_name":"Red Hat","repositories":[],"replicas":[{"name":"example.org"}]}"#)
    } else if path.ends_with(".cvmfs_status.json") {
        Reply::ok(r#"{"last_snapshot":"Fri Jun 21 17:40:02 UTC 2024","last_gc":"unresolved local date"}"#)
    } else { normal_reply(path) }).await;
    let result = fixture
        .server(ServerBackendType::CVMFS)
        .scrape(only(&["example.org"]))
        .await;
    assert!(result.is_ok());
    assert!(!result.is_failed());
    assert!(result.as_failed_server().is_none());
    assert!(result.clone().into_failed_server().is_err());
    let populated = result.into_populated_server().unwrap();
    assert_eq!(populated.backend(), BackendResolution::ConfiguredCvmfs);
    assert_eq!(populated.metadata().schema_version(), Some(1));
    assert_eq!(populated.metadata().os_id(), Some("rhel"));
    assert_eq!(populated.metadata().os_version_id(), Some("9"));
    assert_eq!(populated.metadata().os_pretty_name(), Some("Red Hat"));
    assert_eq!(
        populated.metadata().cvmfs_version().unwrap().to_string(),
        "2.11.3-1"
    );
    let repository = &populated.repositories()[0];
    assert_eq!(repository.revision().get(), 42);
    assert_eq!(
        repository
            .last_snapshot()
            .unwrap()
            .datetime()
            .unwrap()
            .to_rfc3339(),
        "2024-06-21T17:40:02+00:00"
    );
    assert_eq!(
        repository.last_gc().unwrap().as_str(),
        "unresolved local date"
    );
    assert!(repository.last_gc().unwrap().datetime().is_none());
    assert_eq!(
        serde_json::to_value(populated.metadata()).unwrap()["cvmfs_version"],
        "2.11.3-1"
    );
    assert!(populated
        .metadata()
        .last_geodb_update()
        .unwrap()
        .datetime()
        .is_some());
    let failure = fixture
        .server(ServerBackendType::S3)
        .scrape(only(&[]))
        .await;
    assert!(failure.is_failed());
    assert!(failure.as_populated_server().is_none());
    assert!(failure.clone().into_populated_server().is_err());
    assert!(failure.into_failed_server().is_ok());
}

#[rstest]
#[case::only(RepositorySelection::only(["a.org".parse().unwrap(), "b.org".parse().unwrap()]))]
#[case::discovery(RepositorySelection::discover(["a.org".parse().unwrap()], ["b.org".parse().unwrap()]))]
#[tokio::test]
async fn configured_repository_limit_fails_before_network_work(
    #[case] selection: RepositorySelection,
) {
    let fixture = Fixture::new(normal_reply).await;
    let result = Scraper::new()
        .options(ScrapeOptions::default().with_limits(
            ScrapeLimits::default().with_repository_count(RepositoryLimit::new(1).unwrap()),
        ))
        .with_servers(vec![fixture.server(ServerBackendType::CVMFS)])
        .repository_selection(selection)
        .validate();
    assert!(matches!(
        result,
        Err(ScrapeError::RepositoryLimit {
            actual: 2,
            limit: 1
        })
    ));
    assert!(fixture.requests.paths().is_empty());
}

#[tokio::test]
async fn discovered_and_configured_union_also_obeys_repository_limit() {
    let fixture = Fixture::new(normal_reply).await;
    let options = ScrapeOptions::default()
        .with_selection(RepositorySelection::discover(
            ["additional.org".parse().unwrap()],
            [],
        ))
        .with_limits(
            ScrapeLimits::default().with_repository_count(RepositoryLimit::new(1).unwrap()),
        );
    let result = fixture
        .server(ServerBackendType::CVMFS)
        .scrape(options)
        .await;
    assert!(matches!(
        result.as_failed_server().unwrap().error(),
        CVMFSScraperError::Scrape(ScrapeError::RepositoryLimit {
            actual: 2,
            limit: 1
        })
    ));
    assert_eq!(
        fixture.requests.paths(),
        ["/cvmfs/info/v1/repositories.json"]
    );
}

#[rstest]
#[case::missing_status(".cvmfs_status.json", 404)]
#[case::failed_manifest(".cvmfspublished", 503)]
#[tokio::test]
async fn required_http_failures_do_not_discard_other_servers(
    #[case] resource: &'static str,
    #[case] status: u16,
) {
    let failed = Fixture::new(move |path| {
        if path.ends_with(resource) {
            Reply::status(status, "error")
        } else {
            normal_reply(path)
        }
    })
    .await;
    let healthy = Fixture::new(normal_reply).await;
    let scraper = Scraper::default()
        .with_servers(vec![
            failed.server(ServerBackendType::S3),
            healthy.server(ServerBackendType::S3),
        ])
        .repository_selection(RepositorySelection::only(["example.org".parse().unwrap()]))
        .geoapi(GeoapiProbe::Disabled)
        .validate()
        .unwrap();
    assert_eq!(scraper.plan().servers().len(), 2);
    assert!(matches!(
        scraper.plan().options().geoapi(),
        GeoapiProbe::Disabled
    ));
    let results = scraper.scrape().await;
    let failure = results[0].as_failed_server().unwrap();
    assert_eq!(failure.server().endpoint(), &failed.endpoint);
    assert!(
        matches!(failure.error(), CVMFSScraperError::Scrape(ScrapeError::HttpStatus {status: actual, url})
        if *actual == status && *url == format!("{}cvmfs/example.org/{resource}", failed.endpoint))
    );
    assert!(!failed
        .requests
        .paths()
        .iter()
        .any(|path| path.ends_with("meta.json")));
    let populated = results[1].as_populated_server().unwrap();
    assert_eq!(populated.server().endpoint(), &healthy.endpoint);
    assert_eq!(populated.repositories().len(), 1);
}

#[rstest]
#[case::index("repositories.json", ServerBackendType::AutoDetect)]
#[case::status(".cvmfs_status.json", ServerBackendType::S3)]
#[tokio::test]
async fn invalid_required_json_preserves_url_and_source(
    #[case] resource: &'static str,
    #[case] backend: ServerBackendType,
) {
    let fixture = Fixture::new(move |path| {
        if path.ends_with(resource) {
            Reply::ok("not JSON")
        } else {
            normal_reply(path)
        }
    })
    .await;
    let result = fixture.server(backend).scrape(only(&["example.org"])).await;
    let error = result.as_failed_server().unwrap().error();
    let CVMFSScraperError::Scrape(ScrapeError::Json { url, source }) = error else {
        panic!("expected JSON error, got {error}");
    };
    assert!(url.starts_with(fixture.endpoint.as_url().as_str()));
    assert!(url.ends_with(resource));
    assert!(source.is_syntax());
    assert!(std::error::Error::source(error).is_some());
    if backend == ServerBackendType::AutoDetect {
        assert_eq!(fixture.requests.paths().len(), 1);
    }
}

#[rstest]
#[case::stratum0_with_replicas(ServerType::Stratum0, true, false)]
#[case::sync_with_primaries(ServerType::SyncServer, false, false)]
#[case::sync_with_replicas(ServerType::SyncServer, true, true)]
#[tokio::test]
async fn index_role_validation_applies_to_stratum0_and_sync_servers(
    #[case] role: ServerType,
    #[case] replicas: bool,
    #[case] accepted: bool,
) {
    let fixture = Fixture::new(move |path| {
        if path.ends_with("repositories.json") && !replicas {
            Reply::ok(r#"{"schema":1,"repositories":[{"name":"example.org"}],"replicas":[]}"#)
        } else {
            normal_reply(path)
        }
    })
    .await;
    let server = Server::new(role, ServerBackendType::CVMFS, fixture.endpoint.clone());
    assert_eq!(server.server_type(), role);
    let result = server.scrape(only(&["example.org"])).await;
    assert_eq!(result.is_populated(), accepted);
    if !accepted {
        assert!(matches!(
            result.as_failed_server().unwrap().error(),
            CVMFSScraperError::Scrape(ScrapeError::ServerTypeMismatch(_))
        ));
        assert_eq!(fixture.requests.paths().len(), 1);
    }
}

#[tokio::test]
async fn empty_cvmfs_selection_skips_geoapi_without_inventing_a_repository() {
    let fixture = Fixture::new(normal_reply).await;
    let populated = fixture
        .server(ServerBackendType::CVMFS)
        .scrape(ScrapeOptions::default().with_selection(RepositorySelection::only([])))
        .await
        .into_populated_server()
        .unwrap();
    assert!(populated.repositories().is_empty());
    assert!(matches!(
        populated.geoapi(),
        GeoapiOutcome::Skipped(GeoapiSkipReason::NoRepositories)
    ));
    assert!(!fixture
        .requests
        .paths()
        .iter()
        .any(|path| path.contains("/geo/") || path.contains("/.cvmfs")));
}

#[tokio::test]
async fn custom_geoapi_hosts_are_bound_to_the_query_and_response() {
    let fixture = Fixture::new(|path| {
        if path.contains("/geo/") {
            Reply::ok("2,1")
        } else {
            normal_reply(path)
        }
    })
    .await;
    let hosts = vec!["two.org".parse().unwrap(), "one.org".parse().unwrap()];
    let populated = Scraper::new()
        .geoapi(GeoapiProbe::Enabled(
            GeoapiHosts::new(hosts.clone()).unwrap(),
        ))
        .with_servers(vec![fixture.server(ServerBackendType::CVMFS)])
        .validate()
        .unwrap()
        .scrape()
        .await
        .remove(0)
        .into_populated_server()
        .unwrap();
    let GeoapiOutcome::Available(query) = populated.geoapi() else {
        panic!("expected GeoAPI result")
    };
    assert_eq!(query.endpoint(), &fixture.endpoint);
    assert_eq!(query.ordering().hosts(), hosts);
    assert!(query.check_against_expected_order_by_id(&[
        GeoapiHostId::new(2).unwrap(),
        GeoapiHostId::new(1).unwrap()
    ]));
    assert!(!query.check_against_expected_order_by_hostname(&hosts));
    assert!(fixture
        .requests
        .paths()
        .iter()
        .any(|path| path.ends_with("/two.org,one.org")));
}

#[rstest]
#[case::non_utf8(Reply::ok(b"\xff\xfe"))]
#[case::http_error(Reply::status(503, "unavailable"))]
#[tokio::test]
async fn geoapi_transport_and_encoding_failures_preserve_repositories(#[case] reply: Reply) {
    let fixture = Fixture::new(move |path| {
        if path.contains("/geo/") {
            Reply::raw(reply.bytes.clone())
        } else {
            normal_reply(path)
        }
    })
    .await;
    let populated = fixture
        .server(ServerBackendType::CVMFS)
        .scrape(ScrapeOptions::default())
        .await
        .into_populated_server()
        .unwrap();
    assert_eq!(populated.repositories().len(), 1);
    let GeoapiOutcome::Failed(error) = populated.geoapi() else {
        panic!("expected GeoAPI failure")
    };
    assert!(error
        .to_string()
        .contains("/cvmfs/example.org/api/v1.0/geo/"));
}

#[rstest]
#[case::content_length(false)]
#[case::chunked(true)]
#[tokio::test]
async fn manifest_body_exactly_at_the_limit_is_accepted(#[case] chunked: bool) {
    let body = manifest("example.org");
    let limit = body.len();
    let fixture = Fixture::new(move |path| {
        if path.ends_with(".cvmfspublished") {
            if chunked {
                let head = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", body.len());
                Reply::raw([head.as_bytes(), &body, b"\r\n0\r\n\r\n"].concat())
            } else { Reply::ok(&body) }
        } else { normal_reply(path) }
    }).await;
    let options = only(&["example.org"]).with_limits(
        ScrapeLimits::default().with_manifest_bytes(ResponseByteLimit::new(limit).unwrap()),
    );
    assert!(fixture
        .server(ServerBackendType::S3)
        .scrape(options)
        .await
        .is_populated());
}
