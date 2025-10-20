//! A library for scraping CVMFS servers and extracting their metadata.
//!
//! CVMFS servers provide a number of public metadata files that can be scraped to extract information about the server and its repositories.
//! However, some of these files are not required to be present, depending on the server backend or its administrators, and even in files present,
//! a number of keys are optional. This library provides a way to scrape these files and extract the metadata in a structured way.
//!
//! The following files are currently supported:
//!
//! - cvmfs/info/v1/repositories.json : The list of repositories and replicas hosted on the server (not present on servers with S3 backends)
//! - cvmfs/info/v1/meta.json : Contact points and human-generated metadata about the server (optional)
//!
//! And for each repository, it fetches:
//!
//! - cvmfs/\<repo\>/.cvmfs_status.json : Information about the last garbage collection and snapshot.
//! - cvmfs/\<repo\>/.cvmfspublished : Manifest of the repository.
//!
//! Due to the nature of repositories.json, one may force repositories to be scraped by providing an explicit list of repositories by name.
//!
//! # Examples
//!
//! ```no_run
//! use cvmfs_server_scraper::{Hostname, Server, ServerBackendType, ServerType,
//!     ScrapedServer, ScraperCommon, Scraper, CVMFSScraperError, DEFAULT_GEOAPI_SERVERS};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), CVMFSScraperError> {
//!     let servers = vec![
//!         Server::new(
//!             ServerType::Stratum1,
//!             ServerBackendType::CVMFS,
//!             Hostname::try_from("azure-us-east-s1.eessi.science").unwrap(),
//!         ),
//!         Server::new(
//!             ServerType::Stratum1,
//!             ServerBackendType::AutoDetect,
//!             Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
//!         ),
//!         Server::new(
//!             ServerType::SyncServer,
//!             ServerBackendType::S3,
//!             Hostname::try_from("aws-eu-west-s1-sync.eessi.science").unwrap(),
//!         ),
//!     ];
//!
//!     let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
//!     let ignored_repos = vec!["nope.eessi.io"];
//!
//!    // Build a Scraper and scrape all servers in parallel
//!    let scraped_servers = Scraper::new()
//!       .forced_repositories(repolist)
//!       .ignored_repositories(ignored_repos)
//!       .only_scrape_forced_repositories(false) // Only scrape forced repositories if true, overrides ignored_repositories, default false
//!       .geoapi_servers(DEFAULT_GEOAPI_SERVERS.clone())? // This is the default list
//!       .with_servers(servers) // Transitions to a WithServer state.
//!       .validate()? // Transitions to a ValidatedAndReady state, now immutable.
//!       .scrape().await; // Perform the scrape, return servers.
//!
//!    for server in scraped_servers {
//!        match server {
//!            ScrapedServer::Populated(populated_server) => {
//!                 println!("{}", populated_server);
//!                 populated_server.output();
//!                 println!();
//!            }
//!            ScrapedServer::Failed(failed_server) => {
//!                panic!("Error! {} failed scraping: {:?}", failed_server.hostname, failed_server.error);
//!            }
//!       }
//!     }
//!     Ok(())
//! }
//! ```

mod constants;
mod errors;
mod models;
mod scraper;
mod utilities;

pub use constants::DEFAULT_GEOAPI_SERVERS;
pub use errors::{CVMFSScraperError, HostnameError, ManifestError, ScrapeError};
pub use models::{
    FailedServer, GeoapiServerQuery, Hostname, Manifest, MaybeRfc2822DateTime,
    PopulatedRepositoryOrReplica, PopulatedServer, ScrapedServer, Server, ServerBackendType,
    ServerMetadata, ServerType,
};
pub use scraper::{Scraper, ScraperCommon};

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    use futures::future::join_all;

    #[tokio::test]
    async fn test_online_cvmfs_servers_manually() {
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

        let futures = servers.into_iter().map(|server| {
            let repolist = repolist.clone();
            async move {
                match server.scrape(repolist.clone(), vec![], false, None).await {
                    ScrapedServer::Populated(popserver) => {
                        for repo in repolist {
                            assert!(popserver.has_repository(repo));
                        }
                    }
                    ScrapedServer::Failed(failedserver) => {
                        panic!("Error: {:?}", failedserver.error);
                    }
                }
            }
        });

        join_all(futures).await;
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_mismatch_s0_is_s1() {
        let server = Server::new(
            ServerType::Stratum0,
            ServerBackendType::CVMFS,
            Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io"];

        match server.scrape(repolist.clone(), vec![], false, None).await {
            ScrapedServer::Populated(_) => {
                panic!("Error, should not have succeeded");
            }
            ScrapedServer::Failed(failedserver) => {
                assert_eq!(failedserver.error.to_string(), "Scrape error: Server type mismatch: aws-eu-central-s1.eessi.science is a Stratum0 server, but replicas were found in the repositories.json");
            }
        }
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_s1_has_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::CVMFS,
            Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let repoparams: Vec<String> = Vec::new();
        let servers = server.scrape(repoparams, vec![], false, None).await;
        for repo in repolist {
            match servers.clone() {
                ScrapedServer::Populated(popserver) => {
                    assert!(popserver.has_repository(repo))
                }
                ScrapedServer::Failed(failedserver) => {
                    panic!("Error: {:?}", failedserver.error);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_autodetect_s1_with_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
            Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let popserver = server
            .scrape(repolist.clone(), vec![], false, None)
            .await
            .get_populated_server()
            .unwrap();
        assert_eq!(popserver.backend_type, ServerBackendType::AutoDetect);
        assert_eq!(popserver.backend_detected, ServerBackendType::CVMFS);
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_autodetect_s1_without_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
            Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
        );

        let repoparams: Vec<String> = Vec::new();
        let popserver = server.scrape(repoparams, vec![], false, None).await;
        assert!(popserver.is_ok());
        let popserver = popserver.get_populated_server().unwrap();
        assert_eq!(popserver.backend_type, ServerBackendType::AutoDetect);
        assert_eq!(popserver.backend_detected, ServerBackendType::CVMFS);
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_autodetect_s3_with_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
            Hostname::try_from("aws-eu-west-s1-sync.eessi.science").unwrap(),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let popserver = server
            .scrape(repolist.clone(), vec![], false, None)
            .await
            .get_populated_server()
            .unwrap();
        assert_eq!(popserver.backend_type, ServerBackendType::AutoDetect);
        assert_eq!(popserver.backend_detected, ServerBackendType::S3);
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_s1_cvmfs_backend_metadata() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::CVMFS,
            Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let popserver = server
            .scrape(repolist.clone(), vec![], false, None)
            .await
            .get_populated_server()
            .unwrap();
        assert!(popserver.metadata.schema_version.is_some());
        assert!(popserver.metadata.cvmfs_version.is_some());
        assert!(popserver.metadata.last_geodb_update.is_some());
        assert!(popserver.metadata.os_version_id.is_some());
        assert!(popserver.metadata.os_pretty_name.is_some());
        assert!(popserver.metadata.os_id.is_some());
        assert_eq!(
            popserver.metadata.administrator,
            Some("EESSI CVMFS Administrators".to_string())
        );
        assert_eq!(
            popserver.metadata.email,
            Some("support@eessi.io".to_string())
        );
        assert_eq!(popserver.metadata.organisation, Some("EESSI".to_string()));
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_s1_s3_backend_no_metadata() {
        let server = Server::new(
            ServerType::SyncServer,
            ServerBackendType::S3,
            Hostname::try_from("aws-eu-west-s1-sync.eessi.science").unwrap(),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let popserver = server
            .scrape(repolist.clone(), vec![], false, None)
            .await
            .get_populated_server()
            .unwrap();
        assert!(popserver.metadata.schema_version.is_none());
        assert!(popserver.metadata.cvmfs_version.is_none());
        assert!(popserver.metadata.last_geodb_update.is_none());
        assert!(popserver.metadata.os_version_id.is_none());
        assert!(popserver.metadata.os_pretty_name.is_none());
        assert!(popserver.metadata.os_id.is_none());
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_s1_ignored_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::CVMFS,
            Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let ignored_repos = vec!["riscv.eessi.io"];
        let popserver = server
            .scrape(repolist.clone(), ignored_repos.clone(), false, None)
            .await
            .get_populated_server()
            .unwrap();
        assert!(popserver.has_repository("software.eessi.io"));
        assert!(popserver.has_repository("dev.eessi.io"));
        assert!(!popserver.has_repository("riscv.eessi.io"));
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_s1_only_forced_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::CVMFS,
            Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io"];
        let popserver = server
            .scrape(repolist.clone(), vec![], true, None)
            .await
            .get_populated_server()
            .unwrap();
        assert!(popserver.has_repository("software.eessi.io"));
        assert!(popserver.has_repository("dev.eessi.io"));

        assert!(popserver.repositories.len() == 2);
    }

    #[tokio::test]
    async fn test_online_scraping_using_builder_interface() {
        let scraper = Scraper::new();
        let scraper = scraper
            .forced_repositories(vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"])
            .geoapi_servers(vec![DEFAULT_GEOAPI_SERVERS[0].clone()])
            .unwrap()
            .with_servers(vec![
                Server::new(
                    ServerType::Stratum1,
                    ServerBackendType::CVMFS,
                    Hostname::try_from("azure-us-east-s1.eessi.science").unwrap(),
                ),
                Server::new(
                    ServerType::Stratum1,
                    ServerBackendType::AutoDetect,
                    Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
                ),
                Server::new(
                    ServerType::SyncServer,
                    ServerBackendType::S3,
                    Hostname::try_from("aws-eu-west-s1-sync.eessi.science").unwrap(),
                ),
            ]);

        let results = scraper.validate().unwrap().scrape().await;
        for result in results {
            match result {
                ScrapedServer::Populated(popserver) => {
                    for repo in vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"] {
                        assert!(popserver.has_repository(repo));
                    }
                }
                ScrapedServer::Failed(failedserver) => {
                    panic!("Error: {:?}", failedserver.error);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_online_geoapi() {
        let repos = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let scraper = Scraper::new();
        let scraper = scraper
            .forced_repositories(repos.clone())
            .with_servers(vec![Server::new(
                ServerType::Stratum1,
                ServerBackendType::AutoDetect,
                Hostname::try_from("aws-eu-central-s1.eessi.science").unwrap(),
            )]);

        let results = scraper.validate().unwrap().scrape().await;
        for result in results {
            match result {
                ScrapedServer::Populated(popserver) => {
                    let geoapi = popserver.geoapi.clone();
                    let responses = geoapi.response.clone();
                    assert_eq!(responses.len(), repos.len());
                }
                ScrapedServer::Failed(failedserver) => {
                    panic!("Error: {:?}", failedserver.error);
                }
            }
        }
    }
}
