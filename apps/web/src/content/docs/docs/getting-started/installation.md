---
title: Installation
description: Install the Labcoat CLI and its matching managed runtime.
---

Labcoat publishes CLI executables for macOS and Linux on arm64 and x86_64.
Windows is not supported.

## Platform support

The CLI builds, tests, and deploys contracts on every platform below. Whether
`labcoat up` can run Labcoat Network locally depends on the release:

| Platform | CLI | Labcoat Network, 0.3.0 and earlier | Labcoat Network, later releases |
| --- | --- | --- | --- |
| macOS arm64 | Yes | Yes | Yes |
| macOS x86_64 | Yes | No | Yes |
| Linux x86_64 | Yes | Yes | Yes |
| Linux arm64 | Yes | No | Yes |

Releases up to 0.3.0 run Labcoat Network on a downloaded `qubitcoind`, which is
published for two platforms. Later releases run it inside the `labcoat`
executable. This page describes the later releases; run `labcoat --version` to
see which one you have, and `labcoat docs --llm` for the reference that matches
it. Moving between the two is covered in
[Upgrading from a Qubitcoin-based release](/docs/labcoat-network/#upgrading-from-a-qubitcoin-based-release).

## Inspect and install

Review the installer before executing it when your environment requires it:

```bash
curl -fsSL https://labcoat.sh/install -o /tmp/install-labcoat.sh
less /tmp/install-labcoat.sh
sh /tmp/install-labcoat.sh
```

For a direct install:

```bash
curl -fsSL https://labcoat.sh/install | sh
```

The installer selects the latest `cli-v*` release, requires `sha256sum` or
`shasum`, verifies the published SHA-256 checksum, and writes `labcoat` to
`${LABCOAT_INSTALL_DIR:-$HOME/.local/bin}`. It exits without replacing the
current executable if downloading or verification fails.

## CLI and runtime versions

Each CLI release owns one exact runtime bundle. On the first `labcoat up`,
Labcoat downloads `runtime-manifest.json` and the Alkanes indexer module,
`alkanes.wasm`, from the same `cli-vX.Y.Z` GitHub release:

- macOS: `~/Library/Application Support/Labcoat/runtimes/cli-vX.Y.Z/`
- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/labcoat/runtimes/cli-vX.Y.Z/`

After the bundle is present and valid, normal commands use the local cache and
do not check GitHub for a newer runtime. Upgrade the CLI to receive a newer
runtime. Previous version directories remain available for rollback and are
never removed automatically.

Run `labcoat binaries --verbose` to inspect the active version and paths.
Passing `--no-download` to `labcoat up` requires the complete matching bundle
to already exist.

## Install a specific version

```bash
curl -fsSL https://labcoat.sh/install | sh -s -- X.Y.Z
```

Pin a version in CI, development containers, and reproducible setup scripts.
The website tracks `main` and may document changes newer than a pinned
executable; `labcoat docs --llm` is the installed-version reference.

## Verify artifact provenance

GitHub publishes build-provenance attestations for every CLI and runtime asset.
With the [GitHub CLI](https://cli.github.com/) installed:

```bash
gh release download cli-vX.Y.Z --repo jonatns/labcoat --pattern 'labcoat-*'
gh attestation verify ./labcoat-* --repo jonatns/labcoat
```

The installer verifies the CLI checksum automatically. Runtime downloads are
verified against the exact release manifest before installation.

## Upgrade or roll back

Run the installer again to atomically replace the executable:

```bash
# Upgrade to the latest stable release
curl -fsSL https://labcoat.sh/install | sh

# Roll back to a known version
curl -fsSL https://labcoat.sh/install | sh -s -- X.Y.Z
```

Confirm the active executable after either operation:

```bash
command -v labcoat
labcoat --version
labcoat --help
labcoat binaries
```

Then bring each existing project in line with the new release:

```bash
labcoat upgrade --dry-run
labcoat upgrade
labcoat test
```

`labcoat upgrade` updates the project's alkanes-rs revisions,
`labcoat-deploy` and `labcoat-test` release tags, and `rust-toolchain.toml`
to match the installed CLI. Commit the result with the updated
`Cargo.lock`. `labcoat doctor` lists any pin that still differs.

## Compilation prerequisites

Contract compilation requires [rustup](https://rustup.rs), LLVM Clang with a
WebAssembly backend, and the Protocol Buffers compiler, `protoc`, which the
Alkanes support crates run at build time.

```bash
# macOS
brew install llvm protobuf

# Debian or Ubuntu
sudo apt install clang wasi-libc protobuf-compiler
```

You do not need to install a particular Rust version or WebAssembly target.
`labcoat init` writes a `rust-toolchain.toml` that pins the Rust release
Labcoat is tested with, and rustup installs that toolchain with the
`wasm32-unknown-unknown` and `wasm32-wasip1` targets on the project's first
build. Commit the file: it keeps every machine and CI run on the same
compiler, which keeps contract bytecode reproducible. When you upgrade the
CLI, run `labcoat upgrade` in each project to move its pins to the new
release.

Then run:

```bash
labcoat doctor
```

If `$HOME/.local/bin` is not on `PATH`, the installer prints the exact export
command to add.

## Uninstall

Remove the CLI from the same directory used during installation:

```bash
rm "$HOME/.local/bin/labcoat"
```

This does not remove projects, wallets, chain data, logs, or versioned runtime
directories. Review `labcoat binaries --verbose` and your platform’s Labcoat
data directory before removing that data manually.

## Runtime and security boundaries

Labcoat Network is one `labcoat node` process holding the chain and the
Alkanes indexer. It is local/private and uses an unauthenticated
Bitcoin regtest RPC endpoint bound to loopback; it is not Signet. Keep it local
and never use production wallet seed phrases with Labcoat.

Labcoat is early-stage software for local Alkanes development. Interfaces may
change before 1.0; mainnet deployment controls are not production-ready. Read
the [security policy](https://github.com/jonatns/labcoat/blob/main/SECURITY.md)
and [Stability and releases](/docs/reference/stability/) before relying on it.
