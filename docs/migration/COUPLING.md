# Coupling-point inventory (Phase 0)

Everything hardcoded that the migration must repoint or centralize.
Recorded 2026-07-14 against isomer `feat/ui-revamp` (+ main merge) and
labcoat `main`.

## Ports

Canonical defaults: `apps/isomer/src-tauri/src/config.rs`
(`PortConfig::default()`):

| Service | Port |
|---|---|
| bitcoind RPC | 18443 |
| bitcoind P2P | 18444 |
| metashrew | 8080 |
| ord | 8090 |
| esplora HTTP | 50010 |
| esplora electrum | 50001 |
| unified JSON-RPC | 18888 |
| espo RPC | 8083 |
| espo explorer | 8081 |

Duplicated/hardcoded references to fix or keep in sync:

- `process_manager.rs:183-190` orphan-kill port list; `:962-967`
  `get_port_for_service` re-hardcodes the table (fixed in Phase 2 by
  reading `PortConfig`).
- Frontend: `apps/isomer/src/lib/rpc/isomerRpc.ts` (18888),
  `espoClient.ts` (8081), `Dashboard.tsx` (18888), `vite.config.ts` proxy
  (8081), `commands.rs:502` (8081 carousel API).
- Extension: `background/index.ts`, `popup/App.tsx` (18888).
- Isomer README documented stale ports (3001/3002) — superseded.
- Vite dev server 1420 / HMR 1421 ↔ `tauri.conf.json` `devUrl`.

## Binary download URLs (`binary_manager.rs`)

- Bitcoin Core 29.2 from bitcoincore.org (per-platform, real SHA256s).
- ord 0.22.1 from github.com/ordinals/ord releases (only darwin-arm64 has
  a checksum).
- rockshrew-mono / flextrs / espo / jsonrpc bundle / extension zip from
  `github.com/jonatns/isomer/releases/download/binaries-v0.1.3/`.
- **Inconsistency:** `alkanes.wasm` still points at `binaries-v0.1.0`.
- `CHECKSUMS_URL` → `binaries-v0.1.3/checksums.json`.
- All `jonatns/isomer` release URLs must eventually repoint to
  `jonatns/labcoat` releases (Phase 5).

## Workflows (from isomer, imported disabled until Phase 5)

- `release.yml`: `v*` tags → Tauri bundles (mac arm64/x64, ubuntu, win).
- `release-binaries.yml`: `binaries-v*` tags → builds upstreams:
  metashrew `v9.0.2-alpha.1`, flextrs `504bd533…`, espo
  `explorer-v9.0.1-rc1-metashrew`, jsonrpc bundle from
  `kungfuflex/alkanes@main`, and **alkanes-rs at unpinned `develop`** —
  must build at the pinned rev (see TOOLCHAIN.md). pnpm versions
  inconsistent across jobs (8 vs 9).
- Two-stage release flow (RELEASING.md): binaries release requires a
  manual `binary_manager.rs` URL bump afterward — `labcoat doctor` should
  detect drift.

## Other hardcoded repo references

- `install.sh`: `REPO="jonatns/isomer"`, curl'd from raw main.
- `cli/commands/init.ts` (labcoat): downloads templates zip from
  `jonatns/labcoat-templates@main`.
- `cargo-template.ts` (labcoat): `alkanes-runtime`/`alkanes-support` git
  deps **unpinned** (tracked default branch = main → violates the develop
  pin constraint; fixed in Phase 3).
- Port 18888 service is a Node.js bundle built from `kungfuflex/alkanes`
  `jsonrpc/` — requires `node` on PATH at runtime.

## Environment notes (migration container)

- `pkg.alkanes.build` (extension's `@alkanes/ts-sdk` tarball) is denied by
  the sandbox network policy → extension excluded from in-container
  installs/builds; CI covers it.
- Git tag pushes are rejected by the environment (branch-restricted
  pushes). Local safety tags `isomer@pre-monorepo` / `labcoat@pre-monorepo`
  exist in the session clones; origin branches `feat/ui-revamp` and `main`
  are untouched and serve as the rollback anchors. Maintainer may recreate
  the tags directly on GitHub if wanted.
