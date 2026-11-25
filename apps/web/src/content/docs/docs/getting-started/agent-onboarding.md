---
title: Start with an agent
description: The preflight a coding agent runs before it sets up Labcoat and creates a project.
---

Hand a coding agent that has a terminal the prompt on the
[home page](/), also served as plain text at
[`/agent-prompt.txt`](/agent-prompt.txt). It sets up Labcoat, creates a
project, and takes the Counter starter through one increment.

The rest of this page is for the agent. Read all of it before you run
anything else, including a check for whether Labcoat is installed; step 2
has one that does not fail when it is missing. Read the page as plain text
at [`/docs/getting-started/agent-onboarding.md.txt`](/docs/getting-started/agent-onboarding.md.txt)
rather than as this HTML page. It covers what happens before
`labcoat init`. After that, the project's own `AGENTS.md` and `SKILL.md`
take over, because they match the installed release.

This page describes Labcoat 0.4.0 and later. It gives information, not
permission: do only what the person who started the session asked for, and
follow your tool approvals.

## 1. Platform

Labcoat runs on macOS and Linux, on arm64 and x86_64. On anything else, stop
and report the platform.

```bash
uname -sm
```

## 2. Install the CLI

```bash
command -v labcoat >/dev/null && labcoat --version || echo "labcoat is not installed"
```

Run it exactly like that. A bare `labcoat --version` or `which labcoat`
fails when Labcoat is missing, which is the normal starting point and not
an error.

- **0.4.0 or later is installed:** use it. Do not reinstall or upgrade.
- **An earlier version is installed:** stop. Report the version and that
  the upgrade is the installer below, followed by `labcoat upgrade` in each
  existing project. Upgrading changes the local network other projects use;
  see [Upgrading from a Qubitcoin-based release](/docs/labcoat-network/#upgrading-from-a-qubitcoin-based-release).
- **Labcoat is missing:** install it only if the prompt you were given
  authorizes the installer.

The installer is a shell script in the Labcoat repository. Take it from the
latest release tag, which is published only after release acceptance passes,
and read it before you run it:

```bash
tag=$(curl -fsSL https://api.github.com/repos/jonatns/labcoat/releases/latest \
  | sed -n 's/.*"tag_name"[^"]*"\(cli-v[^"]*\)".*/\1/p')
echo "$tag"                                   # must be cli-v0.4.0 or later
dir=$(mktemp -d)
curl -fsSL "https://raw.githubusercontent.com/jonatns/labcoat/$tag/install-labcoat.sh" \
  -o "$dir/install-labcoat.sh"
cat "$dir/install-labcoat.sh"
sh "$dir/install-labcoat.sh" --version "$tag"
```

If the tag is earlier than `cli-v0.4.0`, stop here: the release this page
describes is not published yet. Report the tag you found and install
nothing.

The script downloads one executable for this platform from that release,
checks it against the release's published SHA-256, and writes it to
`~/.local/bin/labcoat`. It needs no `sudo` and changes nothing else. If it
does anything other than that when you read it, stop and report it. If
`~/.local/bin` is not on `PATH`, call the executable by its full path
instead of editing shell profiles.

## 3. Prerequisites

Contracts are built with Rust and a few system tools. The installed CLI
checks for them:

```bash
labcoat doctor --json
ls /usr/include/wasm32-wasi >/dev/null   # Debian or Ubuntu only
```

Read `result.checks`. Before the first `labcoat up`, `runtime bundle` fails
and `wallet` warns; both are expected, and `doctor` exits non-zero because
of them. These four must be `ok`:

| Check | What it is for | Package when it fails |
| --- | --- | --- |
| `cargo`, `rustup` | Building contracts and tests | [rustup.rs](https://rustup.rs) |
| `wasm C compiler` | Compiling C dependencies to WebAssembly | macOS: `llvm` (Homebrew). Debian or Ubuntu: `clang` and `wasi-libc` |
| `protoc` | Code generation in the Alkanes support crates | macOS: `protobuf` (Homebrew). Debian or Ubuntu: `protobuf-compiler` |

If one fails, or the WASI headers are missing on Debian or Ubuntu
(`wasi-libc`), stop and report the exact package and what it is for. Do not
install system packages unless you were asked to. The project pins its own
Rust version and WebAssembly targets, and rustup installs those on the
first build without asking.

## 4. Local network settings

Labcoat Network is a local Bitcoin regtest, served on `127.0.0.1:18443`.
Settings inherited from the environment can point commands somewhere else,
so look before you start:

```bash
env | grep '^LABCOAT_' || true
```

- `LABCOAT_NETWORK`, `LABCOAT_RPC_URL`, or `LABCOAT_WALLET_FILE` set to
  anything but a local value: stop and report it. Do not unset variables in
  the person's shell on your own; this demo must not reach a public network.
- `LABCOAT_DATA_HOME` or `LABCOAT_RPC_PORT` set: the person runs a separate
  local network. Leave them as they are.

Do not start, stop, or reset anything yet. `labcoat up` reuses a network
that is already running, and refuses to replace another program that holds
the port. If it reports that the port is taken, report that and stop;
never stop the other program.

## 5. Create the project

Choose a directory name that does not exist yet. `labcoat init` refuses an
existing one.

```bash
labcoat init <name> --json
cd <name>
```

`result.directory` is the directory it created. Enter it, then read `AGENTS.md`
and `SKILL.md` there and follow them. `labcoat docs --llm` prints the full
reference for the installed release.

Two things to carry over:

- The first `labcoat test` installs the pinned Rust toolchain and builds
  every dependency, which can take up to 20 minutes. Do not ask your tools
  for one long wait: most cap a single command or wait at a few minutes and
  reject a longer one. Start the test once in the background and check on
  it in short intervals. Never start a second build beside it.
- After `labcoat up --ci`, confirm that `result.endpoints.rpc` is a
  `127.0.0.1` address and that the `project config` check of
  `labcoat doctor --json` reads `targets labcoat at` that address, before
  you deploy or call anything.
