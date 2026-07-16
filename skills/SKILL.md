---
name: labcoat
description: Build, test, deploy, call, and trace Alkanes smart contracts on Bitcoin with the Rust-first Labcoat CLI and its local devnet. Use when working in a labcoat project (labcoat.toml / labcoat.lock / contracts/*/Cargo.toml) or when asked to develop Alkanes contracts.
---

# Labcoat: the Alkanes contract workflow

New projects start with `labcoat init <directory>`. Native integration
tests live in `tests/*.rs`; run them with `labcoat test`.

Every command supports `--json` and then emits exactly one envelope on
stdout (`{ok, command, schema, result | error{code, message, hint}}`),
logs on stderr, exit 0 whenever an envelope was printed. On any error,
`error.hint` names the next command to run — follow it.

## 1. Boot infrastructure

```bash
labcoat up --json          # downloads binaries if missing, boots the stack
labcoat status --json      # poll until result.is_ready == true
```

`up` returns `result.endpoints` — the unified JSON-RPC gateway
(`http://localhost:18888`) proxies everything.

## 2. Wallet

```bash
labcoat wallet init --json                 # creates .labcoat/wallet.json
labcoat wallet addresses --json            # p2tr address is the primary
labcoat fund <p2tr-address> --json         # faucet 1 BTC
labcoat mine 1 --json                      # confirm it
labcoat wallet utxos --json                # verify spendable balance
```

Secrets: `LABCOAT_WALLET_PASSPHRASE` env (regtest has a dev default);
mnemonic via `LABCOAT_MNEMONIC` env or `wallet init --mnemonic-stdin`.
Never place either on argv.

## 3. Compile

```bash
labcoat compile my-token --json
```

Result: `build/my-token.wasm` (raw — what deploy consumes),
`build/my-token.wasm.gz`, `build/my-token.abi.json`. The ABI is extracted
from the compiled Wasm's `__meta` export. Commit the `Cargo.lock` created by
the first build.

## 4. Deploy

```bash
labcoat deploy build/my-token.wasm --json          # add --dry-run to preview
```

Deploys via commit/reveal envelope, waits for the `create` trace, returns
`result.alkanesId` (`block:tx`), and records the deployment in
`labcoat.lock` under the current network. Always pass the RAW `.wasm`,
never `.wasm.gz`.

## 5. Call & simulate

```bash
# look up the opcode in build/my-token.abi.json first
labcoat simulate my-token 99 --json          # read-only; decoded result
labcoat call my-token 77 500 --json          # state-changing; auto-mines
```

Contract references: the labcoat.lock name (`my-token`) or a raw
`block:tx` id. Args: decimal u128, `0x`-hex, or short strings (≤16 bytes,
packed little-endian). `result.status` is `success` or `revert` (with
`result.revertReason` decoded).

## 6. Trace

```bash
labcoat trace <txid> --wait --json
```

Returns decoded events for every protostone in the tx (`create`,
`invoke`, `return`, per-protostone vouts computed automatically).

## MCP mode

`labcoat mcp serve` exposes the same operations as MCP tools over stdio
(devnet_up/down/status/mine/fund/reset/logs, wallet_init/addresses/utxos,
compile, test, abi_fetch, abi_verify, deploy, call, simulate, trace). Prefer it when a host supports
MCP; the JSON envelopes above are the fallback.

## Ground rules

- One devnet per machine: `up`/`down`/`reset` manage shared local state.
- `reset -y` wipes the chain — deployments in labcoat.lock become stale;
  redeploy after a reset.
- alkanes-rs is pinned (TOOLCHAIN.md). Never point anything at a branch.
- `labcoat docs --llm` prints the full reference document.
