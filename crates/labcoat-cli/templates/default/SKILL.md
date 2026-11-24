---
name: labcoat
description: Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts on Labcoat Network, a managed local Bitcoin regtest.
---

# Labcoat workflow

Add another minimal contract and matching test with
`labcoat new <name>` instead of copying an existing package.

1. Run `labcoat test --json` for host-side Rust integration tests. The first
   run installs the pinned Rust toolchain and builds every dependency: allow
   up to 20 minutes. If your command timeout is shorter, start it once in
   the background and poll it; never start a second build beside it.
2. Run `labcoat up --ci`. It prints one JSON envelope and exits non-zero
   unless the network is ready and its wallet is funded. A network that is
   already running is reused. If the port belongs to something else, `up`
   fails and leaves it running: report that instead of stopping anything.
3. The network comes with a funded wallet: `result.wallet` in that output.
   Never create, import, or move keys unless asked.
4. Before the first write, check where commands will go: `result.endpoints.rpc`
   from `up` is a `127.0.0.1` address, and the `project config` check of
   `labcoat doctor --json` reads `targets labcoat at` that address.
   `LABCOAT_NETWORK`, `LABCOAT_RPC_URL`, and `LABCOAT_WALLET_FILE` in the
   environment override `labcoat.toml`; if the target is not `labcoat`, stop
   and report it.
5. Preview the deployment with `labcoat plan --json`; it signs nothing.
6. Deploy what `deploy.rs` declares with `labcoat deploy --json`;
   Labcoat recompiles each package first and skips steps that already ran.
   `labcoat deploy counter --json` runs that one step.
7. Use ABI method names for the contract loop:
   - `labcoat simulate counter get_count --json` reads state;
     `result.decoded.uint` is the value.
   - `labcoat call counter increment --json` writes; check
     `result.status == "success"` and keep `result.txid`.
   - `labcoat trace <txid> --wait --json` returns `result.traces[].trace[]`,
     a list of typed events: `call`, then `return`, or `revert` on failure.
     `return_data` is raw little-endian hex, not a decoded value.

Every `--json` command prints one envelope: read `ok`, not the exit code
(only `up --ci` and `doctor` also use it). If a `call` or `deploy` ends
unclearly, it may already have broadcast: trace its transaction and re-read
state with `simulate` before running it again.

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
