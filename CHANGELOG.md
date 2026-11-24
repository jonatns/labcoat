# Changelog

All notable Labcoat CLI and test-harness changes are documented here. Releases use Semantic Versioning and tags named `cli-vX.Y.Z`.

## [Unreleased]

### Breaking changes

- move default keystores out of the project into Labcoat's data directory, one per network. `labcoat.toml` no longer sets `wallet_file` in new projects; existing projects that set it are unaffected
- make `labcoat deploy` run the project's `deploy.rs`: `labcoat deploy` executes every pending step and `labcoat deploy <step>` runs one, both recorded in `deployments/<network>.json`. `labcoat deploy <package>` no longer builds and deploys a Cargo package on its own; declare it with `d.contract(name, package)` (a new project's `deploy.rs` already declares its Counter). `labcoat deploy --wasm <path>` still deploys a prebuilt artifact outside `deploy.rs`, and `--name`, `--args`, `--reserve`, and the transaction flags now apply only to it
- replace the `deploy` MCP tool's `package` argument with `step`, `replace`, and `dryRun`

### Added

- add Subfrost support: `--network signet` and `--network mainnet` default to Subfrost's hosted Alkanes JSON-RPC, `rpc_url = "subfrost"` names it, and `SUBFROST_API_KEY` is sent when set. An `rpc_url` in `labcoat.toml` now applies only to the network that file names
- give Labcoat Network a built-in funded wallet: `labcoat up` (and the `network_up` MCP tool) creates it, tops it up, and reports it, so `wallet init` and `fund` are no longer needed before the first deploy. It is refused on every other network
- add deployment scripts: declare a multi-step deployment in `deploy.rs` (the project's `deploy` Cargo example target) with `labcoat`, review it with `labcoat plan`, and execute it with `labcoat deploy`. The script only declares steps — contracts (with reserved ids, named initializers, and named arguments), template clones, external references bound per network, and configuration calls — and Labcoat runs each one once per network, recording it in `deployments/<network>.json`; see the deployment scripts guide
- pin deployment steps instead of converging them: a contract, clone, or call that already ran and no longer matches `deploy.rs`, or that references a replaced contract, is a `conflict` that `deploy` refuses with `DEPLOYMENT_CONFLICT`; `plan` and `deploy` take `--replace <step>` to deploy a new instance (or run a call again), keeping the earlier instance in the record's history. Labcoat Network is the exception: changed contracts and calls, and whatever references them, are replaced automatically with new ids and empty storage
- add upgradeable contract deployments: `d.implementation` redeploys when its code changes, and `d.beacon`, `d.beacon_proxy`, `d.proxy`, and `d.setting` keep an upgradeable pointer in sync by reading the current value and writing only when it differs, spending and returning the auth token the deploy minted
- add `labcoat new <name> --template auth-token|upgradeable|upgradeable-beacon|beacon-proxy`: vendor the standard alkanes-rs upgradeable contracts into a project as contract packages
- add `labcoat import <name> <block:tx>` (and the `import` MCP tool): record a contract that already exists on chain as a deployment step after comparing its ABI with the local build, without deploying
- add `--abi <contract>` to `labcoat call` and `labcoat simulate` (and `abi` to the matching MCP tools): resolve the method name against another contract's ABI, for calls through a proxy
- add `labcoat test --e2e` support for deployment scripts: the run resets the network and deploys `deploy.rs` before the tests; `E2e::contract` resolves any contract or external step, `E2e::created` returns an Alkane a deploy created, and `Call::abi`, `E2e::simulate_via`, and `E2e::simulate_uint_via` call through a proxy
- add an upgradeable contract tutorial and an `upgradeable-counter` example
- add `labcoat upgrade [--dry-run]`: align an existing project's alkanes-rs revisions, `labcoat-test` release tag, and `rust-toolchain.toml` with the installed CLI, preserving the rest of `Cargo.toml`; `labcoat doctor` adds a project pins check that names every stale pin
- add MCP tools `state_list`, `state_show`, `state_migrate`, and `generate_web`, matching their CLI commands
- add `labcoat doctor` checks for rustup and for whether the project's pinned Rust toolchain is missing, installed, pending installation, or different from the installed CLI's
- add `labcoat state migrate`: create version-2 per-environment durable state at `.labcoat/state/<environment>/state.json` from the v1 `labcoat.lock` ledger, with a timestamped backup, a compare-and-swap serial, an exclusive environment lease, and a byte-identical regenerated address book; see docs/STATE.md
- add `labcoat state list` and `labcoat state show <resource> [--history]`: inspect durable resources and their append-only instance history
- add an `environment` setting (`labcoat.toml`, `LABCOAT_ENVIRONMENT`, global `--environment`) selecting the durable-state environment, defaulting to `default`
- add a persistent Labcoat Network instance UUID, regenerated by `labcoat reset` and shown by `labcoat status`, recorded in durable state as chain identity
- record deployments and reserve adoptions into durable state when it exists, and refuse to broadcast onto a reset or foreign chain with `STATE_CHAIN_MISMATCH`; `labcoat.lock` remains the canonical address book

- add `labcoat generate web`: deterministic TypeScript browser read artifacts (network manifest, typed ABI descriptors, and a dependency-free fetch client for indexed height, Alkanes balances, and ABI-typed simulate) derived from `labcoat.lock` and built ABIs with no network access; see docs/GENERATE-WEB.md
- add `labcoat exchange-plan --request <file>`: build the plan from a version-1 exchange request file (canonical-decimal-string JSON) instead of positional flags
- add `exchange.ts` to `labcoat generate web`: a browser-side exchange request builder that validates and serializes the request file and renders the `exchange-plan` → `wallet sign-psbt` → `exchange-settle` ceremony commands; no signing or transaction construction in the browser

### Changed

- upgrade alkanes-rs to `v3.0.0-rc.4` (`ea38c87`) for the CLI, generated projects, and the managed Alkanes indexer: it brings the Halborn audit fixes and DIESEL v3. On Labcoat Network both the post-audit consensus rules and DIESEL v3 are active from block 0, matching post-fork mainnet behavior; projects created by earlier releases keep their pinned revision until updated. Metashrew and Qubitcoin are unchanged
- fail closed with `CONFIG_INVALID` when alkanes-rs rejects the selected network's parameters, instead of continuing with no network configured
- `labcoat init` writes a `rust-toolchain.toml` pinning Rust 1.100.0 with the `wasm32-unknown-unknown` and `wasm32-wasip1` targets, so rustup installs the tested toolchain on the first build with no manual `rustup target add`; generated projects now require rustup
- drop the Rust 1.86 compatibility pins (`time`, `serde_with`) from generated projects, removing their unused-dependency warnings
- upgrade `labcoat-test` to wasmtime 51, resolving the wasmtime and WASI security advisories in the native contract test harness
- build the CLI with Rust 1.100.0; the release workflow had still pinned 1.86.0

### Fixed

- fund a deploy's commit transaction from the wallet's largest UTXOs: in a wallet holding several small change outputs, the commit could be funded with almost no change, leaving the reveal to re-select an input the commit had just spent and fail as a conflicting replacement
- stop `labcoat reset` from declaring its own `--yes`, which clashed with the global `--yes`: every `labcoat reset` invocation panicked in debug builds and the flag was ambiguous in release builds. `-y` and the global `--yes` both skip the prompt
- index the first block mined after `labcoat down` and `labcoat up`: an upstream runtime defect made both indexers drop it, so funding right after a restart never reached the wallet. Resuming a chain now mines two empty warm-up blocks first; the release acceptance flow covers the restart
- keep every atomic exchange output single-asset: surplus from partially spent inputs now returns to its owner in its own change output instead of joining the counterparty's delivery, so both wallets can use their exchange outputs in later exchanges
- make `labcoat test --e2e` honor `LABCOAT_TEST_CRATE_PATH` like `labcoat test`, so CI and packagers can run e2e tests before the release tag exists
- stop cargo from printing two `invalid character '{' in package name` errors on a project's first build: the `labcoat init` and `labcoat new` manifest templates are now stored as `Cargo.toml.tmpl`, so cargo no longer parses them when it scans the labcoat git repository for `labcoat-test`
- keep tracing logs out of normal human output: the default log level is now `warn` (`-v/--verbose` adds Labcoat's info logs, and `RUST_LOG` still overrides), so `labcoat test` no longer prints a timestamped `INFO labcoat_core::compile` line. Log lines on stderr follow `--color`, `NO_COLOR`, and terminal detection instead of always carrying ANSI codes
- report a missing wallet as `no wallet keystore exists at <path>` (`WALLET_MISSING`) for `labcoat wallet addresses` and every other command that needs the keystore, instead of upstream's misleading "wallet is in address-only or external key mode"

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
