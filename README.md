# CVMFS server scraper

This library scrapes the public metadata sources from a CVMFS server and validates the data. The files fetched are:

- cvmfs/info/v1/repositories.json
- cvmfs/info/v1/meta.json

And for each repository, it fetches:

- cvmfs/\<repo\>/.cvmfs_status.json
- cvmfs/\<repo\>/.cvmfspublished

## Usage

```rust
use cvmfs_server_scraper::{Hostname, Server, ServerBackendType, ServerType,
    ScrapedServer, ScraperCommon, Scraper, CVMFSScraperError, DEFAULT_GEOAPI_SERVERS};

#[tokio::main]
async fn main() -> Result<(), CVMFSScraperError> {
    let servers = vec![
        Server::new(
            ServerType::Stratum1,
            ServerBackendType::CVMFS,
            Hostname::try_from("azure-us-east-s1.eessi.science")?,
        ),
        Server::new(
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
            Hostname::try_from("aws-eu-central-s1.eessi.science")?,
        ),
        Server::new(
            ServerType::SyncServer,
            ServerBackendType::S3,
            Hostname::try_from("aws-eu-west-s1-sync.eessi.science")?,
        ),
    ];
    let repolist = vec!["software.eessi.io", "dev.eessi.io", "riscv.eessi.io"];
    let ignored_repos = vec!["nope.eessi.io"];

    // Build a Scraper and scrape all servers in parallel
    let scraped_servers = Scraper::new()
       .forced_repositories(repolist)
       .ignored_repositories(ignored_repos)
       .only_scrape_forced_repositories(false) // Only scrape forced repositories if true, overrides ignored_repositories
       .geoapi_servers(DEFAULT_GEOAPI_SERVERS.clone())? // This is the default list
       .with_servers(servers) // Transitions to a WithServer state.
       .validate()? // Transitions to a ValidatedAndReady state, now immutable.
       .scrape().await; // Perform the scrape, return servers.
    for server in scraped_servers {
        match server {
            ScrapedServer::Populated(populated_server) => {
               println!("{}", populated_server);
               populated_server.output();
               println!();
            }
            ScrapedServer::Failed(failed_server) => {
               panic!("Error! {} failed scraping: {:?}", failed_server.hostname, failed_server.error);
            }
        }
    }
    Ok(())
}
```

## A word about server backends

There are three valid options for backends for a given server. These are:

- `CVMFS`: This backend requires `cvmfs/info/v1/repositories.json` to be present on the server. Scrape fails if it is missing.
- `S3`: Does not even attempt to fetch `cvmfs/info/v1/repositories.json`. Note that if any server has S3 as a backend a list of repositories *must* be passed to the scraper as there is no other way to determine the list of repositories for S3 servers. Due to the async scraping of all servers, there is currently no support for falling back on repositories detected from other server types (including possibly the Stratum0).
- `AutoDetect`: This backend Aatempts to fetch `cvmfs/info/v1/repositories.json` but does not fail if it is missing. If the scraper fails to fetch the file, the backend will be assumed to be S3. If the list of repositories is empty, the scraper will return an empty list. If your S3 server has no repositories, setting the backend to AutoDetect will allow the scraper to continue without failing.

For populated servers, the field `backend_detected` will be set to the detected backend, which for explicit S3 or CVMFS servers will be the same as requested type.

## What repositories are scraped?

If `only_scrape_forced_repositories` is set to true, only the repositories explicitly passed to the scraper will be scraped, ignoring any ignored repositories. Otherwise, the following rules apply:

- For servers that are set to or detected as CVMFS, the scraper will scrape the union of the detected and configurations explicitly stated repositories.
- For servers that are set to or detected as S3, only the explicitly stated repositories will be scraped (and the scraper will fail if the server type is explicitly set to S3 and no repositories are passed).

## License

Licensed under the MIT license. See the LICENSE file for details.
