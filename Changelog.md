# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this will adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once
we reach version 0.1.0, up until then, expect breaking changes.

## [Unreleased]

### Fixed

- Reject malformed manifests without panicking, validate one-based GeoAPI permutations, and enforce hostname/repository validation during deserialization.
- Bound request duration, streamed body size, discovery count, and concurrency; disable redirects by default and offer bounded same-origin redirects.
- Correct forced-only selection and validate effective S3 selections before network work. Autodetection falls back only on index HTTP 404 and reports that assumption explicitly.
- Support optional manifest fields, supported digest algorithms, unsigned protocol ranges, owned JSON deserialization, identity binding, and exact binary signature preservation.
- Preserve unsupported timestamp text without assigning an incorrect UTC instant. Preserve optional contact/GeoAPI failure details without discarding repository results.

### Breaking changes

- Introduced validated `ServerEndpoint`, `RepositoryName`, manifest scalar/hash types, `RepositorySelection`, immutable `ValidatedScrapePlan`, `RepositoryManifest`, and `GeoapiOrdering`.
- Populated objects now expose read-only accessors. Server construction takes an explicit HTTP(S) origin, and single-server scraping takes `ScrapeOptions`.
- Backend resolution and optional probe outcomes use dedicated enums. See the README migration table for API and serialization changes.
- Replaced default production-network tests with deterministic loopback regressions; public smoke tests are opt-in.
- Narrowed runtime dependency features, moved test dependencies to dev-dependencies, and added CI linting, caching, bounded jobs and advisory checks.

### Changed

- Restored crate and API documentation for the validated types, including manifest fields, builder transitions, server roles, and optional metadata semantics.
- Added an `online-tests` PR label to enable public EESSI checks covering discovery, metadata, repository selection, GeoAPI, S3 backends, mixed-server scraping, and repeated cycles using the same validated scraper.
- Replaced `yare` with `rstest` named cases, expanded domain and loopback regression coverage, and added downloadable CI coverage reports.
- Updated all Rust dependencies to their latest compatible releases.
- Raised the minimum supported Rust version to 1.88 for the updated dependencies.
- Updated GitHub Actions checkout to v7.0.1 and added tests of the locked dependencies on Rust 1.88 and stable.

## [0.0.7] - 2026-06-08

### Changed

- Updated dependencies, including moving to the current `reqwest` 0.13 and `rand` 0.10 lines.
- Set the minimum supported Rust version to 1.85.
- `ScrapedServer::Populated` now stores `PopulatedServer` behind a `Box` to reduce enum size. Consumers that directly pattern match on this variant may need to dereference or unbox the value.
- Added `ScrapedServer` helper methods: `is_populated`, `as_populated_server`, `as_failed_server`, `into_populated_server`, and `into_failed_server`.
- Removed the inherent `Hostname::to_string` method; use the standard `ToString` implementation from `Display` instead.

## [0.0.6] - 2025-10-20

### Added

- `scrape` for a server now takes a boolean argument to indicate if only the explicitly listed repositories for that server are to be scraped, overriding `ignored_repositories`.
  This parameter is also added to the `scrape_servers` API, in both cases requiring consumers to update their code accordingly. To retain previous behavior, pass `false` to
  either function. If using the builder interface, `only_scrape_forced_repositories(true|false)` is available. The default is `false`, retaining previous behavior and requiring no changes.

## [0.0.5] - 2024-10-18

### Added

- ServerMetadata is now serializable.

## [0.0.4] - 2024-09-16

### Fixed

- Do not try to scrape GeoAPI information from stratum0.

## [0.0.3] - 2024-09-16

### Added

- Made last_gc and last_snapshot in .cvmfs_status.json properly optional.
- GeoAPI support.
- A builder interface for scraping, `Scraper`, allowing for easier configuration of the scraper and more flexibility in the future.
- Pre-flight validation of the scraper configuration when using the builder interface.
- Support for ignoring repositories to prevent them from being part of the scan. Note that ignoring takes precedence over even explicit including.
- A changelog...

### Changed

- Updated dependencies.
- The `server_scraper` function now takes a fourth argument, an optional list of GeoAPI servers to test against.

## [0.0.2] - 2024-06-30

### Added

- Improved documentation for relevant types.
- Re-exported MaybeRfc2822DateTime and Manifest.
  
### Changed

- Moved from using a from_str-like interface to create Manifests to implementing FromStr and thus allowing the use of parse().

## [0.0.1] - 2024-06-30

### Added

- Initial release.
