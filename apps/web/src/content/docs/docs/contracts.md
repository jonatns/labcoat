---
title: Contracts
description: Test, compile, deploy, simulate, call, and trace Alkanes contracts.
---

## Test before compiling

```bash
labcoat test
labcoat test contracts/MyToken.rs
```

Native Rust integration tests run contracts through `labcoat-test`, keeping the
fast feedback loop outside the chain. Each `ContractHarness` has isolated
in-memory storage that persists across calls on that harness. Use
`storage_value` to inspect raw state or `set_storage` to seed a test fixture.

## Compile

```bash
labcoat compile contracts/MyToken.rs
```

Compilation writes:

- `build/MyToken.wasm`: raw deployable module.
- `build/MyToken.wasm.gz`: compressed distribution artifact.
- `build/MyToken.abi.json`: opcode, input, and output metadata.

## Deploy

```bash
labcoat deploy build/MyToken.wasm --dry-run
labcoat deploy build/MyToken.wasm --name MyToken
```

Deployment uses a Bitcoin commit/reveal envelope, waits for the create trace,
and records the resulting `block:tx` ID in `labcoat.lock`.

## Simulate and call

```bash
labcoat simulate MyToken 99
labcoat call MyToken 77 500
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
