---
title: Devnet and wallets
description: Operate the managed local Bitcoin devnet services and project wallet.
---

`labcoat up` checks required service binaries, downloads missing pinned builds,
starts the managed local services, and exposes one JSON-RPC gateway.

| Service | Purpose |
| --- | --- |
| bitcoind | Bitcoin regtest chain |
| metashrew | Alkanes state index |
| ord | Ordinals and inscription index |
| esplora | Chain query API |
| espo | Explorer and trace services |
| gateway | Unified JSON-RPC endpoint on port 18888 |

## Operate the local services

```bash
labcoat up
labcoat status --json
labcoat logs --service metashrew --limit 100
labcoat snapshot clean
labcoat restore clean
labcoat down
```

Only one Labcoat devnet should run per machine. `status` reports each service,
chain height, mempool size, and overall readiness.

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

`labcoat reset -y` stops services and permanently removes local chain and index
data. Snapshots are the safer choice when you expect to return to a known state.
