# Releasing Labcoat

Labcoat has one product release track. A `cli-vX.Y.Z` release contains the CLI,
its exact managed runtime, and the matching `labcoat-test` source. Generic `v*`
and `runtime-v*` tags are historical and must not be moved or reused.

## One-time repository setup

1. Allow GitHub Actions to create pull requests.
2. Protect `main` with the repository CI checks.
3. Enable immutable GitHub releases after a complete dry run succeeds.

No registry token, crates.io trusted publisher, or release environment is
required.

## Prepare a release

Normal merges to `main` do not create or update a release PR.

1. Open **Actions → Prepare Labcoat Release**.
2. Run the workflow from `main`.
3. Review the generated `release-plz-*` PR:
   - confirm the SemVer inferred from Conventional Commits;
   - edit the generated `CHANGELOG.md` section if user-facing context is
     missing;
   - confirm the generated CLI and MCP reference changes;
   - wait for the normal CI checks.
4. Merge the release PR when the product is ready. The merge is the publication
   approval.

If there are no releasable changes, Release-plz does not create a PR. Re-running
the preparation workflow updates the existing release PR instead of opening
another one.

## What publication does

The **Release Labcoat** workflow runs only for a merged, labeled
`release-plz-*` PR. Before creating a public release it:

1. validates the workspace version, changelog, runtime source pins, and release
   target;
2. builds the Qubitcoin executable for macOS arm64 and Linux x86_64;
3. builds the Alkanes and Esplorashrew WASM modules;
4. generates `runtime-manifest.json` with exact sizes and SHA-256 checksums;
5. builds the CLI for macOS and Linux on arm64 and x86_64;
6. runs the Linux runtime, wallet, contract, deployment, simulation, and trace
   acceptance flow against the assembled local artifacts;
7. creates a draft, uploads and attests all thirteen assets, then publishes the
   immutable `cli-vX.Y.Z` release;
8. installs the published CLI and verifies that generated projects pin
   `labcoat-test` to the same Git tag.

Run **Release Labcoat** manually with `dry_run=true` to build and validate the
complete product without creating a tag or GitHub release.

## Completion checklist

- `labcoat --version` matches the release.
- The installer selects the new `cli-v*` release and verifies its checksum.
- `labcoat up` downloads `runtime-manifest.json` and assets from that same tag.
- A second `labcoat up` uses the cached versioned bundle without a release
  lookup.
- `labcoat init` writes the matching `labcoat-test` Git tag.
- All thirteen assets have GitHub build-provenance attestations.
- Release notes match the version section in `CHANGELOG.md`.

## Failure policy

- No public release is created unless every build and runtime acceptance job
  succeeds.
- A failed publication may leave a draft. Fix the cause and rerun against the
  same commit; draft assets may be replaced, published assets may not.
- Never move a published tag or replace a published asset. Prepare a new patch
  release instead.
- Runtime source changes are ordinary product changes. They become available to
  users only in the next CLI release; the CLI never adopts an independently
  newer runtime.
- Old versioned runtime caches and historical releases are not deleted
  automatically.
