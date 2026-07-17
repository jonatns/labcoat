---
name: labcoat
description: Build, test, deploy, call, and trace Alkanes smart contracts with the Rust-first Labcoat CLI and its local devnet.
---

# Labcoat workflow

Add another minimal contract and matching test with
`labcoat new <name>` instead of copying an existing package.

1. Run `labcoat test` for host-side Rust integration tests.
2. Run `labcoat up --json` and wait for `result.status.is_ready`.
3. Initialize the keystore with `labcoat wallet init --json`.
4. Build without deploying with `labcoat build counter --json` when needed.
5. Deploy by package name with `labcoat deploy counter --json`; Labcoat recompiles it first.
6. Use `simulate`, `call`, and `trace --wait` for the contract loop.

Secrets belong in environment variables or mnemonic stdin, never argv or
`labcoat.toml`. Commit the `Cargo.lock` created by the first build. Run
`labcoat docs --llm` for the full reference.
