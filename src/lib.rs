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
//! Due to the nature of repositories.json, one may force repositories to be scraped by providing an explicit list of repositories to scrape.
//!
//! # Examples
//!
//! ```no_run
//! use cvmfs_server_scraper::{Hostname, Server, ServerBackendType, ServerType, scrape_servers};
//! use futures::future::join_all;
//!
//! #[tokio::main]
//! async fn main() {
//!     let servers = vec![
//!         Server::new(
//!             ServerType::Stratum1,
//!             ServerBackendType::CVMFS,
//!             Hostname("azure-us-east-s1.eessi.science".to_string()),
//!         ),
//!         Server::new(
//!             ServerType::Stratum1,
//!             ServerBackendType::CVMFS,
//!             Hostname("aws-eu-central-s1.eessi.science".to_string()),
//!         ),
//!         Server::new(
//!             ServerType::SyncServer,
//!             ServerBackendType::S3,
//!             Hostname("aws-eu-west-s1-sync.eessi.science".to_string()),
//!         ),
//!     ];
//!
//!     let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
//!
//!    // Scrape all servers in parallel
//!    let results = scrape_servers(servers, repolist).await;
//!
//!    for result in results {
//!        match result {
//!            Ok(populated_server) => {
//!                 println!("{}", populated_server);
//!                 populated_server.display();
//!                 println!();
//!            }
//!            Err(e) => {
//!                panic!("Error: {:?}", e);
//!            }
//!       }
//!     }
//! }
//! ```
//!

mod errors;
mod models;
mod utilities;

use crate::errors::CVMFSScraperError;
use crate::models::PopulatedServer;

pub use models::{Hostname, Server, ServerBackendType, ServerType};

use futures::future::join_all;

pub async fn scrape_servers(
    servers: Vec<Server>,
    repolist: Vec<&str>,
) -> Vec<Result<PopulatedServer, CVMFSScraperError>> {
    let futures = servers.into_iter().map(|server| {
        let repolist = repolist.clone();
        async move { server.scrape(repolist.clone()).await }
    });

    join_all(futures).await
}

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
                Hostname("azure-us-east-s1.eessi.science".to_string()),
            ),
            Server::new(
                ServerType::Stratum1,
                ServerBackendType::CVMFS,
                Hostname("aws-eu-central-s1.eessi.science".to_string()),
            ),
            Server::new(
                ServerType::SyncServer,
                ServerBackendType::S3,
                Hostname("aws-eu-west-s1-sync.eessi.science".to_string()),
            ),
        ];

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];

        let futures = servers.into_iter().map(|server| {
            let repolist = repolist.clone();
            async move {
                match server.scrape(repolist.clone()).await {
                    Ok(popserver) => {
                        for repo in repolist {
                            assert!(popserver.has_repository(repo));
                        }
                    }
                    Err(e) => {
                        panic!("Error: {:?}", e);
                    }
                }
            }
        });

        join_all(futures).await;
    }

    #[tokio::test]
    async fn test_online_cvmfs_servers_using_scan_servers() {
        let servers = vec![
            Server::new(
                ServerType::Stratum1,
                ServerBackendType::CVMFS,
                Hostname("azure-us-east-s1.eessi.science".to_string()),
            ),
            Server::new(
                ServerType::Stratum1,
                ServerBackendType::CVMFS,
                Hostname("aws-eu-central-s1.eessi.science".to_string()),
            ),
            Server::new(
                ServerType::SyncServer,
                ServerBackendType::S3,
                Hostname("aws-eu-west-s1-sync.eessi.science".to_string()),
            ),
        ];

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let results = scrape_servers(servers, repolist.clone()).await;

        for result in results {
            match result {
                Ok(popserver) => {
                    for repo in repolist.clone() {
                        assert!(popserver.has_repository(repo));
                    }
                }
                Err(e) => {
                    panic!("Error: {:?}", e);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_mismatch_s0_is_s1() {
        let server = Server::new(
            ServerType::Stratum0,
            ServerBackendType::CVMFS,
            Hostname("aws-eu-central-s1.eessi.science".to_string()),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io"];

        match server.scrape(repolist.clone()).await {
            Ok(_) => {
                panic!("Error, should not have succeeded");
            }
            Err(e) => {
                assert_eq!(e.to_string(), "Scrape error: Server type mismatch: aws-eu-central-s1.eessi.science is a Stratum0 server, but replicas were found in the repositories.json");
            }
        }
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_s1_has_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::CVMFS,
            Hostname("aws-eu-central-s1.eessi.science".to_string()),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let popserver = server.scrape(vec![]).await.unwrap();
        for repo in repolist {
            assert!(popserver.has_repository(repo))
        }
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_autodetect_s1_with_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
            Hostname("aws-eu-central-s1.eessi.science".to_string()),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let popserver = server.scrape(repolist.clone()).await.unwrap();
        assert_eq!(popserver.backend_type, ServerBackendType::AutoDetect);
        assert_eq!(popserver.backend_detected, ServerBackendType::CVMFS);
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_autodetect_s1_without_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
            Hostname("aws-eu-central-s1.eessi.science".to_string()),
        );

        let popserver = server.scrape(vec![]).await.unwrap();
        assert_eq!(popserver.backend_type, ServerBackendType::AutoDetect);
        assert_eq!(popserver.backend_detected, ServerBackendType::CVMFS);
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_autodetect_s3_with_repos() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
            Hostname("aws-eu-west-s1-sync.eessi.science".to_string()),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let popserver = server.scrape(repolist.clone()).await.unwrap();
        assert_eq!(popserver.backend_type, ServerBackendType::AutoDetect);
        assert_eq!(popserver.backend_detected, ServerBackendType::S3);
    }

    #[tokio::test]
    async fn test_online_cvmfs_server_s1_cvmfs_backend_metadata() {
        let server = Server::new(
            ServerType::Stratum1,
            ServerBackendType::CVMFS,
            Hostname("aws-eu-central-s1.eessi.science".to_string()),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let popserver = server.scrape(repolist.clone()).await.unwrap();
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
            Hostname("aws-eu-west-s1-sync.eessi.science".to_string()),
        );

        let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
        let popserver = server.scrape(repolist.clone()).await.unwrap();
        assert!(popserver.metadata.schema_version.is_none());
        assert!(popserver.metadata.cvmfs_version.is_none());
        assert!(popserver.metadata.last_geodb_update.is_none());
        assert!(popserver.metadata.os_version_id.is_none());
        assert!(popserver.metadata.os_pretty_name.is_none());
        assert!(popserver.metadata.os_id.is_none());
    }
}
