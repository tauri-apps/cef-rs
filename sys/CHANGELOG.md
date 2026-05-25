# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [148.2.0+148.0.8](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v148.1.0+147.0.14...cef-dll-sys-v148.2.0+148.0.8) - 2026-05-25

### Other

- *(release)* update CEF version to 148.0.8

## [148.0.0+147.0.10](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v147.1.0+147.0.10...cef-dll-sys-v148.0.0+147.0.10) - 2026-05-07

### Other

- Copy CEF files to target directory on Windows and Linux so the binary can locate them

## [147.0.0+147.0.9](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v146.7.0+146.0.12...cef-dll-sys-v147.0.0+147.0.9) - 2026-04-25

### Other

- update bindings

## [146.5.0+146.0.10](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v146.4.1+146.0.9...cef-dll-sys-v146.5.0+146.0.10) - 2026-04-05

### Other

- update bindings

## [146.4.1+146.0.9](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v146.4.0+146.0.9...cef-dll-sys-v146.4.1+146.0.9) - 2026-04-03

### Fixed

- download CEF from custom URL

## [146.4.0+146.0.9](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v146.3.0+146.0.9...cef-dll-sys-v146.4.0+146.0.9) - 2026-04-02

### Added

- allow configuring CEF_DOWNLOAD_URL for build script

### Other

- Merge pull request #384 from tauri-apps/release-plz-2026-03-27T20-54-43Z
- add missing rerun-if-changed instruction [skip ci]

## [146.3.0+146.0.9](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v146.2.0+146.0.9...cef-dll-sys-v146.3.0+146.0.9) - 2026-04-01

### Added

- improve CEF_PATH usage

## [145.6.1+145.0.28](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v145.6.0+145.0.28...cef-dll-sys-v145.6.1+145.0.28) - 2026-03-08

### Other

- update bindings
- Merge pull request #368 from tauri-apps/feat/cef_color_ids

## [144.3.0+144.0.12](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v144.2.0+144.0.11...cef-dll-sys-v144.3.0+144.0.12) - 2026-01-31

### Other

- update bindings

## [144.0.1+144.0.6](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v144.0.0+144.0.6...cef-dll-sys-v144.0.1+144.0.6) - 2026-01-22

### Other

- release v144.0.0+144.0.6

## [144.0.0+144.0.6](https://github.com/tauri-apps/cef-rs/releases/tag/cef-dll-sys-v144.0.0+144.0.6) - 2026-01-22

### Other

- update bindings

## [143.4.0+143.0.13](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v143.3.0+143.0.13...cef-dll-sys-v143.4.0+143.0.13) - 2025-12-30

### Added

- add cef_task_manager_capi.h

### Other

- update bindings

## [143.2.0+143.0.10](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v143.1.0+143.0.10...cef-dll-sys-v143.2.0+143.0.10) - 2025-12-23

### Added

- add all C API headers to wrapper.h, except one for which bindings are still invalid

### Other

- update bindings

## [143.1.0+143.0.10](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v143.0.0+143.0.9...cef-dll-sys-v143.1.0+143.0.10) - 2025-12-13

### Other

- *(release)* update CEF version to 143.0.10
- Wrap more C API headers

## [143.0.0+143.0.9](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.5.1+142.0.17...cef-dll-sys-v143.0.0+143.0.9) - 2025-12-11

### Other

- update bindings
- *(release)* update CEF version to 143.0.9

## [142.5.1+142.0.17](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.5.0+142.0.17...cef-dll-sys-v142.5.1+142.0.17) - 2025-12-09

### Other

- release v142.5.1+142.0.17

## [142.5.0+142.0.17](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.4.1+142.0.15...cef-dll-sys-v142.5.0+142.0.17) - 2025-11-27

### Other

- update bindings
- *(release)* update CEF version to 142.0.17
- *(deps)* update rust crate convert_case to 0.10

## [142.4.1+142.0.15](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.4.0+142.0.15...cef-dll-sys-v142.4.1+142.0.15) - 2025-11-22

### Fixed

- use build-time OUT_DIR variable in get_cef_dir

### Other

- release v142.4.1+142.0.15

## [142.4.0+142.0.15](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.3.1+142.0.14...cef-dll-sys-v142.4.0+142.0.15) - 2025-11-21

### Other

- update bindings
- *(release)* update CEF version to 142.0.15

## [142.3.1+142.0.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.3.0+142.0.14...cef-dll-sys-v142.3.1+142.0.14) - 2025-11-20

### Other

- release v142.3.1+142.0.14

## [142.3.0+142.0.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.2.1+142.0.10...cef-dll-sys-v142.3.0+142.0.14) - 2025-11-20

### Other

- update bindings
- *(release)* update CEF version to 142.0.14

## [142.2.1+142.0.10](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.2.0+142.0.10...cef-dll-sys-v142.2.1+142.0.10) - 2025-11-16

### Other

- release v142.2.1+142.0.10
- update bindings

## [142.2.0+142.0.10](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.1.0+142.0.8...cef-dll-sys-v142.2.0+142.0.10) - 2025-11-13

### Other

- update bindings
- *(release)* update CEF version to 142.0.10

## [142.1.0+142.0.8](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v142.0.0+142.0.8...cef-dll-sys-v142.1.0+142.0.8) - 2025-11-12

### Other

- release v142.1.0+142.0.8

## [142.0.0+142.0.8](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.6.1+141.0.11...cef-dll-sys-v142.0.0+142.0.8) - 2025-11-11

### Other

- update bindings
- *(release)* update CEF version to 142.0.8

## [141.6.1+141.0.11](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.6.0+141.0.11...cef-dll-sys-v141.6.1+141.0.11) - 2025-11-09

### Other

- release v141.6.1+141.0.11
- *(deps)* update rust crate convert_case to 0.9
- *(deps)* update rust crate libloading to 0.9

## [141.6.0+141.0.11](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.5.0+141.0.10...cef-dll-sys-v141.6.0+141.0.11) - 2025-10-26

### Other

- update bindings
- *(release)* update CEF version to 141.0.11

## [141.5.0+141.0.10](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.4.1+141.0.9...cef-dll-sys-v141.5.0+141.0.10) - 2025-10-24

### Other

- update bindings
- *(release)* update CEF version to 141.0.10

## [141.4.1+141.0.9](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.4.0+141.0.9...cef-dll-sys-v141.4.1+141.0.9) - 2025-10-23

### Other

- release v141.4.1+141.0.9

## [141.4.0+141.0.9](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.3.1+141.0.8...cef-dll-sys-v141.4.0+141.0.9) - 2025-10-23

### Other

- update bindings
- *(release)* update CEF version to 141.0.9

## [141.3.1+141.0.8](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.3.0+141.0.8...cef-dll-sys-v141.3.1+141.0.8) - 2025-10-22

### Other

- release v141.3.0+141.0.7

## [141.3.0+141.0.8](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.2.0+141.0.7...cef-dll-sys-v141.3.0+141.0.8) - 2025-10-22

### Other

- update bindings
- *(release)* update CEF version to 141.0.8

## [141.2.0+141.0.7](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.1.0+141.0.6...cef-dll-sys-v141.2.0+141.0.7) - 2025-10-19

### Other

- update bindings
- *(release)* update CEF version to 141.0.7

## [141.1.0+141.0.6](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v141.0.0+141.0.5...cef-dll-sys-v141.1.0+141.0.6) - 2025-10-17

### Other

- update bindings
- *(release)* update CEF version to 141.0.6

## [141.0.0+141.0.5](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.3.6+140.1.14...cef-dll-sys-v141.0.0+141.0.5) - 2025-10-16

### Other

- update bindings
- *(release)* update CEF version to 141.0.5

## [140.3.6+140.1.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.3.5+140.1.14...cef-dll-sys-v140.3.6+140.1.14) - 2025-10-14

### Other

- release v140.3.6+140.1.14

## [140.3.5+140.1.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.3.4+140.1.14...cef-dll-sys-v140.3.5+140.1.14) - 2025-10-13

### Other

- release v140.3.5+140.1.14

## [140.3.4+140.1.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.3.3+140.1.14...cef-dll-sys-v140.3.4+140.1.14) - 2025-10-13

### Added

- port SimpleApplication from original cefsimple
- add CefAppProtocol bindings

### Fixed

- resolve cargo build warning about default-features on macOS

### Other

- release v140.3.4+140.1.14

## [140.3.3+140.1.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.3.2+140.1.14...cef-dll-sys-v140.3.3+140.1.14) - 2025-10-11

### Other

- release v140.3.3+140.1.14

## [140.3.2+140.1.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.3.1+140.1.14...cef-dll-sys-v140.3.2+140.1.14) - 2025-10-11

### Fixed

- macos build with wgpu@27

### Other

- release v140.3.2+140.1.14
- cleanup dependencies
- upgrade wgpu to ^26
- move osr_texture_import onto main cef crate

## [140.3.1+140.1.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.3.0+140.1.14...cef-dll-sys-v140.3.1+140.1.14) - 2025-10-03

### Other

- release v140.3.1+140.1.14

## [140.3.0+140.1.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.2.0+140.1.14...cef-dll-sys-v140.3.0+140.1.14) - 2025-09-23

### Added

- add --mirror-url cli args
- Allow to download CEF binaries with custom base url set in env variable

### Other

- release

## [140.2.0+140.1.14](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.1.0+140.1.13...cef-dll-sys-v140.2.0+140.1.14) - 2025-09-21

### Other

- update bindings
- *(release)* update CEF version to 140.1.14

## [140.1.0+140.1.13](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v140.0.0+140.1.13...cef-dll-sys-v140.1.0+140.1.13) - 2025-09-19

### Other

- release

## [140.0.0+140.1.13](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.8.0+139.0.40...cef-dll-sys-v140.0.0+140.1.13) - 2025-09-19

### Other

- update bindings
- *(release)* update CEF version to 140.1.13

## [139.8.0+139.0.40](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.7.2+139.0.38...cef-dll-sys-v139.8.0+139.0.40) - 2025-09-12

### Other

- update bindings
- *(release)* update CEF version to 139.0.40

## [139.7.2+139.0.38](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.7.1+139.0.38...cef-dll-sys-v139.7.2+139.0.38) - 2025-09-08

### Fixed

- handle out-params ([#173](https://github.com/tauri-apps/cef-rs/issues/173))

### Other

- release v139.7.2+139.0.38
- update bindings

## [139.7.1+139.0.38](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.7.0+139.0.38...cef-dll-sys-v139.7.1+139.0.38) - 2025-09-07

### Other

- release v139.7.1+139.0.38
- *(deps)* update rust crate windows-sys to 0.61

## [139.7.0+139.0.38](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.6.0+139.0.37...cef-dll-sys-v139.7.0+139.0.38) - 2025-08-31

### Other

- *(release)* update CEF version to 139.0.38

## [139.6.0+139.0.37](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.5.0+139.0.30...cef-dll-sys-v139.6.0+139.0.37) - 2025-08-29

### Other

- *(release)* update CEF version to 139.0.37

## [139.5.0+139.0.30](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.4.0+139.0.28...cef-dll-sys-v139.5.0+139.0.30) - 2025-08-28

### Other

- *(release)* update CEF version to 139.0.30

## [139.4.0+139.0.28](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.3.0+139.0.26...cef-dll-sys-v139.4.0+139.0.28) - 2025-08-23

### Other

- *(release)* update CEF version to 139.0.28

## [139.3.0+139.0.26](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.2.1+139.0.23...cef-dll-sys-v139.3.0+139.0.26) - 2025-08-22

### Other

- *(release)* update CEF version to 139.0.26

## [139.2.1+139.0.23](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.2.0+139.0.23...cef-dll-sys-v139.2.1+139.0.23) - 2025-08-16

### Other

- release

## [139.2.0+139.0.23](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.1.0+139.0.20...cef-dll-sys-v139.2.0+139.0.23) - 2025-08-16

### Other

- *(release)* update CEF version to 139.0.23

## [139.1.0+139.0.20](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.0.1+139.0.17...cef-dll-sys-v139.1.0+139.0.20) - 2025-08-15

### Other

- *(release)* update CEF version to 139.0.20

## [139.0.1+139.0.17](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v139.0.0+139.0.17...cef-dll-sys-v139.0.1+139.0.17) - 2025-08-08

### Other

- release v139.0.1+139.0.17

## [139.0.0+139.0.17](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.9.0+138.0.36...cef-dll-sys-v139.0.0+139.0.17) - 2025-08-08

### Other

- update bindings
- *(release)* update CEF version to 139.0.17

## [138.9.0+138.0.36](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.8.0+138.0.34...cef-dll-sys-v138.9.0+138.0.36) - 2025-08-07

### Other

- *(release)* update CEF version to 138.0.36

## [138.8.0+138.0.34](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.7.1+138.0.33...cef-dll-sys-v138.8.0+138.0.34) - 2025-08-02

### Fixed

- remove cef version from example dependencies

### Other

- *(release)* update CEF version to 138.0.34

## [138.7.1+138.0.33](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.7.0+138.0.33...cef-dll-sys-v138.7.1+138.0.33) - 2025-07-29

### Other

- release v138.7.1+138.0.33
- move examples into separate crates

## [138.7.0+138.0.33](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.6.1+138.0.27...cef-dll-sys-v138.7.0+138.0.33) - 2025-07-29

### Other

- update bindings
- *(release)* update CEF version to 138.0.33

## [138.6.1+138.0.27](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.6.0+138.0.27...cef-dll-sys-v138.6.1+138.0.27) - 2025-07-28

### Fixed

- embed git-cliff as a library in get-latest

### Other

- *(release)* bump version for get-latest updates

## [138.6.0+138.0.27](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.5.1+138.0.26...cef-dll-sys-v138.6.0+138.0.27) - 2025-07-28

### Added

- update CEF version to 138.0.27

### Fixed

- bump version for release

## [138.5.1+138.0.26](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.5.0+138.0.26...cef-dll-sys-v138.5.1+138.0.26) - 2025-07-22

### Other

- release
- *(doc)* regenerate CHANGELOG.md

## [138.5.0+138.0.26](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.4.0+138.0.25...cef-dll-sys-v138.5.0+138.0.26) - 2025-07-19

### Other

- update CEF version

## [138.4.0+138.0.25](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.3.0+138.0.23...cef-dll-sys-v138.4.0+138.0.25) - 2025-07-18

### Other

- update CEF version

## [138.3.0+138.0.23](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.2.2+138.0.21...cef-dll-sys-v138.3.0+138.0.23) - 2025-07-17

### Other

- update CEF version

## [138.2.2+138.0.21](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.2.1+138.0.21...cef-dll-sys-v138.2.2+138.0.21) - 2025-07-14

### Other

- release
- seed CHANGELOG.md files

## [138.2.1+138.0.21](https://github.com/tauri-apps/cef-rs/compare/cef-dll-sys-v138.2.0+138.0.21...cef-dll-sys-v138.2.1+138.0.21) - 2025-07-14

### Fixed

- bump major version of download-cef [#145](https://github.com/tauri-apps/cef-rs/issues/145)

