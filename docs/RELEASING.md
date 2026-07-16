# Releasing Labcoat

Labcoat has two independent release tracks. Generic `v*` tags are historical
and must not be reused.

## One-time repository setup

1. Create a GitHub environment named `release`.
2. If `labcoat-test` has never been published, an owner must publish version
   `0.7.0` once with `cargo publish --locked -p labcoat-test` to claim the
   crate. crates.io requires this initial publication before trusted
   publishing can be configured. Do not merge the bootstrap release PR first.
3. In the `labcoat-test` crates.io settings, add a trusted publisher for this
   repository, `.github/workflows/release-cli.yml`, and the `release`
   environment. No `CARGO_REGISTRY_TOKEN` secret is needed.
4. Allow GitHub Actions to create pull requests.
5. Enable immutable GitHub releases after the first CLI and runtime dry runs
   succeed. Published assets and tags then cannot be replaced.
6. Protect `main` with the repository CI checks.

Actions are pinned to full commit SHAs. Dependabot proposes grouped weekly
updates to those pins.

The previous `release.yml` and `release-binaries.yml` workflows are retained
temporarily as a rollback path. Do not invoke them for a new release. Remove
them only after both new dry runs and the first `cli-v0.7.0` publication have
succeeded.

## CLI release (`cli-vX.Y.Z`)

Release-plz maintains one release PR from Conventional Commit history. The
three Labcoat crates share the version in `[workspace.package]`, but only
`labcoat-test` is published to crates.io.

Because Cargo correctly marks the CLI and core packages non-publishable,
Release-plz tracks them through the deterministic `labcoat-test/RELEASE_TRIGGER`
digest refreshed by the PR workflow. This makes changes in any of the three
packages part of the one release without weakening their publish settings.

1. Review the bot's `release-plz-*` PR and generated `CHANGELOG.md`.
2. Approve its GitHub Actions run when prompted. Bot-created PR workflows need
   this approval because the bot uses the built-in `GITHUB_TOKEN`.
3. Merge the PR. The merge is the publication approval.

The CLI release workflow builds four native binaries, verifies their embedded
version, creates checksums and attestations, uploads all eight assets to a
draft, publishes `labcoat-test` through crates.io OIDC, then publishes the
GitHub release as latest. Reruns are safe only when the existing tag points to
the same merge commit.

Run **Release Labcoat CLI** manually with `dry_run=true` to exercise all builds
without creating a tag, release, or crate version.

The first release on this track is a bootstrap release PR that keeps version
`0.7.0`; merging it creates `cli-v0.7.0`.

## Runtime release (`runtime-vYYYY.MM.DD.N`)

`runtime.json` is the reviewed source of truth for active downloads, exact
upstream refs, component metadata, supported platforms, and legacy checksums.
Never use `main`, `master`, `develop`, `trunk`, or `HEAD` as a source ref.

1. Change upstream refs and versions in `runtime.json` through a normal PR.
2. Merge that PR and run **Build and release runtime bundle** from `main`.
3. Start with `dry_run=true`. When it passes, rerun with `dry_run=false`.
4. Review the generated `runtime-promotion/*` PR and merge it when ready.

The workflow calculates the calendar version, builds only the assets used by
the CLI, creates a checksum file and machine-readable release manifest,
attests and publishes the runtime release without marking it latest, then opens
the promotion PR. Promotion aborts if the source section changed after the
build began.

The active legacy bundle remains `jonatns/isomer@binaries-v0.1.3` until the
first promotion PR is merged.

## Legacy desktop app

The Isomer desktop application has no automatic release trigger. Use **Build
legacy Isomer desktop** only for a deliberate maintenance build. It can create
a draft `isomer-v*` release and can optionally attempt the standalone browser
extension build. Desktop and extension artifacts never enter CLI runtime
releases.

## Failure policy

- A missing or invalid checksum is fatal; runtime downloads never continue
  unverified.
- Never move a published tag or replace a published asset. Issue a new patch
  CLI release or a new runtime calendar sequence instead.
- A failed crates.io publication leaves the GitHub release as a draft. Fix the
  cause and rerun the same workflow.
- Runtime builds and promotions are independent from CLI SemVer.
