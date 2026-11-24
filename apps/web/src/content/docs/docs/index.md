---
title: Labcoat documentation
description: Build, test, deploy, simulate, and trace Alkanes smart contracts with one CLI and Labcoat Network.
slug: docs
---

Labcoat is the Rust-native CLI for building, testing, and operating Alkanes
smart contracts on Labcoat Network, a managed local Bitcoin regtest. It
connects project scaffolding, native tests, WebAssembly builds, wallets,
deployment, calls, simulation, and decoded traces.

> Early-stage software for local Alkanes development. Interfaces may change
> before 1.0; mainnet deployment controls are not production-ready.

```bash
curl -fsSL https://labcoat.sh/install | sh
labcoat init hello-alkane
cd hello-alkane
labcoat test
```

## Choose a path

<ul class="docs-paths not-content">
  <li>
    <a href="/docs/getting-started/quickstart/">
      <span class="path-label">Start</span>
      <strong>Quick start</strong>
      Go from an empty directory to a deployed, traced contract.
      <code>labcoat init</code>
    </a>
  </li>
  <li>
    <a href="/docs/contracts/">
      <span class="path-label">Contracts</span>
      <strong>Write and test contracts</strong>
      Native Rust tests, Wasm builds, deploys, calls, and traces.
      <code>labcoat test</code>
    </a>
  </li>
  <li>
    <a href="/docs/deployments/">
      <span class="path-label">Deploy</span>
      <strong>Deployment scripts</strong>
      Declare several contracts and their upgrades in one script; each step runs once.
      <code>labcoat deploy</code>
    </a>
  </li>
  <li>
    <a href="/docs/tutorials/first-contract/">
      <span class="path-label">Tutorial</span>
      <strong>Your first contract</strong>
      Read the starter Counter, deploy and call it, then add a method.
      <code>labcoat deploy</code>
    </a>
  </li>
  <li>
    <a href="/docs/web-apps/">
      <span class="path-label">Web</span>
      <strong>Read from a browser</strong>
      Generate a typed TypeScript client from your deployments.
      <code>labcoat generate web</code>
    </a>
  </li>
  <li>
    <a href="/docs/automation/">
      <span class="path-label">Agents</span>
      <strong>Automate with JSON and MCP</strong>
      Give scripts and coding agents the same typed operations.
      <code>labcoat mcp serve</code>
    </a>
  </li>
</ul>

## Key concepts

**Labcoat Network.** Labcoat's local, private, resettable Alkanes network. It
runs Bitcoin regtest under the hood with the Alkanes and Esplora indexes built
in, and it is not Signet. `labcoat up` starts it and `labcoat reset` wipes it.
See [Labcoat Network and wallets](/docs/labcoat-network/).

**A project.** `labcoat init` creates a Cargo workspace with a Counter
contract, native tests, `labcoat.toml` for settings, and a `rust-toolchain.toml`
that pins the Rust release Labcoat is tested with. See
[Projects and configuration](/docs/projects-configuration/).

**`labcoat.lock`.** The per-network record of what you deployed and where:
each contract name maps to its `block:tx` ID, so later commands can use the
name. Commit it when deployments are shared project state.

**`deploy.rs`.** A deployment script declaring every contract, reference,
and upgrade in a deployment, which `labcoat plan` reviews and `labcoat deploy`
runs once per network.
See [Deployment scripts](/docs/deployments/).

**One release unit.** Each `cli-vX.Y.Z` release ships the CLI, its exact
Labcoat Network runtime, and the matching `labcoat-test` harness together.
After upgrading the CLI, run `labcoat upgrade` in each project. See
[Stability and releases](/docs/reference/stability/).

## New in 0.3.0

- [Deployment scripts](/docs/deployments/) with `deploy.rs`, `plan`, `deploy`,
  and `import`, upgradeable proxies and beacons, and network e2e tests through
  `labcoat test --e2e`. `labcoat deploy` now runs `deploy.rs`. The
  [upgradeable contract tutorial](/docs/tutorials/upgradeable-contract/)
  walks through a full deploy and upgrade.
- A [built-in funded wallet](/docs/labcoat-network/#wallet-workflow) on Labcoat
  Network: `labcoat up` creates and funds it, and keystores live outside the
  project.
- [Subfrost endpoints](/docs/projects-configuration/#signet-and-mainnet-through-subfrost)
  for `--network signet` and `--network mainnet`.
- [Atomic exchanges](/docs/exchanges/) between two wallets, locally or through
  a seller-last PSBT ceremony, with single-asset outputs.
- [Generated web clients](/docs/web-apps/) for reading contracts from a browser.
- [Deployment state](/docs/deployment-state/) with an append-only history and
  reset protection.
- `labcoat upgrade`, which moves an existing project's pins to the installed CLI.
- alkanes-rs `v3.0.0-rc.4`, with the post-audit rules and DIESEL v3 active from
  block 0 on Labcoat Network.

The [changelog](https://github.com/jonatns/labcoat/blob/main/CHANGELOG.md) has
the full list, including fixes.

## The supported interface

The `labcoat` executable is the public interface. Internal Rust crates may evolve,
but CLI commands, `labcoat/v1/*` JSON envelopes, and MCP tools are designed for
automation.

Every JSON error includes a stable code, a human-readable message, and a
recovery hint naming the next useful command. See
[Errors and recovery](/docs/reference/errors/).
