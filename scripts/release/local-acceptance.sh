#!/usr/bin/env bash
# Run the release acceptance flow on this machine, from this checkout:
#
#   scripts/release/local-acceptance.sh [--boot-only] [--release] [--indexer <alkanes.wasm>]
#
# It builds the CLI, builds the Alkanes indexer module from the revision
# pinned in crates/labcoat-cli/runtime-sources.json (once; later runs reuse
# it), describes it with a runtime manifest, and hands both to
# scripts/release/acceptance.sh.
#
# Nothing of yours is touched: the run uses a scratch data home and its own
# port, so a Labcoat Network you already have running keeps running.
#
#   --boot-only        boot, fund, mine, and stop; skips the project steps
#   --release          build the CLI with --release, as the release does
#   --indexer <file>   use this alkanes.wasm instead of building one
#
# Environment:
#   LABCOAT_RPC_PORT   port for the scratch network (default 18555)
#   LABCOAT_ACCEPTANCE_DIR  where the indexer build is kept
#                      (default target/acceptance)
#
# Needs: cargo and rustup, clang with a wasm32 backend, protoc, node, jq,
# curl, and git. `labcoat doctor` checks the first three once the CLI is
# built.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cache="${LABCOAT_ACCEPTANCE_DIR:-$root/target/acceptance}"
profile=debug
indexer=""
passthrough=()
while [ $# -gt 0 ]; do
  case "$1" in
    --boot-only) passthrough+=(--boot-only); shift ;;
    --release) profile=release; shift ;;
    --indexer) indexer="${2:?--indexer needs a file}"; shift 2 ;;
    -h|--help) sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

step() { printf '\n== %s\n' "$*" >&2; }
for tool in cargo node jq curl git; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 2; }
done

step "build the CLI ($profile)"
cd "$root"
if [ "$profile" = release ]; then
  cargo build --locked --release -p labcoat-cli
else
  cargo build --locked -p labcoat-cli
fi
labcoat="$root/target/$profile/labcoat"
version=$("$labcoat" --version | awk '{print $2}')

sources="$root/crates/labcoat-cli/runtime-sources.json"
repository=$(jq -er '.sources["alkanes-wasm"].repository' "$sources")
revision=$(jq -er '.sources["alkanes-wasm"].revision' "$sources")
bundle="$cache/bundle-$revision"
mkdir -p "$bundle"

if [ -n "$indexer" ]; then
  step "use the indexer at $indexer"
  cp "$indexer" "$bundle/alkanes.wasm"
elif [ -f "$bundle/alkanes.wasm" ]; then
  step "reuse the indexer built from $repository@${revision:0:7}"
else
  step "build the indexer from $repository@${revision:0:7} (several minutes, once)"
  src="$cache/alkanes-src"
  if [ ! -d "$src/.git" ]; then
    git init -q "$src"
    git -C "$src" remote add origin "https://github.com/$repository"
  fi
  git -C "$src" fetch -q --depth 1 origin "$revision"
  git -C "$src" checkout -q --detach FETCH_HEAD
  # Its C dependencies need a clang that can target wasm32, which Apple's
  # does not. Homebrew's LLVM does.
  if [ "$(uname -s)" = Darwin ] && [ -z "${CC_wasm32_unknown_unknown:-}" ] && command -v brew >/dev/null; then
    llvm="$(brew --prefix llvm 2>/dev/null || true)"
    if [ -x "$llvm/bin/clang" ]; then
      export CC_wasm32_unknown_unknown="$llvm/bin/clang"
      export AR_wasm32_unknown_unknown="$llvm/bin/llvm-ar"
    fi
  fi
  # The same command the release workflow runs, with that repository's own
  # pinned toolchain.
  (cd "$src" && cargo build --locked --release --target wasm32-unknown-unknown --features regtest -p alkanes)
  cp "$src/target/wasm32-unknown-unknown/release/alkanes.wasm" "$bundle/alkanes.wasm"
fi

step "describe the bundle for labcoat $version"
node "$root/scripts/release/build-runtime-manifest.mjs" --version "$version" --directory "$bundle" >&2

data_home=$(mktemp -d)
trap 'rm -rf "$data_home"' EXIT
export LABCOAT_DATA_HOME="$data_home"
export LABCOAT_RPC_PORT="${LABCOAT_RPC_PORT:-18555}"
# Projects the run scaffolds use this checkout's test and deploy crates:
# the release tag they would otherwise pin does not exist yet.
export LABCOAT_TEST_CRATE_PATH="$root/crates/labcoat-test"

step "acceptance on port $LABCOAT_RPC_PORT, data home $LABCOAT_DATA_HOME"
"$root/scripts/release/acceptance.sh" "$labcoat" --bundle "$bundle" ${passthrough[@]+"${passthrough[@]}"}
