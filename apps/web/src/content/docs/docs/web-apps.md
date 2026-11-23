---
title: Web apps
description: Generate a typed TypeScript client for reading deployed contracts from a browser.
---

`labcoat generate web` turns a project's deployments into a dependency-free
TypeScript module tree that a browser application can import.

```bash
labcoat build
labcoat generate web
```

Generation reads `labcoat.lock` and the `build/*.abi.json` files written by
`labcoat build`. It makes no network requests, so it is safe in CI. Pass
`--out-dir` to change the default `generated/web` destination and
`--build-dir` to read ABIs from somewhere other than `build`.

## What it writes

```text
generated/web/
  manifest.ts     network, RPC endpoint, chain id, and one record per contract
  abi/            typed method descriptors, one module per build artifact
  client.ts       fetch-based read client
  exchange.ts     exchange request builder
```

Every file begins with a "do not edit" header. Regenerating deletes only files
carrying that header, so hand-written files in the same directory survive and
removed contracts leave no stale modules. Commit the output or regenerate it
in your build.

## Read contract state

```ts
import { LabcoatWebClient } from './generated/web/client';

// Point baseUrl at a path your dev server proxies to the manifest's rpcUrl.
const client = new LabcoatWebClient({ baseUrl: '/rpc' });

const height = await client.getIndexedHeight();

const count = await client.simulate('counter', 'get_count');
if (count.status === 'success') {
  console.log(count.value); // 1n
}

const balances = await client.getAlkanesBalances('bcrt1p...');
for (const { key, symbol, balance } of balances) {
  console.log(key, symbol, balance);
}
```

- `simulate(contract, method, args)` runs an ABI-typed simulation against live
  indexed state. Return values are decoded from the ABI: `u128` as `bigint`,
  `String` as `string`, `AlkaneId` as `{ block, tx }` decimal strings, and
  `Vec<u8>` as `Uint8Array`.
- `getAlkanesBalances(address)` sums full-precision balances across every
  outpoint the address controls.
- `getIndexedHeight()` returns the Alkanes index height.

`baseUrl` defaults to the RPC endpoint recorded in the manifest. Browsers
usually reach it through a same-origin proxy or rewrite in your app's dev
server, which avoids configuring CORS on Labcoat Network.

The client is read-only: it signs nothing, holds no keys, and cannot change
chain state.

## Detect a reset chain

The manifest records the chain id (the hash of block 1) its deployments belong
to. After `labcoat reset`, redeploy and regenerate; an app can compare
`MANIFEST.chainId` with the live chain to notice stale artifacts.

## Exchange requests

`exchange.ts` lets a browser prepare an [atomic exchange](/docs/exchanges/)
without signing anything:

```ts
import {
  exchangeCeremonyCommands,
  serializeExchangeRequest,
} from './generated/web/exchange';

const requestFile = serializeExchangeRequest({
  offered: { block: '4', tx: '65011' },
  offeredAmount: 100n,
  payment: { block: '4', tx: '65012' },
  paymentAmount: 7500n,
  sellerAddress: 'bcrt1p...',
  buyerAddress: 'bcrt1p...',
});

const commands = exchangeCeremonyCommands({
  requestPath: 'exchange-request.json',
});
```

`serializeExchangeRequest` validates the request and produces the version 1
file that `labcoat exchange-plan --request` reads, byte for byte.
`exchangeCeremonyCommands` returns the plan, sign, and settle commands to run
with it. Planning, signing, and settlement stay in the CLI.
