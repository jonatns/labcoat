# Labcoat documentation

**Labcoat** is a smart contract development toolkit for
[Alkanes](https://alkanes.build) on Bitcoin. **Isomer** is the one-click
local devnet inside it — desktop app or headless (`labcoat up`).

## Guides

- [Migrating from the old labcoat / Isomer repos](MIGRATING.md)
- [Releasing](RELEASING.md)
- [Isomer release notes archive (pre-monorepo)](RELEASING-isomer.md)
- [Migration working docs](migration/) — pins, audits, coupling inventory

## Reference

- `labcoat docs --llm` — the full command reference + protocol
  cheatsheet as one document (also the best starting point for humans).
- [`TOOLCHAIN.md`](../TOOLCHAIN.md) — toolchain versions, the alkanes-rs
  develop pin, and the oyl-sdk ban.
- [`skills/SKILL.md`](../skills/SKILL.md) — the agent workflow.

## Architecture (one core, many surfaces)

```
crates/isomer-core    devnet engine: binaries, processes, chain control
crates/labcoat-core   contract toolkit on pinned alkanes-rs develop
crates/labcoat-cli    `labcoat` — CLI + MCP server over both cores
apps/isomer           Tauri desktop app (thin UI over isomer-core)
packages/labcoat      @jonatns/labcoat — TS API delegating to the CLI
packages/create-labcoat  project scaffolder (npm create labcoat)
```

Every surface — desktop app, CLI, TS scripts, MCP agents — drives the
same two core crates, so behavior is identical everywhere.
