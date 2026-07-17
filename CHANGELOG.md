# Changelog

All notable Labcoat CLI and test-harness changes are documented here. Releases use Semantic Versioning and tags named `cli-vX.Y.Z`.

## [Unreleased]

## [0.2.0](https://github.com/jonatns/labcoat/compare/cli-v0.1.0...cli-v0.2.0) - 2026-07-17

### Added

- add Labcoat branding, documentation, and icon components

### Other

- Refactor assert_revert function and update test cases for improved error handling
- Add storage_u128 method to ContractHarness for decoding u128 values
- Remove shared crate templates and update related logic in project files
- Refactor contract initialization logic and update test assertions
- Enhance contract scaffolding and testing framework
- Refresh labcoat-test release trigger

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
