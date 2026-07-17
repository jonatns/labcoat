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
labcoat simulate my-token 99
labcoat call my-token 77 500
```

Arguments may be decimal `u128`, `0x` hexadecimal, or strings up to 16 bytes
packed little-endian. Simulation never broadcasts. Calls create a transaction
and wait for indexing and decoded execution status.

## Trace

```bash
labcoat trace <txid> --wait
```

Trace output includes decoded create, invoke, return, and revert events across
every protostone in the transaction.
