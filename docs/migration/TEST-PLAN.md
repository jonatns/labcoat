# Manual test plan — Rust-first CLI

CI covers formatting, workspace tests, clippy, installer unit tests, the
generated-project smoke test, MCP discovery, and the stub devnet. Complete
the checks below with real service binaries before merging or releasing.

## 1. Build and diagnose

```bash
cargo build --release -p labcoat-cli
export PATH="$PWD/target/release:$PATH"
labcoat doctor
```

- [ ] Rust, Cargo, both WebAssembly targets, protoc, and LLVM are available.
- [ ] The CLI reports missing service binaries clearly before first setup.

## 2. Real devnet

```bash
labcoat up
labcoat status
labcoat mine 5
labcoat logs --service metashrew --limit 20
```

- [ ] Downloads pass checksum verification.
- [ ] All services report running and ready.
- [ ] Initial block height is at least 101.
- [ ] Mining increases height by exactly five.
- [ ] Services remain alive when checked from a new shell.

Verify snapshot and restore:

```bash
labcoat snapshot before-test
labcoat up --no-download
labcoat mine 3
labcoat restore before-test
labcoat up --no-download
```

- [ ] Restored height matches the snapshot.

## 3. Generated project and contract loop

```bash
PROJECT=$(mktemp -d)/project
labcoat init "$PROJECT"
cd "$PROJECT"
labcoat test
labcoat new stateful
labcoat compile counter
labcoat wallet init
labcoat wallet addresses
labcoat fund <address>
labcoat mine 1
labcoat deploy counter --dry-run
labcoat deploy counter
labcoat simulate counter 2
labcoat call counter 1
labcoat trace <txid> --wait
```

- [ ] The test harness initializes the Counter, increments twice, and reads `2`.
- [ ] Raw `.wasm`, `.wasm.gz`, and ABI artifacts are produced.
- [ ] Deploy and test artifacts expose identical ABI JSON through `__meta`.
- [ ] `labcoat abi fetch counter` and `labcoat abi verify counter` succeed.
- [ ] Dry-run broadcasts nothing.
- [ ] Deploy recompiles the exact selected package instead of reusing stale Wasm.
- [ ] Deployment is recorded in `labcoat.lock`.
- [ ] Simulation and state-changing calls return expected data.
- [ ] A bogus opcode returns a typed error instead of panicking.
- [ ] A non-empty `labcoat init` target is rejected without `--force`.

## 4. Configuration and migration

- [ ] CLI flags override environment, file, and default values.
- [ ] `LABCOAT_*` variables override `labcoat.toml`.
- [ ] Mnemonic and passphrase keys are rejected from `labcoat.toml`.
- [ ] An existing mnemonic produces the expected BIP-86/84/49/44 addresses.
- [ ] `labcoat lock migrate` imports a legacy deployment manifest.

## 5. Automation

- [ ] `labcoat docs --llm` renders the complete command reference.
- [ ] `labcoat mcp serve` lists tools from a real MCP host.
- [ ] MCP results match equivalent CLI JSON envelopes.
- [ ] Error envelopes include `code`, `message`, and `hint`.

## 6. Release assets

- [ ] A `kind=cli` workflow dispatch creates four CLI binaries.
- [ ] Every binary has a valid SHA-256 file.
- [ ] The installer selects the latest published `cli-v*` tag.
- [ ] An explicit installer version selects that exact tag.
- [ ] A corrupted download is rejected.

## 7. Teardown

```bash
labcoat reset -y
labcoat status
```

- [ ] All processes stop and ports are free.
