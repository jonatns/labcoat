---
name: labcoat
description: Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts with a complete local Bitcoin devnet. Use when working in a Labcoat project or developing Alkanes contracts.
---

# Labcoat: the Alkanes contract workflow

New projects start with `labcoat init <project-name>` (or `labcoat init` for an
interactive prompt). Native integration
tests live under `tests/`; use `labcoat new <name>` to add a minimal
contract package and matching test without copying the example.
Run integration tests with `labcoat test`.

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

## 3. Build

```bash
labcoat build my-token --json
```

Result: `build/my-token.wasm` (raw — what deploy consumes),
`build/my-token.wasm.gz`, `build/my-token.abi.json`. The ABI is extracted
from the compiled Wasm's `__meta` export. Commit the `Cargo.lock` created by
the first build.

## 4. Deploy

```bash
labcoat deploy my-token --json          # add --dry-run to preview
```

Deploys via commit/reveal envelope, waits for the `create` trace, returns
`result.alkanesId` (`block:tx`), and records the deployment in
`labcoat.lock` under the current network. Use `--wasm <raw-file.wasm>` only
when intentionally deploying an explicit artifact instead of a Cargo package.

## 5. Call & simulate

```bash
labcoat simulate counter get_count --json    # read-only; decoded result
labcoat call counter increment --json        # state-changing; auto-mines
labcoat call my-token mint 500 --json        # ABI-typed u128 parameter
```

Contract references: the labcoat.lock name (`my-token`) or a raw
`block:tx` id. Use an exact ABI method name with one shell argument per
parameter; `u128`, arbitrary UTF-8 `String`, and decimal `block:tx`
`AlkaneId` values are encoded from the deployed ABI. A numeric opcode keeps
the raw cellpack format for advanced calls. `result.status` is `success` or
`revert` (with `result.revertReason` decoded). Compose multi-step operations
with ordinary shell scripts; Labcoat has no contract script runner.

## 6. Trace

```bash
labcoat trace <txid> --wait --json
```

Returns decoded events for every protostone in the tx (`create`,
`invoke`, `return`, per-protostone vouts computed automatically).

## MCP mode

`labcoat mcp serve` exposes the installed capability set as MCP tools over
stdio. Prefer it when a host supports MCP; the JSON envelopes above are the
fallback.

<!-- BEGIN GENERATED MCP TOOLS -->
- `devnet_up` — Boot the managed local Alkanes devnet services (downloads binaries when missing). Returns service status and the endpoint manifest.
- `devnet_down` — Stop all devnet services.
- `devnet_status` — Devnet service health, block height, and mempool size.
- `devnet_mine` — Mine blocks on the devnet.
- `devnet_fund` — Send BTC from the devnet faucet wallet to an address.
- `devnet_reset` — Stop services and wipe all devnet chain data.
- `devnet_logs` — Recent devnet service logs.
- `wallet_init` — Create or load the project wallet keystore. Optional mnemonic (else generated).
- `wallet_addresses` — Wallet receive addresses per script type.
- `wallet_utxos` — Spendable wallet UTXOs.
- `build` — Build Cargo contract packages and extract their Wasm-exported ABIs.
- `test` — Build every contract for WASIp1 and run host integration tests; the first build may take several minutes.
- `abi_fetch` — Fetch ABI metadata from a deployed contract through Metashrew.
- `abi_verify` — Compare a deployed ABI with a locally built contract package.
- `deploy` — Build and deploy an exact Cargo contract package, or deploy an explicit raw Wasm. Provide exactly one of package or wasm.
- `call` — Execute a state-changing contract call and wait for its trace.
- `simulate` — Read-only simulation of a contract call (no transaction).
- `trace` — Decoded protostone traces for a transaction.
<!-- END GENERATED MCP TOOLS -->

## Ground rules

- One devnet per machine: `up`/`down`/`reset` manage shared local state.
- `reset -y` wipes the chain — deployments in labcoat.lock become stale;
  redeploy after a reset.
- alkanes-rs is pinned (TOOLCHAIN.md). Never point anything at a branch.
- `labcoat docs --llm` prints the full reference document.
