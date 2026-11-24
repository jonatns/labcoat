---
name: labcoat
description: Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts on Labcoat Network, a managed local Bitcoin regtest.
---

# Labcoat workflow

Add another minimal contract and matching test with
`labcoat new <name>` instead of copying an existing package.

1. Run `labcoat test` for host-side Rust integration tests.
2. Run `labcoat up --json` and wait for `result.status.is_ready`.
3. The network comes with a funded wallet: `result.wallet` in that output.
   Never create, import, or move keys unless asked.
4. Preview the deployment with `labcoat plan --json`; it signs nothing.
5. Deploy what `deploy.rs` declares with `labcoat deploy --json`;
   Labcoat recompiles each package first and skips steps that already ran.
   `labcoat deploy counter --json` runs that one step.
6. Use ABI method names such as `simulate counter get_count` and
   `call counter increment`, then `trace --wait`, for the contract loop.

Named methods accept one shell argument per ABI parameter. Labcoat encodes
`u128`, `String`, and `AlkaneId`; use a numeric opcode for raw cellpack args
when a method uses an unsupported complex type. Simulation always targets the
deployed contract and live indexed state; use `labcoat test <package>` for an
undeployed local build. Declare every contract the project deploys in
`deploy.rs` with `labcoat` (`d.contract(name, package)` deploys once, with
`.reserve(n)` and `.arg(name, value)` named after ABI parameters;
`d.external(name).on(network, block, tx)` names an existing Alkane;
reference another step with `step.id()`) and run `labcoat plan` /
`labcoat deploy`. Add `--dry-run` to preview. A step runs once
per network. On Labcoat Network an edited contract redeploys with a new id
and empty storage; on other networks editing it is a conflict that deploy
refuses until you restore it or pass `--replace <name>`. Public networks need
approval for each signature, or `--yes`: never pass it unless asked. For upgradeable contracts use `d.implementation`,
`d.beacon`, `d.beacon_proxy`, and `labcoat new <name> --template`, and call
through a proxy with `--abi <implementation>`. The script is deployment
only — done when the contracts are deployed and inert; value-moving,
actor-specific operations belong in `tests/e2e.rs` (`labcoat test --e2e`)
or application flows.

Secrets belong in environment variables or mnemonic stdin, never argv or
`labcoat.toml`. Commit the `Cargo.lock` created by the first build. Run
`labcoat docs --llm` for the full reference.
