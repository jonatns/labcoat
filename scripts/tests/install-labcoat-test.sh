#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
LABCOAT_INSTALLER_SOURCE_ONLY=1 . "$ROOT/install-labcoat.sh"

[ "$(LABCOAT_UNAME_S=Darwin LABCOAT_UNAME_M=arm64 detect_platform)" = "darwin-arm64" ]
[ "$(LABCOAT_UNAME_S=Darwin LABCOAT_UNAME_M=x86_64 detect_platform)" = "darwin-x86_64" ]
[ "$(LABCOAT_UNAME_S=Linux LABCOAT_UNAME_M=x86_64 detect_platform)" = "linux-x86_64" ]
[ "$(LABCOAT_UNAME_S=Linux LABCOAT_UNAME_M=aarch64 detect_platform)" = "linux-arm64" ]
if LABCOAT_UNAME_S=Plan9 LABCOAT_UNAME_M=x86_64 detect_platform >/dev/null 2>&1; then
    echo "unsupported platform was accepted" >&2
    exit 1
fi
[ "$(normalize_version 0.1.0)" = "cli-v0.1.0" ]
[ "$(normalize_version v0.1.0)" = "cli-v0.1.0" ]

curl() {
    printf '%s\n' \
        '[{"tag_name":"isomer-v9.9.9"},' \
        '{"tag_name":"cli-v0.8.1"},' \
        '{"tag_name":"cli-v0.1.0"}]'
}
[ "$(latest_version)" = "cli-v0.8.1" ]

tmp=$(mktemp -d "${TMPDIR:-/tmp}/labcoat-installer-test.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
printf 'binary' > "$tmp/labcoat"
sha256_file "$tmp/labcoat" > "$tmp/labcoat.sha256"
verify_checksum "$tmp/labcoat" "$tmp/labcoat.sha256"
printf 'bad' > "$tmp/labcoat.sha256"
if verify_checksum "$tmp/labcoat" "$tmp/labcoat.sha256" 2>/dev/null; then
    echo "corrupt checksum was accepted" >&2
    exit 1
fi

INSTALL_DIR="$tmp/not-on-path"
hint=$(path_hint)
case "$hint" in
    *"export PATH="*) ;;
    *) echo "PATH hint missing" >&2; exit 1 ;;
esac

# A missing release asset must abort without leaving an installed binary.
curl() {
    out=""
    while [ "$#" -gt 0 ]; do
        if [ "$1" = "-o" ]; then out="$2"; shift 2; else shift; fi
    done
    [ -n "$out" ] || return 1
    case "$out" in
        *.sha256) return 22 ;;
        *) printf 'binary' > "$out" ;;
    esac
}
INSTALL_DIR="$tmp/missing-asset"
if (LABCOAT_UNAME_S=Darwin LABCOAT_UNAME_M=arm64 main --version 0.1.0) >/dev/null 2>&1; then
    echo "installer accepted a release with a missing checksum asset" >&2
    exit 1
fi
[ ! -e "$INSTALL_DIR/labcoat" ]

echo "install-labcoat tests passed"
