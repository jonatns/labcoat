# Labcoat

**Smart contract development toolkit for [Alkanes](https://alkanes.build) on Bitcoin — with Isomer, the desktop devnet, inside it.**

Think Foundry ⊃ Anvil: **Labcoat** is the toolkit (compile, deploy, call,
simulate, trace); **Isomer** is the one-click local devnet it runs against,
available both as a desktop app and headless via `labcoat up`.

> ⚠️ **Monorepo migration in progress.** This repository now contains both
> the former `jonatns/labcoat` npm toolkit and the former `jonatns/isomer`
> desktop app, with full git history from both. See
> [`docs/migration/`](docs/migration/) and [`TOOLCHAIN.md`](TOOLCHAIN.md)
> for status, pins, and constraints.

## Layout

```
crates/
  isomer-core/       # headless devnet engine (binaries, processes, chain control)
  labcoat-core/      # contract toolkit core, built on pinned alkanes-rs develop
  labcoat-cli/       # `labcoat` CLI: devnet verbs + contract ops
apps/
  isomer/            # Isomer desktop app (Tauri) — thin UI over isomer-core
  isomer-extension/  # browser extension companion
packages/
  labcoat/           # `@jonatns/labcoat` npm package (TS API + CLI wrapper)
  create-labcoat/    # project scaffolding (WIP)
skills/              # agent-facing workflow docs (WIP)
docs/
```

## Development

```bash
pnpm install                 # JS workspaces
pnpm build                   # build all packages/apps
cargo check --workspace      # Rust workspace
pnpm dev:isomer              # run the Isomer desktop app
```

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
