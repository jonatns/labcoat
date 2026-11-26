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
mode, the `node` service, chain height, mempool size, overall readiness, and
whether the [dev signer](#dev-signer) can sign.

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
- signing: `labcoat_dev_addresses` and `labcoat_dev_sign_psbt` sign with the
  built-in wallet; see [Dev signer](#dev-signer);
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

## Dev signer

While Labcoat Network is up, its JSON-RPC endpoint signs transactions for the
built-in wallet. A web page or a headless test builds an unsigned PSBT, posts
it, and gets it back signed, with no wallet extension and no key in the app.

The client `labcoat generate web` writes uses it as its dev wallet; see
[Send transactions](/docs/web-apps/#send-transactions). The rest of this
section is the wire contract, for other clients.

`labcoat up --ci` and `labcoat status --json` say whether it can sign and
where, and `labcoat doctor` has a `dev signer` check:

```json
"dev_signer": {
  "available": true,
  "rpc": "http://127.0.0.1:18443",
  "methods": ["labcoat_dev_addresses", "labcoat_dev_sign_psbt"],
  "address": "bcrt1p..."
}
```

`dev_signer` is at `result.status.dev_signer` for `up` and
`result.dev_signer` for `status`. When `available` is `false`, `reason` says
why, and `address` is absent. `labcoat_info` also answers
`"devSigner": true` on a network that serves the methods.

### `labcoat_dev_addresses`

Parameters: `[]`. Result:

```json
{
  "network": "regtest",
  "fingerprint": "a6de2bdb",
  "addresses": [
    {
      "address": "bcrt1p...",
      "type": "p2tr",
      "derivationPath": "m/86'/1'/0'/0/0",
      "internalKey": "<32-byte x-only key, hex>",
      "outputKey": "<32-byte x-only key, hex>",
      "scriptPubKey": "5120<outputKey>"
    }
  ]
}
```

`addresses[0]` is the wallet's primary address, the one `labcoat up` funds
and the CLI spends from. `internalKey` is the BIP-86 internal key, a PSBT
input's `tapInternalKey`; `outputKey` is that key tweaked, which
`scriptPubKey` pays to. `fingerprint` is the BIP-32 master fingerprint, or
`null`.

### `labcoat_dev_sign_psbt`

Parameters: `[psbt, options]`.

- `psbt`: a BIP-174 (version 0) PSBT, base64. Every input needs its
  `witnessUtxo`, including inputs the wallet does not own: a taproot
  signature commits to the output each input spends.
- `options`: optional, `{ "finalize": boolean }`. `finalize` defaults to
  `true`.

Result:

```json
{
  "psbt": "<base64>",
  "signedInputs": [0],
  "complete": true,
  "hex": "<raw transaction, hex>",
  "txid": "<transaction id>"
}
```

- `psbt` is the same PSBT with the wallet's inputs signed. With `finalize`
  they carry `finalScriptWitness`; without it, `tapKeySig`.
- `signedInputs` lists the input indexes this call signed. It is empty, and
  `psbt` is unchanged, when none of the inputs are the wallet's.
- `complete` is `true` when every input is final. `hex` and `txid` are
  present only then, so with `finalize: false` they never are.

The wallet signs taproot key-path inputs (BIP-86, no script tree) of its
first 1,000 receive and change addresses, with `SIGHASH_DEFAULT`. Everything
else is left exactly as it was sent: inputs of other wallets, inputs already
signed or finalized, script-path spends, and P2WPKH, P2SH, and P2PKH inputs.
An input of the wallet's that asks for another sighash type is an error
rather than a signature the PSBT did not ask for.

### Errors

Errors are JSON-RPC error objects with HTTP status 200, like the rest of the
endpoint, and use Bitcoin Core's codes where it has one:

| Code | Meaning |
| --- | --- |
| `-32601` | Method not found: the endpoint is not a Labcoat Network with a dev signer, such as an earlier release or another network's RPC |
| `-32000` | The request did not come from this machine; see [who can call it](#who-can-call-it) |
| `-18` | The built-in wallet does not exist yet; run `labcoat up` |
| `-4` | The built-in wallet could not be opened or could not sign |
| `-22` | `psbt` is not a base64 PSBT |
| `-8` | Invalid parameters: no `psbt`, `options` is not an object, an input has no `witnessUtxo`, or an input of the wallet's asks for a sighash type other than `SIGHASH_DEFAULT` |

### Broadcast and confirm

The same endpoint broadcasts and mines:

1. `sendrawtransaction` with `[hex]` validates the transaction, adds it to
   the mempool, and returns its txid. A rejection is an error with Bitcoin
   Core's code and reason, such as `-26` with `min relay fee not met`.
2. Nothing is mined automatically: the transaction stays in the mempool until
   someone asks for a block. `generatetoaddress` with `[1, address]` mines
   one holding every mempool transaction and returns its hash. The Alkanes
   index includes the block by the time the call returns.
3. `getrawtransaction` with `[txid, true]` then reports `confirmations: 1`.
   Before the block it has no `confirmations` field.

```bash
rpc() { curl -s -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":$2}" http://127.0.0.1:18443; }
address=$(rpc labcoat_dev_addresses '[]' | jq -r '.result.addresses[0].address')
signed=$(rpc labcoat_dev_sign_psbt "[\"$PSBT\"]")
rpc sendrawtransaction "[\"$(echo "$signed" | jq -r .result.hex)\"]"
rpc generatetoaddress "[1, \"$address\"]"
```

### Who can call it

The dev signer signs anything it is asked to, without a prompt, so what it
signs with is restricted instead:

- It exists only on Labcoat Network. No other network has a Labcoat process
  to serve it, and no flag or `LABCOAT_*` setting moves it.
- It signs only with the built-in wallet: a random key that `labcoat up`
  generated under a public passphrase, which
  [cannot be used on another network](/docs/projects-configuration/#wallets)
  and cannot have a mnemonic imported into it. A keystore named by
  `--wallet-file`, `LABCOAT_WALLET_FILE`, or `labcoat.toml` is never used,
  and neither is a link placed at the built-in wallet's path.
- The endpoint listens on `127.0.0.1` only.
- A browser can still be sent to a local port by a page from anywhere, so the
  signing methods check where a request came from. A request with an `Origin`
  header is answered only when the origin is `localhost`, a name under
  `.localhost`, or a loopback address, and a request whose `Host` header names
  anything else is refused, which is what a DNS rebinding attack looks like.
  Scripts and tests send no `Origin` and are answered.

A web app reaches the endpoint through its own dev server's same-origin
proxy, so open the app at `http://localhost:<port>`. Opened through a LAN
address or a tunnel, the page's origin is no longer this machine and signing
fails with `-32000`.

What is left exposed is the built-in wallet's regtest coins, to programs
running on this machine and to pages they serve. Those coins are worthless,
and the wallet's passphrase is public already. Keep it that way: never send
anything of value to the built-in wallet's addresses on another network,
because a taproot key signs the same on every network.

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

## Where Labcoat keeps its data

Everything Labcoat writes outside a project is in one directory:

- macOS: `~/Library/Application Support/Labcoat/`
- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/labcoat/`

```text
network/               the chain, its index, and the faucet
snapshots/             chains saved by `labcoat snapshot`
logs/                  `labcoat logs` reads these
wallets/               one keystore per network
runtimes/cli-vX.Y.Z/   the indexer module for each installed release
config.json            the network's port, when you change it
```

`LABCOAT_DATA_HOME` moves the directory. `labcoat doctor` prints where it
is.

## Reset carefully

`labcoat reset -y` stops Labcoat Network and permanently removes `network/`:
its chain, index, and faucet data. Wallets and snapshots are kept. Data and
snapshots from releases that ran Qubitcoin are in a separate `Isomer` folder;
they are left untouched and are not read.

## Upgrading from a Qubitcoin-based release

Labcoat 0.3.0 and earlier ran Labcoat Network on a downloaded `qubitcoind`.
Later releases run it inside the `labcoat` executable and start a new chain.
Nothing from the old chain is converted, and nothing of it is deleted:

| What | After upgrading |
| --- | --- |
| Wallets | Kept. Keystores, including the built-in Labcoat Network wallet, stay where they are, and the next `labcoat up` funds the built-in wallet on the new chain. |
| Chain data | Not read. Earlier releases kept the chain in a folder named `Isomer`, beside Labcoat's data directory, and it stays there. The new chain is created in the data directory itself, under `network/`. |
| Snapshots | Not read. `labcoat snapshot` and `labcoat restore` use `snapshots/` in the data directory; snapshots from the old chain stay under `Isomer/snapshots-v2/`. |
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
