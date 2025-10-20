# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this will adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once
we reach version 0.1.0, up until then, expect breaking changes.

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
