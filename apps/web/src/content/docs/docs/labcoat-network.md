---
title: Labcoat Network and wallets
description: Operate Labcoat Network and the project wallet.
---

Labcoat Network is Labcoat's local, private, resettable Alkanes network. It
runs Bitcoin regtest under the hood. It is not Signet.

`labcoat up` downloads the Qubitcoin executable and its Alkanes and Esplora
WASM modules from the installed CLI's exact `cli-vX.Y.Z` release, then starts
one `qubitcoind` process. The payloads live under a matching version directory:

- macOS: `~/Library/Application Support/Labcoat/runtimes/cli-vX.Y.Z/`
- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/labcoat/runtimes/cli-vX.Y.Z/`

Labcoat does not check for an independently newer runtime. Updating the CLI
selects a new versioned bundle.

| Service | Purpose |
| --- | --- |
| qubitcoind | Bitcoin regtest chain plus in-process Alkanes and Esplora indexes; RPC on port 18443 |

## Operate the local services

```bash
labcoat up
labcoat status --json
labcoat logs --service qubitcoind --limit 100
labcoat snapshot clean
labcoat restore clean
labcoat down
```

Only one Labcoat Network should run per machine. `status` reports its
`labcoat` identity, underlying Bitcoin `regtest` mode, `qubitcoind`, chain
height, mempool size, and overall readiness.

## Wallet workflow

```bash
labcoat wallet init
labcoat wallet addresses --count 3
labcoat fund <p2tr-address> 1
labcoat mine 1
labcoat wallet utxos
```

The wallet derives BIP-86, BIP-84, BIP-49, and BIP-44 addresses. P2TR is the
primary address for Alkanes operations.

## Reset carefully

`labcoat reset -y` stops Qubitcoin and permanently removes v2 chain, index, and
faucet data. Legacy runtime data and snapshots are left untouched.

## Migrating an existing project

Change `network = "regtest"` to `network = "labcoat"` in `labcoat.toml`.
Labcoat does not migrate `networks.regtest` records in `labcoat.lock`; redeploy
contracts so they are recorded under `networks.labcoat`. Only rename the
lockfile key manually when you are intentionally preserving the exact same
local chain state. Existing wallets, snapshots, and runtime data remain
compatible.
