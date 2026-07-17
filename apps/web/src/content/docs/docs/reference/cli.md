---
title: CLI reference
description: Generated command, option, MCP tool, and protocol reference for Labcoat.
editUrl: false
---

> Generated from Labcoat 0.1.0. Run `pnpm sync:reference` after changing CLI or MCP metadata.

Rust-native toolkit for building, testing, deploying, and operating Alkanes smart contracts on Bitcoin.

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
labcoat compile counter
labcoat deploy counter
labcoat abi verify counter
labcoat call counter <opcode> [args...]
labcoat trace <txid> --wait
labcoat down
```

## JSON envelopes (agent mode)

Every command accepts `--json` and prints exactly one envelope on stdout. Logs go to stderr. When an envelope is printed, inspect its `ok` field instead of the process exit code.

```json
{"ok":true,"command":"status","schema":"labcoat/v1/status","result":{}}
{"ok":false,"command":"deploy","schema":"labcoat/v1/error","error":{"code":"WALLET_MISSING","message":"...","hint":"run `labcoat wallet init` first"}}
```

Secrets never ride argv: use `LABCOAT_WALLET_PASSPHRASE`, `LABCOAT_MNEMONIC`, or mnemonic stdin. Configuration precedence is CLI flags → environment → `labcoat.toml` → defaults.

## Commands

### `labcoat init`

Scaffold a Rust-first Labcoat workspace with a Counter starter

```text
init [OPTIONS] [DIRECTORY]
```

Arguments and options:

- `directory` (optional): Destination directory (defaults to the current directory)
- `force` (optional): Overlay the template onto a non-empty directory Values: `true`, `false`.

### `labcoat new`

Add a minimal contract package and host integration test to this project

```text
new <NAME>
```

Arguments and options:

- `name` (required): Contract package name in kebab-case

### `labcoat test`

Compile WASIp1 WebAssembly and run native Rust integration tests

```text
test [PACKAGE]
```

Arguments and options:

- `package` (optional): Optional Cargo contract package whose host test should run

### `labcoat up`

Download binaries if needed and boot the full devnet stack

```text
up [OPTIONS]
```

Arguments and options:

- `no_download` (optional): Skip the binary download/check step Values: `true`, `false`.
- `ci` (optional): CI mode: wait (bounded) for full readiness, then emit the machine-readable endpoint manifest; non-zero exit if the stack never becomes ready Values: `true`, `false`.

### `labcoat down`

Stop all devnet services

```text
down
```

### `labcoat status`

Show devnet status (services, block height, mempool)

```text
status
```

### `labcoat mine`

Mine blocks on the devnet

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

- `service` (optional): Filter to one service (bitcoind, metashrew, ord, esplora, espo, jsonrpc)
- `limit` (optional): Max entries

### `labcoat reset`

Stop services and wipe all chain/index data

```text
reset [OPTIONS]
```

Arguments and options:

- `yes` (optional): Skip the confirmation prompt Values: `true`, `false`.

### `labcoat snapshot`

Snapshot the devnet data directory (stops services first)

```text
snapshot [OPTIONS] [NAME]
```

Arguments and options:

- `name` (optional)
- `list` (optional): List existing snapshots Values: `true`, `false`.

### `labcoat restore`

Restore a devnet snapshot (stops services first)

```text
restore <NAME>
```

Arguments and options:

- `name` (required)

### `labcoat binaries`

Check (and with --download, fetch) service binaries

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

### `labcoat compile`

Compile Cargo contract packages to build/<package>.{wasm,wasm.gz,abi.json}

```text
compile [OPTIONS] [PACKAGE]
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

Compile and deploy a contract package, or deploy an explicit raw Wasm

```text
deploy [OPTIONS] [PACKAGE]
```

Arguments and options:

- `package` (optional): Exact Cargo contract package name
- `wasm` (optional): Explicit path to a raw .wasm artifact (skips compilation)
- `name` (optional): Contract name for --wasm deployments (defaults to file stem)
- `args` (optional): Constructor cellpack args (u128 / 0x-hex / short strings)
- `dry_run` (optional): Validate inputs and show what would happen without broadcasting Values: `true`, `false`.

### `labcoat call`

Execute a state-changing call on a deployed contract

```text
call [OPTIONS] <CONTRACT> <OPCODE> [ARGS]...
```

Arguments and options:

- `contract` (required): Contract: labcoat.lock name or block:tx alkanes id
- `opcode` (required): Opcode number
- `args` (optional): Cellpack args (u128 / 0x-hex / short strings)
- `dry_run` (optional): Validate inputs and show what would happen without broadcasting Values: `true`, `false`.

### `labcoat simulate`

Read-only simulation of a contract call

```text
simulate <CONTRACT> <OPCODE> [ARGS]...
```

Arguments and options:

- `contract` (required): Contract: labcoat.lock name or block:tx alkanes id
- `opcode` (required): Opcode number
- `args` (optional): Cellpack args (u128 / 0x-hex / short strings)

### `labcoat trace`

Decoded protostone traces for a transaction

```text
trace [OPTIONS] <TXID>
```

Arguments and options:

- `txid` (required)
- `wait` (optional): Poll until the trace is available Values: `true`, `false`.

### `labcoat lock`

labcoat.lock utilities

```text
lock <COMMAND>
```

#### `labcoat lock migrate`

Migrate a legacy deployments/manifest.json into labcoat.lock

```text
migrate
```

#### `labcoat lock show`

Show the lockfile

```text
show
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
| `devnet_up` | Boot the full Alkanes devnet stack (downloads binaries when missing). Returns service status and the endpoint manifest. |
| `devnet_down` | Stop all devnet services. |
| `devnet_status` | Devnet service health, block height, and mempool size. |
| `devnet_mine` | Mine blocks on the devnet. |
| `devnet_fund` | Send BTC from the devnet faucet wallet to an address. |
| `devnet_reset` | Stop services and wipe all devnet chain data. |
| `devnet_logs` | Recent devnet service logs. |
| `wallet_init` | Create or load the project wallet keystore. Optional mnemonic (else generated). |
| `wallet_addresses` | Wallet receive addresses per script type. |
| `wallet_utxos` | Spendable wallet UTXOs. |
| `compile` | Compile Cargo contract packages and extract their Wasm-exported ABIs. |
| `test` | Build every contract for WASIp1 and run host integration tests; the first build may take several minutes. |
| `abi_fetch` | Fetch ABI metadata from a deployed contract through Metashrew. |
| `abi_verify` | Compare a deployed ABI with a locally built contract package. |
| `deploy` | Compile and deploy an exact Cargo contract package, or deploy an explicit raw Wasm. Provide exactly one of package or wasm. |
| `call` | Execute a state-changing contract call and wait for its trace. |
| `simulate` | Read-only simulation of a contract call (no transaction). |
| `trace` | Decoded protostone traces for a transaction. |

## Error codes

| Code | Meaning | Recovery |
|---|---|---|
| `CONFIG_INVALID` | configuration is invalid | run `labcoat doctor` |
| `WALLET_MISSING` | the project wallet does not exist | run `labcoat wallet init` |
| `WALLET_LOCKED` | the keystore could not be unlocked | set `LABCOAT_WALLET_PASSPHRASE` |
| `RPC_UNREACHABLE` | the configured gateway cannot be reached | run `labcoat status` |
| `INDEXER_LAG` | indexed height did not catch chain height | inspect metashrew logs |
| `INSUFFICIENT_FUNDS` | spendable BTC cannot cover the operation | fund the wallet and mine a block |
| `EXECUTION_REVERT` | the contract explicitly reverted | inspect the revert reason and trace |
| `TRACE_TIMEOUT` | a decoded trace did not arrive in time | retry `labcoat trace --wait` |
| `ENVELOPE_INVALID` | an Alkanes transaction envelope is invalid | check the contract and arguments |
| `COMPILE_FAILED` | Rust or WebAssembly compilation failed | read stderr and run `labcoat doctor` |
| `PACKAGE_NOT_FOUND` | the requested Cargo contract package was not discovered | run `labcoat compile` or pass a package listed in the error |
| `ABI_MISMATCH` | local and deployed __meta output differ | compile the deployed source revision and verify the contract ID |
| `CONTRACT_NOT_FOUND` | a contract name or ID could not be resolved | run `labcoat lock show` |
| `TOOLKIT_ERROR` | the underlying contract toolkit failed | read the error hint |
| `BINARY_CRASH` | a managed devnet service exited | inspect `labcoat logs` |

## Protocol cheatsheet

- **Cellpack**: [block, tx, opcode, ...args] as u128 values; strings up to 16 bytes are packed little-endian.
- **Deploy**: Targets [1, 0]; raw Wasm is compressed inside a taproot commit/reveal envelope.
- **Protostone outputs**: Trace output for protostone i is transaction.output.len + 1 + i; Labcoat computes it automatically.
- **Synchronization**: State-changing operations wait until the Alkanes index reaches chain height before reading fresh state.
- **labcoat.lock**: Per-network deployment ledger mapping names to Alkanes IDs, Wasm hashes, transaction IDs, and status.
- **Contract ABI**: Compile and test execute the Wasm __meta export locally; abi fetch and abi verify use Metashrew only for explicit deployed-bytecode inspection.

## alkanes-rs pin

All alkanes-rs code paths are pinned to commit `5b7f43567b828d0bb7b8907ce78fa0242943c54d` on the `develop` branch. See TOOLCHAIN.md before changing the pin.
