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
```

The network comes with a funded wallet, so you can deploy right away. See
[Wallets](/docs/projects-configuration/#wallets) for other networks.

## Plan and deploy

```bash
labcoat plan
labcoat deploy
```

The project's `deploy.rs` declares one step: its Counter contract.
`labcoat plan` shows that step as pending and signs nothing.
`labcoat deploy` recompiles the Cargo package, creates the
commit/reveal envelope, and records the resulting Alkanes ID in
`labcoat.lock`. The step runs once, so deploying again does nothing.

## Interact and inspect

```bash
labcoat simulate counter get_count
labcoat call counter increment
labcoat trace <txid> --wait
```

Method names resolve against the deployed contract. A matching generated ABI
avoids an indexer metadata request; if the local build differs, Labcoat warns
and uses deployed metadata. Numeric opcodes remain available when you need to
provide raw cellpack arguments. Simulation always uses deployed code and live
indexed state; use `labcoat test counter` for an undeployed local build.

Finish by stopping the shared Labcoat Network:

```bash
labcoat down
```

## Next steps

- [Tokens and exchanges](/docs/tutorials/token-exchange/) builds on this
  project: a fixed-supply token, two wallets, and an atomic swap.
- [Deployment scripts](/docs/deployments/) covers `deploy.rs` in full:
  contracts that reference each other, existing Alkanes, and upgrades.
- [An upgradeable contract](/docs/tutorials/upgradeable-contract/) puts this
  project's Counter behind a proxy and upgrades it.
