# CVMFS server scraper

This library scrapes the public metadata sources from a CVMFS server and validates the data. The files fetched are:

- cvmfs/info/v1/repositories.json
- cvmfs/info/v1/meta.json

And for each repository, it fetches:

- cvmfs/\<repo\>/.cvmfs_status.json
- cvmfs/\<repo\>/.cvmfspublished

## Usage

```rust
use cvmfs_server_scraper::{Hostname, Server, ServerBackendType, ServerType, scrape_servers, ScrapedServer};
use futures::future::join_all;

#[tokio::main]
async fn main() {
    let servers = vec![
        Server::new(
            ServerType::Stratum1,
            ServerBackendType::CVMFS,
            Hostname("azure-us-east-s1.eessi.science".to_string()),
        ),
        Server::new(
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
            Hostname("aws-eu-central-s1.eessi.science".to_string()),
        ),
        Server::new(
            ServerType::SyncServer,
            ServerBackendType::S3,
            Hostname("aws-eu-west-s1-sync.eessi.science".to_string()),
        ),
    ];
    let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
   // Scrape all servers in parallel
   let servers = scrape_servers(servers, repolist).await;
   for server in servers {
       match server {
           ScrapedServer::Populated(populated_server) => {
                println!("{}", populated_server);
                populated_server.display();
                println!();
           }
           ScrapedServer::Failed(failed_server) => {
               panic!("Error! {} failed scraping: {:?}", failed_server.hostname, failed_server.error);
           }
      }
    }
}
```
