# Working in this Labcoat project

This is an Alkanes smart-contract project on Bitcoin. The `labcoat` CLI
drives everything; every command takes `--json` and returns one envelope
(`{ok, command, schema, result | error{code, message, hint}}`). On errors,
follow `error.hint` — it names the next command to run.

## Layout

- `contracts/*.rs` — contract sources (`#[opcode(n)]` message grammar)
- `build/` — compile artifacts: `<name>.wasm` (deploy this one),
  `<name>.wasm.gz`, `<name>.abi.json` (opcode ↔ method map)
- `labcoat.lock` — per-network deployment ledger (name → alkanesId)
- `labcoat.config.ts` — network + rpcUrl + optional mnemonic
- `.labcoat/wallet.json` — the wallet keystore (never commit)

## The loop

```bash
labcoat up --json                         # local devnet (regtest)
labcoat wallet init --json
labcoat compile contracts/Example.rs --json
labcoat deploy build/Example.wasm --json  # → result.alkanesId
labcoat simulate Example 1 World --json   # read-only
labcoat call Example <opcode> [args] --json
labcoat trace <txid> --wait --json
```

Full reference: `labcoat docs --llm`. MCP server: `labcoat mcp serve`.
The SKILL.md next to this file has the step-by-step workflow.

## Rules

- Deploy the RAW `.wasm`, never `.wasm.gz`.
- Secrets via env (`LABCOAT_WALLET_PASSPHRASE`, `LABCOAT_MNEMONIC`) or
  stdin — never argv, never committed.
- `labcoat reset -y` wipes the chain; redeploy afterwards.
- Look up opcodes in `build/<name>.abi.json`; don't guess them.
