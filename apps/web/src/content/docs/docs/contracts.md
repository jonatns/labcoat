---
title: Contracts
description: Test, build, deploy, simulate, call, and trace Alkanes contracts.
---

## Test before compiling

```bash
labcoat test
labcoat test my-token
```

Native Rust integration tests run contracts through `labcoat-test`, keeping the
fast feedback loop outside the chain. Each `ContractHarness` has isolated
in-memory storage that persists across calls on that harness. Use
`storage_value` to inspect raw state or `set_storage` to seed a test fixture.

## Build

```bash
labcoat build my-token
```

Compilation writes:

- `build/my-token.wasm`: raw deployable module.
- `build/my-token.wasm.gz`: compressed distribution artifact.
- `build/my-token.abi.json`: opcode, input, and output metadata.

## Deploy

```bash
labcoat deploy my-token --dry-run
labcoat deploy my-token

# Advanced: deploy an external raw artifact without compiling a package
labcoat deploy --wasm /path/to/my-token.wasm --name my-token
```

Package deployment always recompiles the selected contract, uses a Bitcoin
commit/reveal envelope, waits for the create trace, and records the resulting
`block:tx` ID in `labcoat.lock`.

## Simulate and call

```bash
labcoat simulate counter get_count
labcoat call counter increment
labcoat call my-token mint 500
labcoat call registry set_name "Alice Smith"
labcoat call registry set_owner 2:3
```

Named selectors are resolved for the deployed contract. When the generated
local ABI belongs to the exact Wasm recorded in `labcoat.lock`, Labcoat uses it
without an indexer metadata request. If the local build differs, Labcoat warns
and transparently uses deployed metadata. Pass one shell argument per ABI
parameter: decimal or `0x` hexadecimal for `u128`, one UTF-8 argument for
`String`, and decimal `block:tx` for `AlkaneId`. Strings are encoded across as
many little-endian cells as needed. Use a numeric opcode to pass raw cellpack
arguments for `Vec<T>`, custom types, or other advanced calls.

Simulation never broadcasts, but it always executes the deployed contract
against live indexed chain state. Use `labcoat test <package>` to execute an
undeployed local build in the isolated host test runtime. Calls create a
transaction and wait for indexing and decoded execution status.

Use ordinary shell scripts to compose multiple commands. Labcoat does not
currently include a contract script runner.

## Trace

```bash
labcoat trace <txid> --wait
```

Trace output includes decoded create, invoke, return, and revert events across
every protostone in the transaction.
