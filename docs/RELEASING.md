# Releasing from the monorepo

Three release tracks, all tag/PR driven. Never release with a dirty
`Cargo.lock` or unpinned alkanes-rs refs (CI guards will stop you).

## 1. npm packages (`@jonatns/labcoat`, `create-labcoat`)

Changesets-driven (`.changeset/`). Land PRs with a changeset
(`pnpm changeset`); the **Release (npm packages)** workflow keeps a
"Version Packages" PR open on `main` — merging it publishes to npm
(requires the `NPM_TOKEN` secret).

## 2. Service binaries (`binaries-v*`)

Stage 1 of the devnet flow — rebuilds pinned upstreams (metashrew,
flextrs, espo, the alkanes.wasm from the **pinned alkanes-rs develop
commit**, the JSON-RPC gateway bundle) and publishes them plus
`checksums.json` as a `binaries-v*` release on this repo.

```bash
git tag binaries-v0.2.0 && git push origin binaries-v0.2.0
```

**Afterwards** (manual, deliberate): bump `CHECKSUMS_URL` and
`isomer_release_base` in `crates/isomer-core/src/binary_manager.rs` to
the new tag — and, on the first monorepo release, from
`jonatns/isomer` to `jonatns/labcoat`. Commit that; app/CLI releases
built from it will download the new binaries. A `binaries-v*` release
without this bump changes nothing for users (that's a feature).

## 3. App + CLI (`isomer-v*`, `cli-v*`)

Stage 2. **`cli-v*` is the flagship track**; the desktop app is in
maintenance mode — tag `isomer-v*` only when an app release is actually
wanted. Bump versions first:

- Isomer app: `apps/isomer/package.json` + `apps/isomer/src-tauri/Cargo.toml`
- labcoat CLI: `crates/labcoat-cli/Cargo.toml` (+ `Cargo.lock` via `cargo check`)

```bash
git tag isomer-v0.2.0 && git push origin isomer-v0.2.0   # Tauri bundles
git tag cli-v0.7.0    && git push origin cli-v0.7.0      # labcoat binaries
```

Both produce **draft** releases — verify the assets, write notes, publish.

## Upgrading the alkanes-rs pin

A release unto itself. Follow TOOLCHAIN.md: update the rev everywhere
(workspace `Cargo.toml`, `cargo-template.ts`, `compile.rs`,
`release-binaries.yml`, `TOOLCHAIN.md`), rebuild `Cargo.lock`, run the
full loop against `labcoat up`, land as its own reviewed PR.
