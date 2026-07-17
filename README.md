# Labcoat

**A Rust-first CLI for building, testing, deploying, and operating
[Alkanes](https://alkanes.build) smart contracts on Bitcoin.**

Labcoat gives contract developers one native tool for the complete local
workflow:

- scaffold Rust contract projects;
- build deployable WebAssembly;
- run contracts in a native test harness;
- start and control a complete Bitcoin regtest stack;
- manage project wallets;
- deploy, call, simulate, and trace Alkanes contracts;
- automate every command with JSON envelopes or MCP.

The supported public interface is the **`labcoat` CLI**.

[Website](https://labcoat.sh) · [Documentation](https://labcoat.sh/docs/) ·
[Agent index](https://labcoat.sh/llms.txt)

## Install

macOS and Linux binaries are published for arm64 and x86_64. Windows CLI
support is not available yet.

```bash
curl -fsSL https://labcoat.sh/install | sh
```

The installer verifies the release checksum and writes the binary to
`${LABCOAT_INSTALL_DIR:-$HOME/.local/bin}`. Install a specific version with:

```bash
curl -fsSL https://labcoat.sh/install \
  | sh -s -- 0.1.0
```

Contract compilation requires an LLVM Clang with a WebAssembly backend.

```bash
brew install llvm       # macOS
sudo apt install clang wasi-libc  # Debian/Ubuntu
```

Check the complete environment with:

```bash
labcoat doctor
```

## Quick start

Create a project and run its Rust integration test:

```bash
labcoat init hello-alkane
cd hello-alkane
labcoat test
```

Every new project includes a fixed Counter starter. Add another minimal
contract from anywhere inside the project with:

```bash
labcoat new token
```

Start the local devnet and initialize the project wallet:

```bash
labcoat up
labcoat status
labcoat wallet init
labcoat wallet addresses
```

Fund the displayed address, mine a block, and inspect its UTXOs:

```bash
labcoat fund <address>
labcoat mine 1
labcoat wallet utxos
```

Compile the Counter without deploying, or deploy it directly by package name:

```bash
labcoat build counter
labcoat deploy counter --dry-run
labcoat deploy counter
labcoat abi fetch counter
labcoat abi verify counter
```

Interact with the deployed contract:

```bash
labcoat simulate counter 2
labcoat call counter 1
labcoat trace <txid> --wait
```

Stop the devnet when finished:

```bash
labcoat down
```

## Projects and configuration

`labcoat init` creates:

```text
contracts/          Cargo contract packages
tests/              Native integration tests using labcoat-test
Cargo.toml          Host-side test package and workspace manifest
Cargo.lock          Reproducible dependency lock (created on first build)
labcoat.toml        Public project configuration
AGENTS.md           Agent instructions
SKILL.md            Agent workflow
```

Settings resolve in this order:

```text
CLI flags → LABCOAT_* environment variables → labcoat.toml → defaults
```

`labcoat.toml` supports `network`, `rpc_url`, `wallet_file`, and
`fee_rate`. Mnemonics and passphrases are rejected from the file; use
`LABCOAT_MNEMONIC`, mnemonic stdin, and `LABCOAT_WALLET_PASSPHRASE`.

Deployments are recorded by network in `labcoat.lock`. Commit this file
when deployments are part of the project state.

Each contract is an ordinary Cargo package under `contracts/`, so normal
crates.io, git, path dependencies, modules, and shared workspace crates work.
The first build creates `Cargo.lock`; commit it and avoid bare `cargo update`.
Host tests use isolated in-memory contract storage that persists across calls
on the same `ContractHarness`.

When multiple contracts need common Rust code, add a Cargo library under
`crates/<name>/` and add `"crates/<name>"` to the root workspace `members`.
New projects omit both the directory and its workspace glob until shared code
is needed.

Add another minimal contract package and matching host test without copying files:

```bash
labcoat new token
```

## CLI map

| Area | Commands |
|---|---|
| Project | `init`, `new`, `doctor`, `docs` |
| Test and build | `test`, `build` |
| Devnet | `up`, `down`, `status`, `mine`, `fund`, `logs`, `reset`, `snapshot`, `restore`, `binaries` |
| Wallet | `wallet init`, `wallet addresses`, `wallet utxos` |
| Contracts | `deploy`, `call`, `simulate`, `trace`, `abi`, `lock` |
| Automation | `mcp serve`, global `--json` |

Run `labcoat --help`, `labcoat <command> --help`, or `labcoat docs --llm`
for the full command reference.

## Automation

Every command accepts `--json` and emits a stable `labcoat/v1/*` envelope.
Errors include a typed code, human-readable message, and recovery hint.

```bash
labcoat status --json
labcoat deploy counter --dry-run --json
labcoat mcp serve
```

## Develop Labcoat

The CLI and its runtime are a Rust workspace pinned to Rust 1.86.0.

```bash
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --locked -- -D warnings
cargo build --release -p labcoat-cli
export PATH="$PWD/target/release:$PATH"
```

Core layout:

```text
crates/isomer-core/   headless devnet orchestration engine
crates/labcoat-core/  contract, wallet, deployment, and trace operations
crates/labcoat-cli/   labcoat command-line interface and MCP server
crates/labcoat-test/  native WebAssembly contract test harness
```

See [TOOLCHAIN.md](TOOLCHAIN.md) for pinned upstream revisions and build
requirements, [docs/MIGRATING.md](docs/MIGRATING.md) for migration from the
retired TypeScript package, and [docs/RELEASING.md](docs/RELEASING.md) for
the CLI release process.
