---
name: labcoat
description: Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts on Labcoat Network, a managed local Bitcoin regtest. Use when working in a Labcoat project or developing Alkanes contracts.
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
labcoat up --json          # prepares this CLI version's runtime, boots the stack
labcoat status --json      # poll until result.is_ready == true
```

`up` returns `result.endpoints.qubitcoin_rpc`, the direct local Qubitcoin
endpoint (`http://127.0.0.1:18443`).

## 2. Wallet

```bash
labcoat wallet init --json                 # creates .labcoat/wallet.json
labcoat wallet addresses --json            # p2tr address is the primary
labcoat fund <p2tr-address> --json         # faucet 1 BTC
labcoat mine 1 --json                      # confirm it
labcoat wallet utxos --json                # verify spendable balance
```

Secrets: `LABCOAT_WALLET_PASSPHRASE` env (Labcoat Network and custom regtest
have a development default);
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
`AlkaneId` values are encoded for the deployed contract. A matching generated
build ABI avoids the metadata request; a different local build produces a
warning and the deployed ABI is used. A numeric opcode keeps the raw cellpack
format for advanced calls. Simulation always uses deployed code and live
indexed state; use `labcoat test <package>` for an undeployed local build.
`result.status` is `success` or `revert` (with `result.revertReason` decoded).
Both `deploy` and `call` accept transaction shaping: `--inputs
block:tx:amount` (incoming alkanes; `B:sats` for bitcoin), `--to <address>`
(protostone recipient), `--pointer`/`--refund` (`vN`/`pN` routing), repeated
`--edict block:tx:amount:target`, and `deploy --reserve N` targets `[3,N]`.
`labcoat balance <address> --json` lists alkanes token balances.

Compose multi-step deployments declaratively in `alkanes.hcl`: `contract`
blocks are managed deployments, `alkane` blocks bind names to external
on-chain ids (no deploy, no dependency edge), and `call` blocks run after
deploys — reserve them for configuration that completes the deployment
(wiring references, setting admins); the manifest is done when the topology
is correct and inert, so value-moving, actor-specific operations (funding,
transfers, exercising) belong in `tests/e2e.rs` or application flows, not
here. An alkane's `id` is one binding for every network
(`id = [4, 65012]`) or a per-network map
(`id = { regtest = [4, 65012], signet = [2, 190213] }`) — one manifest
serves every environment, and plan fails fast when the active network has
no binding. References are namespaced — `contract.<name>.id` (orders that
deploy first) and `alkane.<name>.id` (resolves immediately), plus
`.block`/`.tx` — with `height` arithmetic and `"${...}"` templates, but no
conditionals/loops/functions. Prefer named constructor args matched to the
ABI constructor's parameters (`args = { supply = 100, ... }`; requires a
typed opcode-0 constructor) over positional arrays. Then `labcoat plan` (read-only
reconcile against labcoat.lock + chain) and `labcoat apply --broadcast`
(idempotent; re-run resumes). Imperative
multi-step flows belong in `tests/e2e.rs` (`labcoat test --e2e`), which
resets the chain, applies the manifest, and runs `#[ignore]`d Rust tests
using `labcoat_test::e2e::E2e` (disposable wallets, calls, balances,
mining).

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
- `network_up` — Boot Labcoat Network using the exact runtime bundle for this CLI release. Returns service status and the endpoint manifest.
- `network_down` — Stop all Labcoat Network services.
- `network_status` — Labcoat Network service health, block height, and mempool size.
- `network_mine` — Mine blocks on Labcoat Network.
- `network_fund` — Send BTC from the Labcoat Network faucet wallet to an address.
- `network_reset` — Stop services and wipe all Labcoat Network chain data.
- `network_logs` — Recent Labcoat Network service logs.
- `wallet_init` — Create or load the project wallet keystore. Optional mnemonic (else generated).
- `wallet_addresses` — Wallet receive addresses per script type.
- `wallet_utxos` — Spendable wallet UTXOs.
- `build` — Build Cargo contract packages and extract their Wasm-exported ABIs.
- `test` — Build every contract for WASIp1 and run host integration tests; the first build may take several minutes.
- `abi_fetch` — Fetch ABI metadata from the in-process Alkanes indexer.
- `abi_verify` — Compare a deployed ABI with a locally built contract package.
- `deploy` — Build and deploy an exact Cargo contract package, or deploy an explicit raw Wasm. Provide exactly one of package or wasm.
- `call` — Execute a state-changing contract call and wait for its trace.
- `simulate` — Simulate a deployed contract against live indexed chain state (no transaction).
- `trace` — Decoded protostone traces for a transaction.
- `balance` — Alkanes token balances held by an address.
- `plan` — Reconcile the alkanes.hcl deployment manifest against labcoat.lock and chain state; shows pending actions without loading a signer.
- `apply` — Execute the deployment manifest's pending actions. Requires broadcast: true to transact; otherwise returns the plan.
<!-- END GENERATED MCP TOOLS -->

## Ground rules

- One Labcoat Network per machine: `up`/`down`/`reset` manage shared local state.
- `reset -y` wipes the chain — deployments in labcoat.lock become stale;
  redeploy after a reset.
- alkanes-rs is pinned (TOOLCHAIN.md). Never point anything at a branch.
- `labcoat docs --llm` prints the full reference document.
