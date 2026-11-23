---
title: Atomic exchanges
description: Swap two Alkane assets between two wallets in one transaction, locally or through a PSBT ceremony.
---

An atomic exchange moves a seller's Alkane asset to a buyer and the buyer's
payment asset to the seller in a single Bitcoin transaction. Either both
transfers happen or neither does.

Amounts are complete quantities in base units: the buyer receives exactly the
offered amount and the seller receives exactly the payment amount. Assets are
`labcoat.lock` names or `block:tx` ids. The buyer's wallet pays the network
fee.

## Local exchanges

On Labcoat Network and custom regtest, one command plans, signs with both
keystores, and broadcasts:

```bash
labcoat exchange token 100 usd 7500 --seller-wallet-file seller-wallet.json
```

The project wallet, or the global `--wallet-file`, is the buyer. This
two-keystore coordinator exists for local integration tests and is refused on
public networks, where one process should never hold both parties' keys.

## The PSBT ceremony

Everywhere else, the exchange is a three-step ceremony in which each party
signs only their own inputs:

```bash
# 1. Buyer: build the plan and the unsigned PSBT.
labcoat --wallet-file buyer.json exchange-plan token 100 usd 7500 \
  --seller-address <seller-address> --buyer-address <buyer-address> \
  --plan-out exchange-plan.json --psbt-out exchange.psbt

# 2. Buyer: sign the buyer's inputs.
labcoat --wallet-file buyer.json wallet sign-psbt \
  --in exchange.psbt --out exchange.buyer-signed.psbt

# 3. Seller: validate the plan, sign last, and broadcast.
labcoat --wallet-file buyer.json exchange-settle \
  --plan exchange-plan.json --psbt exchange.buyer-signed.psbt \
  --seller-wallet-file seller.json --broadcast
```

The plan file is content-addressed. `exchange-settle` checks the buyer-signed
PSBT against the plan before the seller signs anything. Without `--broadcast`,
it validates and signs but does not publish the transaction.

Signing is seller-last: the buyer may hold an unsigned or partially signed
PSBT, but the seller never releases a fully signed transaction before
broadcasting it.

### Request files

Instead of positional assets and address flags, `exchange-plan` accepts a
version 1 request file:

```bash
labcoat --wallet-file buyer.json exchange-plan --request exchange-request.json \
  --plan-out exchange-plan.json --psbt-out exchange.psbt
```

Request files carry every u64 as a decimal string, so JavaScript can produce
them without losing precision. The generated web client's
`serializeExchangeRequest` writes this exact format; see
[Web apps](/docs/web-apps/#exchange-requests).

## Sign with an external wallet

`wallet sign-psbt` signs any PSBT file (base64 or hex) with a Labcoat
keystore, so the buyer's step can happen on another machine.

For any state-changing command, `--signer psbt-file:<dir>` writes the unsigned
PSBT into `<dir>` and waits for a signed copy. Sign it with an air-gapped
machine, a hardware-wallet app, Sparrow, or `labcoat wallet sign-psbt` on
another keystore; Labcoat never holds that key material.

```bash
labcoat deploy my-token --signer psbt-file:./signing
```
