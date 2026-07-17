---
title: Projects and configuration
description: Understand the Labcoat project layout, configuration precedence, secrets, and deployment lockfile.
---

`labcoat init` creates a Rust-native workspace with a fixed Counter starter.
Add another minimal contract later with `labcoat new token`.

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

`labcoat.toml` supports `network`, `rpc_url`, `wallet_file`, and `fee_rate`.
The default network is `regtest` and the default gateway is
`http://localhost:18888`.

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
