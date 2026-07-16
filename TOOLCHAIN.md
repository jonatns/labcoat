# Toolchain & Pins

Single source of truth for toolchain versions and upstream pins in this
monorepo. Bumping anything here is a deliberate, reviewed change — never a
side effect of `cargo update` or a lockfile refresh.

## alkanes-rs pin (hard constraint)

Every reference to `alkanes-rs` — Cargo `git` dependencies, CI, contract
templates, docs — MUST point at the **`develop`** branch, pinned to the
exact commit below. Never `main`, never a moving branch ref.

| | |
|---|---|
| Repo | `https://github.com/kungfuflex/alkanes-rs` |
| Branch | `develop` |
| **Pinned commit** | `5b7f43567b828d0bb7b8907ce78fa0242943c54d` |
| Recorded | 2026-07-14 |
| For reference, `main` was | `8336eb517577c8a6ba5e6d707e5fd6d0d60eccc0` (do not use) |

Transitive git deps of alkanes-rs are declared as branch refs upstream
(`metashrew@develop`, `emasm-rs@master`). Cargo forbids `[patch]`-ing a git
source with itself at a rev, so the reproducibility pin is the **committed
`Cargo.lock`** — it records the exact commits (metashrew at
`eca790ca1eeddc7cdac201b741637b8f18234924`, matching alkanes-rs's own lock
at the pinned commit) and CI builds with `--locked`. Never run a bare
`cargo update`.

Generated project templates carry these same direct revisions and create their
own `Cargo.lock` on first build. Commit that lockfile; it is the reproducibility
boundary for a Labcoat project.

**Upgrade procedure:** update the rev here and in every `Cargo.toml` /
contract template / workflow, `cargo update` only the affected git deps, run the
full integration suite against `labcoat up`, and land it as its own
reviewed PR. CI verifies the pin is reachable from `develop`.

## Toolchains

| Tool | Version | Where enforced |
|---|---|---|
| Rust | 1.86.0 | `rust-toolchain.toml` (matches alkanes-rs upstream) |
| wasm targets | `wasm32-unknown-unknown`, `wasm32-wasip1` | deploy artifacts use unknown-unknown; native contract tests use WASIp1 |
| protoc | any ≥3 (`protobuf-compiler`) | required to build `labcoat-core` (prost-build 0.12 does not vendor protoc) |
| LLVM Clang | wasm32 backend | required by secp256k1-sys while compiling contracts; Homebrew LLVM is auto-detected on macOS |
| WASI libc | system package | required for `wasm32-wasip1` contract tests on Linux (`apt install wasi-libc`) |

Contract compilation needs `clang`; on macOS install Homebrew `llvm`
because Apple Clang does not ship a WebAssembly backend. Debian and Ubuntu
also need `wasi-libc` for the WASIp1 C sysroot used by `labcoat test`.

## Banned dependencies (hard constraint)

`oyl-sdk` / `@oyl/sdk` must not appear anywhere in the resolved dependency
tree. CI enforces the ban. No new dependency may be added without checking
that it does not pull either package in.
