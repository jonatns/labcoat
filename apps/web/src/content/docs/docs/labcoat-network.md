---
title: Labcoat Network and wallets
description: Operate Labcoat Network and the project wallet.
---

Labcoat Network is Labcoat's local, private, resettable Alkanes network. It
runs Bitcoin regtest under the hood. It is not Signet.

The `labcoat` executable runs Labcoat Network itself: `labcoat up` starts it in
the background as one process holding the regtest chain, its mempool, and the
Alkanes indexer, and serves Bitcoin Core-style JSON-RPC on port 18443. It
validates transactions as Bitcoin Core relays them: signatures and scripts,
coinbase maturity, absolute and relative timelocks, fees of at least 1 sat/vB,
dust, and the 400,000 weight-unit limit.

The only download is the Alkanes indexer module, `alkanes.wasm`, from the
installed CLI's exact `cli-vX.Y.Z` release. It lives under a matching version
directory, beside a compiled copy made the first time it runs:

- macOS: `~/Library/Application Support/Labcoat/runtimes/cli-vX.Y.Z/`
- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/labcoat/runtimes/cli-vX.Y.Z/`

Labcoat does not check for an independently newer runtime. Updating the CLI
selects a new versioned bundle.

| Service | Purpose |
| --- | --- |
| node | Bitcoin regtest chain, mempool, and Alkanes index; RPC on port 18443 |

## Operate the local services

```bash
labcoat up
labcoat status --json
labcoat logs --service node --limit 100
labcoat snapshot clean
labcoat restore clean
labcoat down
```

Only one Labcoat Network should run per machine. `status` reports its
`labcoat` identity, underlying Bitcoin `regtest` mode, the `node` service, chain
height, mempool size, and overall readiness.

`labcoat down` keeps the chain; the next `labcoat up` resumes it in a fraction
of a second, because it reloads the saved index instead of indexing the blocks
again. Use `labcoat reset` to start from a fresh chain. The mempool is not
saved: mine pending transactions before `labcoat down`.

## JSON-RPC

Labcoat Network answers Bitcoin Core-style JSON-RPC at
`http://127.0.0.1:18443`, for tools and web apps as well as the CLI:

- chain and transactions: `getblockcount`, `getblockhash`, `getblock`,
  `getrawtransaction`, `gettxout`, `sendrawtransaction`, `testmempoolaccept`,
  `getmempoolinfo`, `getrawmempool`, and `generatetoaddress`;
- indexes: `secondaryheight` and `secondaryview` for `alkanes` and `esplora`,
  and `metashrew_height` and `metashrew_view`;
- snapshots: `labcoat_snapshot` returns an id, and `labcoat_revert` with that
  id returns the chain, mempool, and index to that moment in memory. Reverting
  uses the snapshot and every later one, as Hardhat's `evm_revert` does;
- identity: `labcoat_info` names the serving release and data directory.
  Labcoat only reuses a node whose answer matches it, and stops anything else
  holding the port, such as a `qubitcoind` from an earlier release.

```bash
id=$(curl -s -d '{"jsonrpc":"2.0","id":1,"method":"labcoat_snapshot"}' http://127.0.0.1:18443 | jq .result)
labcoat mine 5
curl -s -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"labcoat_revert\",\"params\":[$id]}" http://127.0.0.1:18443
```

`labcoat snapshot` and `labcoat restore` save whole chains to disk instead.

## Wallet workflow

`labcoat up` creates a wallet for the network and tops it up to 10 BTC when
it holds less than 1. Every project on the machine shares it, as they share
the chain. `labcoat reset` keeps its key, and the next `up` funds it again.

```bash
labcoat wallet addresses --count 3
labcoat wallet utxos
labcoat fund <p2tr-address> 1   # fund any other address
labcoat mine 1
```

The wallet works only on Labcoat Network; see
[Wallets](/docs/projects-configuration/#wallets).

The wallet derives BIP-86, BIP-84, BIP-49, and BIP-44 addresses. P2TR is the
primary address for Alkanes operations.

## Reset carefully

`labcoat reset -y` stops Labcoat Network and permanently removes its chain,
index, and faucet data. Data and snapshots from releases that ran Qubitcoin
are left untouched and are not read.

## Migrating an existing project

Change `network = "regtest"` to `network = "labcoat"` in `labcoat.toml`.
Labcoat does not migrate `networks.regtest` records in `labcoat.lock`; redeploy
contracts so they are recorded under `networks.labcoat`. Only rename the
lockfile key manually when you are intentionally preserving the exact same
local chain state. Existing wallets, snapshots, and runtime data remain
compatible.
