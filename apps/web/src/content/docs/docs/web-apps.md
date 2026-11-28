---
title: Web apps
description: Generate a typed TypeScript client that reads deployed contracts and sends wallet-signed calls from a browser.
---

`labcoat generate web` turns a project's deployments into a dependency-free
TypeScript client for reading contracts and sending wallet-signed calls.
For the starter Counter, run these commands from the project root:

```bash
labcoat up
labcoat deploy
labcoat generate web
```

For an existing deployment, run `labcoat build` if you need to refresh the
ABIs. Use [Deploy to signet](/docs/deploy-to-signet/) for a public-network
deployment. Set your app's TypeScript target to ES2020 or newer for
`bigint` support.

Generation reads `labcoat.lock` and the `build/*.abi.json` files written by
`labcoat build`. It makes no network requests, so it is safe in CI. Pass
`--out-dir` to change the default `generated/web` destination and
`--build-dir` to read ABIs from somewhere other than `build`.

Both defaults are in the project root, from any directory inside the project.
A path you pass is relative to the directory you run the command in, so in an
app that lives in `web/`:

```bash
cd web
labcoat generate web --out-dir src/generated/web
```

The command prints the directories it used as absolute paths (`Out dir` and
`Build dir`; `outDir` and `buildDir` with `--json`).

This page is the reference. To build a page step by step, from `labcoat init`
to a signed transaction in the browser, follow the
[web app tutorial](/docs/tutorials/web-app/).

## What it writes

```text
generated/web/
  manifest.ts     network, RPC endpoint and dialect, chain id, and one record per contract
  abi/            typed method descriptors, one module per build artifact
  client.ts       fetch-based client: typed reads and wallet-signed writes
  wallet.ts       the wallet interface, the dev wallet, browser extension adapters
  transaction.ts  builds the unsigned transaction of a call
  errors.ts       the errors the client throws
  exchange.ts     exchange request builder
```

Every file begins with a "do not edit" header. Regenerating deletes only files
carrying that header, so hand-written files in the same directory survive and
removed contracts leave no stale modules. Commit the output or regenerate it
in your build.

The files need TypeScript 4.9 or newer and a `target` of ES2020 or later in
`tsconfig.json`, because they use `bigint` literals. A new Next.js app targets
ES2017; raise it, or type checking fails with TS2737 in every generated file.

## Read contract state

[Forward `/rpc` to the local RPC endpoint](#proxy-the-local-rpc-endpoint),
or [configure the Subfrost route](#reach-subfrost-from-a-browser) for signet
or mainnet. In an app file next to the generated directory:

```ts
import { createClient } from './generated/web/client';

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

### Ids and numbers packed into bytes

A contract often returns a `Vec<u8>` that packs Alkane ids, 32 bytes each:
the block, then the tx, both little-endian `u128`s. A factory's list of the
clones it made is one, and so is the id a `mint` returns. The client exports
decoders for these bytes:

```ts
import { decodeAlkaneId, decodeAlkaneIds, decodeU128 } from './generated/web/client';

const clones = decodeAlkaneIds(await factory.read.getClones());
// [{ block: "2", tx: "12" }, { block: "2", tx: "13" }]

const receipt = await (await factory.write.mint({ wallet })).wait();
const minted = decodeAlkaneId(receipt.value);
const clone = labcoat.contract(minted, abiOrganism);
```

| Function                         | Reads                           | Returns           |
| -------------------------------- | ------------------------------- | ----------------- |
| `decodeU128(bytes, offset?)`     | 16 bytes at `offset`, default 0 | `bigint`          |
| `decodeAlkaneId(bytes, offset?)` | 32 bytes at `offset`, default 0 | `{ block, tx }`   |
| `decodeAlkaneIds(bytes)`         | all of it, 32 bytes per id      | `{ block, tx }[]` |

An id is `{ block, tx }` with decimal strings, the shape a read returns for
an `AlkaneId`, so it can be passed to `contract()` or as an `AlkaneId`
argument as it is; `` `${id.block}:${id.tx}` `` is its `"block:tx"` form. The
offset forms read a value out of a larger layout and ignore the bytes after
it. Data too short for the value, or a list whose length is not a multiple of
32, throws `LabcoatClientError` with code `RESPONSE_INVALID`.

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

Reads started together, as in the `Promise.all` above, share one height
lookup when no height is pinned. On Labcoat Network they are also sent as one
JSON-RPC batch request. A hosted endpoint such as Subfrost's does not answer
batches, so there each read is its own request; see
[Endpoints](#endpoints).

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
  in your app's dev server, which avoids configuring CORS on Labcoat Network,
  and have to for Subfrost's endpoints.
  `baseUrl` is the earlier name for this option and still works.
- `dialect` is the JSON-RPC dialect the endpoint speaks, `'labcoat'` or
  `'alkanes'`. It defaults to the manifest's `rpcDialect`; see
  [Endpoints](#endpoints).
- `fetchFn` replaces `fetch`, for tests and runtimes without a global one.
- `batch` sends calls made together as one JSON-RPC batch request. It
  defaults to `true` in the `'labcoat'` dialect and `false` in the
  `'alkanes'` one.
- `headers` are extra HTTP headers for every request, for a client that runs
  on a server. Do not put an API key here in code that ships to a browser.
- `wallet` is the wallet `write` calls use when none is passed to them, and
  `feeRate` their fee rate in sat/vB.

`createClient(options)` is the same as `new LabcoatWebClient(options)`.

## Endpoints

The client speaks two JSON-RPC dialects, and one client speaks one of them for
everything it does: reads, funding a transaction, broadcasting it, and waiting
for it.

| Dialect     | Served by                                                  | Indexed height             | Alkanes views                               | An address's outputs, a transaction's hex    |
| ----------- | ---------------------------------------------------------- | -------------------------- | ------------------------------------------- | -------------------------------------------- |
| `'labcoat'` | Labcoat Network, a Qubitcoin node                          | `secondaryheight`          | `secondaryview ["alkanes", view, hex]`      | `secondaryview ["esplora", view, hex]`       |
| `'alkanes'` | a hosted provider of the standard Alkanes JSON-RPC         | `metashrew_height`         | `metashrew_view [view, hex, "latest"]`      | `esplora_address::utxo`, `esplora_tx::hex`   |

`gettxout` and `sendrawtransaction` are the same in both.

`labcoat generate web` records the dialect of the endpoint it generates for
as `rpcDialect` in the manifest, by the rule the CLI uses: `'alkanes'` for a
`subfrost.io` endpoint, which is the default for `--network signet` and
`--network mainnet`, and `'labcoat'` for anything else. Because it is in the
manifest, it still holds when `rpc` is a same-origin path your server forwards
to that endpoint. For another provider that speaks the standard Alkanes JSON-RPC, set
`dialect: 'alkanes'`. In a browser, point `rpc` at your forwarding route:

```ts
const labcoat = createClient({ rpc: '/rpc', dialect: 'alkanes' });
```

With the wrong dialect the first request fails with the endpoint's own
refusal. Subfrost's reads `method not permitted on the public endpoint:
secondaryheight`.

### Proxy the local RPC endpoint

For Labcoat Network, forward `/rpc` to the URL from `labcoat up`. For
example, if you generated into `web/src/generated/web`, add this to
`web/next.config.ts`:

```ts
import type { NextConfig } from 'next';
import { MANIFEST } from './src/generated/web/manifest';

const config: NextConfig = {
  async rewrites() {
    return [{ source: '/rpc', destination: MANIFEST.rpcUrl }];
  },
};

export default config;
```

Start Next.js with `npm run dev -- --hostname localhost` when using
the dev wallet. For a private upstream URL, use the server-side route below.

### Reach Subfrost from a browser

A page cannot call Subfrost's endpoints directly. They answer a CORS
preflight, but the response to the request itself carries no
`Access-Control-Allow-Origin` header, so the browser blocks it and the client
throws `RPC_UNREACHABLE`. Serve a same-origin path and create the client
with that path. When `labcoat generate web` sets the manifest's `rpcUrl` to
`/rpc` for a private endpoint, set `LABCOAT_RPC_TARGET` to the full upstream
URL in the app server's environment. In a Next.js app:

```ts
// app/rpc/route.ts
import { MANIFEST } from '@/generated/web/manifest';

export async function POST(request: Request) {
  const rpcUrl = process.env.LABCOAT_RPC_TARGET ?? MANIFEST.rpcUrl;
  const target = URL.canParse(rpcUrl) ? new URL(rpcUrl) : null;
  if (!target || !['http:', 'https:'].includes(target.protocol)) {
    return new Response('Set LABCOAT_RPC_TARGET to the upstream RPC URL', { status: 500 });
  }

  const headers = new Headers({ 'content-type': 'application/json' });
  const key = process.env.SUBFROST_API_KEY;
  const subfrost = target.hostname === 'subfrost.io' || target.hostname.endsWith('.subfrost.io');
  if (key && subfrost) headers.set('x-subfrost-api-key', key);
  if (target.username || target.password) {
    const user = decodeURIComponent(target.username);
    const password = decodeURIComponent(target.password);
    headers.set('authorization', 'Basic ' + Buffer.from(user + ':' + password).toString('base64'));
    target.username = '';
    target.password = '';
  }

  const upstream = await fetch(target, {
    method: 'POST',
    headers,
    body: await request.text(),
    redirect: 'manual',
  });

  const answer = new Headers({ 'content-type': 'application/json' });
  const retryAfter = upstream.headers.get('retry-after');
  if (retryAfter !== null) answer.set('retry-after', retryAfter);
  return new Response(upstream.body, { status: upstream.status, headers: answer });
}
```

```ts
const labcoat = createClient({ rpc: '/rpc' });
```

For Vite, a dev proxy at `server.proxy['/rpc']` can forward the same path.
The manifest's `rpcDialect` still applies when the browser uses `/rpc`.

### Subfrost's request limit

Without an API key, Subfrost's public endpoints answer 20 requests a minute
for each IP address. Behind a forwarding route that address is your server's,
so every visitor shares the 20. Over the limit the endpoint answers HTTP 429
with `retry-after: 60`, which the client throws as `LabcoatClientError` with
code `RPC_RATE_LIMITED`. The client does not retry, except that `tx.wait()`
and `waitForTransaction` keep polling through it.

Subfrost does not answer JSON-RPC batches, so every call is one request:

| What the page does                          | Requests                                                        |
| ------------------------------------------- | --------------------------------------------------------------- |
| One read                                    | 2: the indexed height, then the call                            |
| `n` reads in one `Promise.all`              | `n` + 1                                                         |
| `n` reads on a client pinned with `at()`    | `n`                                                             |
| `getAlkanesBalances`, `getIndexedHeight`    | 1                                                               |
| A write, up to the wallet's prompt          | 3, plus 2 for each wallet address, plus 1 for each input chosen and each output newer than 100 blocks |
| Broadcasting it                             | 1                                                               |
| `tx.wait()`                                 | 1 every 10 seconds                                              |
| `waitForTransaction(txid)`                  | 1, then 2 every 10 seconds                                      |

One person who reads a few values, sends a write, and waits for it fits in
the limit. Several visitors at once, or a page that polls reads every few
seconds, do not. Pin reads to one height with `at()`, read again when
`getIndexedHeight()` changes instead of on a timer, and get a key before
other people use the app.

### API keys

A key from [api.subfrost.io](https://api.subfrost.io) raises the limit. The
endpoint takes it in the `X-Subfrost-Api-Key` header, the header `labcoat`
itself sends when `SUBFROST_API_KEY` is set.

Everything in a browser bundle is public, so the key cannot be in the page.
It belongs in the forwarding route, which is the only thing that talks to
Subfrost: the route above adds it when `SUBFROST_API_KEY` is set in the
server's environment. A plain rewrite cannot add a header, so an app with a
key needs a route or a proxy that can.

That route spends your key's quota for anyone who can reach it. Limit what
it forwards to the methods the client uses: `metashrew_height`,
`metashrew_view`, `esplora_address::utxo`, `esplora_tx::hex`, `esplora_tx::status`, `gettxout`, and
`sendrawtransaction`.

A client that runs on a server, in a script or a server component, has no
CORS to satisfy and can send the key itself:
`createClient({ headers: { 'x-subfrost-api-key': key } })`.

## Send transactions

`write` has the same methods as `read`. Where a read simulates a call, a write
sends it as a transaction that a wallet signs:

```ts
const wallet = await labcoat.connect(); // Labcoat Network's dev wallet

const tx = await counter.write.increment({ wallet });
await tx.wait();
console.log(tx.txid, await counter.read.getCount());
```

The client builds an unsigned PSBT using the same transaction rules as
`labcoat call`. The wallet signs it with its own keys, and the client
broadcasts the signed transaction.

A write takes the method's arguments and then an options object:

```ts
await token.write.transfer('bcrt1p...', 100n, {
  wallet,
  alkanes: [{ id: '2:5', amount: 300n }],
  pay: [{ address: 'bcrt1q...', sats: 10_000 }],
  feeRate: 5,
});
```

| Option     | Meaning                                                                                   | `labcoat call` |
| ---------- | ----------------------------------------------------------------------------------------- | -------------- |
| `wallet`   | The wallet that pays and signs. Defaults to the client's `wallet` option                  | the keystore   |
| `alkanes`  | Alkanes to send in with the call, taken from the wallet's outputs                         | `--inputs`     |
| `pay`      | Bitcoin payment outputs, in order; the first is output 1                                  | `--pay`        |
| `to`       | Address the call's results and refunds go to. Defaults to the wallet's own                | `--to`         |
| `feeRate`  | Fee rate in sat/vB. Defaults to 2 on Labcoat Network; required on other networks          | `--fee-rate`   |
| `simulate` | `false` skips the simulation described under [Reverts and rejections](#reverts-and-rejections) | none      |

`--pointer`, `--refund`, and `--edict` have no counterpart: results and
refunds go to output 0, the `to` address. Neither do bitcoin entries in
`--inputs` (`B:sats`); use `pay`. An amount of an Alkane to send must fit in
64 bits, as on the command line.

An output that carries Alkanes is never spent to pay for a transaction. It is
spent only when it holds an Alkane the call sends in, and then the call
receives exactly the amount asked for and the rest returns to output 0. If
that output also holds another Alkane, the write fails with
`TRANSACTION_INVALID` and nothing is signed; send that Alkane too, or move it
to its own output first.

### Connect a wallet

#### Labcoat Network: the dev wallet

```ts
const wallet = await labcoat.connect();
```

`labcoat up` funds the built-in wallet. Open the app at
`http://localhost:<port>` so the
[dev signer](/docs/labcoat-network/#dev-signer) can sign. The dev wallet
mines a block after broadcasting. For a local approve or reject flow,
use `labcoat.connect({ approve: true })`.

#### Public networks: choose a wallet

Connect the wallet your app selects, then pass it as `{ wallet, feeRate }`
to the write. A public-network call to `connect()` without a wallet name
throws `WALLET_UNAVAILABLE`. Use the
[wallet integrations guide](/docs/web-wallets/) to wrap a wallet library
or to prepare and sign a PSBT on another device.

The client can spend taproot and native SegWit (P2WPKH) outputs. If a wallet
uses a nested SegWit payment address, fund its taproot address for fees.
For custom regtest, provide your own wallet or a compatible adapter.
Calling `connect()` without a wallet name on regtest attempts to use the
Labcoat dev signer.

#### Built-in extension adapters

When the app already knows which extension the user selected, pass its ID:

```ts
const wallet = await labcoat.connect({ wallet: 'unisat' });
const tx = await counter.write.increment({ wallet, feeRate: 2 });
await tx.wait();
```

The built-in IDs are `unisat`, `xverse`, `okx`, `leather`, and
`subfrost`. `labcoat.availableWallets()` lists the available adapters for
an app's wallet picker. Confirm that the chosen extension supports the
network before testing a write.

### Wait for the result

`tx.wait()` resolves when the Alkanes index has the transaction, with the
call's return value decoded by its ABI:

```ts
const { value, gasUsed } = await tx.wait();
```

`labcoat.waitForTransaction(txid)` does the same from a transaction id alone
and leaves the return value as bytes. Both take `{ timeoutMs, pollMs, onStatus }` and
throw a `LabcoatClientError` with code `TRANSACTION_TIMEOUT` when the time is
up. The time allowed is 60 seconds on Labcoat Network and regtest, and 30
minutes on a public network. The index is asked every 500 ms in the
`'labcoat'` dialect and every 10 seconds in the `'alkanes'` one, to stay
inside a hosted endpoint's [request limit](#subfrosts-request-limit).

On a public network, wait for the transaction to enter a block and for
the index to catch up. The timing depends on the network and transaction.
The dev wallet on Labcoat Network mines after broadcasting. To exercise
the pending state locally, connect with `{ mine: false }` and mine a block
with `labcoat mine`.

On a public network the wait is minutes long. `onStatus` tells the page where
the transaction is:

```ts
await tx.wait({
  onStatus: (status) => {
    if (status.status === 'broadcast') show('Sent. Waiting for the network…');
    if (status.status === 'pending') show('In the mempool. Waiting for a block…');
    if (status.status === 'confirmed') show('Confirmed. Waiting for the index…');
    if (status.status === 'indexed') show('Indexed. Checking the result…');
  },
});
```

| Status      | Meaning                                                              |
| ----------- | -------------------------------------------------------------------- |
| `broadcast` | The wait has started                                                 |
| `pending`   | The endpoint has the transaction, and it is not in a block           |
| `confirmed` | It is in a block. `blockHeight` is available when the endpoint supplies it. |
| `indexed`   | The index has the call's result; `wait()` resolves or throws next    |

The callback reports each observed status at most once, in order. A fast
transaction may go straight from `broadcast` to `indexed`. Wait for
`tx.wait()` to resolve before showing success, since an indexed call can
still have reverted.

Without `onStatus` a wait makes only the index lookup. With it, the client
also asks whether the transaction is in a block: with every poll in the
`'labcoat'` dialect, and with every third poll (every 30 seconds) in the
`'alkanes'` one, and not at all once the answer is yes. On Subfrost without
a key that is 8 requests a minute for `tx.wait()` and 14 for
`waitForTransaction`, which looks in two places, out of the 20 allowed.

### Reverts and rejections

```ts
import { ContractRevertError, WalletRejectedError } from './generated/web/client';

try {
  const tx = await counter.write.increment({ wallet });
  await tx.wait();
} catch (error) {
  if (error instanceof WalletRejectedError) {
    // The person declined to connect or to sign. Nothing was sent.
  } else if (error instanceof ContractRevertError) {
    console.log(error.reason, error.txid);
  } else {
    throw error;
  }
}
```

Before asking the wallet, a write simulates the call, with the Alkanes it
sends in, against indexed state. If the call would revert, the write throws
the `ContractRevertError` a read throws, and the wallet is never asked. Its
`txid` is `null`.

The simulation does not see the transaction's outputs. For a method that
checks its payment outputs, pass `simulate: false`.

A transaction that reverts once mined still costs its fee. `wait()` then
throws `ContractRevertError` with the `txid` set.

A wallet on a public network asks in its own window. The dev wallet signs at once; to
exercise the approve and reject paths locally, connect it with an approval
step:

```ts
const wallet = await labcoat.connect({ approve: true });
```

`approve: true` shows a dialog in the page before each signature, listing the
contract, the method, what is sent and paid, and the fee. A contract called
through its name is shown as `counter (2:5)`, and one addressed by id as the
id. Rejecting, or closing it, throws `WalletRejectedError`. It is a native
`<dialog>` with no framework and no stylesheet. Pass a function instead to
decide in code: `approve: (summary) => summary.fee < 1_000n`. The summary
has `contract` (the id), `contractName` (set when the call went through a
manifest name), `method`, `fee`, `recipient`, `alkanes`, `payments`, and
`network`; a wallet of your own gets the same object in `signPsbt`.

### Check a browser wallet on signet

[Deploy to signet](/docs/deploy-to-signet/) and generate a signet client
before testing a browser wallet. Use the
[signet verification checklist](/docs/web-wallets/#check-a-browser-wallet-on-signet)
to check the selected account, declined signatures, and confirmation.
The built-in UniSat adapter has been tested by hand on signet. The
LaserEyes and Sats Connect recipes have run on Labcoat Network with
stand-in extension providers.

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
