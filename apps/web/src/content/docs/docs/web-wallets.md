---
title: Wallet integrations
description: Connect a wallet to the generated Labcoat client, prepare and sign PSBTs separately, and verify your wallet flow on signet.
---

Choose and connect a wallet in your app, then pass it to the
[generated web client](/docs/web-apps/#connect-a-wallet) as a
`LabcoatWallet`. On Labcoat Network, `labcoat.connect()` gives you the
dev wallet. For an installed browser wallet, use a
[named adapter](/docs/web-apps/#built-in-extension-adapters) or wrap the
wallet library your app already uses.

## Wallet interface

A `LabcoatWallet` has a display `name`, a `getAccounts()` method that
returns the selected account's addresses, and a `signPsbt(request)` method.

- `getAccounts()` runs before each write. Return the current addresses.
  If the wallet keeps separate addresses, label them `purpose: 'payment'`
  and `purpose: 'ordinals'`. A signer can omit `publicKey`.
- `signPsbt({ psbt, inputs, summary })` receives a base64 PSBT, the inputs
  to sign and their addresses, and a human-readable transaction summary.
  Return `{ psbt }` with the signed PSBT in base64 or hex, or `{ hex }`
  with the finished transaction. Ask the wallet to sign and let Labcoat
  broadcast. Map a declined prompt to `WalletRejectedError`.

Pass the wallet to a write or set it once in `createClient({ wallet })`.
Signing can take as long as the wallet needs. The confirmation timeout
starts later, when the app calls `tx.wait()`.

## Wallet recipes

Use the recipe for the library your app uses. The examples import from
`./generated/web/`. Adjust those paths to your generated directory.

### LaserEyes

This React hook wraps the app’s connected wallet into a `LabcoatWallet`.
It uses the `useLaserEyes` API from `@omnisat/lasereyes` 0.0.163.

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

### Sats Connect

This adapter uses `sats-connect` 4.2.

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
remembers the choice. The adapter requests the current accounts before each
write.

`response.result.network.bitcoin.name` is the network the wallet is on
(`'Mainnet'`, `'Signet'`, ...); compare it with `labcoat.network` if you want
to say so before the client refuses the address.

### Subfrost remote signing

Subfrost's [remote signing reference](https://docs.subfrost.io/api-reference/guides/walletconnect)
describes a phone pairing flow. Display the returned `pairingUri` and
`pairingCode`, then wait for `accepted` to resolve with the session. The
example wraps that session. It was type-checked against the session interface.

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

The pairing service can wait up to five minutes for the phone. Check which
networks the paired wallet supports before using this flow.

## Prepare and broadcast separately

Use `prepare` and `broadcast` when a separate device or process signs the
transaction. The app keeps the prepared transaction while it waits for the
signed PSBT.

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

`broadcast` verifies the signed transaction against the prepared one. To
let a wallet raise the fee by reducing only the change output, pass
`{ allowFeeChange: true }` to `broadcast` or set `raisesFee: true` on a
`LabcoatWallet`. The client caps the total fee at 100,000 sats. `tx.fee`
reports the actual fee.

## Check a browser wallet on signet

Run these checks with the wallet and account your app uses:

1. [Deploy to signet](/docs/deploy-to-signet/), generate the signet client,
   and [configure the app's RPC route](/docs/web-apps/#reach-subfrost-from-a-browser).
2. Switch the wallet to signet and fund its taproot address with signet coins.
   Connect it from the app. Check that the app displays the expected account.
3. Start a write with an explicit fee rate. Inspect the fee and outputs in
   the wallet, then decline the signature. Confirm that the app handles the
   rejection and produces no transaction ID.
4. Repeat the write and approve it. Check the transaction ID in a signet
   explorer and wait for the Alkanes index to return the call's result.
   The client can skip status events it never observes.
5. Switch accounts and verify the next transaction spends from the new
   account. If the app accepts Alkane inputs, verify the outputs preserve
   the other assets.

The Labcoat repository includes a test page that runs the recipes from this
guide. Run it from the checkout with a built `labcoat` binary:

```bash
./scripts/tests/generate-web-wallet-check.sh
./scripts/tests/generate-web-wallet-check.sh signet <counter-id>
```

The first command starts Labcoat Network. Open the URL it prints with
`?sim=unisat,xverse` to use stand-in extension providers that sign with
the dev wallet. The second command uses a deployed signet counter and
your browser's wallet. If the manifest contains `/rpc`, set
`LABCOAT_RPC_TARGET` to the upstream URL in the server environment.

The built-in UniSat adapter has signed a real signet transaction by hand.
The LaserEyes and Sats Connect recipes ran the actual libraries on Labcoat
Network with stand-in providers. This verifies the library calls and the
local transaction flow. The other wallet paths, including Subfrost remote
signing, still need a signing run on a real wallet.

## Built-in Subfrost adapter

The extension reports the selected taproot address. It uses the same
error for declined prompts and other failures, which the adapter reports
as `WalletRejectedError`. The adapter reports a 60-second request timeout
as `WALLET_ERROR`.

The adapter accepts fee increases that come only from reducing change.
When the extension adds an input or output, the client rejects the signed
transaction. Prepare again with a fee rate the wallet accepts. These fee
paths have been tested with a stand-in signer.
