# Migrating from the old labcoat / Isomer repos

Both projects now live here: **Labcoat is the toolkit, Isomer is the
desktop devnet inside it** (think Foundry ⊃ Anvil). Full git history of
both repos was preserved (`git log --follow` works across the moves).

## If you used `@jonatns/labcoat` (the npm toolkit)

The public API is unchanged — `labcoat.setup()` still returns
`{ config, account, provider, signer, deploy, simulate, execute }` and
your scripts keep working — but the engine underneath is new:

- **oyl-sdk is gone.** Wallet, deploy, execute, simulate, and trace now
  run through a Rust core built on a pinned `alkanes-rs` (develop)
  commit. The same mnemonic derives the same addresses (standard
  BIP-86/84/49/44 paths).
- The TS package shells out to the `labcoat` CLI. In a dev checkout it
  finds `target/{release,debug}/labcoat` automatically; otherwise set
  `LABCOAT_CORE_BIN` or put `labcoat` on PATH.
- `signer` from `setup()` is now `null` (signing happens in the Rust
  core; it was only ever an oyl-sdk internal).
- `network: "oylnet"` is deprecated → use `"regtest"` (auto-mapped with
  a warning). `projectId` is ignored.
- Deployments moved to **labcoat.lock** (per-network). Run
  `labcoat lock migrate` once to import `deployments/manifest.json`;
  the legacy manifest is still written for compatibility.
- `deploy` now consumes the **raw `.wasm`** artifact (compile emits it
  alongside the old `.wasm.gz`). Recompile once after upgrading.
- Wallet secrets: passphrase via `LABCOAT_WALLET_PASSPHRASE`, mnemonic
  via `labcoat.config.ts` / `LABCOAT_MNEMONIC` — never on argv.

## If you used the Isomer desktop app

Nothing changes day-to-day. Releases move to this repo's
`isomer-v*` tags (the old repo's downloads keep working). The devnet
engine is now also available headless:

```bash
labcoat up      # the entire Isomer stack, no GUI
labcoat status
labcoat mine 5
labcoat down
```

## If you're an agent (or wiring one up)

- `labcoat docs --llm` — the whole toolkit as one document.
- Every command takes `--json` and emits one envelope with typed error
  codes and next-step hints.
- `labcoat mcp serve` — the full toolkit as MCP tools over stdio.
- New projects: `npm create labcoat` scaffolds with AGENTS.md + SKILL.md.
