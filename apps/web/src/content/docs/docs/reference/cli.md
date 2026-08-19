---
title: CLI reference
description: Generated command, option, MCP tool, and protocol reference for Labcoat.
editUrl: false
---

> Generated from Labcoat 0.2.0. Run `pnpm sync:reference` after changing CLI or MCP metadata.

Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts on Labcoat Network, a managed local Bitcoin regtest.

## Install

```bash
curl -fsSL https://labcoat.sh/install | sh
```

## The core loop

```bash
labcoat init my-project
cd my-project && labcoat test
labcoat up
labcoat wallet init
labcoat fund <address> && labcoat mine 1
labcoat build counter
labcoat deploy counter
labcoat abi verify counter
labcoat simulate counter get_count
labcoat call counter increment
labcoat trace <txid> --wait
labcoat down
```

## Output modes

Human-readable output is the default, including when stdout is redirected. Add `--verbose` for raw return data, ABI and artifact metadata, and complete traces. `--color auto|always|never` controls styling and `NO_COLOR` is honored.

Every command accepts `--json` and prints exactly one stable envelope on stdout for agents and automation. Logs and diagnostics go to stderr. When an envelope is printed, inspect its `ok` field instead of the process exit code.

```json
{"ok":true,"command":"status","schema":"labcoat/v1/status","result":{}}
{"ok":false,"command":"deploy","schema":"labcoat/v1/error","error":{"code":"WALLET_MISSING","message":"...","hint":"run `labcoat wallet init` first"}}
```

Secrets never ride argv: use `LABCOAT_WALLET_PASSPHRASE`, `LABCOAT_MNEMONIC`, or mnemonic stdin. Configuration precedence is CLI flags → environment → `labcoat.toml` → defaults.

## Commands

### `labcoat init`

Scaffold a Rust-native Labcoat workspace with a Counter starter

```text
init [NAME]
```

Arguments and options:

- `name` (optional): Project name (prompted for when omitted in an interactive terminal)

### `labcoat new`

Add a minimal contract package and host integration test to this project

```text
new <NAME>
```

Arguments and options:

- `name` (required): Contract package name in kebab-case

### `labcoat test`

Build WASIp1 WebAssembly and run native Rust integration tests

```text
test [OPTIONS] [PACKAGE]
```

Arguments and options:

- `package` (optional): Optional Cargo contract package whose host test should run (with --e2e: a test-name filter instead)
- `e2e` (optional): Run tests/e2e.rs against Labcoat Network: reset the chain, apply alkanes.hcl, then execute the ignored e2e tests Values: `true`, `false`.
- `no_reset` (optional): With --e2e: keep the current chain state instead of resetting Values: `true`, `false`.

### `labcoat up`

Prepare this CLI release's exact runtime bundle and boot Labcoat Network

```text
up [OPTIONS]
```

Arguments and options:

- `no_download` (optional): Skip runtime bundle verification and download Values: `true`, `false`.
- `ci` (optional): CI mode: wait (bounded) for full readiness, then emit the machine-readable endpoint manifest; non-zero exit if the stack never becomes ready Values: `true`, `false`.

### `labcoat down`

Stop all Labcoat Network services

```text
down
```

### `labcoat status`

Show Labcoat Network status (services, block height, mempool)

```text
status
```

### `labcoat mine`

Mine blocks on Labcoat Network

```text
mine [OPTIONS] [COUNT]
```

Arguments and options:

- `count` (optional): Number of blocks
- `address` (optional): Address to mine to (defaults to the dev address)

### `labcoat fund`

Send BTC from the dev wallet to an address

```text
fund <ADDRESS> [AMOUNT]
```

Arguments and options:

- `address` (required)
- `amount` (optional): Amount in BTC

### `labcoat logs`

Show recent service logs

```text
logs [OPTIONS]
```

Arguments and options:

- `service` (optional): Filter to the Qubitcoin service (qubitcoind) Values: `qubitcoind`.
- `limit` (optional): Max entries

### `labcoat reset`

Stop services and wipe all chain/index data

```text
reset [OPTIONS]
```

Arguments and options:

- `yes` (optional): Skip the confirmation prompt Values: `true`, `false`.

### `labcoat snapshot`

Snapshot the Labcoat Network data directory (stops services first)

```text
snapshot [OPTIONS] [NAME]
```

Arguments and options:

- `name` (optional)
- `list` (optional): List existing snapshots Values: `true`, `false`.

### `labcoat restore`

Restore a Labcoat Network snapshot (stops services first)

```text
restore <NAME>
```

Arguments and options:

- `name` (required)

### `labcoat binaries`

Inspect (and with --download, repair) this CLI release's runtime bundle

```text
binaries [OPTIONS]
```

Arguments and options:

- `download` (optional): Values: `true`, `false`.

### `labcoat wallet`

Wallet management (keystore at --wallet-file)

```text
wallet <COMMAND>
```

#### `labcoat wallet init`

Create (or load) the project wallet. Mnemonic is read from LABCOAT_MNEMONIC or — with --mnemonic-stdin — from stdin; never argv

```text
init [OPTIONS]
```

Arguments and options:

- `mnemonic_stdin` (optional): Read the mnemonic from stdin (one line) Values: `true`, `false`.
- `show_mnemonic` (optional): Include a freshly generated mnemonic in machine-readable output (--json / MCP). Interactive terminal output always shows it — that is the one chance to write it down Values: `true`, `false`.

#### `labcoat wallet addresses`

Show receive addresses

```text
addresses [OPTIONS]
```

Arguments and options:

- `count` (optional)

#### `labcoat wallet utxos`

Show spendable UTXOs

```text
utxos
```

#### `labcoat wallet sign-psbt`

Sign a PSBT file with this wallet's keys (the offline half of the psbt-file signer workflow)

```text
sign-psbt [OPTIONS] --in <INPUT>
```

Arguments and options:

- `input` (required): Unsigned PSBT file (base64 or hex)
- `output` (optional): Output path (defaults to `<in stem>.signed.psbt`)

#### `labcoat wallet sign-digest`

Sign a 32-byte application digest with the tweaked key controlling an owned P2TR address

```text
sign-digest --address <ADDRESS> --digest <DIGEST>
```

Arguments and options:

- `address` (required)
- `digest` (required): Exactly 32 bytes as lowercase or uppercase hexadecimal

### `labcoat build`

Build Cargo contract packages into build/<package>.{wasm,wasm.gz,abi.json}

```text
build [OPTIONS] [PACKAGE]
```

Arguments and options:

- `package` (optional): Optional Cargo package name (omitting it builds every contract)
- `out_dir` (optional): Output directory

### `labcoat abi`

Fetch or verify Wasm-exported contract ABI metadata

```text
abi <COMMAND>
```

#### `labcoat abi fetch`

Fetch ABI metadata from a deployed contract's __meta export

```text
fetch [OPTIONS] <CONTRACT>
```

Arguments and options:

- `contract` (required): Contract name from labcoat.lock, or a raw block:tx id
- `out` (optional): Write the exact ABI bytes to a file

#### `labcoat abi verify`

Compare deployed ABI metadata with a locally built contract

```text
verify [OPTIONS] <CONTRACT>
```

Arguments and options:

- `contract` (required): Contract name from labcoat.lock, or a raw block:tx id
- `package` (optional): Local Cargo contract package (required for raw ids or renamed deployments)

### `labcoat deploy`

Build and deploy a contract package, or deploy an explicit raw Wasm

```text
deploy [OPTIONS] [PACKAGE]
```

Arguments and options:

- `package` (optional): Exact Cargo contract package name
- `wasm` (optional): Explicit path to a raw .wasm artifact (skips compilation)
- `name` (optional): Contract name for --wasm deployments (defaults to file stem)
- `args` (optional): Constructor args, one per ABI constructor parameter (raw u128 / 0x-hex cellpack values when the artifact exposes no ABI constructor)
- `reserve` (optional): Deploy to reserved number N (cellpack target [3,N]) instead of the next free id ([1,0])
- `inputs` (optional): Extra transaction inputs: comma-separated alkanes `block:tx:amount` (amount 0 means all) or bitcoin `B:sats`
- `to` (optional): Recipient address for the protostone outputs (defaults to the wallet's primary address)
- `pointer` (optional): Protostone pointer target: vN (physical output) or pN (protostone)
- `refund` (optional): Protostone refund target (defaults to the pointer target)
- `edicts` (optional): Edict `block:tx:amount:target` appended to the protostone (repeatable)
- `dry_run` (optional): Validate inputs and show what would happen without broadcasting Values: `true`, `false`.

### `labcoat call`

Execute a state-changing call on a deployed contract

```text
call [OPTIONS] <CONTRACT> <SELECTOR> [ARGS]...
```

Arguments and options:

- `contract` (required): Contract: labcoat.lock name or block:tx alkanes id
- `selector` (required): Exact ABI method name or decimal opcode
- `args` (optional): One typed value per ABI parameter, or raw cellpack args for numeric opcodes
- `inputs` (optional): Extra transaction inputs: comma-separated alkanes `block:tx:amount` (amount 0 means all) or bitcoin `B:sats`
- `to` (optional): Recipient address for the protostone outputs (defaults to the wallet's primary address)
- `pointer` (optional): Protostone pointer target: vN (physical output) or pN (protostone)
- `refund` (optional): Protostone refund target (defaults to the pointer target)
- `edicts` (optional): Edict `block:tx:amount:target` appended to the protostone (repeatable)
- `dry_run` (optional): Validate inputs and show what would happen without broadcasting Values: `true`, `false`.

### `labcoat exchange`

Atomically exchange one wallet's Alkane asset for another wallet's asset

```text
exchange --seller-wallet-file <SELLER_WALLET_FILE> <OFFERED> <OFFERED_AMOUNT> <PAYMENT> <PAYMENT_AMOUNT>
```

Arguments and options:

- `offered` (required): Asset sold by the seller: labcoat.lock name or block:tx id
- `offered_amount` (required): Complete offered quantity delivered to the buyer
- `payment` (required): Asset paid by the buyer: labcoat.lock name or block:tx id
- `payment_amount` (required): Complete payment quantity delivered to the seller
- `seller_wallet_file` (required): Seller keystore; --wallet-file is the buyer keystore

### `labcoat exchange-plan`

Build an owner-partitioned exchange plan and unsigned PSBT

```text
exchange-plan [OPTIONS] --plan-out <PLAN_OUT> --psbt-out <PSBT_OUT> [OFFERED] [OFFERED_AMOUNT] [PAYMENT] [PAYMENT_AMOUNT]
```

Arguments and options:

- `offered` (optional): Asset sold by the seller: labcoat.lock name or block:tx id
- `offered_amount` (optional): Complete offered quantity delivered to the buyer
- `payment` (optional): Asset paid by the buyer: labcoat.lock name or block:tx id
- `payment_amount` (optional): Complete payment quantity delivered to the seller
- `request` (optional): Exchange request file (version 1 JSON, e.g. from a generated web client); replaces the positional assets and address options
- `seller_address` (optional)
- `buyer_address` (optional)
- `plan_out` (required)
- `psbt_out` (required)

### `labcoat exchange-settle`

Validate a buyer-signed exchange PSBT, sign as seller, and optionally broadcast

```text
exchange-settle [OPTIONS] --plan <PLAN> --psbt <PSBT> --seller-wallet-file <SELLER_WALLET_FILE>
```

Arguments and options:

- `plan` (required)
- `psbt` (required)
- `seller_wallet_file` (required)
- `broadcast` (optional): Values: `true`, `false`.

### `labcoat plan`

Reconcile the deployment manifest against the chain and show pending actions

```text
plan [OPTIONS]
```

Arguments and options:

- `manifest` (optional): Manifest path (default alkanes.hcl)

### `labcoat apply`

Execute the deployment manifest's pending actions

```text
apply [OPTIONS]
```

Arguments and options:

- `manifest` (optional): Manifest path (default alkanes.hcl)
- `broadcast` (optional): Broadcast the pending transactions (without this flag apply only shows the plan) Values: `true`, `false`.

### `labcoat simulate`

Simulate a deployed contract against live indexed chain state

```text
simulate <CONTRACT> <SELECTOR> [ARGS]...
```

Arguments and options:

- `contract` (required): Contract: labcoat.lock name or block:tx alkanes id
- `selector` (required): Exact ABI method name or decimal opcode
- `args` (optional): One typed value per ABI parameter, or raw cellpack args for numeric opcodes

### `labcoat balance`

Alkanes token balances held by an address

```text
balance <ADDRESS>
```

Arguments and options:

- `address` (required): Bitcoin address to query

### `labcoat trace`

Decoded protostone traces for a transaction

```text
trace [OPTIONS] <TXID>
```

Arguments and options:

- `txid` (required)
- `wait` (optional): Poll until the trace is available Values: `true`, `false`.

### `labcoat generate`

Generate typed application artifacts from labcoat.lock and built ABIs

```text
generate <COMMAND>
```

#### `labcoat generate web`

Emit a self-contained TypeScript browser read client: network manifest, typed ABI descriptors, and a fetch-based client for indexed height, Alkanes balances, and ABI-typed simulate calls. Reads labcoat.lock and built ABIs only — no network access

```text
web [OPTIONS]
```

Arguments and options:

- `out_dir` (optional): Output directory for the generated TypeScript module tree
- `build_dir` (optional): Directory containing <package>.abi.json build artifacts

### `labcoat lock`

labcoat.lock utilities

```text
lock <COMMAND>
```

#### `labcoat lock show`

Show the lockfile

```text
show
```

### `labcoat state`

Durable deployment state (version-2, per environment)

```text
state <COMMAND>
```

#### `labcoat state list`

List resources and active instances in this environment's durable state

```text
list
```

#### `labcoat state show`

Show one resource's active instance (with --history, every instance)

```text
show [OPTIONS] <RESOURCE>
```

Arguments and options:

- `resource` (required): Resource address ("contract.counter") or bare contract name
- `history` (optional): Include the full append-only instance history Values: `true`, `false`.

#### `labcoat state migrate`

Create version-2 durable state from the v1 labcoat.lock ledger, backing the ledger up first. labcoat.lock stays in place as the active-address book

```text
migrate
```

### `labcoat mcp`

Model Context Protocol server (agent integration)

```text
mcp <COMMAND>
```

#### `labcoat mcp serve`

Serve MCP over stdio (newline-delimited JSON-RPC)

```text
serve
```

### `labcoat docs`

Print documentation

```text
docs [OPTIONS]
```

Arguments and options:

- `llm` (optional): Emit the full command reference + protocol cheatsheet as one LLM-ready markdown document Values: `true`, `false`.

### `labcoat doctor`

Diagnose the environment (toolchain, ports, binaries, project state)

```text
doctor
```

## MCP mode

`labcoat mcp serve` exposes the same operations over stdio using MCP protocol version `2024-11-05`.

| Tool | Description |
|---|---|
| `network_up` | Boot Labcoat Network using the exact runtime bundle for this CLI release. Returns service status and the endpoint manifest. |
| `network_down` | Stop all Labcoat Network services. |
| `network_status` | Labcoat Network service health, block height, and mempool size. |
| `network_mine` | Mine blocks on Labcoat Network. |
| `network_fund` | Send BTC from the Labcoat Network faucet wallet to an address. |
| `network_reset` | Stop services and wipe all Labcoat Network chain data. |
| `network_logs` | Recent Labcoat Network service logs. |
| `wallet_init` | Create or load the project wallet keystore. Optional mnemonic (else generated). Generated mnemonics are redacted from the response unless showMnemonic is true. |
| `wallet_addresses` | Wallet receive addresses per script type. |
| `wallet_utxos` | Spendable wallet UTXOs. |
| `build` | Build Cargo contract packages and extract their Wasm-exported ABIs. |
| `test` | Build every contract for WASIp1 and run host integration tests; the first build may take several minutes. |
| `abi_fetch` | Fetch ABI metadata from the in-process Alkanes indexer. |
| `abi_verify` | Compare a deployed ABI with a locally built contract package. |
| `deploy` | Build and deploy an exact Cargo contract package, or deploy an explicit raw Wasm. Provide exactly one of package or wasm. |
| `call` | Execute a state-changing contract call and wait for its trace. |
| `exchange_plan` | Build an owner-partitioned atomic exchange plan and return its base64 PSBT. |
| `exchange_settle` | Validate a buyer-signed PSBT, sign seller inputs, and optionally broadcast. broadcast must be true to transact. |
| `simulate` | Simulate a deployed contract against live indexed chain state (no transaction). |
| `trace` | Decoded protostone traces for a transaction. |
| `balance` | Alkanes token balances held by an address. |
| `plan` | Reconcile the alkanes.hcl deployment manifest against labcoat.lock and chain state; shows pending actions without loading a signer. |
| `apply` | Execute the deployment manifest's pending actions. Requires broadcast: true to transact; otherwise returns the plan. |

## Error codes

| Code | Meaning | Recovery |
|---|---|---|
| `LABCOAT_NETWORK_ERROR` | a Labcoat Network operation failed | run `labcoat status` and inspect `labcoat logs` |
| `CONFIG_INVALID` | configuration is invalid | run `labcoat doctor` |
| `WALLET_MISSING` | the project wallet does not exist | run `labcoat wallet init` |
| `WALLET_LOCKED` | the keystore could not be unlocked | set `LABCOAT_WALLET_PASSPHRASE` |
| `WALLET_ERROR` | wallet metadata, ownership, or signing failed | inspect the wallet, PSBT prevouts, and expected derivation path |
| `SIGNER_UNSUPPORTED` | the selected signer lacks a required capability | use the keystore signer or a compatible PSBT signer |
| `SIGNER_TIMEOUT` | an external signer did not return a PSBT in time | sign the request file or raise `LABCOAT_PSBT_TIMEOUT_SECS` |
| `SIGNER_MISMATCH` | external signer output does not match the requested transaction | sign the exact PSBT without changing inputs or outputs |
| `EXCHANGE_PLAN_INVALID` | exchange terms or fixed output layout are invalid | rebuild the exchange plan from current wallet state |
| `EXCHANGE_PLAN_MISMATCH` | the supplied PSBT differs from its content-addressed plan | use the PSBT emitted by `labcoat exchange-plan` |
| `EXCHANGE_INPUT_OWNERSHIP` | an exchange input is unsafe, ambiguous, or owned by the wrong party | use clean P2TR inputs containing only the participant's required asset |
| `EXCHANGE_ASSET_UNSAFE` | an exchange input or output contains an unrelated or misrouted Alkane | use single-asset owner inputs and rebuild the exchange plan |
| `EXCHANGE_SELLER_DEBIT` | the transaction would consume seller bitcoin value | rebuild with buyer-funded outputs and fees |
| `EXCHANGE_SIGNATURE_MISSING` | a required buyer or seller signature is absent | sign the PSBT with the expected participant wallet |
| `EXCHANGE_SIGNATURE_INVALID` | an exchange input signature failed verification | discard the PSBT and recreate the plan |
| `EXCHANGE_SIGHASH_UNSUPPORTED` | an exchange signature is not Taproot SIGHASH_DEFAULT | sign the complete unchanged transaction with SIGHASH_DEFAULT |
| `EXCHANGE_NETWORK_MISMATCH` | the live chain instance differs from the exchange plan | discard stale plans after a network reset |
| `EXCHANGE_TIP_STALE` | the observed planning tip is no longer in the active chain | rebuild the plan after the reorganization |
| `EXCHANGE_INPUT_SPENT` | a planned input has already been spent | rebuild the plan with current UTXOs |
| `RPC_UNREACHABLE` | the configured Qubitcoin endpoint cannot be reached | run `labcoat status` |
| `INDEXER_LAG` | indexed height did not catch chain height | inspect `qubitcoind` logs |
| `INSUFFICIENT_FUNDS` | spendable BTC cannot cover the operation | fund the wallet and mine a block |
| `EXECUTION_REVERT` | the contract explicitly reverted | inspect the revert reason and trace |
| `TRACE_TIMEOUT` | a decoded trace did not arrive in time | retry `labcoat trace --wait` |
| `ENVELOPE_INVALID` | an Alkanes transaction envelope is invalid | check the contract and arguments |
| `COMPILE_FAILED` | Rust or WebAssembly compilation failed | read stderr and run `labcoat doctor` |
| `PACKAGE_NOT_FOUND` | the requested Cargo contract package was not discovered | run `labcoat build` or pass a package listed in the error |
| `ABI_MISMATCH` | local and deployed __meta output differ | build the deployed source revision and verify the contract ID |
| `CONTRACT_NOT_FOUND` | a contract name or ID could not be resolved | run `labcoat lock show` |
| `LOCKFILE_INVALID` | labcoat.lock exists but cannot be read or parsed | repair the JSON, or delete labcoat.lock to start a fresh ledger |
| `MANIFEST_INVALID` | the alkanes.hcl deployment manifest failed to parse or validate | fix the reported block; references are `alkane.<name>.<field>` / `contract.<name>.<field>`, and conditionals, loops, and functions are not supported |
| `STATE_INVALID` | a .labcoat/state file (the apply call journal or durable environment state) cannot be read or parsed | for the journal: repair or delete the file (calls may re-execute); for durable state: restore backups/state.json.prev |
| `STATE_MISSING` | no version-2 durable state exists for this environment | run `labcoat state migrate` |
| `STATE_UNSUPPORTED` | the durable state schema version is not supported by this labcoat | upgrade labcoat, or restore a backup from .labcoat/state/<environment>/backups |
| `STATE_LOCKED` | another labcoat process holds this environment's lease | wait for the other process; a crashed holder releases the lease automatically |
| `STATE_CHAIN_MISMATCH` | durable state belongs to a different chain instance (e.g. before a `labcoat reset`) | archive .labcoat/state/<environment> or use a different --environment |
| `STATE_CONFLICT` | durable state changed underneath the command, or already exists where none may | re-run against current state (`labcoat state list`) |
| `APPLY_BLOCKED` | an action cannot proceed without manual intervention | read the action's detail in `labcoat plan` |
| `TOOLKIT_ERROR` | the underlying contract toolkit failed | read the error hint |
| `BINARY_CRASH` | a Labcoat Network service exited | inspect `labcoat logs` |

## Protocol cheatsheet

- **Cellpack**: [block, tx, opcode, ...args] as u128 values; strings up to 16 bytes are packed little-endian.
- **Deploy**: Targets [1, 0]; raw Wasm is compressed inside a taproot commit/reveal envelope.
- **Protostone outputs**: Trace output for protostone i is transaction.output.len + 1 + i; Labcoat computes it automatically.
- **Synchronization**: State-changing operations wait until the Alkanes index reaches chain height before reading fresh state.
- **labcoat.lock**: Per-network deployment ledger mapping names to Alkanes IDs, Wasm hashes, transaction IDs, and status. Remains the active-address book; `labcoat state migrate` regenerates it from durable state.
- **Durable state**: .labcoat/state/<environment>/state.json is the version-2 per-environment operational state (lineage, serial, chain identity, append-only instance history), created by `labcoat state migrate` and guarded by an OS lease (state.lock). Deploys append instances when it exists and refuse a reset or foreign chain before broadcasting. The flat .labcoat/state/<network>.json apply call journal is separate.
- **Contract ABI**: Named calls use the generated local ABI when its Wasm hash matches labcoat.lock; otherwise they use deployed __meta metadata. Execution always targets deployed code, and numeric opcodes remain the raw cellpack escape hatch.
- **Generated web client**: `labcoat generate web` derives a self-contained TypeScript module tree (manifest, typed ABI descriptors, fetch read client) from labcoat.lock and built ABIs, offline. The client is read-only — indexed height, Alkanes balances, ABI-typed simulate — and holds no keys; browsers reach the unified JSON-RPC endpoint through the app's own dev proxy or rewrite.

## alkanes-rs pin

All alkanes-rs code paths are pinned to commit `714843c416e2ab57352a33f05b8461cf3f540f5a` on the `main` branch. See TOOLCHAIN.md before changing the pin.
