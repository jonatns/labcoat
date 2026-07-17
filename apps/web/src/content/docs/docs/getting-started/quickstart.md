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

Every project starts with a fixed Counter contract. Add another minimal
contract package and matching host test from anywhere inside the project:

```bash
labcoat new resolver
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
labcoat compile counter
labcoat deploy counter --dry-run
labcoat deploy counter
```

Deploy selects and recompiles the exact Cargo package before creating the
commit/reveal envelope and recording the resulting Alkanes ID in
`labcoat.lock`. Use `--wasm <path>` only for an external raw Wasm artifact.

## Interact and inspect

```bash
labcoat simulate counter 2
labcoat call counter 1
labcoat trace <txid> --wait
```

Finish by stopping the shared local devnet:

```bash
labcoat down
```
