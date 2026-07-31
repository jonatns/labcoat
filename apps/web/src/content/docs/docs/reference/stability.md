---
title: Stability and releases
description: Understand Labcoat's pre-1.0 compatibility, documentation channel, and product release model.
---

Labcoat is early-stage software for local Alkanes development. Interfaces may
change before 1.0; mainnet deployment controls are not production-ready.

## Documentation channel

The public website documents the current `main` branch and may be ahead of the
latest release. The reference bundled with an installed executable is
authoritative for that version:

```bash
labcoat docs --llm
labcoat --version
labcoat --help
```

This single channel notice replaces release-specific compatibility banners.
Release notes and the changelog describe changes between versions.

## One product version

A `cli-vX.Y.Z` release is one tested compatibility unit containing:

- native CLI executables and checksums;
- the exact managed Qubitcoin and indexer runtime;
- the matching `labcoat-test` source tag used by generated projects.

Labcoat does not independently update the runtime. An installed CLI requests
only the manifest for its own release tag and uses a versioned local cache after
the first verified download. Updating the CLI is the explicit runtime upgrade.

## Compatibility expectations

| Surface | Pre-1.0 expectation |
| --- | --- |
| CLI commands and flags | May change between minor releases; release notes call out breaking changes. |
| JSON envelopes | Use versioned `labcoat/v1/*` schemas; consumers should handle typed errors and unknown additive fields. |
| MCP tools | Generated from the installed CLI capability set; pin Labcoat in automation. |
| Runtime | Exactly tied to the CLI release; never selected from an independent latest channel. |
| `labcoat-test` | Generated projects pin the same immutable `cli-vX.Y.Z` Git tag. |
| Web documentation | Tracks `main`; it may describe unreleased changes. |
| Project files | Commit `Cargo.lock`, `labcoat.toml`, and relevant `labcoat.lock` state. |

## Supported scope

Current:

- CLI on macOS and Linux, arm64 and x86_64;
- managed devnet runtime on macOS arm64 and Linux x86_64;
- Rust contract scaffolding, native Wasm tests, and package builds;
- local wallets, deployment, calls, simulation, traces, JSON, and MCP.

Planned, not available:

- production-ready mainnet deployment controls;
- durable production runtime state and hosted operation;
- team access controls.

Unsupported:

- Windows;
- managed devnet runtime on macOS x86_64 or Linux arm64;
- treating the local devnet or its wallet defaults as a production security
  boundary;
- assuming website examples match an older unpinned executable.

See [Installation](/docs/getting-started/installation/) for version pinning and
the [security policy](https://github.com/jonatns/labcoat/blob/main/SECURITY.md)
for runtime and wallet threat boundaries.
