---
title: CLI reference
description: Generated command, option, MCP tool, and protocol reference for Labcoat.
editUrl: false
---

> Generated from Labcoat 0.4.0. Run `pnpm sync:reference` after changing CLI or MCP metadata.

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
labcoat plan
labcoat deploy
labcoat abi verify counter
labcoat simulate counter get_count
labcoat call counter increment
labcoat trace <txid> --wait
labcoat down
```

## Output modes

Human-readable output is the default, including when stdout is redirected. Add `--verbose` for raw return data, ABI and artifact metadata, and complete traces. `--color auto|always|never` controls styling and `NO_COLOR` is honored.

Every command accepts `--json` and prints exactly one stable envelope on stdout for agents and automation. Logs and diagnostics go to stderr. When an envelope is printed, inspect its `ok` field instead of the process exit code. Two commands also use the exit code: `labcoat up --ci` exits non-zero unless Labcoat Network and its funded wallet are ready, and `labcoat doctor` exits non-zero when a check fails.

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
new [OPTIONS] <NAME>
```

Arguments and options:

- `name` (required): Contract package name in kebab-case
- `template` (optional): Vendor a standard upgradeable contract instead of a minimal one: auth-token, upgradeable, upgradeable-beacon, or beacon-proxy

### `labcoat upgrade`

Align this project's alkanes-rs, labcoat-test, and Rust toolchain pins with the installed CLI

```text
upgrade [OPTIONS]
```

Arguments and options:

- `dry_run` (optional): Show the pins that would change without writing anything Values: `true`, `false`.

### `labcoat test`

Build WASIp1 WebAssembly and run native Rust integration tests

```text
test [OPTIONS] [PACKAGE]
```

Arguments and options:

- `package` (optional): Optional Cargo contract package whose host test should run (with --e2e: a test-name filter instead)
- `e2e` (optional): Run tests/e2e.rs on the project's own e2e network, apart from the one `labcoat up` starts: start it on a fresh chain, run the deploy.rs deployment, execute the ignored e2e tests, and stop it Values: `true`, `false`.
- `no_reset` (optional): With --e2e: keep the chain the previous e2e run left instead of starting a fresh one Values: `true`, `false`.

### `labcoat up`

Prepare this CLI release's exact runtime bundle and boot Labcoat Network

```text
up [OPTIONS]
```

Arguments and options:

- `no_download` (optional): Skip runtime bundle verification and download Values: `true`, `false`.
- `ci` (optional): CI mode: wait (bounded) for full readiness, then print one JSON envelope with the endpoint manifest, including the dev signer; exits non-zero unless the network and its funded wallet are ready Values: `true`, `false`.

### `labcoat down`

Stop all Labcoat Network services

```text
down
```

### `labcoat status`

Show Labcoat Network status (services, block height, mempool, dev signer)

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

- `service` (optional): Filter to one service (node) Values: `node`.
- `limit` (optional): Max entries

### `labcoat reset`

Stop services and wipe all chain/index data

```text
reset [OPTIONS]
```

Arguments and options:

- `yes` (optional): Skip the confirmation prompt (the global --yes also skips it) Values: `true`, `false`.

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

Wallet management

```text
wallet <COMMAND>
```

#### `labcoat wallet init`

Create (or load) the selected network's wallet; Labcoat Network already has one after `labcoat up`. Mnemonic is read from LABCOAT_MNEMONIC or — with --mnemonic-stdin — from stdin; never argv

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
- `out_dir` (optional): Output directory, relative to the working directory [default: build in the project root]

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

### `labcoat plan`

Run deploy.rs and show which deployment steps are pending, complete, or in conflict

```text
plan [OPTIONS]
```

Arguments and options:

- `replace` (optional): Run a step that already ran again: a contract deploys a new instance, a call executes again (repeatable)

### `labcoat deploy`

Execute the pending deployment steps deploy.rs declares, or one of them by name

```text
deploy [OPTIONS] [STEP]
```

Arguments and options:

- `step` (optional): Step name from deploy.rs (every pending step when omitted)
- `dry_run` (optional): Show what would happen without signing or broadcasting Values: `true`, `false`.
- `replace` (optional): Run a step that already ran again: a contract deploys a new instance, a call executes again (repeatable; runs the whole deployment, so it takes no step name)
- `wasm` (optional): Deploy this raw .wasm artifact outside deploy.rs instead of a step; it is recorded in labcoat.lock only
- `name` (optional): Contract name for --wasm deployments (defaults to file stem)
- `args` (optional): Constructor args for --wasm deployments, one per ABI constructor parameter (raw u128 / 0x-hex cellpack values when the artifact exposes no ABI constructor)
- `reserve` (optional): Deploy a --wasm artifact to reserved number N (cellpack target [3,N]) instead of the next free id ([1,0])
- `inputs` (optional): Extra transaction inputs: comma-separated alkanes `block:tx:amount` (amount 0 means all) or bitcoin `B:sats`
- `to` (optional): Recipient address for the protostone outputs (defaults to the wallet's primary address)
- `pointer` (optional): Protostone pointer target: vN (physical output) or pN (protostone)
- `refund` (optional): Protostone refund target (defaults to the pointer target)
- `edicts` (optional): Edict `block:tx:amount:target` appended to the protostone (repeatable)

### `labcoat import`

Record a contract that already exists on chain as a deployment step, without deploying

```text
import [OPTIONS] <NAME> <ID>
```

Arguments and options:

- `name` (required): Contract step name from deploy.rs
- `id` (required): The deployed contract's block:tx id
- `txid` (optional): Deployment or reveal transaction ID, used to recover auth tokens and other created IDs

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
- `pay` (optional): Add an output paying SATS of bitcoin to ADDRESS in the same transaction (repeatable). Payment outputs follow the outputs --pointer, --refund, and edicts address: the first is v1 unless one of those names a higher output
- `abi` (optional): Resolve the method name against this contract's ABI instead (labcoat.lock name or block:tx id) — for a call through a proxy, name the implementation
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

### `labcoat simulate`

Simulate a deployed contract against live indexed chain state

```text
simulate [OPTIONS] <CONTRACT> <SELECTOR> [ARGS]...
```

Arguments and options:

- `contract` (required): Contract: labcoat.lock name or block:tx alkanes id
- `selector` (required): Exact ABI method name or decimal opcode
- `args` (optional): One typed value per ABI parameter, or raw cellpack args for numeric opcodes
- `abi` (optional): Resolve the method name against this contract's ABI instead (labcoat.lock name or block:tx id) — for a read through a proxy, name the implementation

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

- `out_dir` (optional): Output directory for the generated TypeScript module tree, relative to the working directory [default: generated/web in the project root]
- `build_dir` (optional): Directory containing <package>.abi.json build artifacts, relative to the working directory [default: build in the project root]

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
| `network_up` | Boot Labcoat Network using the exact runtime bundle for this CLI release. Creates and funds the built-in wallet, then returns service status, the endpoint manifest, and the wallet. |
| `network_down` | Stop all Labcoat Network services. |
| `network_status` | Labcoat Network service health, block height, and mempool size. |
| `network_mine` | Mine blocks on Labcoat Network. |
| `network_fund` | Send BTC from the Labcoat Network faucet wallet to an address. |
| `network_reset` | Stop services and wipe all Labcoat Network chain data. |
| `network_logs` | Recent Labcoat Network service logs. |
| `wallet_init` | Create or load the selected network's wallet keystore. Not needed on Labcoat Network, which comes with a funded wallet. Optional mnemonic (else generated). Generated mnemonics are redacted from the response unless showMnemonic is true. |
| `wallet_addresses` | Wallet receive addresses per script type. |
| `wallet_utxos` | Spendable wallet UTXOs. |
| `build` | Build Cargo contract packages and extract their Wasm-exported ABIs. |
| `test` | Build every contract for WASIp1 and run host integration tests; the first build may take several minutes. |
| `abi_fetch` | Fetch ABI metadata from the in-process Alkanes indexer. |
| `abi_verify` | Compare a deployed ABI with a locally built contract package. |
| `call` | Execute a state-changing contract call and wait for its trace. |
| `exchange_plan` | Build an owner-partitioned atomic exchange plan and return its base64 PSBT. |
| `exchange_settle` | Validate a buyer-signed PSBT, sign seller inputs, and optionally broadcast. broadcast must be true to transact. |
| `simulate` | Simulate a deployed contract against live indexed chain state (no transaction). |
| `trace` | Decoded protostone traces for a transaction. |
| `balance` | Alkanes token balances held by an address. |
| `plan` | Run the project's deploy.rs deployment script and reconcile its steps against the network's run file and chain state; shows pending, complete, and conflicting steps without loading a signer. |
| `deploy` | Execute the pending deployment steps deploy.rs declares, or only the step named in step. dryRun: true returns the plan without signing. On Labcoat Network a changed contract, and whatever references it, redeploys with empty storage; on other networks deploy refuses while a step is in conflict. Public networks refuse to sign unless the server was started with --yes. With wasm, deploys that raw Wasm artifact outside deploy.rs instead, recorded in labcoat.lock only. |
| `import` | Record a contract that already exists on chain as a deploy.rs contract step, after comparing its ABI with the local build. Broadcasts nothing. |
| `state_list` | List resources and active instances in this environment's durable deployment state. |
| `state_show` | Show one resource's active instance from durable state; history: true includes every earlier instance. |
| `state_migrate` | Create version-2 durable state from labcoat.lock, backing up the ledger first. Refuses to overwrite existing state. |
| `generate_web` | Write a typed TypeScript browser read client from labcoat.lock and built ABIs (no network access). |

## Error codes

| Code | Meaning | Recovery |
|---|---|---|
| `LABCOAT_NETWORK_ERROR` | a Labcoat Network operation failed | run `labcoat status` and inspect `labcoat logs` |
| `CONFIG_INVALID` | configuration is invalid | run `labcoat doctor` |
| `WALLET_MISSING` | the project wallet does not exist | run `labcoat up` on Labcoat Network, or `labcoat wallet init` for another network |
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
| `RPC_UNREACHABLE` | the configured RPC endpoint cannot be reached | run `labcoat status` |
| `INDEXER_LAG` | indexed height did not catch chain height | run `labcoat logs` |
| `INSUFFICIENT_FUNDS` | spendable BTC cannot cover the operation | fund the wallet and mine a block |
| `EXECUTION_REVERT` | the contract explicitly reverted | inspect the revert reason and trace |
| `TRACE_TIMEOUT` | a decoded trace did not arrive in time | retry `labcoat trace --wait` |
| `ENVELOPE_INVALID` | an Alkanes transaction envelope is invalid | check the contract and arguments |
| `COMPILE_FAILED` | Rust or WebAssembly compilation failed | read stderr and run `labcoat doctor` |
| `TOOLCHAIN_MISSING` | Cargo, a Rust WebAssembly target, a C toolchain for WebAssembly, or protoc is not installed | install the prerequisite the message names, then run `labcoat doctor` |
| `PACKAGE_NOT_FOUND` | the requested Cargo contract package was not discovered | run `labcoat build` or pass a package listed in the error |
| `ABI_MISMATCH` | local and deployed __meta output differ | build the deployed source revision and verify the contract ID |
| `CONTRACT_NOT_FOUND` | a contract name or ID could not be resolved | run `labcoat lock show` |
| `LOCKFILE_INVALID` | labcoat.lock exists but cannot be read or parsed | repair the JSON, or delete labcoat.lock to start a fresh ledger |
| `DEPLOY_SCRIPT_INVALID` | the deploy.rs deployment script is missing, failed to build or run, or declared an invalid graph | fix deploy.rs; `cargo run --example deploy` reproduces a build or runtime failure outside labcoat |
| `DEPLOYMENT_CONFLICT` | a deployment step that already ran no longer matches deploy.rs, or references an instance that was replaced | restore the step, or name it in `labcoat deploy --replace <name>` to deploy a new instance (or run a call again) |
| `DEPLOYMENT_LOCKED` | another process is updating the project's deployments | retry after the other deploy or import finishes |
| `RUN_FILE_INVALID` | deployments/<network>.json, the record of the steps that ran, cannot be read, parsed, or written | restore the file from version control or repair the JSON; deleting it makes every step run again |
| `STATE_INVALID` | the durable environment state under .labcoat/state cannot be read or parsed | restore backups/state.json.prev |
| `STATE_MISSING` | no version-2 durable state exists for this environment | run `labcoat state migrate` |
| `STATE_UNSUPPORTED` | the durable state schema version is not supported by this labcoat | upgrade labcoat, or restore a backup from .labcoat/state/<environment>/backups |
| `STATE_LOCKED` | another labcoat process holds this environment's lease | wait for the other process; a crashed holder releases the lease automatically |
| `STATE_CHAIN_MISMATCH` | durable state belongs to a different chain instance (e.g. before a `labcoat reset`) | archive .labcoat/state/<environment> or use a different --environment |
| `STATE_CONFLICT` | durable state changed underneath the command, or already exists where none may | re-run against current state (`labcoat state list`) |
| `DEPLOYMENT_BLOCKED` | a deployment step cannot proceed without manual intervention (an interrupted broadcast, an occupied reserve, an unreadable setting, a step deployed on its own before the steps it references) | read the step's detail in `labcoat plan` |
| `RPC_REJECTED` | the RPC endpoint refused the request without a JSON-RPC error, typically a hosted endpoint's rate limit | set `SUBFROST_API_KEY` for Subfrost, or wait and retry |
| `TOOLKIT_ERROR` | the underlying contract toolkit failed | read the error hint |
| `BINARY_CRASH` | a Labcoat Network service exited | inspect `labcoat logs` |

## Protocol cheatsheet

- **Cellpack**: [block, tx, opcode, ...args] as u128 values; strings up to 16 bytes are packed little-endian.
- **Deploy**: Targets [1, 0] for the next free id 2:n, or [3, N] for the reserved id 4:N; raw Wasm is compressed inside a taproot commit/reveal envelope. A template clone targets [5, n] (a 2:n template) or [6, n] (a 4:n template) and carries no envelope.
- **Deployment script**: deploy.rs, the project's `deploy` Cargo example target, declares the deployment with the `labcoat` Rust API and never executes it: `labcoat plan` runs the script to read its steps, and only `labcoat deploy` signs. `labcoat deploy <step>` runs one step, once everything it references is deployed. Steps run in declaration order and may reference only earlier steps. d.contract(name, package) deploys once (.reserve(n), .init(method), .arg(name, value), .positional(value)); d.contract_wasm(name, path) deploys a prebuilt wasm; d.clone_of(name, &template) clones deployed code; d.external(name).on(network, block, tx) or .id_everywhere(block, tx) names an existing Alkane; d.call(name, &contract, method) runs once (.arg, .positional, .inputs, .to, .pointer, .refund, .edict, .pay(address, sats) for a bitcoin payment output, .abi_of(&implementation) for a call through a proxy); d.implementation(name, package) is code behind an upgradeable pointer; d.beacon(name, package, &implementation), d.beacon_proxy(name, package, &beacon), and d.proxy(name, package, &implementation) declare the standard upgradeable contracts; d.setting(name, &contract, value).read(method).write(method) keeps a mutable value in sync. Values: handle.id(), .block(), .tx(), .amount(n), contract.created(i), and d.height() + n.
- **Step lifecycles**: Contracts, clones, and calls are pinned: they run once per network, and a later edit (code, target, initializer, arguments) or a replaced dependency is a `conflict` that deploy refuses with DEPLOYMENT_CONFLICT until the step is restored or named in `--replace <name>`, which deploys a new instance and keeps the old one in the record's history. On Labcoat Network deploy replaces changed steps, and whatever references them, without being asked: new id, empty storage. An implementation is replaceable: changed code deploys a new instance without a flag. A setting converges: deploy reads the current value and sends the write call only when it differs. `d.height()` is evaluated once, when the step runs.
- **Run file**: deployments/<network>.json records every step that ran: its definition hash, Alkane id, every Alkane its transaction created, the ids it referenced, the arguments as executed, the transaction id, and earlier instances. It is what makes a step run once; commit it for shared networks. The Labcoat Network file is disposable: `labcoat reset` starts a new chain, and records from an earlier chain instance are ignored.
- **Standard upgradeable contracts**: `labcoat new <name> --template auth-token|upgradeable|upgradeable-beacon|beacon-proxy` vendors the alkanes-rs standard contracts into the project. A proxy or beacon initializes with opcode 0x7fff, which clones the auth token template at 4:65517 and sends one unit to the deployer; upgrading (0x7ffe) spends that token as an input and returns it. A beacon answers 0x7ffd with its current implementation. A fresh chain has no auth token template: deploy the auth-token package at reserve 65517 with `.init(100)` first.
- **Protostone outputs**: A call transaction's outputs are, in order: v0 the protostone recipient (--to, default the wallet's primary address); any higher vN a --pointer, --refund, or --edict names, paying the wallet; the --pay <address>:<sats> payment outputs in the order given; the wallet's change, when there is any; and the OP_RETURN. The first payment is therefore v1 unless a pointer, refund, or edict names a higher output, and payments never renumber the outputs those address. Trace output for protostone i is transaction.output.len + 1 + i; Labcoat computes it automatically from the built transaction.
- **Synchronization**: State-changing operations wait until the Alkanes index reaches chain height before reading fresh state.
- **labcoat.lock**: Per-network deployment ledger mapping names to Alkanes IDs, Wasm hashes, transaction IDs, and status. Remains the active-address book; `labcoat state migrate` regenerates it from durable state.
- **Durable state**: .labcoat/state/<environment>/state.json is the version-2 per-environment operational state (lineage, serial, chain identity, append-only instance history), created by `labcoat state migrate` and guarded by an OS lease (state.lock). Deploys append instances when it exists and refuse a reset or foreign chain before broadcasting. The deployment run file (deployments/<network>.json) is separate.
- **Contract ABI**: Named calls use the generated local ABI when its Wasm hash matches labcoat.lock; otherwise they use deployed __meta metadata. Execution always targets deployed code, and numeric opcodes remain the raw cellpack escape hatch.
- **Generated web client**: `labcoat generate web` derives a self-contained TypeScript module tree (manifest, typed ABI descriptors, fetch client, wallet interface, transaction builder) from labcoat.lock and built ABIs, offline. It writes `generated/web` and reads `build`, both in the project root; a typed `--out-dir` or `--build-dir` is relative to the working directory, and the result reports both as absolute paths. `createClient({ rpc })` returns the client: `contract(name).read.<method>()` is typed from the ABI, returns the decoded value, and throws `ContractRevertError` on a revert (`tryRead` returns it instead); `contract(id, abi)` calls a contract outside labcoat.lock, such as a factory's clone; `at(height)` pins reads to one indexed height; `decodeU128(bytes, offset?)`, `decodeAlkaneId(bytes, offset?)`, and `decodeAlkaneIds(bytes)` decode numbers and Alkane ids a contract packs into a `Vec<u8>` (32 bytes per id) into `bigint` and `{ block, tx }`, and throw `RESPONSE_INVALID` on a length that does not fit. `contract(name).write.<method>(...args, { wallet })` sends the call as a transaction a wallet signs and resolves to `{ txid, fee, payments, wait() }`; `wait()` and `waitForTransaction(txid)` resolve once the Alkanes index has it and throw `ContractRevertError` if it reverted. Write options: `alkanes` (send in), `pay` (bitcoin outputs), `to`, `feeRate`, `simulate` (default true: a call that would revert throws before the wallet is asked). `connect()` returns the dev wallet on Labcoat Network (signed by the dev signer; it mines a block after each broadcast) and otherwise an installed browser wallet (UniSat, Xverse, OKX Wallet, Leather); any object with `name`, `getAccounts`, and `signPsbt` is a wallet. `connect({ approve: true })` asks in the page before each signature and names the contract (`counter (2:5)`); an `approve` function gets the summary, with `contractName` when the call went through a manifest name. A declined request throws `WalletRejectedError`. The client holds no keys and builds the transaction `labcoat call` builds; browsers reach the unified JSON-RPC endpoint through the app's own dev proxy or rewrite, at `http://localhost:<port>` when the dev wallet signs.
- **Dev signer**: While Labcoat Network is up, its JSON-RPC endpoint signs for the built-in wallet, so a page or a headless test needs no wallet extension: `labcoat_dev_addresses` (params `[]`) returns the primary P2TR address with its `internalKey`, `outputKey`, and `scriptPubKey`, and `labcoat_dev_sign_psbt` (params `[psbtBase64, {"finalize": true}]`) signs the wallet's taproot key-path inputs with SIGHASH_DEFAULT, leaves every other input untouched, and returns `psbt`, `signedInputs`, `complete`, and, once every input is final, `hex` and `txid`. Broadcast `hex` with `sendrawtransaction`, then mine it with `generatetoaddress`: nothing is mined automatically. It exists only on Labcoat Network, signs only with the built-in wallet, and answers only callers on this machine (error -32000 otherwise). `labcoat up --ci` and `labcoat status --json` report it as `dev_signer`.

## alkanes-rs pin

All alkanes-rs code paths are pinned to commit `ea38c878931ff4b6dd64dd011962f2203d6e0a35` on the `main` branch. See TOOLCHAIN.md before changing the pin.
