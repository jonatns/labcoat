# Labcoat

**Rust-first smart contract development toolkit for [Alkanes](https://alkanes.build) on Bitcoin — with Isomer, the desktop devnet, inside it.**

Think Foundry ⊃ Anvil: **Labcoat** is the toolkit (compile, deploy, call,
simulate, trace); **Isomer** is the one-click local devnet it runs against.
The **`labcoat` CLI is the flagship surface** — `labcoat up` boots the full
devnet headless. The Isomer desktop app is in **maintenance mode**: it keeps
compiling in CI over the same engine (`isomer-core`), but new features land
in the CLI first and app releases are tagged on demand only.

> **Rust-first monorepo.** This repository contains the native Labcoat
> toolkit and the Isomer desktop app, with full git history from their
> former repositories. See
> [`docs/migration/`](docs/migration/) and [`TOOLCHAIN.md`](TOOLCHAIN.md)
> for status, pins, and constraints.

## Layout

```
crates/
  isomer-core/       # headless devnet engine (binaries, processes, chain control)
  labcoat-core/      # contract toolkit core, built on pinned alkanes-rs develop
  labcoat-cli/       # `labcoat` CLI: devnet verbs + contract ops
  labcoat-test/      # native host harness for Rust contract integration tests
apps/
  isomer/            # Isomer desktop app (Tauri, maintenance mode) — thin UI over isomer-core
  isomer-extension/  # browser extension companion (maintenance mode)
skills/              # agent-facing workflow docs
docs/
```

## Development

```bash
pnpm install                 # Isomer frontend workspace
pnpm build                   # build the Isomer frontend
cargo check --workspace      # Rust workspace
pnpm dev:isomer              # run the Isomer desktop app
```

Install the CLI from a published macOS/Linux release:

```bash
curl -fsSL https://raw.githubusercontent.com/jonatns/labcoat/main/install-labcoat.sh | sh
labcoat init my-project
cd my-project && labcoat test
```

The former `@jonatns/labcoat` SDK and `create-labcoat` packages are
retired. See [`docs/MIGRATING.md`](docs/MIGRATING.md) for the direct CLI
equivalents.

Toolchain versions and upstream pins live in [`TOOLCHAIN.md`](TOOLCHAIN.md).
Two hard constraints inherited by all contributions:

1. Every `alkanes-rs` reference points at a **pinned commit on `develop`**
   — never `main`, never a moving ref.
2. `oyl-sdk` / `@oyl/sdk` is **banned** from the dependency tree, direct or
   transitive (CI-enforced).

## History

- `jonatns/isomer` was imported under `apps/isomer` with full history
  (`git log --follow` works across the move).
- The original repos are tagged `*@pre-monorepo` at their import states.
