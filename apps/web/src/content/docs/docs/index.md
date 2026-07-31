---
title: Labcoat documentation
description: Build, test, deploy, simulate, and trace Alkanes smart contracts with one CLI and Labcoat Network.
slug: docs
---

Labcoat is the Rust-native CLI for building, testing, and operating Alkanes
smart contracts on Labcoat Network, a managed local Bitcoin regtest. It
connects project scaffolding, native tests, WebAssembly builds, wallets,
deployment, calls, simulation, and decoded traces.

Labcoat Network is Labcoat's local, private, resettable Alkanes network. It
runs Bitcoin regtest under the hood. It is not Signet.

> Early-stage software for local Alkanes development. Interfaces may change
> before 1.0; mainnet deployment controls are not production-ready.

```bash
labcoat init hello-alkane
cd hello-alkane
labcoat test
labcoat up
```

## Pick your path

- [Install Labcoat](/docs/getting-started/installation/) on macOS or Linux.
- Follow the [quick start](/docs/getting-started/quickstart/) from an empty
  directory to a deployed contract.
- Integrate an AI agent through [MCP or JSON envelopes](/docs/automation/).
- Read the generated [CLI reference](/docs/reference/cli/).
- Review [stability and releases](/docs/reference/stability/) before
  pinning automation.

## The supported interface

The `labcoat` executable is the public interface. Internal Rust crates may evolve,
but CLI commands, `labcoat/v1/*` JSON envelopes, and MCP tools are designed for
automation.

Every JSON error includes a stable code, a human-readable message, and a
recovery hint naming the next useful command.
