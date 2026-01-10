#!/usr/bin/env bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

REPO="jonatns/isomer"

echo -e "${BLUE}"
echo "  ╦┌─┐┌─┐┌┬┐┌─┐┬─┐"
echo "  ║└─┐│ ││││├┤ ├┬┘"
echo "  ╩└─┘└─┘┴ ┴└─┘┴└─"
echo -e "${NC}"
echo -e "${GREEN}Alkanes Development Environment Installer${NC}"
echo ""

# Detect OS and architecture
detect_platform() {
    local os=""
    local arch=""
    
    case "$(uname -s)" in
        Linux*)
            if grep -q Microsoft /proc/version 2>/dev/null; then
                os="linux"  # WSL is treated as Linux
            else
                os="linux"
            fi
            ;;
        Darwin*)
            os="macos"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            os="windows"
            ;;
        *)
            echo -e "${RED}Unsupported operating system${NC}"
            exit 1
            ;;
    esac
    
    case "$(uname -m)" in
        x86_64|amd64)
            arch="x64"
            ;;
        arm64|aarch64)
            arch="arm64"
            ;;
        *)
            echo -e "${RED}Unsupported architecture: $(uname -m)${NC}"
            exit 1
            ;;
    esac
    
    echo "${os}-${arch}"
}

# Get the latest release download URL
get_download_url() {
    local platform=$1
    # Get all releases (including drafts if we have permissions, otherwise just published ones)
    local api_url="https://api.github.com/repos/${REPO}/releases"
    
    echo -e "${BLUE}Fetching latest release info...${NC}" >&2
    
    local release_info
    release_info=$(curl -sS "$api_url")
    
    local asset_pattern=""
    case "$platform" in
        macos-arm64)
            asset_pattern="aarch64.*\\.dmg"
            ;;
        macos-x64)
            asset_pattern="x64.*\\.dmg"
            ;;
        linux-*)
            asset_pattern="\\.AppImage"
            ;;
        windows-*)
            asset_pattern="\\.msi"
            ;;
    esac
    
    # Find the first release that has our desired asset (skip binaries-v* releases)
    local download_url
    download_url=$(echo "$release_info" | grep -E "\"browser_download_url\".*${asset_pattern}" | head -1 | grep -o 'https://[^"]*')
    
    if [ -z "$download_url" ]; then
        echo -e "${RED}Could not find download URL for ${platform}${NC}"
        echo -e "${YELLOW}No Isomer app release found with ${asset_pattern} assets.${NC}"
        echo -e "${YELLOW}The release may still be in draft state.${NC}"
        echo ""
        echo -e "Please check: https://github.com/${REPO}/releases"
        exit 1
    fi
    
    echo "$download_url"
}

# Install system dependencies for Linux/WSL
install_linux_deps() {
    echo -e "${BLUE}Installing system dependencies...${NC}"
    
    if [ "$EUID" -ne 0 ]; then
        SUDO="sudo"
    else
        SUDO=""
    fi
    
    $SUDO apt update
    $SUDO apt install -y \
        libwebkit2gtk-4.1-0 \
        libgtk-3-0 \
        libayatana-appindicator3-1 \
        curl \
        wget
    
    echo -e "${GREEN}✓ Dependencies installed${NC}"
}

# Download and install
install_isomer() {
    local platform=$1
    local download_url=$2
    local filename=$(basename "$download_url")
    local install_dir="$HOME/.local/bin"
    
    echo -e "${BLUE}Downloading Isomer...${NC}"
    echo -e "${YELLOW}URL: ${download_url}${NC}"
    
    local tmp_dir=$(mktemp -d)
    cd "$tmp_dir"
    
    curl -L -o "$filename" "$download_url"
    
    case "$platform" in
        macos-*)
            echo -e "${BLUE}Mounting DMG...${NC}"
            
            # Mount and capture output
            hdiutil attach "$filename" -nobrowse
            
            # Find the mount point (Isomer volume)
            local mount_point
            mount_point=$(ls -d /Volumes/Isomer* 2>/dev/null | head -1)
            
            if [ -z "$mount_point" ] || [ ! -d "$mount_point" ]; then
                echo -e "${RED}Failed to find mounted DMG${NC}"
                echo -e "${YELLOW}Looking for Isomer.app in /Volumes...${NC}"
                ls -la /Volumes/
                exit 1
            fi
            
            echo -e "${BLUE}Found mount at: ${mount_point}${NC}"
            echo -e "${BLUE}Copying Isomer.app to /Applications...${NC}"
            cp -R "${mount_point}/Isomer.app" /Applications/
            
            hdiutil detach "$mount_point" -quiet 2>/dev/null || true
            echo -e "${GREEN}✓ Isomer installed to /Applications/Isomer.app${NC}"
            ;;
        linux-*)
            echo -e "${BLUE}Installing AppImage...${NC}"
            mkdir -p "$install_dir"
            chmod +x "$filename"
            mv "$filename" "$install_dir/isomer"
            echo -e "${GREEN}✓ Isomer installed to ${install_dir}/isomer${NC}"
            echo -e "${YELLOW}Add ${install_dir} to your PATH if not already done${NC}"
            ;;
        windows-*)
            echo -e "${BLUE}Running installer...${NC}"
            start "$filename"
            echo -e "${GREEN}✓ Installer launched${NC}"
            ;;
    esac
    
    # Cleanup
    cd -
    rm -rf "$tmp_dir"
}

# Main
main() {
    local platform=$(detect_platform)
    echo -e "${YELLOW}Detected platform: ${platform}${NC}"
    echo ""
    
    # Install dependencies for Linux
    if [[ "$platform" == linux-* ]]; then
        install_linux_deps
        echo ""
    fi
    
    # Get download URL
    local download_url=$(get_download_url "$platform")
    echo ""
    
    # Download and install
    install_isomer "$platform" "$download_url"
    echo ""
    
    echo -e "${GREEN}╔════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║        Installation Complete! ⚗️            ║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════════╝${NC}"
    echo ""
}

main
