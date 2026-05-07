# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.3.2](https://github.com/tauri-apps/cef-rs/compare/download-cef-v2.3.1...download-cef-v2.3.2) - 2026-05-07

### Other

- use `fs_err` instead of `std::fs` for file operations

## [2.3.1](https://github.com/tauri-apps/cef-rs/compare/download-cef-v2.3.0...download-cef-v2.3.1) - 2026-03-08

### Other

- Include CREDITS.html in extracted cef dir

## [2.3.0](https://github.com/tauri-apps/cef-rs/compare/download-cef-v2.2.1...download-cef-v2.3.0) - 2026-01-01

### Added

- *(test)* port tests/shared library from CEF

## [2.2.1](https://github.com/tauri-apps/cef-rs/compare/download-cef-v2.2.0...download-cef-v2.2.1) - 2025-12-23

### Fixed

- `location` was ignored in `extract_target_archive`

## [2.2.0](https://github.com/tauri-apps/cef-rs/compare/download-cef-v2.1.0...download-cef-v2.2.0) - 2025-09-23

### Added

- add --mirror-url cli args
- Allow to download CEF binaries with custom base url set in env variable

## [2.1.0](https://github.com/tauri-apps/cef-rs/compare/download-cef-v2.0.3...download-cef-v2.1.0) - 2025-09-19

### Added

- support pre-downloaded archive ([#197](https://github.com/tauri-apps/cef-rs/pull/197))

## [2.0.3](https://github.com/tauri-apps/cef-rs/compare/download-cef-v2.0.2...download-cef-v2.0.3) - 2025-08-16

### Fixed

- #183

## [2.0.2](https://github.com/tauri-apps/cef-rs/compare/download-cef-v2.0.1...download-cef-v2.0.2) - 2025-07-22

### Other

- *(doc)* regenerate CHANGELOG.md

## [2.0.1](https://github.com/tauri-apps/cef-rs/compare/download-cef-v2.0.0...download-cef-v2.0.1) - 2025-07-14

### Other

- release

## [2.0.0](https://github.com/tauri-apps/cef-rs/compare/download-cef-v1.6.0...download-cef-v2.0.0) - 2025-07-14

### Fixed

- bump major version of download-cef [#145](https://github.com/tauri-apps/cef-rs/issues/145)

### Other

- update CEF version
