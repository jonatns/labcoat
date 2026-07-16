---
title: Installation
description: Install the Labcoat CLI on supported macOS and Linux systems.
---

Labcoat publishes native binaries for macOS and Linux on arm64 and x86_64.

```bash
curl -fsSL https://labcoat.sh/install | sh
```

The installer downloads the latest `cli-v*` release, verifies its SHA-256
checksum, and writes `labcoat` to `${LABCOAT_INSTALL_DIR:-$HOME/.local/bin}`.

## Install a specific version

```bash
curl -fsSL https://labcoat.sh/install | sh -s -- 0.1.0
```

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
