#!/bin/sh
set -eu

REPO="${LABCOAT_REPO:-jonatns/labcoat}"
INSTALL_DIR="${LABCOAT_INSTALL_DIR:-$HOME/.local/bin}"

detect_platform() {
    os="${LABCOAT_UNAME_S:-$(uname -s)}"
    arch="${LABCOAT_UNAME_M:-$(uname -m)}"
    case "$os" in
        Darwin) os="darwin" ;;
        Linux) os="linux" ;;
        *) echo "Unsupported operating system: $os" >&2; return 1 ;;
    esac
    case "$arch" in
        arm64|aarch64) arch="arm64" ;;
        x86_64|amd64) arch="x86_64" ;;
        *) echo "Unsupported architecture: $arch" >&2; return 1 ;;
    esac
    printf '%s-%s\n' "$os" "$arch"
}

normalize_version() {
    case "$1" in
        cli-v*) printf '%s\n' "$1" ;;
        v*) printf 'cli-%s\n' "$1" ;;
        *) printf 'cli-v%s\n' "$1" ;;
    esac
}

latest_version() {
    curl -fsSL "https://api.github.com/repos/$REPO/releases?per_page=100" \
        | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\(cli-v[^"]*\)".*/\1/p' \
        | head -n 1
}

sha256_file() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    elif command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        echo "A SHA-256 tool (shasum or sha256sum) is required" >&2
        return 1
    fi
}

verify_checksum() {
    expected=$(awk '{print $1}' "$2")
    actual=$(sha256_file "$1")
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
        echo "Checksum verification failed" >&2
        return 1
    fi
}

path_hint() {
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *) printf 'Add %s to PATH, for example:\n  export PATH="%s:$PATH"\n' "$INSTALL_DIR" "$INSTALL_DIR" ;;
    esac
}

main() {
    version=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version) version="${2:?--version requires a value}"; shift 2 ;;
            --install-dir) INSTALL_DIR="${2:?--install-dir requires a value}"; shift 2 ;;
            -h|--help)
                echo "Usage: install-labcoat.sh [VERSION] [--version VERSION] [--install-dir DIR]"
                return 0
                ;;
            -*) echo "Unknown argument: $1" >&2; return 2 ;;
            *)
                if [ -n "$version" ]; then
                    echo "Only one version may be specified" >&2
                    return 2
                fi
                version="$1"
                shift
                ;;
        esac
    done

    platform=$(detect_platform)
    if [ -n "$version" ]; then
        tag=$(normalize_version "$version")
    else
        tag=$(latest_version)
        if [ -z "$tag" ]; then
            echo "No published cli-v* release found for $REPO" >&2
            return 1
        fi
    fi

    asset="labcoat-$platform"
    base="https://github.com/$REPO/releases/download/$tag"
    tmp=$(mktemp -d "${TMPDIR:-/tmp}/labcoat-install.XXXXXX")
    trap 'rm -rf "$tmp"' EXIT HUP INT TERM

    echo "Installing Labcoat $tag for $platform"
    curl -fsSL "$base/$asset" -o "$tmp/$asset"
    curl -fsSL "$base/$asset.sha256" -o "$tmp/$asset.sha256"
    verify_checksum "$tmp/$asset" "$tmp/$asset.sha256"

    mkdir -p "$INSTALL_DIR"
    install -m 0755 "$tmp/$asset" "$INSTALL_DIR/.labcoat.new"
    mv "$INSTALL_DIR/.labcoat.new" "$INSTALL_DIR/labcoat"
    echo "Installed $INSTALL_DIR/labcoat"
    path_hint
}

if [ "${LABCOAT_INSTALLER_SOURCE_ONLY:-0}" != "1" ]; then
    main "$@"
fi
