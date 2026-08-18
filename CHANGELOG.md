# Changelog

All notable Labcoat CLI and test-harness changes are documented here. Releases use Semantic Versioning and tags named `cli-vX.Y.Z`.

## [Unreleased]

### Added

- add `labcoat generate web`: deterministic TypeScript browser read artifacts (network manifest, typed ABI descriptors, and a dependency-free fetch client for indexed height, Alkanes balances, and ABI-typed simulate) derived from `labcoat.lock` and built ABIs with no network access; see docs/GENERATE-WEB.md

## [0.2.0](https://github.com/jonatns/labcoat/compare/cli-v0.1.0...cli-v0.2.0) - 2026-07-31

### Breaking changes

- rename the managed local chain and its selector to Labcoat Network / `labcoat`; existing projects must change `network = "regtest"` to `network = "labcoat"`
- keep `regtest` only for custom regtest RPC endpoints; the default Labcoat endpoint now rejects that selector
- record new local deployments under `networks.labcoat`; existing `networks.regtest` records are not migrated or read automatically, so redeploy or deliberately rename the key only when preserving the exact same local chain
- replace the Rust `Devnet` API with `LabcoatNetwork`, MCP `devnet_*` tools with `network_*`, and `DEVNET_ERROR` with `LABCOAT_NETWORK_ERROR`, without compatibility aliases

### Changed

- replace raw successful-command JSON dumps with concise, color-aware human output
- preserve plain human output when redirected; automation continues to use `--json`
- make `labcoat init` create a workspace with a fixed Counter starter
- replace `labcoat contract new` with top-level `labcoat new <name>`
- remove the `labcoat init --contract` option
- make `labcoat deploy <package>` build and deploy the selected contract directly
- rename the build-only command and MCP tool from `compile` to `build`

### Added

- add global `--verbose` and `--color auto|always|never` output controls

## [0.1.0](https://github.com/jonatns/labcoat/releases/tag/cli-v0.1.0) - 2026-07-16

### Added

- add two-track release automation
- add Playwright tests for homepage accessibility and navigation
- add settings management and test command for Labcoat CLI
