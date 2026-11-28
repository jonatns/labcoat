#!/usr/bin/env bash
# Serves a page that signs writes from the generated web client through real
# wallet libraries, for the checks no automated test can make: the LaserEyes
# and Sats Connect recipes on the Web apps page (copied from the page as
# written), and the built-in extension adapters.
#
#   generate-web-wallet-check.sh
#       Labcoat Network: starts a network, deploys the counter, and serves
#       the page. Open it with ?sim=unisat,xverse for stand-in extensions
#       that sign with the dev signer, so the libraries run end to end.
#
#   generate-web-wallet-check.sh signet <contract-id> [fee-rate]
#       Signet, through Subfrost's endpoint (SUBFROST_API_KEY is forwarded
#       when set): for real extensions. <contract-id> is a deployed counter
#       from `labcoat init`, such as 2:1213. This spends signet coins from
#       the wallet you connect.
#
# Needs `cargo build -p labcoat-cli`, Node, and npm with network access: it
# installs React, Vite, sats-connect, and LaserEyes into a temporary folder.
# Open the address it prints in the browser that has the wallets; stop it
# with Ctrl-C.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
BIN=${LABCOAT_BIN:-"$ROOT/target/debug/labcoat"}
APP="$ROOT/crates/labcoat-core/tests/fixtures/generate-web/wallets"
MODE=${1:-labcoat}
LASEREYES_VERSION=0.0.163

[ -x "$BIN" ] || { echo "error: $BIN not found; run cargo build -p labcoat-cli" >&2; exit 1; }

WORK=${WALLET_CHECK_DIR:-$(mktemp -d)}
mkdir -p "$WORK"
export LABCOAT_DATA_HOME="$WORK/data"
export LABCOAT_TEST_CRATE_PATH="${LABCOAT_TEST_CRATE_PATH:-$ROOT/crates/labcoat-test}"

cleanup() {
  if [ "$MODE" = labcoat ]; then
    (cd "$WORK/project" 2>/dev/null && "$BIN" down --json >/dev/null 2>&1) || true
  fi
  if [ -z "${WALLET_CHECK_DIR:-}" ]; then rm -rf "$WORK"; fi
}
trap cleanup EXIT

cd "$WORK"
[ -d project ] || "$BIN" init project --json >/dev/null
cd project
mkdir -p "$WORK/web"
cp -R "$APP/." "$WORK/web/"

case "$MODE" in
  labcoat)
    LABCOAT_RPC_PORT=$(node -e 'const s=require("net").createServer().listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})')
    export LABCOAT_RPC_PORT
    export LABCOAT_RPC_URL="http://127.0.0.1:$LABCOAT_RPC_PORT"
    if [ -z "${LABCOAT_RUNTIME_DIR:-}" ]; then
      VERSION=$("$BIN" --version | awk '{print $2}')
      INSTALLED="$HOME/Library/Application Support/Labcoat/runtimes/cli-v$VERSION"
      [ "$(uname -s)" = Darwin ] || INSTALLED="${XDG_DATA_HOME:-$HOME/.local/share}/labcoat/runtimes/cli-v$VERSION"
      if [ -d "$INSTALLED" ]; then export LABCOAT_RUNTIME_DIR="$INSTALLED"; fi
    fi
    echo "wallet check: Labcoat Network on port $LABCOAT_RPC_PORT"
    "$BIN" up --json >/dev/null
    "$BIN" deploy --json >"$WORK/deploy.json" || { cat "$WORK/deploy.json" >&2; exit 1; }
    "$BIN" generate web --out-dir "$WORK/web/src/generated" --json >/dev/null
    export LABCOAT_RPC_TARGET="$LABCOAT_RPC_URL"
    VITE_CONTRACT=$(node -e '
      const text = require("fs").readFileSync(process.argv[1], "utf8")
      console.log(/id: "(\d+:\d+)"/.exec(text)[1])' "$WORK/web/src/generated/manifest.ts")
    ;;
  signet)
    VITE_CONTRACT=${2:?"usage: $0 signet <contract-id> [fee-rate]"}
    export VITE_FEE_RATE=${3:-2}
    "$BIN" build --json >/dev/null
    "$BIN" generate web --network signet --out-dir "$WORK/web/src/generated" --json >/dev/null
    if [ -z "${LABCOAT_RPC_TARGET:-}" ]; then
      LABCOAT_RPC_TARGET=$(node -e '
        const text = require("fs").readFileSync(process.argv[1], "utf8")
        console.log(/rpcUrl: "([^"]+)"/.exec(text)[1])' "$WORK/web/src/generated/manifest.ts")
    fi
    case "$LABCOAT_RPC_TARGET" in
      http://*|https://*) ;;
      *) echo "error: set LABCOAT_RPC_TARGET to the upstream RPC URL" >&2; exit 1 ;;
    esac
    export LABCOAT_RPC_TARGET
    ;;
  *)
    echo "usage: $0 [labcoat | signet <contract-id> [fee-rate]]" >&2
    exit 1
    ;;
esac
export VITE_CONTRACT

cd "$WORK/web"
node extract-recipes.mjs "$ROOT/apps/web/src/content/docs/docs/web-apps.md" src/recipes
echo "wallet check: installing the wallet libraries"
npm install --no-audit --no-fund --ignore-scripts --silent
# LaserEyes ships self-contained bundles, and one of its dependencies no
# longer installs, so the published tarballs are unpacked as they are.
for package in lasereyes lasereyes-core lasereyes-react; do
  version=$(npm view "@omnisat/lasereyes@$LASEREYES_VERSION" "dependencies.@omnisat/$package" 2>/dev/null || true)
  [ "$package" = lasereyes ] && version=$LASEREYES_VERSION
  mkdir -p "node_modules/@omnisat/$package"
  npm pack --silent "@omnisat/$package@$version" >/dev/null
  tar -xzf omnisat-"$package"-"$version".tgz -C "node_modules/@omnisat/$package" --strip-components 1
done
echo "wallet check: type checks"
npx tsc -p tsconfig.json
echo "wallet check: $MODE, contract $VITE_CONTRACT"
echo "wallet check: open http://localhost:${WALLET_CHECK_PORT:-5199}/ in the browser that has the wallets"
npx vite
