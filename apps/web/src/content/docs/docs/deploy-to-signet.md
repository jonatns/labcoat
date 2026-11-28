---
title: Deploy to signet
description: Take a Labcoat project from Labcoat Network to Bitcoin signet. Create and fund a wallet, set a Subfrost API key in .env, plan and deploy, then point a web app at the deployment.
---

This page takes a project that works on Labcoat Network and deploys it to
Bitcoin signet through [Subfrost](https://subfrost.io)'s hosted endpoint. The
examples use the Counter contract from `labcoat init`. Every command takes
`--network signet`; to stop typing it, set `network = "signet"` in
`labcoat.toml` or `LABCOAT_NETWORK=signet` in `.env`.

What you need before the first transaction:

| Need | Why | Section |
|---|---|---|
| A signet wallet with a passphrase | Labcoat Network's built-in wallet is refused on every other network | [Create the wallet](#create-the-wallet) |
| Signet coins at the wallet's first taproot address | A deploy pays Bitcoin fees | [Fund it](#fund-the-wallet) |
| A Subfrost API key | Without one the endpoint allows 20 requests a minute, fewer than one deploy makes | [Set the API key](#set-the-api-key) |

## Create the wallet

```bash
labcoat --network signet wallet init
```

The command asks for a passphrase, creates the keystore, and prints the
wallet's address and a 24-word recovery phrase. The phrase is shown once.
Write it down: the keystore is encrypted with the passphrase, and the phrase
is the only other way back to the coins.

The keystore is one file per network in Labcoat's data directory, outside
the project (see [Wallets](/docs/projects-configuration/#wallets)). Run the
command again later and it loads the same wallet instead of making a new one.

### The passphrase

Signet, testnet, and mainnet refuse to create or sign with a keystore
without a passphrase. Labcoat takes it from one of two places:

- A prompt, when the command runs in a terminal. Nothing is stored.
- `LABCOAT_WALLET_PASSPHRASE`, for CI and anywhere without a terminal.
  Without either, the command stops with `WALLET_LOCKED`.

`LABCOAT_WALLET_PASSPHRASE` may also be set in `.env` (below). The keystore
is a file on the same machine, so a passphrase in a file beside the project
protects little; that is acceptable for signet coins and a poor idea on
mainnet, where Labcoat prints a warning each time it reads the passphrase
from `.env`.

### Restore a wallet from a seed phrase

To use a seed phrase you already have, give it to `wallet init` once. It is
never an argument, because arguments are visible to other processes and end
up in shell history. Either let the shell read it without echoing it:

```bash
read -rs LABCOAT_MNEMONIC && export LABCOAT_MNEMONIC
labcoat --network signet wallet init
unset LABCOAT_MNEMONIC
```

or pipe it in. With standard input taken by the phrase there is no terminal
to ask for the passphrase, so it has to be in the environment:

```bash
read -rs LABCOAT_WALLET_PASSPHRASE && export LABCOAT_WALLET_PASSPHRASE
pbpaste | labcoat --network signet wallet init --mnemonic-stdin
unset LABCOAT_WALLET_PASSPHRASE
```

`pbpaste` is the macOS clipboard; any command that prints the phrase on one
line works. After this the keystore holds the seed, encrypted, and the
phrase is not needed again. `LABCOAT_MNEMONIC` is refused in `.env`: a
command run in a project whose `.env` sets it stops with `CONFIG_INVALID`.

## Addresses

```bash
labcoat --network signet wallet addresses --count 5
```

```text
#  Type  Derivation       Address
0  p2tr  m/86'/1'/0'/0/0  tb1p...
1  p2tr  m/86'/1'/0'/0/1  tb1p...
```

The wallet's addresses are taproot, derived at `m/86'/1'/0'/0/N` on signet.
The `1'` is the coin type for every Bitcoin test network; mainnet uses `0'`.

Which address matters:

- **Fund address 0.** That is the address `wallet init` prints.
- Labcoat sends change, and anything a contract returns, to address 0.
- It spends from the first 20 taproot addresses and the first 20 native
  SegWit addresses of the seed, so coins sent to address 3 are still found.
  Coins further out than index 19 are not.

### The same seed in a browser wallet

Importing this seed phrase into a browser wallet does not always show the
same addresses. UniSat derives taproot at `m/86'/0'/0'/0/N` on every
network, with coin type `0'` where Labcoat uses `1'` on signet, and has no
setting for a custom path. The same seed therefore gives UniSat a different
signet address, with a different balance, and neither wallet sees the
other's coins. Nothing is lost, since both sets of keys come from your seed,
but to use both wallets you send coins from one to the other; see
[Move bitcoin](#move-bitcoin).

## Fund the wallet

Send signet coins to address 0 from a signet faucet or another signet wallet,
and wait for the transaction to confirm. A signet block takes about ten
minutes. After [setting the API key](#set-the-api-key), check what arrived:

```bash
labcoat --network signet wallet utxos
```

### How much

A contract deploy is two transactions. The commit creates one output that
pays for the reveal; the reveal spends it and carries the compressed
contract in its witness. Whatever the reveal does not spend on its fee comes
back to address 0.

What the wallet must hold before the commit, at fee rate `r` sat/vB, for a
compressed contract of `S` bytes (`build/<name>.wasm.gz` after
`labcoat build`):

```text
reveal size    ≈ S / 4 + 200 vB
commit output  = reveal size × r × 1.2 + 1,092 sats
commit fee     ≈ 155 × r sats
needed         = commit output + commit fee
```

The 1.2 is a margin on the reveal fee, and the 1,092 sats are two 546-sat
dust outputs. Fund more than this: with about 70,000 sats or more in outputs
that carry no Alkanes, Labcoat puts at least 50,546 sats in the commit
output, so that the reveal never needs a second input.
**100,000 sats is a comfortable amount for one small contract at 2 sat/vB**,
and most of it comes back.

For the Counter template, a 54 KB compressed contract at the default
2 sat/vB:

| | Sats |
|---|---|
| Commit output asked for up front | 34,373 |
| Commit fee paid | 310 |
| Reveal fee paid | 27,977 |
| Spent in total | 28,287 |

A larger contract or a higher `--fee-rate` scales the reveal linearly. A
call is one small transaction; at 2 sat/vB it costs a few hundred sats.

## Set the API key

Subfrost's public endpoints answer 20 requests a minute for each IP address
without a key. Listing the wallet's outputs is one request for each of the
40 addresses Labcoat checks, so `wallet utxos`, `call`, and `deploy` go over
the limit on their own, and Labcoat reports `RPC_REJECTED`. Reads such as
`simulate`, `balance`, and `plan` fit under it.

Create a key at [api.subfrost.io](https://api.subfrost.io) and put it in a
file named `.env` in the project root, beside `labcoat.toml`:

```bash
SUBFROST_API_KEY=your-key
```

A project made by `labcoat init` has a `.env.example` to copy and a
`.gitignore` that already excludes `.env`. In an older project, add `/.env`
to `.gitignore` yourself. Then check:

```bash
labcoat doctor
```

`doctor` reports whether a key is set and whether it came from `.env` or the
environment, never the key itself, and warns when `.env` exists and git does
not ignore it. Labcoat sends the key only to `subfrost.io` hosts, in the
`X-Subfrost-Api-Key` header. How `.env` is read is under
[The .env file](/docs/projects-configuration/#the-env-file).

## Plan

```bash
labcoat --network signet plan
```

`plan` builds the contracts, runs `deploy.rs`, and compares its steps with
`deployments/signet.json` and the chain. It loads no wallet and signs
nothing. On a first run every step is `pending`, and a contract's id reads
`assigned at deploy`, because the protocol picks the id when the reveal
confirms.

## Deploy

```bash
labcoat --network signet deploy
```

The command first prints the network and endpoint it resolved, so a deploy
headed somewhere unexpected is visible before anything is signed. Then, for
each transaction:

1. It prints what it is about to sign: the commit or the reveal, the number
   of inputs, every output with its address and amount, and the fee.
2. It asks whether to sign and broadcast. Anything but `y` stops the deploy
   with `APPROVAL_DECLINED`, and nothing is sent. `--yes` answers for you,
   which is what CI uses; a command with no terminal and no `--yes` stops
   with `APPROVAL_REQUIRED`.

A contract step is two prompts, the commit and then the reveal. After the
reveal is broadcast, the command waits for a block to confirm it and for the
Alkanes index to produce its trace, then records the contract's id in
`labcoat.lock` and `deployments/signet.json`. Expect that wait to take about
ten minutes on signet, sometimes much longer.

If the command is interrupted while it waits, run `labcoat --network signet
plan`. The step shows as `interrupted` with what to do next. When the reveal
was already broadcast, running `deploy` again once it is indexed finishes
the step without sending another transaction.

Once the reveal is confirmed, a second deploy changes nothing and reports
the step as `skipped-complete`:

```bash
labcoat --network signet deploy
labcoat --network signet simulate counter get_count
```

Commit `labcoat.lock` and `deployments/signet.json`. They are what makes the
step run once, for you and for anyone else who deploys this project. See
[Deployment scripts](/docs/deployments/) for editing a deployed step,
`--replace`, and upgrades.

## Call the contract

```bash
labcoat --network signet call counter increment
```

A call is one transaction with the same preview and prompt, and it also
waits for its block. `--dry-run` validates the call and shows what would be
sent without signing.

## Move bitcoin

`labcoat fund` belongs to Labcoat Network. On public networks the CLI has no
plain send command. What it has is `--pay`, which adds a payment output to a
contract call:

```bash
labcoat --network signet call counter increment --pay tb1p...:20000
```

That sends 20,000 sats to the address in the same transaction as the call,
and is how to fund a browser wallet from the CLI's wallet. The amount is in
sats and at least 546; `--pay` repeats for several payments. Use a call you
do not mind making, since the call executes too.

## Use it from a web app

```bash
labcoat --network signet generate web
```

The generated client reads `labcoat.lock`, so it knows the signet contract
ids, and its manifest names Subfrost's signet endpoint. Three things differ
from Labcoat Network:

- **The page cannot call Subfrost directly.** Subfrost's responses carry no
  CORS header, so a browser blocks them. Serve a same-origin path such as
  `/rpc` from your app's server, forward it to the endpoint, and create the
  client with `createClient({ rpc: '/rpc' })`. The route handler is under
  [Reach Subfrost from a browser](/docs/web-apps/#reach-subfrost-from-a-browser).
- **The web app has its own API key setting.** The key cannot be in a
  browser bundle, so it lives in the environment of the server that runs the
  forwarding route: your host's environment variables in production, and the
  web app's own env file while developing. It is the same kind of key, and
  may be the same key, but the project's `.env` configures the `labcoat`
  command only; nothing copies it to the web app. See
  [API keys](/docs/web-apps/#api-keys).
- **Writes need a fee rate.** `feeRate` defaults to 2 sat/vB on Labcoat
  Network and is required elsewhere:
  `counter.write.increment({ wallet, feeRate: 2 })`.

The wallet that signs in the browser is a browser wallet, with its own
signet coins; see [the same seed in a browser wallet](#the-same-seed-in-a-browser-wallet)
and [Check a browser wallet on signet](/docs/web-apps/#check-a-browser-wallet-on-signet).

## When something goes wrong

| What you see | Cause | Do |
|---|---|---|
| `RPC_REJECTED` | Over 20 requests a minute without a key | [Set the API key](#set-the-api-key), or wait a minute |
| `WALLET_LOCKED` | No passphrase and no terminal to ask in | Set `LABCOAT_WALLET_PASSPHRASE` |
| `WALLET_MISSING` | No signet keystore yet | `labcoat --network signet wallet init` |
| `INSUFFICIENT_FUNDS` | Too few confirmed sats in the first 20 addresses | [Fund address 0](#fund-the-wallet) and wait for a block |
| The built-in wallet is refused | `--wallet-file` or `wallet_file` points at Labcoat Network's wallet | Remove it; each network has its own keystore |
| A browser wallet shows another address | A different derivation path for the same seed | [Send coins across](#move-bitcoin) |
| `CONFIG_INVALID` naming `LABCOAT_MNEMONIC` | A seed phrase in `.env` | Delete the line; [restore the wallet once](#restore-a-wallet-from-a-seed-phrase) |

## What was checked

On 27 November 2026 the Counter template was deployed to signet by hand
with Labcoat 0.4.0 plus the changes on `main`. The sat amounts above, the
two prompts and the wait for a block, `skipped-complete` on the second
deploy, funding a UniSat wallet with `--pay`, and UniSat's derivation path
are from that run. Writing this page, the wallet, address, `doctor`, and
`.env` commands were run again, with `plan` and `wallet utxos` against
Subfrost's signet endpoint; nothing was signed or broadcast again. No
automated Labcoat test broadcasts on a public network, and mainnet
deployment is not production-ready: see
[Stability and releases](/docs/reference/stability/).
