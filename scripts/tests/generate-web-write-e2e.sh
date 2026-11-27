#!/usr/bin/env bash
# End-to-end check of wallet-signed writes in the client `labcoat generate
# web` writes, headless:
#
# 1. start a Labcoat Network of its own (its own data home and port, so a
#    network the developer is running is not touched);
# 2. create a project from the default template and deploy its counter;
# 3. generate the web client for it;
# 4. from Node, connect the dev wallet, send writes, wait for them, read the
#    new value back, and send one that reverts
#    (crates/labcoat-core/tests/fixtures/generate-web/e2e/write.e2e.test.ts).
#
# Needs a built CLI (`cargo build -p labcoat-cli`), the workspace's Node
# dependencies, the wasm32 Rust target, and the Labcoat runtime: the one
# already installed for this CLI version is reused, otherwise `labcoat up`
# downloads it. Set LABCOAT_BIN to test another binary and
# LABCOAT_RUNTIME_DIR to name a runtime directory.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
BIN=${LABCOAT_BIN:-"$ROOT/target/debug/labcoat"}
TSC="$ROOT/apps/web/node_modules/.bin/tsc"
E2E="$ROOT/crates/labcoat-core/tests/fixtures/generate-web/e2e"

[ -x "$BIN" ] || { echo "error: $BIN not found; run cargo build -p labcoat-cli" >&2; exit 1; }
[ -x "$TSC" ] || { echo "error: $TSC not found; run pnpm install --frozen-lockfile" >&2; exit 1; }

WORK=$(mktemp -d)
export LABCOAT_DATA_HOME="$WORK/data"
export LABCOAT_TEST_CRATE_PATH="${LABCOAT_TEST_CRATE_PATH:-$ROOT/crates/labcoat-test}"
LABCOAT_RPC_PORT=$(node -e 'const s=require("net").createServer().listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})')
export LABCOAT_RPC_PORT
export LABCOAT_RPC_URL="http://127.0.0.1:$LABCOAT_RPC_PORT"

if [ -z "${LABCOAT_RUNTIME_DIR:-}" ]; then
  VERSION=$("$BIN" --version | awk '{print $2}')
  case "$(uname -s)" in
    Darwin) INSTALLED="$HOME/Library/Application Support/Labcoat/runtimes/cli-v$VERSION" ;;
    *)      INSTALLED="${XDG_DATA_HOME:-$HOME/.local/share}/labcoat/runtimes/cli-v$VERSION" ;;
  esac
  if [ -d "$INSTALLED" ]; then export LABCOAT_RUNTIME_DIR="$INSTALLED"; fi
fi

cleanup() {
  (cd "$WORK/project" 2>/dev/null && "$BIN" down --json >/dev/null 2>&1) || true
  rm -rf "$WORK"
}
trap cleanup EXIT

cd "$WORK"
echo "write e2e: project"
"$BIN" init project --json >/dev/null
cd project
echo "write e2e: Labcoat Network on port $LABCOAT_RPC_PORT"
"$BIN" up --json >/dev/null
echo "write e2e: deploy the counter"
"$BIN" deploy --json >"$WORK/deploy.json" || { cat "$WORK/deploy.json" >&2; exit 1; }
echo "write e2e: generate the client"
"$BIN" generate web --out-dir "$WORK/web/generated" --json >/dev/null

cp "$E2E/write.e2e.test.ts" "$WORK/web/"
cat >"$WORK/web/tsconfig.json" <<JSON
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM"],
    "module": "node16",
    "moduleResolution": "node16",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "skipLibCheck": true,
    "outDir": "out",
    "typeRoots": ["$ROOT/apps/web/node_modules/@types"],
    "types": ["node"]
  },
  "include": ["generated/**/*.ts", "write.e2e.test.ts"]
}
JSON
echo "write e2e: writes from Node"
"$TSC" -p "$WORK/web/tsconfig.json"
node --test "$WORK/web/out/write.e2e.test.js"
