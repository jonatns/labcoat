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

echo "release validation tests passed"
