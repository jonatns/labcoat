#!/usr/bin/env bash
# Install a stub indexer module into the Labcoat runtime bin dir so `labcoat
# up --no-download` can run Labcoat Network without downloading alkanes.wasm.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
VERSION="$("$ROOT/target/debug/labcoat" --version | awk '{print $2}')"

if [ -n "${LABCOAT_DATA_HOME:-}" ]; then
  case "$(uname -s)" in
    Darwin) DATA_DIR="$LABCOAT_DATA_HOME/Labcoat" ;;
    *)      DATA_DIR="$LABCOAT_DATA_HOME/labcoat" ;;
  esac
else
  case "$(uname -s)" in
    Darwin) DATA_DIR="$HOME/Library/Application Support/Labcoat" ;;
    *)      DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/labcoat" ;;
  esac
fi
BIN_DIR="$DATA_DIR/runtimes/cli-v$VERSION"
mkdir -p "$BIN_DIR"

cp "$HERE/stub-indexer.wat" "$BIN_DIR/alkanes.wasm"

echo "Stub indexer installed to $BIN_DIR"
