# Testing and coverage

Run the deterministic suite, including documentation tests:

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
```

The suite uses `rstest` named `#[case::name(...)]` cases for input matrices and Tokio for async tests. `rstest` is a development dependency with its optional features disabled; the tests do not need its timeout runtime or crate-renaming support. HTTP fixtures bind ephemeral loopback ports and do not contact production servers. The nine public-server checks remain opt-in.

## Online EESSI checks

Add the **`online-tests`** label to a pull request to run the separate **Online EESSI tests** workflow. It runs on label addition and subsequent pushes or reopening while the label remains present. Removing the label disables future online runs; unrelated label changes do not restart or cancel an active run. This workflow uses `pull_request`, read-only repository permissions, and no deployment credentials. Normal build, lint, documentation, and coverage jobs remain independent of the label.

Run the same suite locally:

```sh
cargo test --locked --test online -- --ignored --test-threads=1 --nocapture
```

The named cases cover the public deployments already used in the crate examples:

| Scenario | Checks |
| --- | --- |
| AWS EU Central and Azure US East Stratum1 | Explicit CVMFS discovery, AWS autodetection, all three EESSI repositories, server/contact metadata, and default GeoAPI hosts. |
| Repository selection | Exact `dev.eessi.io` / `software.eessi.io` selection, discovery with `riscv.eessi.io` excluded, and a custom GeoAPI host list. |
| AWS EU West S3 sync | Explicit S3 and HTTP-404-based autodetection, all three repositories, absent index metadata, and unsupported GeoAPI. |
| Mixed backends | The builder scrapes all three servers, retains configuration order, and honors disabled GeoAPI. |
| Role mismatch | Claiming the AWS Stratum1 as Stratum0 fails with a role error. |

Successful repository results are checked for name binding, published revisions, signature presence, publication timestamp conversion, and serialization of real manifest/status data. These are deployment checks: they do not assert fixed revision numbers, dates, CVMFS/OS versions, or GeoAPI distance order. Optional snapshot/GC fields remain optional, and signature presence does not establish authenticity.

Tests run serially with at most four in-flight HTTP requests per scraper, a 100-repository limit, and a 60-second deadline per admitted server. The CI job has a 15-minute limit. Outages, access restrictions, changed deployment metadata, and malformed responses fail visibly; there are no automatic retries or silent skips. Online results are excluded from the deterministic coverage baseline. The nine checks passed locally on 2026-09-28.

## Reproduce coverage

```sh
rustup component add llvm-tools-preview --toolchain stable
cargo +stable install cargo-llvm-cov --version 0.9.1 --locked
mkdir -p target/llvm-cov
cargo +stable llvm-cov --locked --all-targets --lcov --output-path target/llvm-cov/lcov.info
cargo +stable llvm-cov report --show-missing-lines
cargo +stable llvm-cov report --html
```

Open `target/llvm-cov/html/index.html` for annotated source. CI runs the same coverage command on Linux and uploads the LCOV and HTML reports as the `coverage` artifact, retained for 14 days. Reports include library source and exclude dependencies and test/fixture code using cargo-llvm-cov's defaults; no library files are explicitly excluded.

## Coverage review: 2026-09-28

Measurements used Rust 1.98.0 on macOS ARM64 and cargo-llvm-cov 0.9.1, with identical library source before and after the added tests. The baseline contains the original 58 regression cases after the direct `yare` to `rstest` migration. The expanded suite contains 118 regression cases. Some additional cases expose existing loop iterations as independent tests; test counts alone do not measure coverage.

| Library metric | Before | After |
| --- | ---: | ---: |
| Lines | 86.23% (1,115/1,293) | 96.29% (1,245/1,293) |
| Functions | 78.10% (189/242) | 93.80% (227/242) |
| Regions | 86.92% (1,462/1,682) | 95.96% (1,614/1,682) |

| Module | Line coverage before | Line coverage after |
| --- | ---: | ---: |
| Manifest parsing | 86.44% | 94.92% |
| Generic domain values and timestamps | 71.43% | 92.86% |
| GeoAPI | 78.12% | 96.88% |
| Manifest scalar and digest types | 85.39% | 96.63% |
| Server scraping and results | 86.03% | 94.79% |
| Scraper configuration and scheduling | 86.59% | 100.00% |
| Transport | 96.25% | 97.92% |

The added assertions exercise required manifest fields and optional flags/hashes/timestamps, CRLF and unterminated final lines, serde validation, exact size and duration boundaries, absent versus unresolved timestamps, GeoAPI host/response bounds, configured and combined discovery limits, role mismatches, required JSON/HTTP failures, independent server outcomes, custom GeoAPI ordering, and response bodies exactly at their byte limit.

Remaining uncovered executable lines are mostly formatting/output helpers, compatibility aliases, simple accessors, and HTTP-client initialization failure handling. No tests were added solely to execute printing helpers or aliases. Macro-generated and compiler-generated code can affect these figures, so compare reports from the same toolchain and platform.

These are line, function, and region measurements, not branch coverage. Stable coverage excludes documentation tests; the six current documentation tests (including two compile-fail checks for proof/state boundaries) run separately with `cargo test`. The ignored public-server checks are also excluded. Local HTTP fixtures do not exercise real DNS/TLS failures, HTTPS downgrades, or connection reuse over a persistent TCP connection; those remain useful future integration-test targets. Covered lines alone do not prove those behaviors.
