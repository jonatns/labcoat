# Toolchain & Pins

Toolchain policy for this monorepo.
`crates/labcoat-cli/runtime-sources.json` is the machine-readable source of
truth for managed-runtime build refs; this file documents the Cargo and
toolchain constraints. Bumping either is a deliberate, reviewed change—never a
side effect of `cargo update` or a lockfile refresh.

## alkanes-rs pin (hard constraint)

Every reference to `alkanes-rs` — Cargo `git` dependencies, CI, contract
templates, docs — MUST point at the **`main`** branch, pinned to the
exact commit below. Never use a moving branch ref.

| | |
|---|---|
| Repo | `https://github.com/kungfuflex/alkanes-rs` |
| Branch | `main` |
| **Pinned commit** | `ea38c878931ff4b6dd64dd011962f2203d6e0a35` |
| Tag | `v3.0.0-rc.4` |
| Recorded | 2026-11-23 |

Transitive git deps of alkanes-rs are declared as branch refs upstream
(`emasm-rs@master`) or immutable tags (`metashrew@v9.0.5-rc.8`). Cargo forbids `[patch]`-ing a git
source with itself at a rev, so the reproducibility pin is the **committed
`Cargo.lock`** — it records the exact commits (metashrew at
`22824e4ce8812751bd85b4dfff0da66b4ee025df`, matching alkanes-rs's own lock
at the pinned commit) and CI builds with `--locked`. Never run a bare
`cargo update`.

Generated project templates pin `alkanes-rs` directly and use the same
`kungfuflex/metashrew@v9.0.5-rc.8` SourceId as that revision's transitive
dependencies. They create their own `Cargo.lock` on first build; commit that
lockfile because it is the reproducibility boundary for a Labcoat project.

**Upgrade procedure:** update the rev here, in the affected Cargo manifests and
templates, and in `runtime-sources.json`; update only the affected git
dependencies; run the full integration suite against `labcoat up`; and land the
change in its own reviewed PR. CI verifies the pin is reachable from `main`.

## Toolchains

| Tool | Version | Where enforced |
|---|---|---|
| Rust | 1.100.0 | `rust-toolchain.toml`, `ci.yml`, `release-cli.yml`, and `PROJECT_RUST_TOOLCHAIN` (latest stable; see below) |
| wasm targets | `wasm32-unknown-unknown`, `wasm32-wasip1` | deploy artifacts use unknown-unknown; native contract tests use WASIp1 |
| protoc | any ≥3 (`protobuf-compiler`) | required to build `labcoat-core` (prost-build 0.12 does not vendor protoc) |
| LLVM Clang | wasm32 backend | required by secp256k1-sys while compiling contracts; Homebrew LLVM is auto-detected on macOS |
| WASI libc | system package | required for `wasm32-wasip1` contract tests on Linux (`apt install wasi-libc`) |

Rust has no LTS channel; the recommended toolchain is the latest stable
release. The workspace pins that release exactly so local, CI, release, and
project builds match. Bump it deliberately: update `rust-toolchain.toml`,
`ci.yml`, `release-cli.yml`, and `PROJECT_RUST_TOOLCHAIN` together, then run
`cargo clippy --workspace --all-targets -- -D warnings`, because new releases
add lints.

Generated projects pin the same release. `labcoat init` writes a
`rust-toolchain.toml` from `PROJECT_RUST_TOOLCHAIN` in
`crates/labcoat-core/src/compile.rs`, and rustup installs that toolchain and
both WebAssembly targets on the first build. A unit test keeps the constant
equal to this workspace's `rust-toolchain.toml`. `labcoat doctor` warns when an
existing project's pin differs from the installed CLI's.

Contract compilation needs `clang`; on macOS install Homebrew `llvm`
because Apple Clang does not ship a WebAssembly backend. Debian and Ubuntu
also need `wasi-libc` for the WASIp1 C sysroot used by `labcoat test`.

## Banned dependencies (hard constraint)

`oyl-sdk` / `@oyl/sdk` must not appear anywhere in the resolved dependency
tree. CI enforces the ban. No new dependency may be added without checking
that it does not pull either package in.
