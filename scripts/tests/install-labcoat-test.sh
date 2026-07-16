#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
LABCOAT_INSTALLER_SOURCE_ONLY=1 . "$ROOT/install-labcoat.sh"

[ "$(LABCOAT_UNAME_S=Darwin LABCOAT_UNAME_M=arm64 detect_platform)" = "darwin-arm64" ]
[ "$(LABCOAT_UNAME_S=Darwin LABCOAT_UNAME_M=x86_64 detect_platform)" = "darwin-x86_64" ]
[ "$(LABCOAT_UNAME_S=Linux LABCOAT_UNAME_M=x86_64 detect_platform)" = "linux-x86_64" ]
[ "$(LABCOAT_UNAME_S=Linux LABCOAT_UNAME_M=aarch64 detect_platform)" = "linux-arm64" ]
[ "$(normalize_version 0.7.0)" = "cli-v0.7.0" ]
[ "$(normalize_version v0.7.0)" = "cli-v0.7.0" ]

curl() {
    printf '%s\n' \
        '[{"tag_name":"isomer-v9.9.9"},' \
        '{"tag_name":"cli-v0.8.1"},' \
        '{"tag_name":"cli-v0.7.0"}]'
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

echo "install-labcoat tests passed"
