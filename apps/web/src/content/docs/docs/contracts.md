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
Generated projects obtain the harness from this repository's matching
`cli-vX.Y.Z` Git tag, so the CLI and test API advance as one release.

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
labcoat plan
labcoat deploy --broadcast

# One step of deploy.rs
labcoat deploy my-token --broadcast

# Advanced: a prebuilt raw artifact that deploy.rs does not declare
labcoat deploy --wasm /path/to/my-token.wasm --name my-token --broadcast
```

A project deploys what its `deploy.rs` declares, and a new project's declares
its Counter contract. Add one with `d.contract("my-token", "my-token")`.
Deploy recompiles every package the script references, uses a Bitcoin
commit/reveal envelope, waits for the create trace, and records each
`block:tx` ID in `labcoat.lock`. A step runs once per network, so deploying
again does nothing until a step is pending.

Deploy needs `--broadcast` to sign and send, or `--dry-run` to preview;
`labcoat plan` is the same preview. See
[Deployment scripts](/docs/deployments/) for references between contracts,
existing Alkanes, and upgrades.

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

Token contracts should follow the Alkanes metadata convention: opcode `99`
returns the name and `100` the symbol, both as UTF-8 text, or leave both
unimplemented. Balance views ask every held token for these values, and a
contract that answers with other bytes makes `labcoat balance`, exchanges, and
`call --inputs` fail for every address holding it.

Use ordinary shell scripts to compose multiple commands. Labcoat does not
currently include a contract script runner.

## Trace

```bash
labcoat trace <txid> --wait
```

Trace output includes decoded create, invoke, return, and revert events across
every protostone in the transaction.
