---
title: Projects and configuration
description: Understand the Labcoat project layout, configuration precedence, secrets, and deployment lockfile.
---

`labcoat init <project-name>` creates a new Rust-native workspace folder with a
fixed Counter starter. Run `labcoat init` without a name to enter it
interactively. Existing destinations are never overlaid. Add another minimal
contract later with `labcoat new token`.

```text
contracts/          Rust contract sources
tests/              Native integration tests
Cargo.toml          Host-side test project
labcoat.toml        Public project configuration
labcoat.lock        Per-network deployment ledger, created on deploy
AGENTS.md            Concise instructions for coding agents
SKILL.md             Complete Labcoat agent workflow
```

## Settings precedence

Settings resolve in this order:

```text
CLI flags → LABCOAT_* environment variables → labcoat.toml → defaults
```

`labcoat.toml` supports `network`, `rpc_url`, `wallet_file`, `fee_rate`,
`signer`, and `environment` (the durable-state environment, default
`default`). The default network is `labcoat` and the default Qubitcoin RPC
endpoint is `http://127.0.0.1:18443`.

`labcoat` selects Labcoat Network, Labcoat's local/private environment. It uses
Bitcoin regtest protocol rules underneath but is not Signet. The separate
`regtest` selector is reserved for custom regtest RPC endpoints and is rejected
with the default Labcoat endpoint.

## Secrets

Never put a mnemonic or passphrase in `labcoat.toml` or on the command line.

- Set `LABCOAT_WALLET_PASSPHRASE` for the keystore passphrase.
- Set `LABCOAT_MNEMONIC` or use `wallet init --mnemonic-stdin` for recovery.
- Mainnet and signet refuse wallet operations without an explicit passphrase.

## Deployment state

`labcoat.lock` maps contract names to network-specific IDs, hashes, transaction
IDs, and deployment status. Commit it when deployments are part of shared
project state. After `labcoat reset -y`, redeploy contracts because the local
chain no longer contains those IDs.

### Pre-1.0 network rename

Existing projects must change `network = "regtest"` to `network = "labcoat"`.
New deployments are stored under `networks.labcoat`. Labcoat deliberately does
not read or migrate old `networks.regtest` entries; redeploy, or manually rename
that key only when preserving the exact same local chain.
