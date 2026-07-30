#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$ROOT"

node scripts/release/validate-release.mjs >/dev/null
[ "$(node scripts/release/validate-release.mjs --workspace-version)" = "0.1.0" ]
node scripts/release/validate-release.mjs --validate-cli-tag cli-v0.1.0
node scripts/release/validate-release.mjs --validate-runtime-tag runtime-v2026.07.16.1

if node scripts/release/validate-release.mjs --validate-cli-tag v0.1.0 2>/dev/null; then
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

expected_trigger=$(node scripts/release/update-release-trigger.mjs --print)
if ! printf '%s\n' "$expected_trigger" | grep -Eq '^[0-9a-f]{64}$'; then
    echo "release trigger printer did not emit a SHA-256 digest" >&2
    exit 1
fi
if ! cmp -s "$trigger" "$saved_trigger"; then
    echo "release trigger printer modified the trigger file" >&2
    exit 1
fi
if node scripts/release/update-release-trigger.mjs >/dev/null 2>&1; then
    echo "release trigger updater accepted a missing mode" >&2
    exit 1
fi
if node scripts/release/update-release-trigger.mjs --unknown >/dev/null 2>&1; then
    echo "release trigger updater accepted an unknown mode" >&2
    exit 1
fi

printf '%064d\n' 0 > "$trigger"
if ! node scripts/release/validate-release.mjs >/dev/null; then
    echo "well-formed stale release trigger was rejected" >&2
    exit 1
fi
printf 'not-a-digest\n' > "$trigger"
if node scripts/release/validate-release.mjs >/dev/null 2>&1; then
    echo "malformed release trigger was accepted" >&2
    exit 1
fi

node scripts/release/update-release-trigger.mjs --write
if [ "$(cat "$trigger")" != "$expected_trigger" ]; then
    echo "release trigger updater wrote an unexpected digest" >&2
    exit 1
fi
node scripts/release/validate-release.mjs >/dev/null
generated_trigger=$(mktemp)
cp "$trigger" "$generated_trigger"
node scripts/release/update-release-trigger.mjs --write
if ! cmp -s "$trigger" "$generated_trigger"; then
    echo "release trigger updater is not idempotent" >&2
    rm -f "$generated_trigger"
    exit 1
fi
rm -f "$generated_trigger"

cp "$saved_trigger" "$trigger"
rm -f "$saved_trigger"
trap - EXIT HUP INT TERM

echo "release validation tests passed"
