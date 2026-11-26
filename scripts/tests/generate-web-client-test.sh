#!/usr/bin/env bash
# Checks the TypeScript client `labcoat generate web` writes, using the
# golden tree the Rust test pins (crates/labcoat-core/tests/generate_web.rs):
#
# 1. it compiles under a strict browser configuration, and the type-level
#    checks in checks/types.ts hold (wrong method names and arguments are
#    compile errors);
# 2. the runtime checks in checks/runtime.test.ts pass against a stub fetch;
# 3. the write checks in checks/write.test.ts pass: for every case in
#    checks/write-vectors.json, which a Rust test records from the call path
#    behind `labcoat call`, the client builds the same transaction byte for
#    byte, and wallets, rejection, and waiting behave against stubs.
#
# Needs the workspace's Node dependencies (`pnpm install --frozen-lockfile`)
# for the TypeScript compiler; no network access.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
CHECKS="$ROOT/crates/labcoat-core/tests/fixtures/generate-web/checks"
TSC="$ROOT/apps/web/node_modules/.bin/tsc"

if [ ! -x "$TSC" ]; then
  echo "error: $TSC not found; run pnpm install --frozen-lockfile" >&2
  exit 1
fi

OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT

echo "generate web: type checks"
"$TSC" -p "$CHECKS/tsconfig.json"

echo "generate web: runtime checks"
"$TSC" -p "$CHECKS/tsconfig.runtime.json" --outDir "$OUT"
node --test "$OUT/checks/runtime.test.js"

echo "generate web: write checks"
LABCOAT_WRITE_VECTORS="$CHECKS/write-vectors.json" node --test "$OUT/checks/write.test.js"
