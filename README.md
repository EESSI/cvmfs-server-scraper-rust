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

## A word about server backends

There are three valid options for backends for a given server. These are:

- `CVMFS`: This backend requires `cvmfs/info/v1/repositories.json` to be present on the server. Scrape fails if it is missing.
- `S3`: Does not even attempt to fetch `cvmfs/info/v1/repositories.json`. Note that if any server has S3 as a backend a list of repositories *must* be passed to the scraper as there is no other way to determine the list of repositories for S3 servers. Due to the async scraping of all servers, there is currently no support for falling back on repositories detected from other server types (including possibly the Stratum0).
- `AutoDetect`: This backend Aatempts to fetch `cvmfs/info/v1/repositories.json` but does not fail if it is missing. If the scraper fails to fetch the file, the backend will be assumed to be S3. If the list of repositories is empty, the scraper will return an empty list. If your S3 server has no repositories, setting the backend to AutoDetect will allow the scraper to continue without failing.

For populated servers, the field `backend_detected` will be set to the detected backend, which for explicit S3 or CVMFS servers will be the same as requested type.

## What repositories are scraped?

- For servers that are set to or detected as CVMFS, the scraper will scrape the union of the detected and configurations explicitly stated repositories.
- For servers that are set to or detected as S3, only the explicitly stated repositories will be scraped (and the scraper will fail if the server type is explicitly set to S3 and no repositories are passed).

## License

Licensed under the MIT license. See the LICENSE file for details.
