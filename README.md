# CVMFS server scraper

This Rust library fetches CVMFS repository indexes, manifests, status files, server metadata, and GeoAPI responses. Domain values validate at input boundaries, HTTP work has explicit resource limits, and results distinguish backend assumptions and optional probe failures.

## Usage

```rust
use cvmfs_server_scraper::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let servers = vec![
        Server::new(
            ServerType::Stratum1,
            ServerBackendType::AutoDetect,
            "http://aws-eu-central-s1.eessi.science".parse()?,
        ),
        Server::new(
            ServerType::SyncServer,
            ServerBackendType::S3,
            "http://aws-eu-west-s1-sync.eessi.science".parse()?,
        ),
    ];
    let selection = RepositorySelection::discover(
        ["software.eessi.io".parse()?, "dev.eessi.io".parse()?],
        ["riscv.eessi.io".parse()?],
    );
    let scraper = Scraper::new()
        .repository_selection(selection)
        .with_servers(servers)
        .validate()?;

    for result in scraper.scrape().await {
        match result {
            ScrapedServer::Collected(server) => {
                println!("{server}: {:?}", server.repository_outcome());
                for repo in server.repositories() {
                    println!("{}: revision {}", repo.name(), repo.revision());
                }
                for failure in server.failed_repositories() {
                    eprintln!("{}: {}", failure.name(), failure.error());
                }
                if let GeoapiOutcome::Failed(error) = server.geoapi() {
                    eprintln!("GeoAPI: {error}");
                }
                if let OptionalFetch::Failed(error) = server.contact() {
                    eprintln!("Contact metadata: {error}");
                }
            }
            ScrapedServer::Failed(server) => {
                eprintln!("{}: {}", server.hostname(), server.error());
            }
        }
    }
    Ok(())
}
```

Applications using `#[tokio::main]` should enable Tokio's `macros` and runtime features in their own Cargo manifest. This library enables only the Tokio features its implementation needs.

## Repository selection and backends

`RepositorySelection::only(names)` scrapes exactly those validated names and has no exclusion list. `RepositorySelection::discover(include, exclude)` scrapes the union of discovered and included names, removing exclusions. Names are deduplicated and results are sorted by repository name. Empty effective lists are rejected for explicit S3 during preflight.

- `CVMFS` requires a valid schema-1 repository index consistent with the configured server role.
- `S3` skips index discovery and requires a nonempty effective repository selection.
- `AutoDetect` uses a valid index when available. Only an HTTP 404 permits an S3 fallback, reported as `BackendResolution::AssumedS3IndexNotFound`. This fallback requires a nonempty effective repository selection; each repository's result is then reported independently. DNS failures, timeouts, malformed indexes, authentication failures, and server errors remain errors.

Configuration and discovery/selection failures produce `ScrapedServer::Failed`. Once repository collection starts, `ScrapedServer::Collected(Box<ServerReport>)` retains a report, including when every repository fails. Each selected name appears exactly once in either `repositories()` or `failed_repositories()`, each sorted by name. A broken status file or manifest affects only its repository; other repositories continue.

Use `ServerReport::repository_outcome()` to inspect the selected repositories:

| `RepositoryOutcome` | Successful repositories | Failed repositories | Meaning |
| --- | --- | --- | --- |
| `Complete` | At least one | None | Every selected repository succeeded |
| `Partial` | At least one | At least one | Preserve successes and handle individual failures |
| `AllFailed` | None | At least one | Every selected repository failed; named errors remain available |
| `Empty` | None | None | No repositories were selected; repository availability was not tested |

Optional contact metadata has `Available`, `Absent` (404), and `Failed` outcomes. GeoAPI has `Available`, `Unsupported`, `Skipped`, and `Failed` outcomes, using the first successful repository in name order. Ancillary failures, including reaching the server deadline, preserve repository results and do not change `repository_outcome()`. A `Complete` repository outcome does not establish overall server health; inspect optional probes according to the consumer's policy.

GeoAPI uses one-based IDs. `GeoapiOrdering` validates a complete permutation against its immutable host list. Configure hosts with `GeoapiProbe::Enabled(GeoapiHosts::new(hosts)?)`, or use `GeoapiProbe::Disabled`. Empty lists never silently select default hosts. S3 and Stratum0 servers are not probed.

## Transport and limits

`ServerEndpoint` accepts an HTTP or HTTPS origin with an optional port, including IP addresses. Credentials, paths, query strings, and fragments are rejected. Repository URLs are assembled from validated path segments. `Hostname` stores lowercase ASCII names, including punycode; `RepositoryName` preserves case and permits only ASCII letters, digits, dots, underscores, and hyphens, with no empty dot components.

Redirects are disabled by default. `RedirectPolicy::SameOrigin` permits at most five redirects within the original scheme, host, and port. Cross-origin redirects and HTTPS downgrades are rejected. Choose an HTTPS endpoint when supported by the server. Hostname/endpoint syntax validation is not network authorization: applications accepting untrusted server configuration must enforce their own permitted destinations and network access policy.

A validated scraper reuses one HTTP client and global request budget across runs. Idle connections expire after five seconds, avoiding stale unused connections between scrape cycles. Defaults are:

| Setting | Default |
| --- | --- |
| Connect / read / total request timeout | 5 / 10 / 30 seconds |
| Total deadline per admitted server | 300 seconds |
| Concurrent servers / repository jobs per server / global requests | 8 / 4 / 32 |
| Repository-index / metadata-or-status / manifest / GeoAPI response limit | 2 MiB / 256 KiB / 1 MiB / 16 KiB |
| Repository count per server | 10,000 |

A repository job fetches its status and manifest concurrently. Server results retain configuration order; repository results retain name order. The per-server deadline covers discovery, queued request permits, repositories, and ancillary probes, starting when the server is admitted. Request permits cover receiving the entire body and are released on cancellation. Body limits apply to bytes actually received, including chunked responses; Content-Length is also checked early. Configured server count determines the number of bounded batches; there is no separate whole-run deadline.

At that deadline, completed repository and probe results are retained. Active and queued repositories receive named timeout failures; unfinished optional requests receive independent timeout outcomes. No new repository jobs or probes start after the deadline. A deadline during index discovery still produces a failed server because the effective repository selection is not yet known.

Collection continues after individual repository failures within those same limits. This can take longer than failing immediately. `scrape()` returns after all configured servers finish, so additional collection work can delay delivery of healthy servers' results too.

Customize settings through checked newtypes:

```rust
use cvmfs_server_scraper::*;
use std::time::Duration;

fn options() -> Result<ScrapeOptions, ConfigurationError> {
    let limits = ScrapeLimits::default()
        .with_request_timeout(RequestTimeout::new(Duration::from_secs(15))?)
        .with_requests(ConcurrencyLimit::new(16)?)
        .with_repository_count(RepositoryLimit::new(2_000)?);
    Ok(ScrapeOptions::default().with_limits(limits))
}
```

Timeouts must be positive and at most one hour; concurrency is 1–1024, response limits 1 byte–64 MiB, and repository limits 1–100,000. Manifest parsing additionally has a hard 1 MiB bound, and GeoAPI parsing a hard 16 KiB bound. A smaller configured limit is honored. Scraper configuration checks the effective selection and configured list sizes before any requests.

## Manifest and time semantics

`Manifest` represents parsed observations, not authenticated data. Its fields use `Revision`, `CatalogSize`, `CatalogTtl`, `UnixTimestamp`, `ContentHash`, `RootPathMd5`, and `RepositoryName`. Optional protocol fields remain optional. SHA-1, RIPEMD-160, and SHAKE-128 hashes are supported with exact lengths and algorithm suffixes. The parser returns errors for malformed/duplicate fields without panicking.

Use `Manifest::from_bytes` for network/file contents. `signature().as_bytes()` (after handling the Option) preserves the exact bytes after the separator, including the checksum line and binary signature. Signatures are not verified. No result type claims cryptographic verification or publisher authorization.

`Manifest::bind_to_repository` rejects a conflicting repository name. `RepositoryManifest` cannot be deserialized or constructed through public fields; its identity is `Matched` when N agrees, or `Unspecified` when the protocol-optional N field is absent. `ReportedTimestamp` retains original text and exposes a parsed UTC instant only for recognized, offset-aware formats. Unknown/localized zone names remain unparsed instead of being silently interpreted as UTC.

## Migrating from 0.0.7

These are breaking API changes, grouped together while the crate is pre-1.0:

| Previously | Now |
| --- | --- |
| `Server::new(..., Hostname)` / JSON `hostname` | Constructor takes `ServerEndpoint`; JSON accepts legacy `hostname` or an explicit HTTP(S) `endpoint` and serializes as `endpoint` |
| Forced/ignored lists plus `only_scrape_forced_repositories(bool)` | `RepositorySelection::only(...)` or `::discover(include, exclude)`, containing `RepositoryName` values |
| `Server::scrape(repos, ignored, only, geoapi)` | `Server::scrape(ScrapeOptions)`; shares builder validation |
| `geoapi_servers(...)` | `geoapi(GeoapiProbe::Enabled(GeoapiHosts::new(hosts)?))` |
| Public populated-server/repository fields | Read-only accessor methods |
| `ScrapedServer::Populated(Box<PopulatedServer>)` | `ScrapedServer::Collected(Box<ServerReport>)`; a report may contain partial results or no successful repositories |
| `is_ok()`, `is_failed()`, `is_populated()` | Removed; match `ScrapedServer`, then inspect `ServerReport::repository_outcome()` and optional probes |
| `as_populated_server()`, `into_populated_server()`, `get_populated_server()` | `as_report()` or `into_report()` retrieves a report regardless of its outcomes |
| `backend_detected: ServerBackendType` | `backend(): BackendResolution`, including an explicit assumption state |
| `metadata.administrator` and other contact fields | `contact(): OptionalFetch<ContactMetadata>` |
| `manifest.s`, `.d`, `.b`, `.n` | `revision()`, `ttl()`, `catalog_size()`, `repository_name()` with typed values |
| Signed revision integers | `Revision::get(): u64` |
| Required `manifest.t: i64` | `published_at(): Option<UnixTimestamp>`; `datetime()` checks representability |
| One repository failure fails the entire server | Successful repositories and named `failed_repositories()` coexist; require `RepositoryOutcome::Complete` for a nonempty selection with no repository failures |
| Required history/metadata/reflog hashes | Optional, validated `ContentHash` values |
| Text signature | `Option<SignatureBytes>`; serialized as bytes, preserving binary data |
| Zero-based GeoAPI fixtures / directly mutable query vectors | One-based `GeoapiHostId` values in a validated `GeoapiOrdering` |
| `Option<MaybeRfc2822DateTime>` in scraped models | `Option<ReportedTimestamp>` |

Error enums now retain endpoint context and underlying causes, so consumers matching error variants must update those matches. Serialized repository results nest the requested name, parsed manifest, and identity outcome inside `RepositoryManifest`; consumers of the old repository JSON layout must update their readers.

The result names and generic predicates are changed deliberately so existing callers must choose how to handle partial, failed, and empty collections. `Collected` and `into_report()` indicate the presence of a report, not successful repository fetches. `as_failed_server()` and `into_failed_server()` remain available only for failures before repository collection; an `AllFailed` report is still `Collected` and retains every repository error. Do not treat `as_failed_server().is_none()` as a health check.

### Configuration and identity

Existing server JSON such as `{"server_type":"Stratum1","hostname":"example.org"}` still deserializes, using `http://example.org/` and the default `AutoDetect` backend. Explicit `backend_type` values remain supported. New configurations may instead specify `"endpoint":"https://example.org:8443"`. Supply exactly one address; configurations with both are rejected. Both forms undergo validation, so invalid legacy hostnames are not grandfathered in. Serialization writes the canonical `endpoint` form; reading old configurations is supported, but writing them back changes their shape.

Applications with a public configuration format should own that format and convert it into scraper types at their network boundary. Keep the application's server identity separate from its transport endpoint. `Server::hostname()` excludes scheme and port, so HTTP and HTTPS endpoints or different ports on the same host share that value. A consumer keyed by hostname must reject those collisions or introduce an explicit stable identity before supporting multiple endpoints per host. Existing hostname history keys should not silently become full URLs. Endpoint hosts can also be IP addresses, which a consumer's hostname type may not support.

### Revisions and timestamps

`Revision` uses `u64` because revisions are non-negative, including zero, and CVMFS stores them as `uint64_t`; see the [CVMFS manifest definition](https://github.com/cvmfs/cvmfs/blob/devel/cvmfs/manifest.h). The previous `i32` representation restricted valid revisions to 2,147,483,647. Consumers should migrate revision observations, replication tracking, and persisted history together to an unsigned revision type. Until then, use `i32::try_from(revision.get())` and report out-of-range observations explicitly. Do not use truncating casts.

The manifest's publication timestamp `T` is optional in the [CVMFS parser](https://github.com/cvmfs/cvmfs/blob/devel/cvmfs/manifest.cc). Absence is `None`; an explicitly supplied zero remains zero. A present `UnixTimestamp` can also exceed the datetime library's range: call `datetime()` to validate it. For a consumer that still requires an `i32` revision and a valid timestamp, a checked adapter can use:

```rust
use cvmfs_server_scraper::Manifest;

fn legacy_observation(manifest: &Manifest) -> Result<(i32, i64), Box<dyn std::error::Error>> {
    let revision = i32::try_from(manifest.revision().get())?;
    let timestamp = manifest.published_at().ok_or("publication timestamp is missing")?;
    Ok((revision, timestamp.datetime()?.timestamp()))
}
```

Treat conversion errors as unavailable facts for the affected repository and preserve other observations. A domain that can represent missing publication time may still compare revisions, while reporting time-dependent checks as unknown. Never substitute zero or scrape time for a missing publication timestamp. Scrape time and publication time describe different events.

Consumers must also preserve partial results at their own boundary: collecting conversions into a single `Result<Vec<_>, _>` recreates the all-or-nothing behavior. Pass the intended deadline through `ScrapeLimits`; an outer timeout that cancels the whole scrape first cannot recover its completed results.

### Other compatibility types

`MaybeRfc2822DateTime` remains available with private storage and a `new(Option<String>)` constructor for compatibility; new models avoid nested optional strings. `HexString` remains available for general hex data, but manifests use algorithm-aware digest types. JSON deserialization routes through validating constructors, including owned values and escaped strings. Scrape plans, bound repository results, and query results intentionally do not implement Deserialize. Deserializing a GeoapiOrdering reruns its relationship validation.

## Development

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
```

Default tests use local loopback fixtures and require permission to bind `127.0.0.1`. They do not contact production servers. A separate CI job runs 17 integration cases against a disposable [CVMFS testbed](https://github.com/terjekv/cvmfs-testbed) on every PR, covering real S0/S1/S3 data, publication, replication lag, outages, and recovery. See [testing and coverage](docs/testing.md) for local testbed setup.

Add the `online-tests` label to a PR to enable the separate online CI workflow, or run `cargo test --locked --test online -- --ignored --test-threads=1` locally. The ten online cases exercise the public EESSI Stratum1 and S3 deployments. Rust 1.88 is the minimum supported version; CI checks it and stable.

Parameterized tests use `rstest` with named `#[case::name(...)]` cases. See [testing and coverage](docs/testing.md) for coverage commands, the measured baseline, remaining gaps, and CI reports.

## License

MIT; see [LICENSE.txt](LICENSE.txt).
