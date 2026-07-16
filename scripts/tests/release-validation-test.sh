#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$ROOT"

node scripts/release/validate-release.mjs >/dev/null
[ "$(node scripts/release/validate-release.mjs --workspace-version)" = "0.7.0" ]
node scripts/release/validate-release.mjs --validate-cli-tag cli-v0.7.0
node scripts/release/validate-release.mjs --validate-runtime-tag runtime-v2026.07.16.1

if node scripts/release/validate-release.mjs --validate-cli-tag v0.7.0 2>/dev/null; then
    echo "generic CLI tag was accepted" >&2
    exit 1
fi
if node scripts/release/validate-release.mjs --validate-runtime-tag runtime-v1.2.3 2>/dev/null; then
    echo "malformed runtime tag was accepted" >&2
    exit 1
fi
if node scripts/release/validate-release.mjs --expect-runtime-source-digest deadbeef 2>/dev/null; then
    echo "runtime promotion accepted a stale source digest" >&2
    exit 1
fi

trigger=crates/labcoat-test/RELEASE_TRIGGER
saved_trigger=$(mktemp)
cp "$trigger" "$saved_trigger"
trap 'cp "$saved_trigger" "$trigger"; rm -f "$saved_trigger"' EXIT HUP INT TERM
printf '%064d\n' 0 > "$trigger"
if node scripts/release/validate-release.mjs 2>/dev/null; then
    echo "stale release trigger was accepted" >&2
    exit 1
fi
cp "$saved_trigger" "$trigger"
rm -f "$saved_trigger"
trap - EXIT HUP INT TERM

echo "release validation tests passed"
