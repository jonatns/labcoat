---
title: Quick start
description: Scaffold, test, fund, compile, deploy, call, and trace an Alkanes contract.
---

## Create and test a contract

```bash
labcoat init hello-alkane
cd hello-alkane
labcoat test
```

The generated project contains Rust contract sources, native integration tests,
public configuration, deployment state, and agent instructions.

## Start the local chain

```bash
labcoat up
labcoat status
labcoat wallet init
labcoat wallet addresses
```

Fund the displayed P2TR address and confirm it:

```bash
labcoat fund <address>
labcoat mine 1
labcoat wallet utxos
```

## Compile and deploy

```bash
labcoat compile contracts/Example.rs
labcoat deploy build/Example.wasm --dry-run
labcoat deploy build/Example.wasm
```

Always deploy the raw `.wasm`. The deployment flow performs compression inside
the commit/reveal envelope and records the resulting Alkanes ID in
`labcoat.lock`.

## Interact and inspect

```bash
labcoat simulate Example 1 World
labcoat call Example 1 World
labcoat trace <txid> --wait
```

Finish by stopping the shared local devnet:

```bash
labcoat down
```
