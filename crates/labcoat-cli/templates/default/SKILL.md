---
name: labcoat
description: Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts with a complete local Bitcoin devnet.
---

# Labcoat workflow

Add another minimal contract and matching test with
`labcoat new <name>` instead of copying an existing package.

1. Run `labcoat test` for host-side Rust integration tests.
2. Run `labcoat up --json` and wait for `result.status.is_ready`.
3. Initialize the keystore with `labcoat wallet init --json`.
4. Build without deploying with `labcoat build counter --json` when needed.
5. Deploy by package name with `labcoat deploy counter --json`; Labcoat recompiles it first.
6. Use ABI method names such as `simulate counter get_count` and
   `call counter increment`, then `trace --wait`, for the contract loop.

Named methods accept one shell argument per ABI parameter. Labcoat encodes
`u128`, `String`, and `AlkaneId`; use a numeric opcode for raw cellpack args
when a method uses an unsupported complex type. Use ordinary shell scripts for
multi-step workflows.

Secrets belong in environment variables or mnemonic stdin, never argv or
`labcoat.toml`. Commit the `Cargo.lock` created by the first build. Run
`labcoat docs --llm` for the full reference.
