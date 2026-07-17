---
title: Installation
description: Install the Labcoat CLI on supported macOS and Linux systems.
---

Labcoat publishes native binaries for macOS and Linux on arm64 and x86_64.
Windows is not supported.

> **Release channel note:** the stable `cli-v0.1.0` release uses
> `labcoat contract new`, `labcoat compile`, and raw-Wasm deployment. The
> current main branch uses `labcoat new`, `labcoat build`, and package-name
> deployment. Run `labcoat docs --llm` for the reference bundled with your
> installed version.

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

The installer downloads the latest `cli-v*` release, requires `sha256sum` or
`shasum`, verifies the SHA-256 checksum automatically, and writes `labcoat` to
`${LABCOAT_INSTALL_DIR:-$HOME/.local/bin}`. It exits rather than installing a
binary when the hashing tool is unavailable or verification fails.

## Install a specific version

```bash
curl -fsSL https://labcoat.sh/install | sh -s -- 0.1.0
```

Pin the version in CI, development containers, and reproducible setup scripts.
The website tracks `main` and may document commands newer than a pinned stable
release.

## Verify the artifact attestation

GitHub publishes an attestation for each release binary and checksum file. With
the [GitHub CLI](https://cli.github.com/) installed, download the asset for your
platform and verify it against this repository:

```bash
gh release download cli-v0.1.0 --repo jonatns/labcoat --pattern 'labcoat-*'
gh attestation verify ./labcoat-* --repo jonatns/labcoat
```

Checksum verification protects the downloaded file against the published
manifest. Attestation verification additionally checks the GitHub Actions build
provenance.

## Upgrade or roll back

Run the installer again to replace the installed executable atomically:

```bash
# Upgrade to the latest stable release
curl -fsSL https://labcoat.sh/install | sh

# Roll back to a known version
curl -fsSL https://labcoat.sh/install | sh -s -- 0.1.0
```

Confirm the active executable and its command surface after either operation:

```bash
command -v labcoat
labcoat --version
labcoat --help
```

## Uninstall

Remove the executable from the same directory used for installation. The
default is:

```bash
rm "$HOME/.local/bin/labcoat"
```

This does not remove project files, Labcoat-managed runtime downloads, wallet
files, or local devnet data. Review paths with `labcoat binaries list` and your
project configuration before removing that data manually.

## Compilation prerequisites

Contract compilation requires an LLVM Clang build with a WebAssembly backend.

```bash
# macOS
brew install llvm

# Debian or Ubuntu
sudo apt install clang wasi-libc
```

Then verify the complete environment:

```bash
labcoat doctor
```

If `$HOME/.local/bin` is not already on `PATH`, the installer prints the exact
export command to add.

## Runtime and security boundaries

Labcoat provides one CLI entry point while downloading and orchestrating
multiple local services. Docker is required, and Node.js is required by the
gateway. Keep regtest ports on trusted local interfaces and never use production
wallet seed phrases with Labcoat.

Labcoat is early-stage software for local Alkanes development. Interfaces may
change before 1.0; mainnet deployment controls are not production-ready. Read
the [security policy](https://github.com/jonatns/labcoat/blob/main/SECURITY.md)
and [Stability and releases](/docs/reference/stability/) before relying on it.
