# Releasing Labcoat

Labcoat has two tag-driven Rust release tracks. Never release with a dirty
`Cargo.lock` or an unpinned `alkanes-rs` reference.

## CLI release (`cli-v*`)

Keep `labcoat-cli`, `labcoat-core`, and `labcoat-test` on the same version.
The generated project template must pin `labcoat-test` to that version too.

Before tagging:

```bash
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --locked -- -D warnings
cargo publish --locked -p labcoat-test --dry-run
./scripts/tests/install-labcoat-test.sh
```

Confirm `CARGO_REGISTRY_TOKEN` is configured, then tag:

```bash
git tag cli-v0.7.0
git push origin cli-v0.7.0
```

The workflow publishes `labcoat-test` first and then builds four CLI assets:

```text
labcoat-darwin-arm64
labcoat-darwin-x86_64
labcoat-linux-arm64
labcoat-linux-x86_64
```

Each binary must have a matching `.sha256` file. Verify the draft assets,
publish the release, and test both latest-version and explicit-version
installer paths. Windows CLI support is deferred.

After the first Rust CLI release is public and the installer has been
verified, retire the old npm entry points:

```bash
npm deprecate '@jonatns/labcoat@*' 'Retired; install the Rust CLI: https://github.com/jonatns/labcoat#readme'
npm deprecate 'create-labcoat@*' 'Retired; use labcoat init: https://github.com/jonatns/labcoat/blob/main/docs/MIGRATING.md'
```

## Service-binary release (`binaries-v*`)

This track rebuilds the pinned devnet dependencies and publishes them with
checksums:

```bash
git tag binaries-v0.2.0
git push origin binaries-v0.2.0
```

After publishing, deliberately update `CHECKSUMS_URL` and the service
release base in `crates/isomer-core/src/binary_manager.rs`. A binary release
does not affect the CLI until that reviewed URL and checksum change lands.

## Updating the Alkanes pin

Update the revision in the workspace, contract template, binary workflow,
and `TOOLCHAIN.md`. Refresh only affected lockfile entries, run the real
devnet and contract loop, and land the pin update separately.
