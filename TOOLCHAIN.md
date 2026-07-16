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

**Upgrade procedure:** update the rev here and in every `Cargo.toml` /
template / workflow, `cargo update` only the affected git deps, run the
full integration suite against `labcoat up`, and land it as its own
reviewed PR. CI verifies the pin is reachable from `develop`.

## Toolchains

| Tool | Version | Where enforced |
|---|---|---|
| Rust | 1.86.0 | `rust-toolchain.toml` (matches alkanes-rs upstream) |
| wasm targets | `wasm32-unknown-unknown`, `wasm32-wasip1` | deploy artifacts use unknown-unknown; native contract tests use WASIp1 |
| Node | 22.x | CI matrix |
| pnpm | 11.13.0 | root `packageManager` + CI (`pnpm/action-setup`) |
| Tauri | 2.x | `apps/isomer/src-tauri/Cargo.toml` |
| TypeScript | ~5.8 / ^5.9 | per-package `package.json` |
| protoc | any ≥3 (`protobuf-compiler`) | required to build `labcoat-core` (prost-build 0.12 does not vendor protoc) |
| LLVM Clang | wasm32 backend | required by secp256k1-sys while compiling contracts; Homebrew LLVM is auto-detected on macOS |

Linux builds of the Tauri app additionally need
`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`.
Contract compilation needs `clang`; on macOS install Homebrew `llvm`
because Apple Clang does not ship a WebAssembly backend.

## Banned dependencies (hard constraint)

`oyl-sdk` / `@oyl/sdk` must not appear anywhere in the dependency tree,
direct or transitive (npm or cargo). CI fails the build if it shows up in
`pnpm ls -r --depth Infinity` or `cargo tree`. No new dependency may be
added without checking it doesn't pull it in.

Known-safe: `@alkanes/ts-sdk` declares `@oyl/sdk` only as an *optional
peerDependency* (never auto-installed); the `alkanes` npm package (removed
in the toolkit rebase anyway) does not depend on it.

## Baseline snapshot (recorded at migration start, 2026-07-14)

- labcoat `npm run build` (tsup): green. `npx jest`: 2 pass, 2 pre-existing
  failures (`compiler.test.ts` array-ABI cases expect shapes the regex
  `parseABI` never produced — slated for the Phase 3 compiler port).
- isomer `pnpm --filter @isomer/desktop build` (tsc + vite): green after
  fixing five TS6133 unused-declaration errors present at the
  `feat/ui-revamp` tip.
- isomer extension (`@isomer/extension`) install/build could not run in the
  migration container: its `@alkanes/ts-sdk` tarball host
  `pkg.alkanes.build` is denied by the sandbox network policy. Verified in
  CI instead.
