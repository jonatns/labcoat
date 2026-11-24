---
name: labcoat
description: Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts on Labcoat Network, a managed local Bitcoin regtest.
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
when a method uses an unsupported complex type. Simulation always targets the
deployed contract and live indexed state; use `labcoat test <package>` for an
undeployed local build. Declare multi-step deployments in `deploy.rs` with
`labcoat` (`d.contract(name, package)` deploys once, with
`.reserve(n)` and `.arg(name, value)` named after ABI parameters;
`d.external(name).on(network, block, tx)` names an existing Alkane;
reference another step with `step.id()`) and run `labcoat plan` /
`labcoat apply --broadcast`. A step runs once per network: editing it
afterwards is a conflict that apply refuses until you restore it or pass
`--replace <name>`. For upgradeable contracts use `d.implementation`,
`d.beacon`, `d.beacon_proxy`, and `labcoat new <name> --template`, and call
through a proxy with `--abi <implementation>`. The script is deployment
only — done when the contracts are deployed and inert; value-moving,
actor-specific operations belong in `tests/e2e.rs` (`labcoat test --e2e`)
or application flows.

Secrets belong in environment variables or mnemonic stdin, never argv or
`labcoat.toml`. Commit the `Cargo.lock` created by the first build. Run
`labcoat docs --llm` for the full reference.
