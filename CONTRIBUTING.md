# Contributing to Labcoat

Thank you for helping improve Labcoat. The project is pre-1.0 and focused on a deterministic local Alkanes development loop.

## Set up the workspace

CLI development requires Rust 1.86.0 plus the `wasm32-unknown-unknown` and
`wasm32-wasip1` targets. Website work additionally requires Node.js 22 or newer
and pnpm 11.13.0. Docker is not required.

```sh
rustup target add wasm32-unknown-unknown wasm32-wasip1
pnpm install --frozen-lockfile
cargo test --workspace --locked
```

The workspace intentionally pins sensitive Alkanes and runtime revisions. Do not update pinned Git dependencies or downloaded-service versions as incidental cleanup; explain and validate each change.

## Generated files

Do not hand-edit these generated surfaces:

- `apps/web/src/generated/cli-reference.json`
- `apps/web/src/content/docs/docs/reference/cli.md`
- `apps/web/public/og.svg`
- `apps/web/public/og.png`

Regenerate the web surfaces with their owning scripts, then commit the source
and generated output together.

## Validate a change

```sh
cargo test -p labcoat-cli
cargo build --locked -p labcoat-cli
node apps/web/scripts/sync-reference.mjs --bin ./target/debug/labcoat
node scripts/validate-brand.mjs
node scripts/release/validate-release.mjs
node scripts/tests/runtime-manifest-test.mjs
pnpm --filter @labcoat/web check
pnpm --filter @labcoat/web build
pnpm --filter @labcoat/web test:e2e
./scripts/tests/install-labcoat-test.sh
./scripts/tests/release-validation-test.sh
git diff --check
```

Run the narrowest relevant checks while iterating and the complete affected suite before opening a pull request.

## Pull requests

- Keep changes scoped and document user-visible behavior, compatibility impact, and verification performed.
- Add tests for behavior changes. Preserve keyboard access, reduced-motion handling, and light/dark contrast in web changes.
- Use Conventional Commit subjects for user-facing changes; the on-demand
  release PR generates the changelog for review.
- Treat CLI commands, JSON envelopes, MCP wire formats, and generated references as compatibility contracts.
- Do not describe planned mainnet, durable-state, hosted, team, or Windows capabilities as shipped.

## Release boundaries

Normal merges never prepare or publish a release. When the product is ready, a
maintainer manually runs **Prepare Labcoat Release**, reviews its single
Release-plz PR, and merges that PR to publish the CLI, runtime, and test harness
as one `cli-vX.Y.Z` release. Never replace published assets or move an existing
tag.

Security issues should follow [SECURITY.md](SECURITY.md), not the public issue tracker.
