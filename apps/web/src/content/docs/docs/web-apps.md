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
  client.ts       fetch-based client for typed contract reads
  exchange.ts     exchange request builder
```

Every file begins with a "do not edit" header. Regenerating deletes only files
carrying that header, so hand-written files in the same directory survive and
removed contracts leave no stale modules. Commit the output or regenerate it
in your build.

## Read contract state

```ts
import { createClient } from './generated/web/client';

// Point rpc at a path your dev server proxies to the manifest's rpcUrl.
const labcoat = createClient({ rpc: '/rpc' });

const counter = labcoat.contract('counter');
const count = await counter.read.getCount(); // bigint
```

`contract(name)` takes a contract name from `labcoat.lock` and returns a
handle typed from that contract's ABI. Each ABI method is a function on
`read`, named in camelCase: `get_count` becomes `getCount`. A method that does
not exist, a missing argument, or an argument of the wrong type is a compile
error.

A read simulates the call against indexed state and returns the decoded value:

| ABI type   | As an argument                    | As a return value                    |
| ---------- | --------------------------------- | ------------------------------------ |
| `u128`     | `bigint` or `number`              | `bigint`                             |
| `String`   | `string`                          | `string`                             |
| `AlkaneId` | `"block:tx"` or `{ block, tx }`   | `{ block, tx }` with decimal strings |
| `Vec<u8>`  | not supported                     | `Uint8Array`                         |
| `void`     | not an argument type              | `undefined`                          |

A return type outside this table comes back as its raw bytes, a
`Uint8Array`. A method with a parameter type outside it cannot be called by
name; use [`simulate`](#raw-calls) with its opcode.

If two methods of one contract have the same camelCase name, such as
`get_count` and `getCount`, both keep their exact ABI names and neither is
renamed.

### Contracts that are not in labcoat.lock

A contract created on chain, such as a clone made by a factory, has no record
in `labcoat.lock`, but it shares its template's ABI. Pass its id and that ABI:

```ts
import { abiOrganism } from './generated/web/abi/organism';

const clone = labcoat.contract('2:12', abiOrganism);
const energy = await clone.read.getEnergy();
```

The id is `"block:tx"` or `{ block, tx }`. Every built contract has a module
under `abi/`, named after its build artifact. An ABI must be a literal
(`as const`) for its methods to be typed; with an ABI loaded at runtime the
handle still works, and its methods are looked up by string.

### Reverts

A read whose call reverts throws `ContractRevertError`. Its `reason` is the
contract's message, and it also carries `contract`, `method`, `gasUsed`, and
`data`.

```ts
import { ContractRevertError } from './generated/web/client';

try {
  await counter.read.getCount();
} catch (error) {
  if (error instanceof ContractRevertError) console.log(error.reason);
}
```

When a revert is an expected answer, such as asking whether anything exists at
an id, use `tryRead`. It has the same methods and returns the revert instead
of throwing it:

```ts
const result = await clone.tryRead.getEnergy();
if (result.status === 'success') {
  console.log(result.value); // bigint
} else {
  console.log(result.reason);
}
```

`tryRead` returns reverts only. A request that fails, or a response the client
cannot decode, still throws `LabcoatClientError`.

### Read at one height

Each read runs at the current indexed height. To make a set of reads describe
one state of the chain, pin them to a height:

```ts
const snapshot = labcoat.at(await labcoat.getIndexedHeight());

const [count, energy] = await Promise.all([
  snapshot.contract('counter').read.getCount(),
  snapshot.contract('2:12', abiOrganism).read.getEnergy(),
]);
```

`at(height)` returns a client that shares the first one's connection. Its
contract reads run at that height and do not look the height up. Balances and
`getIndexedHeight()` are not pinned and answer from the current index.

Reads started together, as in the `Promise.all` above, are sent as one
JSON-RPC batch request. Without a pinned height they also share one height
lookup. Labcoat Network answers batches; for an endpoint that does not, create
the client with `batch: false`.

### Balances and height

```ts
const height = await labcoat.getIndexedHeight();

const balances = await labcoat.getAlkanesBalances('bcrt1p...');
for (const { key, symbol, balance } of balances) {
  console.log(key, symbol, balance);
}
```

- `getAlkanesBalances(address)` sums full-precision balances across every
  outpoint the address controls.
- `getIndexedHeight()` returns the Alkanes index height.

### Raw calls

`simulate(contract, method, args)` is the untyped call underneath `read`.
Nothing is checked at compile time, and a revert is returned as
`status: 'revert'`, not thrown. Use it to call a numeric opcode or to get the
raw return bytes:

```ts
const result = await labcoat.simulate('2:12', '26', [1n]);
if (result.status === 'success') {
  console.log(result.data); // Uint8Array
}
```

`method` is an ABI method name or a decimal opcode. With a method name on an
id that is not in the manifest, pass the ABI as `{ abi }` in the fourth
argument; `{ height }` there reads at a height.

### Client options

- `rpc` is the JSON-RPC endpoint. It defaults to the endpoint recorded in the
  manifest. Browsers usually reach it through a same-origin proxy or rewrite
  in your app's dev server, which avoids configuring CORS on Labcoat Network.
  `baseUrl` is the earlier name for this option and still works.
- `fetchFn` replaces `fetch`, for tests and runtimes without a global one.
- `batch: false` sends every call as its own request.

`createClient(options)` is the same as `new LabcoatWebClient(options)`.

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
