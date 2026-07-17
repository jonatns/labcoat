---
title: Stability and releases
description: Understand Labcoat's pre-1.0 compatibility, documentation channels, and supported scope.
---

Labcoat is early-stage software for local Alkanes development. Interfaces may
change before 1.0; mainnet deployment controls are not production-ready.

## Stable release and current main

The public website documents the current `main` branch. The reference bundled
with an installed release is authoritative for that executable:

```bash
labcoat docs --llm
labcoat --version
labcoat --help
```

> **Temporary `cli-v0.1.0` compatibility note:** the stable release uses
> `labcoat contract new`, `labcoat compile`, and raw-Wasm deployment. The
> current main branch uses `labcoat new`, `labcoat build`, and package-name
> deployment. This notice remains until a newer stable release is published,
> installed through `/install`, and verified.

## Compatibility expectations

| Surface | Pre-1.0 expectation |
| --- | --- |
| CLI commands and flags | May change between minor releases. Breaking changes are called out in release notes and the changelog. |
| JSON envelopes | Use versioned `labcoat/v1/*` schemas. Consumers should handle typed errors and unknown additive fields. |
| MCP tools | Generated from the installed CLI capability set. Pin the Labcoat version used by agents and automation. |
| Web documentation | Tracks `main`; it can be ahead of the latest stable release. |
| `labcoat docs --llm` | Bundled installed-version authority for commands and examples. |
| Project files | Commit `Cargo.lock`, `labcoat.toml`, and relevant `labcoat.lock` state for reproducibility. |

## Supported scope

Current:

- macOS and Linux on arm64 and x86_64;
- Rust contract scaffolding, native Wasm tests, and package builds;
- managed local Bitcoin regtest services;
- local project wallets, deployment, calls, simulation, and traces;
- CLI, JSON envelopes, MCP tools, and generated references.

Release-dependent:

- `cli-v0.1.0` command names and raw-Wasm deployment differ from current main;
- downloaded service revisions are pinned by each Labcoat release.

Planned, not shipped:

- production-ready mainnet deployment controls;
- durable production runtime state and hosted operation;
- team access controls.

Unsupported:

- Windows;
- treating the local devnet or its wallet defaults as a production security
  boundary;
- guarantees that website examples work with an unpinned older executable.

See the [installation guide](/docs/getting-started/installation/) for version
pinning and verification, and the
[security policy](https://github.com/jonatns/labcoat/blob/main/SECURITY.md) for
runtime and wallet threat boundaries.
