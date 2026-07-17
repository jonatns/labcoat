# Migrating to Rust-first Labcoat

The TypeScript SDK, TypeScript CLI, and `create-labcoat` package have been
retired. Install the native binary and use explicit CLI commands instead.

## Project setup

Replace `npm create labcoat` with:

```bash
labcoat init my-project
cd my-project
labcoat test
```

Replace `labcoat.config.ts` with `labcoat.toml`. Settings resolve as CLI
flags, then `LABCOAT_*` environment variables, then the project file, then
defaults.

```toml
network = "regtest"
rpc_url = "http://localhost:18888"
wallet_file = ".labcoat/wallet.json"
fee_rate = 2.0
```

Never put mnemonic or passphrase material in the file. Use
`LABCOAT_MNEMONIC`, mnemonic stdin, and `LABCOAT_WALLET_PASSPHRASE`.

## Contracts are Cargo packages

Loose `contracts/Example.rs` sources are no longer supported. Move each
contract into its own Cargo package:

```text
contracts/counter/
  Cargo.toml
  src/lib.rs
```

The contract manifest needs `[lib] crate-type = ["cdylib", "rlib"]` and may
declare ordinary crates.io, git, workspace, and path dependencies. Put code
shared by several contracts in Cargo libraries under `crates/`. The root
manifest is both the host-test package and workspace manifest.

`labcoat build` builds every discovered contract; `labcoat build counter`
builds one. The first build creates a workspace `Cargo.lock`. Commit it because
upstream transitive git dependencies use moving branch references, and never
run an unscoped `cargo update`.

ABIs now come from the compiled contract's `__meta` export. Contracts using
`MessageDispatch` and `declare_alkane!` provide it automatically. Source-based
ABI scanning and storage-key discovery have been removed.

## Script migration

Replace SDK orchestration such as `labcoat.setup()` with direct commands or
ordinary shell scripts:

```bash
labcoat up
labcoat wallet init
labcoat build counter
labcoat deploy counter
labcoat simulate counter 2
labcoat call counter 1
```

The old `labcoat run` command no longer exists.

## Test migration

Replace `.spec.ts` contract tests with standard `tests/*.rs` files using
the version of `labcoat-test` matching the CLI:

```toml
[dev-dependencies]
labcoat-test = "=0.1.0"
```

Run them with `labcoat test`. Labcoat compiles WASIp1 test artifacts and
executes them through `ContractHarness`.

## Deployment migration

- Deploy raw `.wasm`, not `.wasm.gz`; the reveal envelope handles its own
  compression.
- Deployments now live in `labcoat.lock` by network.
- Run `labcoat lock migrate` once to import a legacy
  `deployments/manifest.json`.
- Wallet paths remain BIP-86/84/49/44. Verify address parity before using
  an existing mnemonic outside regtest.

## Automation

- Add `--json` for typed envelopes and recovery hints.
- Run `labcoat docs --llm` for the complete reference.
- Run `labcoat mcp serve` for MCP over stdio.
- New projects include `AGENTS.md` and `SKILL.md`.
