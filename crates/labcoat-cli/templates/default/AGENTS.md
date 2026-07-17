# Working in this Labcoat project

The Rust `labcoat` CLI owns compilation, testing, wallet operations,
deployment, calls, simulation, tracing, and the local devnet.

- Contracts: Cargo packages under `contracts/*/`
- Shared contract libraries: Cargo packages under `crates/*/`; add each package
  to the root workspace `members` when creating it
- Native integration tests: `tests/*.rs` using `labcoat-test`
- Add contract packages with `labcoat contract new <name>`
- Public configuration: `labcoat.toml`
- Secrets: `LABCOAT_WALLET_PASSPHRASE` and `LABCOAT_MNEMONIC` only
- Deployment ledger: `labcoat.lock`

Commit the `Cargo.lock` created by the first build. Run `labcoat test`, then
`labcoat up`, `labcoat wallet init`, and `labcoat deploy build/example.wasm`.
Use `--json` for machine-readable
envelopes and `labcoat docs --llm` for the full command reference.
