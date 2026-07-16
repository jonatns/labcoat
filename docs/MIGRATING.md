# Migrating to Rust-first Labcoat

Labcoat and Isomer now live in one repository. Labcoat is the native Rust
toolkit; Isomer is its desktop and headless devnet (Foundry ⊃ Anvil).

## From `@jonatns/labcoat`

The TypeScript SDK and CLI have been retired. Install the native binary,
then replace `labcoat.setup()` scripts with explicit commands:

```bash
labcoat init .
labcoat up
labcoat wallet init
labcoat compile contracts/Example.rs
labcoat deploy build/Example.wasm
labcoat simulate Example 1 World
labcoat call Example 1 World
```

- Replace `labcoat.config.ts` with `labcoat.toml`. CLI flags override
  `LABCOAT_*` environment variables, which override the TOML file.
- Keep mnemonic and passphrase only in `LABCOAT_MNEMONIC`, mnemonic stdin,
  and `LABCOAT_WALLET_PASSPHRASE`; secrets are rejected in the TOML file.
- Replace `.spec.ts` tests with standard `tests/*.rs` files using
  `labcoat-test`; run them with `labcoat test`.
- Script orchestration belongs in ordinary shell/CI scripts. The old
  `labcoat run` command no longer exists.
- `oyl-sdk` is gone. Wallet, deploy, execute, simulate, and trace use the
  pinned `alkanes-rs` Rust core.
- Deployments live in `labcoat.lock`; `labcoat lock migrate` imports the
  legacy `deployments/manifest.json` once.
- Deploy the raw `.wasm` artifact. The reveal envelope performs its own
  compression.

The same mnemonic continues to use the standard BIP-86/84/49/44 paths.
Verify address parity before using an existing wallet on a non-regtest
network.

## From the standalone Isomer repository

Desktop usage remains the same. Releases now use `isomer-v*` tags in this
repository. The same engine is also available headlessly:

```bash
labcoat up
labcoat status
labcoat mine 5
labcoat down
```

## Agent and automation interfaces

- Every command supports `--json` envelopes with typed errors and hints.
- `labcoat docs --llm` prints the complete command reference.
- `labcoat mcp serve` exposes the toolkit over MCP stdio.
- `labcoat init` embeds AGENTS.md and SKILL.md in each new project.
