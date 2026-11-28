# Working in this Labcoat project

The Rust `labcoat` CLI owns compilation, testing, wallet operations,
deployment, calls, simulation, tracing, and Labcoat Network.

- Contracts: Cargo packages under `contracts/*/`
- Shared contract libraries: Cargo packages under `crates/*/`; add each package
  to the root workspace `members` when creating it
- Native integration tests: `tests/*.rs` using `labcoat-test`
- Add contract packages with `labcoat new <name>`
- Deployment script: `deploy.rs` using `labcoat-deploy`, run by `labcoat plan` and `labcoat deploy`
- Public configuration: `labcoat.toml`
- Secrets: `LABCOAT_WALLET_PASSPHRASE` and `LABCOAT_MNEMONIC` in the environment; `SUBFROST_API_KEY` in `.env` (git-ignored). Never print or commit them
- Address book: `labcoat.lock`; per-network deployment records:
  `deployments/<network>.json`

Commit the `Cargo.lock` created by the first build. `rust-toolchain.toml` pins
the Rust toolchain and WebAssembly targets this Labcoat release is tested with;
rustup installs them automatically. Change it only together with a Labcoat
upgrade, and run `labcoat doctor` to check it. Run `labcoat test`, then
`labcoat up` (which comes with a funded wallet), `labcoat plan`, and
`labcoat deploy`. The first `labcoat test` builds every dependency and can
take up to 20 minutes; start it once and wait. `SKILL.md` has the step by
step workflow, including what to check in each command's JSON output.
Use `labcoat simulate counter get_count` and `labcoat call counter increment`
for the starter contract. Use `--json` for machine-readable envelopes and
`labcoat docs --llm` for the full command reference. Simulation targets the
deployed contract and live indexed state; use `labcoat test <package>` for an
undeployed local build.
