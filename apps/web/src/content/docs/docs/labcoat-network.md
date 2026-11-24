---
title: Labcoat Network and wallets
description: Operate Labcoat Network and the project wallet.
---

Labcoat Network is Labcoat's local, private, resettable Alkanes network. It
runs Bitcoin regtest under the hood. It is not Signet.

The `labcoat` executable runs Labcoat Network itself: `labcoat up` starts it in
the background as one process holding the regtest chain, its mempool, and the
Alkanes indexer, and serves Bitcoin Core-style JSON-RPC on port 18443 unless
`LABCOAT_RPC_PORT` names another. It
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

`status` reports the network's `labcoat` identity, underlying Bitcoin `regtest`
mode, the `node` service, chain height, mempool size, and overall readiness.

Every project on the machine shares one Labcoat Network, its chain, and its
wallet. `labcoat test --e2e` is the exception: each project has an
[e2e network of its own](/docs/deployments/#end-to-end-tests), so a test run
never resets the chain you are working on.

To run a second, separate network, give it its own data home and port:

```bash
export LABCOAT_DATA_HOME=/path/to/another/home
export LABCOAT_RPC_PORT=18555
labcoat up
```

Commands run with both variables set use that network; contract commands
follow the port without an `rpc_url`. A network belongs to its data home:
`labcoat up`, `down`, and `reset` never stop a network that serves another
one. If the port is already taken by another data home's network, `labcoat up`
fails and says which, instead of replacing it.

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
  uses the snapshot and every later one, as Hardhat's `evm_revert` does. Ids
  are never reused, so retrying a used id returns `false` and changes nothing;
- identity: `labcoat_info` names the serving release and data directory.
  Labcoat only reuses a node whose answer matches it, and only stops what it
  can identify as this data home's: an earlier release's node that answers
  for the same data directory, the process its own pid file records, and a
  `qubitcoind` that an earlier release started on this data home. A network
  that serves another data home, and any program Labcoat does not recognize,
  keeps running: `labcoat up` fails and says the port is taken.

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

## Upgrading from a Qubitcoin-based release

Labcoat 0.3.0 and earlier ran Labcoat Network on a downloaded `qubitcoind`.
Later releases run it inside the `labcoat` executable and start a new chain.
Nothing from the old chain is converted, and nothing of it is deleted:

| What | After upgrading |
| --- | --- |
| Wallets | Kept. Keystores, including the built-in Labcoat Network wallet, stay where they are, and the next `labcoat up` funds the built-in wallet on the new chain. |
| Chain data | Not read. The old chain stays under `Isomer/runtime-v2/` in Labcoat's data directory; the new one is created under `Isomer/runtime-v3/`. |
| Snapshots | Not read. `labcoat snapshot` and `labcoat restore` use `Isomer/snapshots-v3/`; snapshots from the old chain stay under `Isomer/snapshots-v2/`. |
| Deployment records | `deployments/labcoat.json` and the `labcoat` entries of `labcoat.lock` name contracts on the old chain. The next `labcoat deploy` replaces them. |
| Durable state | An environment created by `labcoat state migrate` for the old chain refuses to deploy with `STATE_CHAIN_MISMATCH`. Archive `.labcoat/state/<environment>/` or pass a new `--environment`. |

To upgrade:

```bash
labcoat down                          # stop the old network, with the old CLI
curl -fsSL https://labcoat.sh/install | sh
labcoat up                            # a new chain; the wallet is funded again
labcoat upgrade && labcoat test       # in each project
labcoat deploy                        # redeploy onto the new chain
```

To roll back, stop the new network before reinstalling, because both
releases use port 18443:

```bash
labcoat down
curl -fsSL https://labcoat.sh/install | sh -s -- 0.3.0
labcoat up                            # resumes the old chain and its snapshots
labcoat upgrade                       # in each project, back to the 0.3.0 pins
labcoat deploy                        # if you deployed under the new release
```

The old chain and snapshots are where the old release left them. Deployment
records name whichever chain you last deployed to, so run `labcoat deploy`
again after switching. 0.3.0 runs Labcoat Network only on macOS arm64 and
Linux x86_64; see [Platform support](/docs/getting-started/installation/#platform-support).

## Migrating a project from `network = "regtest"`

Change `network = "regtest"` to `network = "labcoat"` in `labcoat.toml`.
Labcoat does not migrate `networks.regtest` records in `labcoat.lock`; redeploy
contracts so they are recorded under `networks.labcoat`. Only rename the
lockfile key manually when you are intentionally preserving the exact same
local chain state.
