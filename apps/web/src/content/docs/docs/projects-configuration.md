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
.env.example         Names of the secrets .env may hold; copy it to .env
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
CLI flags → environment variables → .env → labcoat.toml → defaults
```

`labcoat.toml` supports `network`, `rpc_url`, `wallet_file`, `fee_rate`,
`signer`, and `environment` (the durable-state environment, default
`default`). The default network is `labcoat` and the default Labcoat Network
RPC endpoint is `http://127.0.0.1:18443`, or the port `LABCOAT_RPC_PORT`
names.

Commands work from any directory inside the project. Labcoat finds the
project root, the nearest directory above the working directory that holds
`labcoat.toml`, and reads settings, `deploy.rs`, build artifacts, and
deployment records from there. A relative `wallet_file` in `labcoat.toml` is
relative to that file. Paths typed on the command line are relative to the
working directory, `--out-dir` and `--build-dir` included; left out, those
two default to `build` and `generated/web` in the project root.

Before `deploy`, `import`, `call`, and the exchange commands write to
anything other than Labcoat Network on this machine, they print the network
and endpoint they resolved, such as
`network: signet (https://signet.subfrost.io/v4/jsonrpc)`.

If `labcoat.toml` cannot be parsed, commands stop with `CONFIG_INVALID`.
`labcoat doctor` and `labcoat docs` still run: `doctor` reports the problem
as its `project config` check, beside the toolchain and runtime checks.

## The .env file

Labcoat reads a file named `.env` in the project root, beside
`labcoat.toml`, before it resolves any setting. It is where a Subfrost API
key goes:

```bash
SUBFROST_API_KEY=your-key
```

A variable already set in the environment wins over the file, including one
set to nothing, so `SUBFROST_API_KEY= labcoat plan` runs one command without
the key. The file is found from any directory inside the project. `.env` is
the only file read: there is no `.env.local` and no per-network file.

| `.env` may set | |
|---|---|
| `SUBFROST_API_KEY` | Sent to `subfrost.io` endpoints |
| `LABCOAT_NETWORK`, `LABCOAT_RPC_URL`, `LABCOAT_WALLET_FILE`, `LABCOAT_FEE_RATE`, `LABCOAT_SIGNER`, `LABCOAT_ENVIRONMENT` | The settings above. A relative `LABCOAT_WALLET_FILE` is relative to the working directory, as it is in the environment; `wallet_file` in `labcoat.toml` is the one relative to the project |
| `LABCOAT_PSBT_TIMEOUT_SECS` | How long `--signer psbt-file:<dir>` waits |
| `LABCOAT_WALLET_PASSPHRASE` | Loaded; on mainnet each command that reads it from `.env` prints a warning |

Everything else is left alone, so the file can be shared with other tools.
Another `LABCOAT_*` name, such as `LABCOAT_DATA_HOME` or `LABCOAT_RPC_PORT`,
is not set from `.env`, and Labcoat says so in a warning.
`LABCOAT_MNEMONIC` is refused: while `.env` sets it, commands stop with
`CONFIG_INVALID`. A seed phrase is needed once, by `wallet init`, and the
keystore holds it encrypted from then on.

### Variables in labcoat.toml

A string setting in `labcoat.toml` can take its value, or part of it, from a
variable with `${NAME}`:

```toml
network = "signet"
rpc_url = "https://alkanes.example/${ALKANES_RPC_TOKEN}/jsonrpc"
```

```bash
# .env
ALKANES_RPC_TOKEN=your-token
```

`${NAME}` is looked up in the environment first and then in `.env`, where
any name works, not only the ones in the table above. A name Labcoat does
not read itself stays out of the commands it runs and reaches a setting only
where `labcoat.toml` asks for it. `network`, `rpc_url`, `wallet_file`,
`signer`, and `environment` expand variables; `fee_rate` is a number and
does not. A variable that is unset or empty stops the command with
`CONFIG_INVALID`, which names the variable.

The expanded `rpc_url` is an ordinary setting from then on. Commands that
write print it, `labcoat doctor` shows it, and `labcoat generate web`
records it in the manifest a browser downloads. A token in the URL is
therefore visible in those places, as it is when the URL comes from
`--rpc-url` or `LABCOAT_RPC_URL`. `SUBFROST_API_KEY` is different: it
travels in a header and is never printed.

### Format

Lines are `NAME=value`. `export NAME=value`, `#` comments, and single- or
double-quoted values are accepted. A line that is not an assignment is
skipped with a warning that gives its line number and never its contents.

Keep `.env` out of git. `labcoat init` writes a `.gitignore` that excludes
it and a `.env.example` to commit in its place; in an older project add
`/.env` to `.gitignore`. `labcoat doctor` warns when `.env` exists and git
does not ignore it, and reports whether an API key is set and whether it
came from `.env` or the environment. No command prints the key, with
`--verbose` or `--json` or otherwise.

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
Subfrost's hosted regtest with `--network regtest`. Set `SUBFROST_API_KEY`,
in [the .env file](#the-env-file) or the environment, to send an API key from
[api.subfrost.io](https://api.subfrost.io). The public endpoints answer
without one but allow only 20 requests a minute, which a deploy exceeds; past
that Labcoat reports `RPC_REJECTED`. An `rpc_url` in `labcoat.toml` applies only to the network that file
names, so a local endpoint never follows `--network signet`.

[Deploy to signet](/docs/deploy-to-signet/) goes from a new wallet to a
deployed contract, step by step. Signing on a public network needs a wallet
for it (`labcoat --network signet wallet init`). A signet deploy has been
run by hand and no automated test broadcasts there; mainnet deployment
controls are not production-ready.

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

- Put `SUBFROST_API_KEY` in [the .env file](#the-env-file).
- Answer the prompt for the keystore passphrase, or set
  `LABCOAT_WALLET_PASSPHRASE` where there is no terminal.
- Set `LABCOAT_MNEMONIC` in the environment or use
  `wallet init --mnemonic-stdin` for recovery, never `.env`. See
  [Restore a wallet from a seed phrase](/docs/deploy-to-signet/#restore-a-wallet-from-a-seed-phrase).
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
