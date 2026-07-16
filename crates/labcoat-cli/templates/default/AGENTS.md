# Working in this Labcoat project

The Rust `labcoat` CLI owns compilation, testing, wallet operations,
deployment, calls, simulation, tracing, and the local Isomer devnet.

- Contracts: `contracts/*.rs`
- Native integration tests: `tests/*.rs` using `labcoat-test`
- Public configuration: `labcoat.toml`
- Secrets: `LABCOAT_WALLET_PASSPHRASE` and `LABCOAT_MNEMONIC` only
- Deployment ledger: `labcoat.lock`

Run `labcoat test`, then `labcoat up`, `labcoat wallet init`, and
`labcoat deploy build/Example.wasm`. Use `--json` for machine-readable
envelopes and `labcoat docs --llm` for the full command reference.
