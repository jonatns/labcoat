# Working in this Labcoat project

The Rust `labcoat` CLI owns compilation, testing, wallet operations,
deployment, calls, simulation, tracing, and the local devnet.

- Contracts: Cargo packages under `contracts/*/`
- Shared contract libraries: Cargo packages under `crates/*/`; add each package
  to the root workspace `members` when creating it
- Native integration tests: `tests/*.rs` using `labcoat-test`
- Add contract packages with `labcoat new <name>`
- Public configuration: `labcoat.toml`
- Secrets: `LABCOAT_WALLET_PASSPHRASE` and `LABCOAT_MNEMONIC` only
- Deployment ledger: `labcoat.lock`

Commit the `Cargo.lock` created by the first build. Run `labcoat test`, then
`labcoat up`, `labcoat wallet init`, and `labcoat deploy counter`.
Use `labcoat simulate counter get_count` and `labcoat call counter increment`
for the starter contract. Use `--json` for machine-readable envelopes and
`labcoat docs --llm` for the full command reference. Simulation targets the
deployed contract and live indexed state; use `labcoat test <package>` for an
undeployed local build.
