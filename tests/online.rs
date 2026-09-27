use cvmfs_server_scraper::*;

#[tokio::test]
#[ignore = "contacts a public CVMFS server; run explicitly"]
async fn online_cvmfs_smoke() {
    let server = Server::new(
        ServerType::Stratum1,
        ServerBackendType::AutoDetect,
        "http://aws-eu-central-s1.eessi.science".parse().unwrap(),
    );
    let result = server.scrape(ScrapeOptions::default()).await;
    let populated = result.into_populated_server().unwrap();
    assert_eq!(populated.backend(), BackendResolution::DiscoveredCvmfs);
    assert!(populated.has_repository(&"software.eessi.io".parse().unwrap()));
    assert!(matches!(populated.geoapi(), GeoapiOutcome::Available(_)));
}
#[tokio::test]
#[ignore = "contacts a public S3 backend; run explicitly"]
async fn online_s3_smoke() {
    let server = Server::new(
        ServerType::SyncServer,
        ServerBackendType::S3,
        "http://aws-eu-west-s1-sync.eessi.science".parse().unwrap(),
    );
    let options =
        ScrapeOptions::default().with_selection(RepositorySelection::only(["software.eessi.io"
            .parse()
            .unwrap()]));
    let populated = server
        .scrape(options)
        .await
        .into_populated_server()
        .unwrap();
    assert_eq!(populated.backend(), BackendResolution::ConfiguredS3);
    assert_eq!(populated.repositories().len(), 1);
    assert!(matches!(populated.geoapi(), GeoapiOutcome::Unsupported));
}
