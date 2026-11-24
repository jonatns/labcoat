---
title: Projects and configuration
description: Understand the Labcoat project layout, configuration precedence, secrets, and deployment lockfile.
---

`labcoat init <project-name>` creates a new Rust-native workspace folder with a
fixed Counter starter. Run `labcoat init` without a name to enter it
interactively. Existing destinations are never overlaid. Add another minimal
contract later with `labcoat new token`.

```text
contracts/           Rust contract sources
tests/               Native integration tests; tests/e2e.rs runs on Labcoat Network
Cargo.toml           Host-side test project
deploy.rs            Deployment script for plan and deploy
deployments/         Per-network run files, created on deploy
labcoat.toml         Public project configuration
labcoat.lock         Per-network address book, created on deploy
rust-toolchain.toml  Pinned Rust toolchain and WebAssembly targets
AGENTS.md            Concise instructions for coding agents
SKILL.md             Complete Labcoat agent workflow
```

Commit `rust-toolchain.toml` and the `Cargo.lock` created by the first build.
The toolchain file pins the Rust release Labcoat is tested with, and rustup
installs it on first use. See [Deployment scripts](/docs/deployments/) for
`deploy.rs` and the run files, and [Deployment state](/docs/deployment-state/)
for durable deployment history.

`deploy.rs` is registered in the root `Cargo.toml` as the `deploy` example
target, which is how `labcoat plan` and `labcoat deploy` run it.

## Settings precedence

Settings resolve in this order:

```text
CLI flags → LABCOAT_* environment variables → labcoat.toml → defaults
```

`labcoat.toml` supports `network`, `rpc_url`, `wallet_file`, `fee_rate`,
`signer`, and `environment` (the durable-state environment, default
`default`). The default network is `labcoat` and the default Labcoat Network
RPC endpoint is `http://127.0.0.1:18443`.

## Signet and mainnet through Subfrost

`--network signet` and `--network mainnet` use [Subfrost](https://subfrost.io)'s
hosted Alkanes JSON-RPC unless you set another endpoint:

```bash
labcoat --network signet simulate 2:0 99
labcoat --network signet plan
```

| Network | Default endpoint |
|---|---|
| `signet` | `https://signet.subfrost.io/v4/jsonrpc` |
| `mainnet` | `https://mainnet.subfrost.io/v4/jsonrpc` |

`rpc_url = "subfrost"` (or `--rpc-url subfrost`) names the same endpoints, and
Subfrost's hosted regtest with `--network regtest`. Set `SUBFROST_API_KEY` to
send an API key. The public endpoints answer without one but allow only 20
requests a minute, which a deploy exceeds; past that Labcoat reports
`RPC_REJECTED`. An `rpc_url` in `labcoat.toml` applies only to the network that file
names, so a local endpoint never follows `--network signet`.

Reads and `labcoat plan` work today. Signing on a public network needs a
wallet for it (`labcoat --network signet wallet init`) and has not been
proven end to end; mainnet deployment controls are not production-ready.

`labcoat` selects Labcoat Network, Labcoat's local/private environment. It uses
Bitcoin regtest protocol rules underneath but is not Signet. The separate
`regtest` selector is reserved for custom regtest RPC endpoints and is rejected
with the default Labcoat endpoint.

## Wallets

Keystores live in Labcoat's data directory, one per network, never in the
project:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/Labcoat/wallets/<network>.json` |
| Linux | `~/.local/share/labcoat/wallets/<network>.json` |

Labcoat Network's wallet is created and funded by `labcoat up`. Its passphrase
is public, so Labcoat refuses to use it on any other network or to import a
mnemonic into it. Every other network needs its own:

```bash
labcoat --network signet wallet init
```

`--wallet-file`, `LABCOAT_WALLET_FILE`, or `wallet_file` in `labcoat.toml`
select a different keystore. A project created before this change sets
`wallet_file = ".labcoat/wallet.json"` and keeps using it on every network;
remove that line to use the defaults.

## Secrets

Never put a mnemonic or passphrase in `labcoat.toml` or on the command line.

- Set `LABCOAT_WALLET_PASSPHRASE` for the keystore passphrase.
- Set `LABCOAT_MNEMONIC` or use `wallet init --mnemonic-stdin` for recovery.
- Mainnet, signet, and testnet refuse keystore signing without an explicit passphrase.

## Deployment lockfile

`labcoat.lock` maps contract names to network-specific IDs, hashes, transaction
IDs, and deployment status. Commit it when deployments are part of shared
project state. After `labcoat reset -y`, redeploy contracts because the local
chain no longer contains those IDs.

## Run files

`deployments/<network>.json` records every `deploy.rs` step that ran on a
network, which is what makes each step run once. Commit the run files of
shared networks. A new project's `.gitignore` excludes
`deployments/labcoat.json`, because `labcoat reset` starts a chain where
nothing has run. See [the run file](/docs/deployments/#the-run-file).

### Pre-1.0 network rename

Existing projects must change `network = "regtest"` to `network = "labcoat"`.
New deployments are stored under `networks.labcoat`. Labcoat deliberately does
not read or migrate old `networks.regtest` entries; redeploy, or manually rename
that key only when preserving the exact same local chain.
