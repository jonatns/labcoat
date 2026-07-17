# Contributing to Labcoat

Thank you for helping improve Labcoat. The project is pre-1.0 and focused on a deterministic local Alkanes development loop.

## Set up the workspace

Prerequisites are Rust 1.86.0, Node.js 22 or newer, pnpm 11.13.0, Docker, and the `wasm32-unknown-unknown` Rust target.

```sh
rustup target add wasm32-unknown-unknown
pnpm install --frozen-lockfile
cargo test --workspace
```

The workspace intentionally pins sensitive Alkanes and runtime revisions. Do not update pinned Git dependencies or downloaded-service versions as incidental cleanup; explain and validate each change.

## Generated files

Do not hand-edit these generated surfaces:

- `apps/web/src/generated/cli-reference.json`
- `apps/web/src/content/docs/docs/reference/cli.md`
- `apps/web/public/og.svg`
- `apps/web/public/og.png`

Regenerate them with the owning script, then commit the source and generated output together.

## Validate a change

```sh
cargo test -p labcoat-cli
cargo build -p labcoat-cli
node apps/web/scripts/sync-reference.mjs --bin ./target/debug/labcoat
node scripts/validate-brand.mjs
pnpm --dir apps/web check
pnpm --dir apps/web build
pnpm --dir apps/web test:e2e
bash scripts/test-installer.sh
bash scripts/release/validate-release.sh
git diff --check
```

Run the narrowest relevant checks while iterating and the complete affected suite before opening a pull request.

## Pull requests

- Keep changes scoped and document user-visible behavior, compatibility impact, and verification performed.
- Add tests for behavior changes. Preserve keyboard access, reduced-motion handling, and light/dark contrast in web changes.
- Update `CHANGELOG.md` for user-facing CLI changes.
- Treat CLI commands, JSON envelopes, MCP wire formats, and generated references as compatibility contracts.
- Do not describe planned mainnet, durable-state, hosted, team, or Windows capabilities as shipped.

## Release boundaries

Pull requests prepare releases; they do not replace published assets or manually move an existing tag. The release-plz workflow owns release preparation. Maintainers follow `docs/RELEASING.md`, verify the installer and attestations, and update stable-versus-main notices only after the new release is published and tested.

Security issues should follow [SECURITY.md](SECURITY.md), not the public issue tracker.
