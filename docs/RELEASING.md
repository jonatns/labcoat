# Releasing Labcoat

Labcoat has one product release track. A `cli-vX.Y.Z` release contains the CLI,
its exact managed runtime, and the matching `labcoat-deploy` and `labcoat-test` source. Generic `v*`
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
2. Run the workflow from `main` and enter the next SemVer version.
3. Review the generated `release-plz-*` PR:
   - confirm the requested SemVer and generated changelog heading;
   - edit the generated `CHANGELOG.md` section if user-facing context is
     missing;
   - confirm the generated CLI and MCP reference changes;
   - wait for the normal CI checks.
4. Merge the release PR when the product is ready. The merge is the publication
   approval.

Do not run the preparation workflow while another release PR is open.

## What publication does

The **Release Labcoat** workflow runs only for a merged, labeled
`release-plz-*` PR. Before creating a public release it:

1. validates the workspace version, changelog, runtime source pins, and release
   target;
2. builds the Alkanes indexer module, `alkanes.wasm`, from its pinned source;
3. generates `runtime-manifest.json` with its exact size and SHA-256 checksum;
4. builds the CLI, which contains Labcoat Network, for macOS and Linux on arm64
   and x86_64;
5. runs `scripts/release/acceptance.sh` against the assembled Linux x86_64
   executable: boot, `init`, `test`, `deploy`, `call`, `trace`, a command from
   a subdirectory, `test --e2e` beside the running network, and a restart,
   checking every JSON envelope and the Counter's decoded value. It then boots
   Labcoat Network from the published executable of each of the four targets
   (`--boot-only`);
6. creates a draft, uploads and attests all ten assets, then publishes the
   immutable `cli-vX.Y.Z` release;
7. installs the published CLI and verifies that generated projects pin
   `labcoat-deploy` and `labcoat-test` to the same Git tag.

Run **Release Labcoat** manually with `dry_run=true` to build and validate the
complete product without creating a tag or GitHub release.

## Run the acceptance flow locally

The acceptance script takes any `labcoat` executable. Give it a scratch data
home, and a port of its own when a Labcoat Network is already running on
18443, so it never touches the network you develop on:

```bash
cargo build -p labcoat-cli
export LABCOAT_DATA_HOME=$(mktemp -d) LABCOAT_RPC_PORT=18555
export LABCOAT_TEST_CRATE_PATH=$PWD/crates/labcoat-test
node scripts/release/build-runtime-manifest.mjs --version 0.3.0 --directory <bundle>
scripts/release/acceptance.sh target/debug/labcoat --bundle <bundle>
```

`<bundle>` is a directory holding an `alkanes.wasm` you built or downloaded;
any module built with the `regtest` feature from the revision in
`crates/labcoat-cli/runtime-sources.json` works. The manifest script
describes it for the workspace version you pass. Leave `--bundle` out to
accept a published release, which downloads its own runtime. `--boot-only` stops after
booting, funding, and mining, and needs no Rust toolchain.

## Completion checklist

- `labcoat --version` matches the release.
- The installer selects the new `cli-v*` release and verifies its checksum.
- `labcoat up` downloads `runtime-manifest.json` and `alkanes.wasm` from that
  same tag.
- A second `labcoat up` uses the cached versioned bundle without a release
  lookup.
- `labcoat init` writes the matching `labcoat-deploy` and `labcoat-test` Git tag.
- All ten assets have GitHub build-provenance attestations.
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
