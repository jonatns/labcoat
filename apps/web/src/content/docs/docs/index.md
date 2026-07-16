---
title: Labcoat documentation
description: Build, test, deploy, and operate Alkanes smart contracts on Bitcoin with one Rust-native CLI.
slug: docs
---

Labcoat is a Rust-first CLI for building Alkanes smart contracts on Bitcoin. It
owns project scaffolding, native tests, WebAssembly compilation, the local
devnet, wallets, deployments, contract calls, simulation, and decoded traces.

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

## The supported interface

The `labcoat` binary is the public interface. Internal Rust crates may evolve,
but CLI commands, `labcoat/v1/*` JSON envelopes, and MCP tools are designed for
automation.

Every JSON error includes a stable code, a human-readable message, and a
recovery hint naming the next useful command.
