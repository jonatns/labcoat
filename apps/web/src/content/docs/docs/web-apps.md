---
title: Web apps
description: Generate a typed TypeScript client that reads deployed contracts and sends wallet-signed calls from a browser.
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
  client.ts       fetch-based client: typed reads and wallet-signed writes
  wallet.ts       the wallet interface, the dev wallet, browser wallet adapters
  transaction.ts  builds the unsigned transaction of a call
  errors.ts       the errors the client throws
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
- `wallet` is the wallet `write` calls use when none is passed to them, and
  `feeRate` their fee rate in sat/vB.

`createClient(options)` is the same as `new LabcoatWebClient(options)`.

## Send transactions

`write` has the same methods as `read`. Where a read simulates a call, a write
sends it as a transaction that a wallet signs:

```ts
const wallet = await labcoat.connect();

const tx = await counter.write.increment({ wallet });
const receipt = await tx.wait();

console.log(tx.txid, await counter.read.getCount());
```

The client holds no key. It builds the unsigned transaction, hands it to the
wallet as a PSBT, and broadcasts what the wallet signs. The transaction is the
one `labcoat call` builds for the same wallet and call, byte for byte.

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

`labcoat.connect()` returns a wallet:

- On Labcoat Network it is the **dev wallet**: the built-in wallet that
  `labcoat up` funds, signed for by the network's
  [dev signer](/docs/labcoat-network/#dev-signer). There is no extension and
  no prompt.
- On any other network it is the first installed browser wallet of UniSat,
  Xverse, OKX Wallet, and Leather, which asks the person to connect.

The same line serves both, so an app has no local-only code path. To choose,
pass `{ wallet: 'xverse' }` (or `'dev'`, `'unisat'`, `'okx'`, `'leather'`);
`labcoat.availableWallets()` lists what this page can connect to.

A wallet is any object with three members, so an app can bring its own:

```ts
import type { LabcoatWallet } from './generated/web/client';

const wallet: LabcoatWallet = {
  name: 'My wallet',
  // { address, publicKey, purpose? }: publicKey is hex, x-only or compressed.
  getAccounts: async () => [{ address, publicKey }],
  // psbt is base64, with witnessUtxo on every input and tapInternalKey on
  // taproot ones. Return { psbt } signed (base64 or hex, finalized or not),
  // or { hex }, the finished transaction.
  signPsbt: async ({ psbt, inputs, summary }) => ({ psbt: await sign(psbt) }),
};
```

The client spends taproot and native SegWit (P2WPKH) outputs. Nested SegWit
addresses, which Xverse uses for payments by default, are not spent: with
such a wallet the bitcoin for fees has to be at its taproot address.

### Wait for the result

`tx.wait()` resolves when the Alkanes index has the transaction, with the
call's return value decoded by its ABI:

```ts
const { value, gasUsed } = await tx.wait();
```

`labcoat.waitForTransaction(txid)` does the same from a transaction id alone
and leaves the return value as bytes. Both take `{ timeoutMs, pollMs }`
(60 seconds and 500 ms by default) and throw a `LabcoatClientError` with code
`TRANSACTION_TIMEOUT` when the time is up.

A transaction is indexed once it is in a block. On a public network that is
the next block. Labcoat Network mines no blocks by itself, so the dev wallet
asks for one after each broadcast, as `labcoat call` does; a write is
confirmed by the time it returns, and `wait()` takes one request. To see a
transaction stay pending, connect with `{ mine: false }` and mine with
`labcoat mine`.

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

A browser wallet asks in its own window. The dev wallet signs at once; to
exercise the approve and reject paths locally, connect it with an approval
step:

```ts
const wallet = await labcoat.connect({ approve: true });
```

`approve: true` shows a dialog in the page before each signature, listing the
contract, the method, what is sent and paid, and the fee. Rejecting, or
closing it, throws `WalletRejectedError`. It is a native `<dialog>` with no
framework and no stylesheet. Pass a function instead to decide in code:
`approve: (summary) => summary.fee < 1_000n`.

### Local and public networks

|                    | Labcoat Network                                   | Signet, testnet, mainnet                      |
| ------------------ | ------------------------------------------------- | --------------------------------------------- |
| `connect()`        | the dev wallet                                    | an installed browser wallet                   |
| Approval           | none, or `connect({ approve: true })`             | the wallet's own window                       |
| Blocks             | the dev wallet mines one per write                | the network's                                 |
| `feeRate`          | 2 sat/vB unless set                               | required                                      |
| Page address       | `http://localhost:<port>`: the dev signer refuses other origins | any                             |

The client is generated for one network (`labcoat generate web --network
signet`), and refuses an address or a wallet of another.

The dev wallet exists only on Labcoat Network. Its coins are worthless and its
key signs for any page this machine serves, so never send value to its
addresses on another network.

### Check a browser wallet on signet

Browser wallets are not exercised by Labcoat's own tests beyond stub
providers. Before relying on one, run this once on signet, for each wallet
you support:

1. Deploy to signet, run `labcoat generate web --network signet`, and serve
   the app.
2. Put the wallet on signet and fund its taproot address with signet coins.
3. Call `labcoat.connect({ wallet: '<id>' })`. The wallet asks to connect, and
   the address the app shows is the wallet's taproot address.
4. Decline the connection once: the app catches `WalletRejectedError`.
5. Send a write with `feeRate` set. The wallet shows one transaction with the
   expected fee and outputs. Reject it: `WalletRejectedError`, and nothing is
   broadcast.
6. Send it again and approve. `tx.txid` appears in a signet explorer, and
   `tx.wait()` resolves after the next block.
7. Send a write with `alkanes` from a wallet that holds that Alkane, and check
   with `labcoat balance` that the Alkanes it did not send are still there.
8. Switch the wallet to another network and connect again: the client refuses
   with the wallet's network in the message.

The adapters were written against UniSat's `window.unisat` wallet API
(`requestAccounts`, `getPublicKey`, `getChain`, `signPsbt`), Xverse's Sats
Connect methods on `window.XverseProviders.BitcoinProvider` (`wallet_connect`,
`signPsbt`; WBIP001), OKX's `window.okxwallet.bitcoin`, `bitcoinTestnet`, and
`bitcoinSignet` providers (extension 2.77.1 or later), and Leather's
`window.LeatherProvider` (`getAddresses`, `signPsbt`; `@leather.io/rpc`
2.25), as documented in November 2026.

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
