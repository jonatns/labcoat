#!/usr/bin/env bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}"
echo "  ╦┌─┐┌─┐┌┬┐┌─┐┬─┐"
echo "  ║└─┐│ ││││├┤ ├┬┘"
echo "  ╩└─┘└─┘┴ ┴└─┘┴└─"
echo -e "${NC}"
echo -e "${GREEN}Alkanes Development Environment Installer${NC}"
echo ""

# Detect OS
detect_os() {
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        if grep -q Microsoft /proc/version 2>/dev/null; then
            echo "wsl"
        else
            echo "linux"
        fi
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        echo "macos"
    else
        echo "unknown"
    fi
}

OS=$(detect_os)
echo -e "${YELLOW}Detected OS: ${OS}${NC}"
echo ""

# Install system dependencies based on OS
install_dependencies() {
    case $OS in
        linux|wsl)
            echo -e "${BLUE}Installing system dependencies...${NC}"
            
            # Check if running with sudo or as root
            if [ "$EUID" -ne 0 ]; then
                SUDO="sudo"
            else
                SUDO=""
            fi
            
            # Update package list
            $SUDO apt update
            
            # Install Tauri dependencies for Linux/WSL
            # Reference: https://tauri.app/start/prerequisites/#linux
            echo -e "${YELLOW}Installing Tauri build dependencies...${NC}"
            $SUDO apt install -y \
                build-essential \
                curl \
                wget \
                file \
                libglib2.0-dev \
                libgtk-3-dev \
                libsoup-3.0-dev \
                libjavascriptcoregtk-4.1-dev \
                libwebkit2gtk-4.1-dev \
                librsvg2-dev \
                pkg-config
            
            echo -e "${GREEN}✓ System dependencies installed${NC}"
            ;;
        macos)
            echo -e "${BLUE}Checking macOS dependencies...${NC}"
            
            # Check for Xcode CLI tools
            if ! xcode-select -p &>/dev/null; then
                echo -e "${YELLOW}Installing Xcode Command Line Tools...${NC}"
                xcode-select --install
            else
                echo -e "${GREEN}✓ Xcode Command Line Tools already installed${NC}"
            fi
            
            # Check for Homebrew
            if ! command -v brew &>/dev/null; then
                echo -e "${YELLOW}Installing Homebrew...${NC}"
                /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
            else
                echo -e "${GREEN}✓ Homebrew already installed${NC}"
            fi
            ;;
        *)
            echo -e "${RED}Unsupported OS. Please install dependencies manually.${NC}"
            exit 1
            ;;
    esac
}

# Install Rust
install_rust() {
    if command -v rustc &>/dev/null; then
        RUST_VERSION=$(rustc --version)
        echo -e "${GREEN}✓ Rust already installed: ${RUST_VERSION}${NC}"
    else
        echo -e "${BLUE}Installing Rust...${NC}"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
        echo -e "${GREEN}✓ Rust installed${NC}"
    fi
}

# Install Node.js via nvm
install_node() {
    if command -v node &>/dev/null; then
        NODE_VERSION=$(node --version)
        echo -e "${GREEN}✓ Node.js already installed: ${NODE_VERSION}${NC}"
    else
        echo -e "${BLUE}Installing Node.js via nvm...${NC}"
        curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
        export NVM_DIR="$HOME/.nvm"
        [ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"
        nvm install --lts
        echo -e "${GREEN}✓ Node.js installed${NC}"
    fi
}

# Install pnpm
install_pnpm() {
    if command -v pnpm &>/dev/null; then
        PNPM_VERSION=$(pnpm --version)
        echo -e "${GREEN}✓ pnpm already installed: ${PNPM_VERSION}${NC}"
    else
        echo -e "${BLUE}Installing pnpm...${NC}"
        npm install -g pnpm
        echo -e "${GREEN}✓ pnpm installed${NC}"
    fi
}

# Clone and setup Isomer
setup_isomer() {
    ISOMER_DIR="$HOME/isomer"
    
    if [ -d "$ISOMER_DIR" ]; then
        echo -e "${YELLOW}Isomer directory already exists at ${ISOMER_DIR}${NC}"
        echo -e "${BLUE}Pulling latest changes...${NC}"
        cd "$ISOMER_DIR"
        git pull
    else
        echo -e "${BLUE}Cloning Isomer repository...${NC}"
        git clone https://github.com/jonatns/isomer.git "$ISOMER_DIR"
        cd "$ISOMER_DIR"
    fi
    
    echo -e "${BLUE}Installing dependencies...${NC}"
    pnpm install
    
    echo -e "${BLUE}Building Isomer...${NC}"
    pnpm tauri build
    
    echo -e "${GREEN}✓ Isomer build complete${NC}"
}

# Main installation flow
main() {
    echo -e "${BLUE}Step 1/5: Installing system dependencies...${NC}"
    install_dependencies
    echo ""
    
    echo -e "${BLUE}Step 2/5: Installing Rust...${NC}"
    install_rust
    echo ""
    
    echo -e "${BLUE}Step 3/5: Installing Node.js...${NC}"
    install_node
    echo ""
    
    echo -e "${BLUE}Step 4/5: Installing pnpm...${NC}"
    install_pnpm
    echo ""
    
    echo -e "${BLUE}Step 5/5: Building Isomer...${NC}"
    setup_isomer
    echo ""
    
    echo -e "${GREEN}╔════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║        Installation Complete! ⚗️            ║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "The Isomer application has been built at:"
    echo -e "  ${YELLOW}~/isomer/src-tauri/target/release/bundle/${NC}"
    echo ""
}

main
