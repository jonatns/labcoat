---
title: Quick start
description: Scaffold, test, fund, build, deploy, call, and trace an Alkanes contract.
---

## Create and test a contract

```bash
labcoat init hello-alkane
cd hello-alkane
labcoat test
```

Run `labcoat init` without a name to enter it interactively. Initialization
always creates a new folder and refuses an existing destination.

Every project starts with a fixed Counter contract. Add another minimal
contract package and matching host test from anywhere inside the project:

```bash
labcoat new token
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

## Build and deploy

```bash
labcoat build counter
labcoat deploy counter --dry-run
labcoat deploy counter
```

Deploy selects and recompiles the exact Cargo package before creating the
commit/reveal envelope and recording the resulting Alkanes ID in
`labcoat.lock`. Use `--wasm <path>` only for an external raw Wasm artifact.

## Interact and inspect

```bash
labcoat simulate counter get_count
labcoat call counter increment
labcoat trace <txid> --wait
```

Method names come from the deployed contract ABI. Numeric opcodes remain
available when you need to provide raw cellpack arguments.

Finish by stopping the shared local devnet:

```bash
labcoat down
```
