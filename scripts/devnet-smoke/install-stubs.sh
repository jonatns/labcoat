#!/usr/bin/env bash
# Install protocol stubs into the Isomer bin dir so `labcoat up
# --no-download` can exercise the full orchestration path in environments
# where the real service binaries cannot be downloaded (sandboxed CI).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"

case "$(uname -s)" in
  Darwin) DATA_DIR="$HOME/Library/Application Support/Isomer" ;;
  *)      DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/Isomer" ;;
esac
BIN_DIR="$DATA_DIR/bin"
mkdir -p "$BIN_DIR/jsonrpc/bin"

cp "$HERE/stub-bitcoind" "$BIN_DIR/bitcoind"
for name in rockshrew-mono ord flextrs espo; do
  cp "$HERE/stub-service" "$BIN_DIR/$name"
done
cp "$HERE/stub-jsonrpc.js" "$BIN_DIR/jsonrpc/bin/jsonrpc.js"
# metashrew's indexer wasm just needs to exist for arg construction
touch "$BIN_DIR/alkanes.wasm"

chmod +x "$BIN_DIR"/bitcoind "$BIN_DIR"/rockshrew-mono "$BIN_DIR"/ord \
  "$BIN_DIR"/flextrs "$BIN_DIR"/espo

echo "Stubs installed to $BIN_DIR"
