#!/usr/bin/env bash
# Install protocol stubs into the Labcoat runtime bin dir so `labcoat up
# --no-download` can exercise Labcoat Network without downloading Qubitcoin.
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

cp "$HERE/stub-qubitcoind" "$BIN_DIR/qubitcoind"
touch "$BIN_DIR/alkanes.wasm" "$BIN_DIR/esplorashrew.wasm"
chmod +x "$BIN_DIR/qubitcoind"

echo "Stubs installed to $BIN_DIR"
