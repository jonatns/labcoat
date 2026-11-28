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

Both defaults are in the project root, from any directory inside the project.
A path you pass is relative to the directory you run the command in, so in an
app that lives in `web/`:

```bash
cd web
labcoat generate web --out-dir src/generated   # writes web/src/generated
```

The command prints the directories it used as absolute paths (`Out dir` and
`Build dir`; `outDir` and `buildDir` with `--json`).

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
to that endpoint. For another provider of the standard Alkanes JSON-RPC, or
any time `rpc` leads somewhere other than the manifest's endpoint, name it:

```ts
const labcoat = createClient({ rpc: 'https://alkanes.example/jsonrpc', dialect: 'alkanes' });
```

With the wrong dialect the first request fails with the endpoint's own
refusal. Subfrost's reads `method not permitted on the public endpoint:
secondaryheight`.

### Reach Subfrost from a browser

A page cannot call Subfrost's endpoints directly. They answer a CORS
preflight, but the response to the request itself carries no
`Access-Control-Allow-Origin` header, so the browser blocks it and the client
throws `RPC_UNREACHABLE`. Serve a same-origin path that forwards to the
manifest's `rpcUrl`, and create the client with that path. In a Next.js app:

```ts
// app/rpc/route.ts
import { MANIFEST } from '@/generated/web/manifest';

export async function POST(request: Request) {
  const headers = new Headers({ 'content-type': 'application/json' });
  const key = process.env.SUBFROST_API_KEY;
  if (key) headers.set('x-subfrost-api-key', key);

  const upstream = await fetch(MANIFEST.rpcUrl, {
    method: 'POST',
    headers,
    body: await request.text(),
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

A dev server's proxy does the same while developing: `server.proxy['/rpc']`
in a Vite config, or a rewrite in `next.config.ts`. The dialect comes from the
manifest, so the client needs nothing else.

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
`metashrew_view`, `esplora_address::utxo`, `esplora_tx::hex`, `gettxout`, and
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

A wallet, to the client, is any object with three members:

```ts
import type { LabcoatWallet } from './generated/web/client';

const wallet: LabcoatWallet = {
  name: 'My wallet',
  // { address, publicKey?, purpose? }: publicKey is hex, x-only or compressed.
  getAccounts: async () => [{ address, publicKey }],
  // psbt is base64, with witnessUtxo on every input and tapInternalKey on
  // taproot ones. Return { psbt } signed (base64 or hex, finalized or not),
  // or { hex }, the finished transaction.
  signPsbt: async ({ psbt, inputs, summary }) => ({ psbt: await sign(psbt) }),
};
```

Where that object comes from depends on the network:

- On **Labcoat Network**, Labcoat provides it: `labcoat.connect()` returns the
  dev wallet.
- On **signet, testnet, and mainnet**, the app provides it. People there
  already have a wallet, and the app already has a way to connect to it: its
  own picker, LaserEyes, Sats Connect, a QR code, a hardware device. Labcoat
  ships no picker and never chooses a wallet for the person; it takes
  whatever the app connected, wrapped in those three members.

#### Labcoat Network: the dev wallet

```ts
const wallet = await labcoat.connect();
```

On Labcoat Network this is the **dev wallet**: the built-in wallet that
`labcoat up` funds, signed for by the network's
[dev signer](/docs/labcoat-network/#dev-signer). There is no extension and no
prompt, and it mines a block after each broadcast.

On any other network `connect()` with no wallet named throws a
`LabcoatClientError` with code `WALLET_UNAVAILABLE`, whatever extensions are
installed. An app that runs on both picks by network:

```ts
const wallet = labcoat.network === 'regtest' ? await labcoat.connect() : myWallet;
```

#### Public networks: bring your own wallet

Build the `LabcoatWallet` from the account and signing calls of whatever the
app uses, and pass it to each write, or once as `createClient({ wallet })`.

- `getAccounts` is called before every write, so return the account selected
  now, not the one from page load. Mark the addresses of a wallet that keeps
  two: `purpose: 'payment'` holds bitcoin, `purpose: 'ordinals'` holds
  Alkanes. Leave `publicKey` out for a signer that reports addresses only.
- `signPsbt` gets the PSBT as base64, the `inputs` to sign with the address
  that owns each, and a `summary` of what the transaction does. It may take
  as long as signing takes; the client sets no time limit. Ask the wallet
  not to broadcast: the client does that. Throw `WalletRejectedError` when
  the person declines.

The client spends taproot and native SegWit (P2WPKH) outputs. Nested SegWit
addresses, which Xverse uses for payments by default, are not spent: with
such a wallet the bitcoin for fees has to be at its taproot address.

The three recipes below compile against the generated client and the
libraries' own types. The LaserEyes and Sats Connect ones have also sent
writes on Labcoat Network through the libraries' real code, with stand-ins
for the extensions that sign with the dev signer. None has signed in a real
wallet yet; run the [signet check](#check-a-wallet-on-signet) with yours
before relying on one.

**LaserEyes** (`@omnisat/lasereyes` 0.0.163, the `useLaserEyes` hook). The app
connects with LaserEyes as it already does; this hook turns the connected
wallet into one the client takes:

```ts
import { useMemo } from 'react';
import { useLaserEyes } from '@omnisat/lasereyes';
import { LabcoatClientError, WalletRejectedError } from './generated/web/client';
import type { LabcoatWallet, WalletAccount } from './generated/web/client';

/** The wallet connected through LaserEyes; null until one is. */
export function useLabcoatWallet(): LabcoatWallet | null {
  const { connected, provider, address, publicKey, paymentAddress, paymentPublicKey, signPsbt } =
    useLaserEyes();
  return useMemo(() => {
    // `connected` can turn true a moment before the address is there.
    if (!connected || address === '') return null;
    return {
      name: provider ?? 'LaserEyes',
      async getAccounts() {
        // One address does both jobs in UniSat and OKX; Xverse and Leather keep two.
        if (paymentAddress === '' || paymentAddress === address) return [{ address, publicKey }];
        const accounts: WalletAccount[] = [
          { address, publicKey, purpose: 'ordinals' },
          { address: paymentAddress, publicKey: paymentPublicKey, purpose: 'payment' },
        ];
        return accounts;
      },
      async signPsbt({ psbt, inputs }) {
        const signed = await signPsbt({
          tx: psbt,
          finalize: true,
          broadcast: false,
          inputsToSign: inputs,
        }).catch((error: unknown) => {
          // LaserEyes rethrows whatever the wallet threw, which need not be an Error.
          const { code, message } = (error ?? {}) as { code?: unknown; message?: unknown };
          if (code === 4001 || /reject|cancel|denied/i.test(String(message))) {
            throw new WalletRejectedError(provider ?? 'LaserEyes', String(message));
          }
          throw error;
        });
        if (signed?.signedPsbtHex === undefined) {
          throw new LabcoatClientError('WALLET_ERROR', 'the wallet returned no signed transaction');
        }
        return { psbt: signed.signedPsbtHex };
      },
    };
  }, [connected, provider, address, publicKey, paymentAddress, paymentPublicKey, signPsbt]);
}
```

The hook returns a new wallet when the account changes, so a write always
uses the current one. Mount `LaserEyesProvider` outside React's `StrictMode`
in development: the second mount disposes the client the first one made, and
`connect` then does nothing. LaserEyes has no regtest for Xverse (it asks the
wallet to switch to mainnet), so on Labcoat Network use the dev wallet. The
0.1 pre-release (`@omnisat/lasereyes-core@next`,
which [lasereyes.build](https://www.lasereyes.build/docs) documents) replaces
this hook with `useAccount` and `useSignPsbt`; the wallet is built the same
way from those.

**Sats Connect** (`sats-connect` 4.2, Xverse):

```ts
import Wallet, { AddressPurpose, RpcErrorCode } from 'sats-connect';
import { LabcoatClientError, WalletRejectedError } from './generated/web/client';
import type { LabcoatWallet, WalletAccount } from './generated/web/client';

const name = 'Sats Connect';

function failure(error: { code: number; message: string }): LabcoatClientError {
  return error.code === RpcErrorCode.USER_REJECTION
    ? new WalletRejectedError(name, error.message)
    : new LabcoatClientError('WALLET_ERROR', `${name}: ${error.message}`);
}

export const satsConnectWallet: LabcoatWallet = {
  name,
  async getAccounts() {
    const response = await Wallet.request('wallet_connect', {
      addresses: [AddressPurpose.Ordinals, AddressPurpose.Payment],
    });
    if (response.status === 'error') throw failure(response.error);
    const accounts: WalletAccount[] = [];
    for (const { address, publicKey, purpose } of response.result.addresses) {
      if (purpose === AddressPurpose.Ordinals) accounts.push({ address, publicKey, purpose: 'ordinals' });
      if (purpose === AddressPurpose.Payment) accounts.push({ address, publicKey, purpose: 'payment' });
    }
    return accounts;
  },
  async signPsbt({ psbt, inputs }) {
    const signInputs: Record<string, number[]> = {};
    for (const { index, address } of inputs) (signInputs[address] ??= []).push(index);
    const response = await Wallet.request('signPsbt', { psbt, signInputs, broadcast: false });
    if (response.status === 'error') throw failure(response.error);
    return { psbt: response.result.psbt };
  },
};
```

The first `Wallet.request` opens Sats Connect's own wallet selector, which
lists the wallets installed in the page that speak its protocol, and
remembers the choice; that selector is the library's, not Labcoat's.
`wallet_connect` is sent before every write, as `getAccounts` is.

`response.result.network.bitcoin.name` is the network the wallet is on
(`'Mainnet'`, `'Signet'`, ...); compare it with `labcoat.network` if you want
to say so before the client refuses the address.

**Subfrost remote signing.** Subfrost pairs a page with its wallet through a
QR code and a pairing code, over its own bridge
([reference](https://docs.subfrost.io/api-reference/guides/walletconnect); it
is not WalletConnect v2). Its reference client, `@subfrost/wc`, was not on
npm in November 2026, so this recipe is written against the session that
page describes: `connect()` returns `pairingUri` and `pairingCode` for the
app to show, and `accepted` resolves with the session once the wallet joins.

```ts
import { WalletRejectedError } from './generated/web/client';
import type { LabcoatWallet } from './generated/web/client';
import { base64ToBytes, bytesToHex } from './generated/web/transaction';

/** What a paired session answers, per Subfrost's remote signing reference. */
interface SubfrostSession {
  getAccounts(): Promise<string[]>;
  signPsbt(psbtHex: string, addresses?: string[]): Promise<string>;
}

export function subfrostRemoteWallet(session: SubfrostSession): LabcoatWallet {
  const name = 'Subfrost';
  return {
    name,
    // The paired wallet reports addresses and no public keys.
    getAccounts: async () => (await session.getAccounts()).map((address) => ({ address })),
    async signPsbt({ psbt, inputs }) {
      const addresses = [...new Set(inputs.map((input) => input.address))];
      try {
        return { psbt: await session.signPsbt(bytesToHex(base64ToBytes(psbt)), addresses) };
      } catch (error) {
        if (error instanceof Error && error.message.startsWith('wc:user_rejected')) {
          throw new WalletRejectedError(name);
        }
        throw error;
      }
    },
  };
}
```

A paired request waits up to five minutes for the phone. Subfrost's
reference does not say which networks a paired wallet signs on; its
extension is mainnet only.

#### Signing that does not fit `signPsbt`

A write is two steps with a signature between them, and an app can take the
steps itself: when the signer is another device, another process, or
anything that is not a function call away.

```ts
const prepared = await counter.prepare.increment({ accounts: [{ address, publicKey }], feeRate: 2 });

// prepared.psbt is the unsigned PSBT (base64); prepared.inputs and
// prepared.summary say what to sign and what it does.
const signed = await showQrAndWaitForSignature(prepared.psbt);

const tx = await labcoat.broadcast(prepared, { psbt: signed });
await tx.wait();
```

`prepare` has the methods and options `write` has, and takes `accounts` in
place of a `wallet`. It simulates the call, chooses the outputs to spend, and
returns without asking anyone to sign. `broadcast` takes the PSBT with its
inputs signed (`{ psbt }`, base64 or hex, finalized or not) or the finished
transaction (`{ hex }`), and resolves with what a write resolves with.
`write` is these two calls with `wallet.signPsbt` between them.

The outputs a prepared transaction spends are not reserved. If one is spent
before the signature comes back, the broadcast fails with the node's error;
prepare again.

`broadcast` refuses a signed transaction that is not the one prepared, so a
signer cannot redirect a call. The one exception is a wallet that raises the
fee by lowering the change output, as the Subfrost extension can: pass
`{ allowFeeChange: true }` to `broadcast`, or set `raisesFee: true` on a
`LabcoatWallet`, and that single difference is accepted, up to the client's
fee ceiling of 100,000 sats. `tx.fee` is then the fee actually paid.

#### Built-in extension adapters

For a prototype, or a page with one known wallet, the client can talk to five
browser extensions itself:

```ts
const wallet = await labcoat.connect({ wallet: 'unisat' }); // or 'xverse', 'okx', 'leather', 'subfrost'
```

`labcoat.availableWallets()` lists the ones installed in the page, for a
picker the app draws. These adapters are a convenience, not the supported
path on a public network: only UniSat has signed a real transaction through
them (see [Check a wallet on signet](#check-a-wallet-on-signet)), and a
wallet library tracks extension changes that this generated file does not.

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

A transaction is indexed once it is in a block. On a public network that is
the next block, about ten minutes away on signet and mainnet. Labcoat Network
mines no blocks by itself, so the dev wallet
asks for one after each broadcast, as `labcoat call` does; a write is
confirmed by the time it returns, and `wait()` takes one request. To see a
transaction stay pending, connect with `{ mine: false }` and mine with
`labcoat mine`.

On a public network the wait is minutes long. `onStatus` tells the page where
the transaction is:

```ts
await tx.wait({
  onStatus: (status) => {
    if (status.status === 'broadcast') show('Sent. Waiting for the network…');
    if (status.status === 'pending') show('In the mempool. Waiting for a block…');
    if (status.status === 'confirmed') show(`In block ${status.blockHeight}. Waiting for the index…`);
    if (status.status === 'indexed') show('Done.');
  },
});
```

| Status      | Meaning                                                              |
| ----------- | -------------------------------------------------------------------- |
| `broadcast` | The wait has started                                                 |
| `pending`   | The endpoint has the transaction, and it is not in a block           |
| `confirmed` | It is in block `blockHeight`; the Alkanes index has not reached it   |
| `indexed`   | The index has the call's result; `wait()` resolves or throws next    |

Each is reported once and in order. One the client never saw is skipped: a
transaction mined between two looks goes from `broadcast` to `confirmed`, and
on Labcoat Network a write is usually `broadcast` and then `indexed`.
`indexed` is reported for a reverted call too, just before `wait()` throws.

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

### Local and public networks

|                    | Labcoat Network                                   | Signet, testnet, mainnet                      |
| ------------------ | ------------------------------------------------- | --------------------------------------------- |
| Wallet             | the dev wallet, from `connect()`                  | the app's own `LabcoatWallet`; `connect()` throws |
| Approval           | none, or `connect({ approve: true })`             | the wallet's own window                       |
| Blocks             | the dev wallet mines one per write                | the network's                                 |
| `feeRate`          | 2 sat/vB unless set                               | required                                      |
| Page address       | `http://localhost:<port>`: the dev signer refuses other origins | any                             |
| Endpoint           | `labcoat up`'s, in the `'labcoat'` dialect        | Subfrost's by default, in the `'alkanes'` dialect, through a same-origin path |
| Requests           | batched, unlimited                                | one per call, 20 a minute without an API key  |
| `wait()`           | 60 seconds, asking every 500 ms                   | 30 minutes, asking every 10 seconds           |
| `onStatus`         | usually `broadcast`, then `indexed`               | all four, minutes apart                       |

The client is generated for one network (`labcoat generate web --network
signet`), and refuses an address or a wallet of another.

The dev wallet exists only on Labcoat Network. Its coins are worthless and its
key signs for any page this machine serves, so never send value to its
addresses on another network.

### Check a wallet on signet

No Labcoat test broadcasts on a public network, and none drives a real
wallet. What has run, as of November 2026:

| Path                              | Real library code | Real wallet                          |
| --------------------------------- | ----------------- | ------------------------------------ |
| Built-in UniSat adapter           | none involved     | UniSat 1.8.5 on signet, by hand      |
| LaserEyes recipe, UniSat          | yes, on Labcoat Network with a stand-in extension | no |
| LaserEyes recipe, other wallets   | no                | no                                   |
| Sats Connect recipe               | yes, on Labcoat Network with a stand-in extension | no |
| Subfrost remote signing recipe    | no: the client is not published | no                     |
| Other built-in adapters           | none involved     | no; Subfrost's connected, and has no signet network |
| `onStatus` on Subfrost's endpoint | n/a               | the status lookups, against signet; not a pending transaction |

A stand-in answers the API an extension documents and signs with the dev
signer. It shows that the library and the recipe agree; it cannot show what
an extension does differently from its documentation.

`scripts/tests/generate-web-wallet-check.sh` in the Labcoat repository serves
a page with every one of these paths behind its own buttons, using the
recipes copied from this page. Run with no arguments it uses Labcoat Network
and the stand-ins (open it with `?sim=unisat,xverse`); run as
`generate-web-wallet-check.sh signet <counter-id>` it uses Subfrost's signet
endpoint and the extensions in your browser.

Before relying on a wallet, run this once on signet with the `LabcoatWallet`
your app passes:

1. Deploy to signet, run `labcoat generate web --network signet`, and serve
   the app. The manifest's `rpcDialect` is `'alkanes'`. Forward a
   same-origin path to the endpoint and pass it as `rpc`, as under
   [Reach Subfrost from a browser](#reach-subfrost-from-a-browser).
2. Put the wallet on signet and fund its taproot address with signet coins.
3. Connect the way your app does: its picker or wallet library, or
   `labcoat.connect({ wallet: '<id>' })` for a built-in adapter. The address
   the app shows is the wallet's taproot address.
4. Decline the connection once and check the app handles it.
5. Send a write with `feeRate` set. The wallet shows one transaction with the
   expected fee and outputs. Reject it: the write throws
   `WalletRejectedError` if your wallet object maps the refusal, the wallet's
   own error otherwise, and nothing is broadcast.
6. Send it again and approve. `tx.txid` appears in a signet explorer,
   `onStatus` reports `pending` and then `confirmed`, and `tx.wait()`
   resolves after the next block, which can take ten minutes or more.
7. Send a write with `alkanes` from a wallet that holds that Alkane, and check
   with `labcoat balance` that the Alkanes it did not send are still there.
8. Switch the wallet to another network, or to another account, and send
   again: the client refuses an address of another network, and a write from
   the new account spends that account's outputs.

The built-in adapters were written against UniSat's `window.unisat` wallet API
(`requestAccounts`, `getPublicKey`, `getChain`, `signPsbt`), Xverse's Sats
Connect methods on `window.XverseProviders.BitcoinProvider` (`wallet_connect`,
`signPsbt`; WBIP001), OKX's `window.okxwallet.bitcoin`, `bitcoinTestnet`, and
`bitcoinSignet` providers (extension 2.77.1 or later), Leather's
`window.LeatherProvider` (`getAddresses`, `signPsbt`; `@leather.io/rpc`
2.25), and Subfrost's `window.subfrost` (`requestAccounts`,
`getPublicKey`, `signPsbt`), as documented in November 2026.

The built-in Subfrost adapter differs from the others in three ways:

- It reports the one address selected in the wallet, and that has to be its
  taproot address.
- A declined prompt and any other failure arrive the same way, so both throw
  `WalletRejectedError`. Only a prompt left unanswered for 60 seconds is told
  apart, as `WALLET_ERROR`.
- It may raise a fee it finds too low by changing the transaction. When it
  lowers the change output to do so, the write goes through and `tx.fee` is
  the fee paid. When it adds an input and an output of its own, the client
  refuses to broadcast a transaction it did not build and the write fails
  with `WALLET_ERROR`; pass a `feeRate` at least as high as the wallet's own.
  Neither path has run against the extension.

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
