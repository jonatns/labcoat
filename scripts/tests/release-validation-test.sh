#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$ROOT"

node scripts/release/validate-release.mjs >/dev/null
workspace_version=$(node scripts/release/validate-release.mjs --workspace-version)
if ! printf '%s\n' "$workspace_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    echo "workspace version is not valid SemVer: $workspace_version" >&2
    exit 1
fi
node scripts/release/validate-release.mjs --validate-cli-tag "cli-v$workspace_version"

if node scripts/release/validate-release.mjs --validate-cli-tag v0.1.0 2>/dev/null; then
    echo "generic CLI tag was accepted" >&2
    exit 1
fi
if node scripts/release/validate-release.mjs --validate-cli-tag cli-v9.9.9 2>/dev/null; then
    echo "mismatched CLI tag was accepted" >&2
    exit 1
fi

source_digest=$(node scripts/release/validate-release.mjs --runtime-source-digest)
if ! printf '%s\n' "$source_digest" | grep -Eq '^[0-9a-f]{64}$'; then
    echo "runtime source digest is invalid: $source_digest" >&2
    exit 1
fi

for obsolete in \
    runtime.json \
    crates/labcoat-test/RELEASE_TRIGGER \
    scripts/release/update-release-trigger.mjs \
    .github/workflows/release-runtime.yml \
    .github/workflows/runtime-acceptance.yml
do
    if [ -e "$obsolete" ]; then
        echo "obsolete release file still exists: $obsolete" >&2
        exit 1
    fi
done

node scripts/tests/runtime-manifest-test.mjs >/dev/null
echo "release validation tests passed"
