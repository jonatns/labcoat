---
title: A web app
description: Put a page in front of the starter Counter that shows its count and changes it with a signed transaction, with no wallet extension.
---

This tutorial builds a web page for the Counter contract every Labcoat
project starts with. The page shows the count, and a button changes it by
sending a signed transaction. Everything runs on Labcoat Network, so you need
no wallet extension and no coins.

It assumes you know some Rust and some TypeScript, can read a React component
that uses `useState`, and have at least skimmed
[Your first contract](/docs/tutorials/first-contract/).

You need a [Labcoat installation](/docs/getting-started/installation/) with
its [compilation prerequisites](/docs/getting-started/installation/#compilation-prerequisites),
Node.js 20.9 or newer, and about twenty minutes. The first build compiles
the Alkanes runtime and takes the longest.

## How the pieces fit

```text
browser page ──▶ /rpc on the page's own address ──▶ Labcoat Network
   │                 (Next.js forwards it)             the chain, the Alkanes
   │                                                   index, the dev signer
   └─ imports the client that `labcoat generate web` writes
```

`labcoat generate web` writes a TypeScript client for the contracts you
deployed. It knows their ids and their methods, so the page calls
`counter.read.getCount()` and never handles an id, an opcode, or raw bytes.

## Create the project and deploy

```bash
labcoat init counter-app
cd counter-app
labcoat up
labcoat deploy
```

```text
✓ Executed 1 of 1 step(s)

Network  labcoat
Chain    7b013d15daef39428bff7a9b1cfc98b3428847c75fdf963bebd72c73c43a7e37

Kind      Name     Outcome   Id   Change
contract  counter  executed  2:1

Step     Transaction
counter  79f21ed1e50a012c05765cbf4b512fa7e5316caa41e3409c1435976681d14425
```

`labcoat up` starts Labcoat Network, a private Bitcoin chain on your machine
with a funded wallet. `labcoat deploy` builds the Counter and deploys it. It
is now the contract `counter`, at the Alkane ID `2:1`.

Keep this terminal in `counter-app`. Every command in this tutorial runs from
this directory.

## Create the web app

The page is a [Next.js](https://nextjs.org/) app in a `web` directory inside
the project:

```bash
npx create-next-app@latest web --ts --app --src-dir --empty --no-tailwind --no-eslint --no-react-compiler --import-alias "@/*" --disable-git --use-npm --yes
```

The flags ask for the smallest app `create-next-app` makes: TypeScript, the
App Router, one page, and no styling or lint setup to explain.

Now generate the client into the app's source directory:

```bash
labcoat generate web --out-dir web/src/labcoat
```

```text
✓ Generate web succeeded

Abis       counter
Build dir  /path/to/counter-app/build
Contracts  1
Files      manifest.ts, abi/document.ts, abi/counter.ts, abi/index.ts, client.ts, errors.ts, transaction.ts, wallet.ts, exchange.ts
Network    labcoat
Out dir    /path/to/counter-app/web/src/labcoat
```

`Out dir` is where the files went, as a full path on your machine.

These files are plain TypeScript with no dependencies, so there is no package
to install. `manifest.ts` records what you deployed and where, `abi/counter.ts`
lists the Counter's methods, and `client.ts` is what the page imports.

They need one change to the app. Alkanes amounts are 128-bit numbers, so the
client uses `bigint`, and TypeScript only accepts `bigint` literals when it
targets ES2020 or later. A new Next.js app targets ES2017. In
`web/tsconfig.json`, change

```json
    "target": "ES2017",
```

to

```json
    "target": "ES2020",
```

This setting only affects type checking. Next.js compiles the code itself and
every current browser has `bigint`.

## Reach the network from the page

Labcoat Network listens on `http://127.0.0.1:18443`. Your page will be served
from `http://localhost:3000`. To a browser those are two different sites, and
it refuses to let a page call another site that has not opted in. Labcoat
Network does not opt in.

So the page calls `/rpc` on its own address, and Next.js forwards those
requests to the network. Replace `web/next.config.ts`:

```ts
import type { NextConfig } from 'next';
import { MANIFEST } from './src/labcoat/manifest';

const nextConfig: NextConfig = {
  // The page calls /rpc on its own address; Next.js forwards it to Labcoat Network.
  async rewrites() {
    return [{ source: '/rpc', destination: MANIFEST.rpcUrl }];
  },
};

export default nextConfig;
```

`MANIFEST.rpcUrl` is the network's address as `labcoat generate web` recorded
it, so the port is not written down twice.

## Read the count

Replace `web/src/app/page.tsx`:

```tsx
'use client';

import { useEffect, useState } from 'react';
import { createClient } from '@/labcoat/client';

const labcoat = createClient({ rpc: '/rpc' });
const counter = labcoat.contract('counter');

export default function Home() {
  const [count, setCount] = useState<bigint | null>(null);

  async function refresh() {
    setCount(await counter.read.getCount()); // bigint
  }

  useEffect(() => {
    refresh();
  }, []);

  return (
    <main>
      <h1>Counter</h1>
      <p>Count: {count === null ? '…' : count.toString()}</p>
    </main>
  );
}
```

`'use client'` makes the page a client component: it runs in the browser,
which is where the wallet will be.

Start the dev server in a **second terminal**, also in `counter-app`, and
leave it running:

```bash
npm --prefix web run dev
```

```text
▲ Next.js 16.4.6 (Turbopack)
- Local:         http://localhost:3000
- Network:       http://192.0.0.2:3000
✓ Ready in 151ms
```

Open `http://localhost:3000/`, the `Local` address. If that port was taken,
Next.js prints the one it chose instead. The page says `Count: 0`.

Use the `localhost` address, not the `Network` one: the dev wallet you will
connect later only signs for pages opened on this machine's own address.

`contract('counter')` looked the name up in the manifest and found `2:1`.
`read.getCount()` ran the contract's `get_count` method against the current
chain state, without a transaction, and decoded the result to a `bigint`. It
is what `labcoat simulate counter get_count` does.

To see that the page shows the chain and not a copy, change the count from
the first terminal and reload the page:

```bash
labcoat call counter increment
```

The page says `Count: 1`.

### The methods are typed

`read` has one function per contract method, with the argument and return
types the contract declares. In `web/src/app/page.tsx`, misspell `getCount`
as `getCont`, then build the app:

```bash
npm --prefix web run build
```

The build stops at:

```text
  Running TypeScript ...
src/app/page.tsx(13,33): error TS2551: Property 'getCont' does not exist on type 'ReadMethods<{ readonly contract: "Counter"; readonly methods: readonly [{ readonly name: "initialize"; readonly opcode: 0n; readonly params: readonly []; readonly returns: "void"; }, { readonly name: "increment"; readonly opcode: 1n; readonly params: readonly []; readonly returns: "u128"; }, { ...; }]; }>'. Did you mean 'getCount'?
Failed to type check.
```

The type TypeScript prints is the Counter's ABI, and its last sentence is the
useful part. Your editor shows the same error as you type. The dev server
does not check types, which is why this took a build. Put `getCount` back.

## Send a transaction

A read costs nothing and changes nothing. Changing the count takes a
transaction, and a transaction needs a wallet to pay for it and sign it.

Replace `web/src/app/page.tsx`:

```tsx
'use client';

import { useEffect, useState } from 'react';
import { createClient } from '@/labcoat/client';

const labcoat = createClient({ rpc: '/rpc' });
const counter = labcoat.contract('counter');

export default function Home() {
  const [count, setCount] = useState<bigint | null>(null);
  const [status, setStatus] = useState('');

  async function refresh() {
    setCount(await counter.read.getCount()); // bigint
  }

  useEffect(() => {
    refresh();
  }, []);

  async function increment() {
    setStatus('Sending…');

    const wallet = await labcoat.connect();
    const tx = await counter.write.increment({ wallet });
    const { value } = await tx.wait();

    setStatus(`Transaction ${tx.txid} set the count to ${value}.`);
    await refresh();
  }

  return (
    <main>
      <h1>Counter</h1>
      <p>Count: {count === null ? '…' : count.toString()}</p>
      <button onClick={increment}>+1</button>
      <p>{status}</p>
    </main>
  );
}
```

The page updates by itself. Click **+1**:

```text
Count: 2

Transaction 5edc226e334d115b0c63fd1bc1e656da5371e7ae4e9338bd8627be7d84c23606 set the count to 2.
```

Three lines did that:

- `labcoat.connect()` returned the **dev wallet**: the funded wallet
  `labcoat up` created. Labcoat Network signs for it, so there is no
  extension to install and no key in your app.
- `counter.write.increment({ wallet })` built the transaction in the browser,
  had the wallet sign it, and broadcast it. `write` has the same methods as
  `read`. It is what `labcoat call counter increment` does, and the
  transaction is the same.
- `tx.wait()` resolved once the transaction was in a block and indexed, with
  the method's return value. Labcoat Network mines no blocks by itself, so
  the dev wallet asks for one after each transaction it sends.

The transaction is an ordinary one. Trace it from the first terminal with the
ID from your own page:

```bash
labcoat trace <txid>
```

```text
› Transaction trace

Transaction  5edc226e334d115b0c63fd1bc1e656da5371e7ae4e9338bd8627be7d84c23606

• Protostone 1
  • call 0:0 -> 2:1  inputs [1, …]  fuel 3500000
  • return 2  fuel used 90140
```

## Change the contract

A page also has to cope with a call the contract refuses, and the Counter
has no method that can fail in normal use. Give it one: a `decrement` that
refuses to go below zero.

In `contracts/counter/src/lib.rs`, add a message to `CounterMessage`:

```rust
    #[opcode(2)]
    #[returns(u128)]
    Decrement,
```

and its handler to `impl Counter`:

```rust
    fn decrement(&self) -> Result<CallResponse> {
        let mut pointer = self.count_pointer();
        let count = pointer
            .get_value::<u128>()
            .checked_sub(1)
            .ok_or_else(|| anyhow!("the count is already zero"))?;
        pointer.set_value::<u128>(count);
        self.response_with_count(count)
    }
```

Deploy the change:

```bash
labcoat deploy
```

```text
✓ Executed 1 of 1 step(s)

Network  labcoat
Chain    7b013d15daef39428bff7a9b1cfc98b3428847c75fdf963bebd72c73c43a7e37

Kind      Name     Outcome     Id   Change
contract  counter  redeployed  2:2  2:1 → 2:2

Step     Transaction
counter  9c5bb837d97ce7da2e94f8cbcd2f783e143307f399203748de8b72af241ba5ba

A redeployed contract has a new id and empty storage; the earlier one stays on chain.
```

Reload the page. It still says `Count: 2`: the client was generated when
`counter` was `2:1`, so that is the contract it reads. The generated files
are a snapshot of your deployments, and nothing updates them but you.

Use the new method before regenerating, to see what the snapshot does for
you. Replace `web/src/app/page.tsx`:

```tsx
'use client';

import { useEffect, useState } from 'react';
import {
  ContractRevertError,
  WalletRejectedError,
  createClient,
  type LabcoatWallet,
  type WriteResult,
} from '@/labcoat/client';

const labcoat = createClient({ rpc: '/rpc' });
const counter = labcoat.contract('counter');

export default function Home() {
  const [count, setCount] = useState<bigint | null>(null);
  const [status, setStatus] = useState('');

  async function refresh() {
    setCount(await counter.read.getCount()); // bigint
  }

  useEffect(() => {
    refresh();
  }, []);

  async function send(write: (wallet: LabcoatWallet) => Promise<WriteResult<bigint>>) {
    setStatus('Sending…');
    try {
      const wallet = await labcoat.connect({ approve: true });
      const tx = await write(wallet);
      const { value } = await tx.wait();
      setStatus(`Transaction ${tx.txid} set the count to ${value}.`);
    } catch (error) {
      if (error instanceof WalletRejectedError) {
        setStatus('Rejected in the wallet. Nothing was sent.');
      } else if (error instanceof ContractRevertError) {
        setStatus(`The contract reverted: ${error.reason}`);
      } else {
        throw error;
      }
    }
    await refresh();
  }

  return (
    <main>
      <h1>Counter</h1>
      <p>Count: {count === null ? '…' : count.toString()}</p>
      <button onClick={() => send((wallet) => counter.write.decrement({ wallet }))}>−1</button>
      <button onClick={() => send((wallet) => counter.write.increment({ wallet }))}>+1</button>
      <p>{status}</p>
    </main>
  );
}
```

```bash
npm --prefix web run build
```

The build stops at:

```text
  Running TypeScript ...
src/app/page.tsx(50,61): error TS2339: Property 'decrement' does not exist on type 'WriteMethods<{ readonly contract: "Counter"; readonly methods: readonly [{ readonly name: "initialize"; readonly opcode: 0n; readonly params: readonly []; readonly returns: "void"; }, { readonly name: "increment"; readonly opcode: 1n; readonly params: readonly []; readonly returns: "u128"; }, { ...; }]; }>'.
Failed to type check.
```

The client does not know `decrement` yet. Regenerate it and build again:

```bash
labcoat generate web --out-dir web/src/labcoat
npm --prefix web run build
```

The build now ends with:

```text
Route (app)
┌ ○ /
└ ○ /_not-found


○  (Static)  prerendered as static content
```

Reload the page. It says `Count: 0`: it now reads `2:2`, whose storage
starts empty, and `counter.write.decrement` exists and is typed.

That is the whole loop for a contract change: edit, `labcoat deploy`,
`labcoat generate web`. If the page and the contract disagree about a method,
the build says so.

## Approve, reject, and revert

The new page does three things the first one did not.

**It asks before signing.** `connect({ approve: true })` makes the dev wallet
show a dialog before each signature, as a browser wallet would. Click **+1**:

```text
Approve transaction

Labcoat dev wallet · regtest

Contract    counter (2:2)
Method      increment
Results to  bcrt1psjun8sn0t5clxdlnrdyu0usnnk5hsc76mnnv5638p7mfle26e3pqhgkg2u
Fee         600 sats

Reject   Approve
```

Your fee may differ. Click **Approve**, and the count becomes
`1`.

**It handles a rejection.** Click **+1** again, and this time click
**Reject**, or press Escape:

```text
Count: 1

Rejected in the wallet. Nothing was sent.
```

`write.increment` threw `WalletRejectedError`. A browser wallet's own Reject
button throws the same error, so this code path is the one your users will
take.

**It handles a revert.** Click **−1** and approve it. The count is `0`. Click
**−1** once more:

```text
Count: 0

The contract reverted: Error: the count is already zero
```

No dialog appeared. Before it asks the wallet, a write runs the call against
the current chain state. If the contract would refuse it, the write throws
`ContractRevertError` with the contract's own message as `reason`, and nothing
is signed or paid for.

That check cannot see everything: the state can change between the check and
the block. A transaction that reverts once mined still costs its fee, and
then `tx.wait()` throws the same `ContractRevertError`, with its `txid` set.
The `catch` above covers both.

## Beyond Labcoat Network

The page has no code that only works locally, but three things change on a
public network such as signet.

**The client is generated for one network.** Deploy there, then run
`labcoat generate web --network signet`. The client refuses a wallet or an
address of another network, and `/rpc` has to lead to that network's
endpoint: the rewrite follows `MANIFEST.rpcUrl`, which regenerating updates.

**`connect()` returns a browser wallet.** There is no dev wallet off Labcoat
Network. `connect()` returns the first installed wallet of UniSat, Xverse,
OKX Wallet, and Leather, and the wallet asks the person to connect. Pass
`{ wallet: 'xverse' }` to choose one; `labcoat.availableWallets()` lists what
the page can connect to. The wallet shows its own approval window, so
`approve: true` no longer applies.

**Transactions cost money and take time.** A write needs a `feeRate`; only
Labcoat Network has a default. Nobody mines a block for you, so `tx.wait()`
resolves after the network's next block. It gives up after 60 seconds unless
you pass `timeoutMs`, and a block can take longer than that.

Know the limits before you rely on it:

- The client spends taproot and native SegWit outputs. It does not spend
  nested SegWit ones, which Xverse uses for payments by default: with such a
  wallet, the bitcoin for fees has to be at its taproot address.
- Labcoat's own tests exercise the browser wallets through stand-ins, not the
  real extensions. Before you support a wallet, run the
  [signet checklist](/docs/web-apps/#check-a-browser-wallet-on-signet) with
  it once.
- The dev wallet's key signs for any page your machine serves. Never send
  value to its addresses on another network.

[Web apps](/docs/web-apps/) is the reference for everything the client does.

## Clean up

Stop the dev server with Ctrl-C in its terminal, then stop the network:

```bash
labcoat down
```

`labcoat down` keeps the chain for next time; `labcoat reset -y` wipes it.
After a reset, deploy and generate again: the client still describes the
chain that is gone.

## Next steps

- [Web apps](/docs/web-apps/) covers the rest of the client: arguments and
  return types, sending Alkanes and bitcoin with a call, contracts a factory
  created, and reading at one height.
- [Labcoat Network and wallets](/docs/labcoat-network/#dev-signer) explains
  the dev signer behind the dev wallet.
- [Tokens and exchanges](/docs/tutorials/token-exchange/) writes a token, the
  kind of contract a page like this one usually fronts.
