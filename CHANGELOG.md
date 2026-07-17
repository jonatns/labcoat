# Changelog

All notable Labcoat CLI and test-harness changes are documented here. Releases use Semantic Versioning and tags named `cli-vX.Y.Z`.

## [Unreleased]

### Changed

- make `labcoat init` create a workspace with a fixed Counter starter
- replace `labcoat contract new` with top-level `labcoat new <name>`
- remove the `labcoat init --contract` option
- make `labcoat deploy <package>` build and deploy the selected contract directly
- rename the build-only command and MCP tool from `compile` to `build`

## [0.1.0](https://github.com/jonatns/labcoat/releases/tag/cli-v0.1.0) - 2026-07-16

### Added

- add two-track release automation
- add Playwright tests for homepage accessibility and navigation
- add settings management and test command for Labcoat CLI

The first release on the new native CLI track will be `0.1.0`.
