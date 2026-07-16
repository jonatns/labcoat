# Announcement draft (Discord: SANDSHREW / alkanes)

> **Labcoat + Isomer are now one project** 🥼⚛️
>
> The labcoat toolkit and the Isomer desktop devnet merged into a single
> monorepo: <https://github.com/jonatns/labcoat> — Labcoat is the
> toolkit, Isomer is the devnet inside it (Foundry ⊃ Anvil), with full
> git history from both repos.
>
> What's new:
> - **`labcoat up`** — the entire Isomer devnet stack, headless. Boot
>   bitcoind regtest + metashrew + ord + esplora + espo + the unified
>   JSON-RPC gateway from one command; `mine`, `fund`, `logs`,
>   `snapshot`/`restore` included. The desktop app and the CLI share one
>   engine (isomer-core).
> - **Rust-first, with oyl-sdk gone.** The native CLI owns compilation,
>   testing, deploy/execute/simulate/trace, and the keystore wallet on a
>   pinned `alkanes-rs` develop commit. The former TypeScript SDK is
>   retired; migration notes map scripts to direct CLI commands.
> - **labcoat.lock** — per-network deployment ledger
>   (`labcoat lock migrate` imports your old manifest).
> - **Agent-ready**: `--json` envelopes with typed errors + hints on
>   every command, `labcoat mcp serve` (MCP tools over stdio),
>   `labcoat docs --llm`, and `labcoat init` templates that ship native
>   Rust tests plus AGENTS.md + SKILL.md.
> - `labcoat doctor`, `labcoat up --ci`, and a hard CI gate keeping
>   oyl-sdk out and alkanes-rs pinned to develop.
>
> Migration notes: <https://github.com/jonatns/labcoat/blob/main/docs/MIGRATING.md>
> The old `jonatns/isomer` repo is archived; its releases stay up.

(Post after the first monorepo releases are tagged; adjust links if the
docs move.)
