# Releasing from the monorepo

Three release tracks, all tag driven. Never release with a dirty
`Cargo.lock` or unpinned alkanes-rs refs.

## 1. Rust test crate (`cli-v*`)

The CLI tag publishes `labcoat-test` to crates.io before building native
binaries. `CARGO_REGISTRY_TOKEN` must exist. `labcoat-core`,
`labcoat-cli`, and `labcoat-test` share one version (0.7.0 for the first
Rust-only release). The retired npm packages are not released.

## 2. Service binaries (`binaries-v*`)

This rebuilds pinned upstreams (metashrew, flextrs, espo, alkanes.wasm,
and the JSON-RPC gateway bundle) and publishes them with
`checksums.json`:

```bash
git tag binaries-v0.2.0 && git push origin binaries-v0.2.0
```

Afterward, deliberately bump `CHECKSUMS_URL` and
`isomer_release_base` in `crates/isomer-core/src/binary_manager.rs`.
A service-binary release changes nothing until that reviewed bump lands.

## 3. App + CLI (`isomer-v*`, `cli-v*`)

Stage 2. **`cli-v*` is the flagship track**; the desktop app is in
maintenance mode — tag `isomer-v*` only when an app release is actually
wanted. Bump versions first:

- Isomer: `apps/isomer/package.json` and its Tauri `Cargo.toml`.
- Labcoat: `labcoat-cli`, `labcoat-core`, and `labcoat-test` together,
  followed by a locked workspace check.

```bash
git tag isomer-v0.2.0 && git push origin isomer-v0.2.0
git tag cli-v0.7.0    && git push origin cli-v0.7.0
```

Both workflows create draft releases. Verify and publish them; the
installer only discovers published `cli-v*` releases. CLI support is
macOS/Linux on arm64 and x86_64. Windows CLI support is deferred.

After the first Rust CLI release is published and its installer link has
been verified, retire the old npm entry points with migration links:

```bash
npm deprecate '@jonatns/labcoat@*' 'Retired; install the Rust CLI: https://github.com/jonatns/labcoat#readme'
npm deprecate 'create-labcoat@*' 'Retired; use labcoat init: https://github.com/jonatns/labcoat/blob/main/docs/MIGRATING.md'
```

## Upgrading the alkanes-rs pin

Update the rev in the workspace, Rust compiler template,
`release-binaries.yml`, and `TOOLCHAIN.md`; rebuild only the affected
lockfile entries, run the full devnet loop, and land it separately.
