#!/usr/bin/env bash
# Candor AI — One-command install
# Usage: curl -sfL https://raw.githubusercontent.com/iknowkungfubar/candor-ai/main/install.sh | sh
set -euo pipefail

REPO="iknowkungfubar/candor-ai"
BIN_NAME="candor"
INSTALL_DIR="${CANDOR_INSTALL_DIR:-/usr/local/bin}"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)  OS_RAW="linux" ; TAG_OS="x86_64-unknown-linux-gnu" ;;
    Darwin)
        OS_RAW="macos"
        case "$ARCH" in
            x86_64|amd64) TAG_OS="x86_64-apple-darwin" ;;
            aarch64|arm64) TAG_OS="aarch64-apple-darwin" ;;
        esac
        ;;
    *)      echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64) ARCH_RAW="x86_64" ;;
    aarch64|arm64) ARCH_RAW="aarch64" ;;
    *)            echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

# Check for existing binary
if command -v "$BIN_NAME" &>/dev/null; then
    echo "  Candor AI already installed at $(which $BIN_NAME)"
    echo "  To upgrade, uninstall first or use: cargo install candor-daemon"
    exit 0
fi

# Check for Rust toolchain as fallback
if command -v cargo &>/dev/null; then
    echo "  Rust toolchain detected — installing via cargo..."
    cargo install candor-daemon 2>/dev/null && {
        echo "  ✅ Candor AI installed via cargo"
        echo "  Run 'candor doctor' to verify"
        exit 0
    }
    echo "  Cargo install failed — try: cargo install candor-daemon"
fi

# Pre-built binary from GitHub Releases
echo "  Downloading pre-built binary for $OS_RAW/$ARCH_RAW..."
RELEASE_URL="https://github.com/$REPO/releases/latest/download/candor-$TAG_OS.tar.gz"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

if curl -sfL "$RELEASE_URL" -o "$TMP_DIR/candor.tar.gz"; then
    tar xzf "$TMP_DIR/candor.tar.gz" -C "$TMP_DIR"
    if [ -f "$TMP_DIR/candor" ]; then
        install -m 755 "$TMP_DIR/candor" "$INSTALL_DIR/$BIN_NAME"
        echo "  ✅ Candor AI installed to $INSTALL_DIR/$BIN_NAME"
        echo "  Run '$BIN_NAME doctor' to verify"
    else
        echo "  ❌ Downloaded archive did not contain expected binary."
        exit 1
    fi
else
    echo "  ❌ Failed to download pre-built binary."
    echo ""
    echo "  Install Rust and run:"
    echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "    cargo install candor-daemon"
    echo ""
    echo "  Or build from source:"
    echo "    git clone https://github.com/iknowkungfubar/candor-ai"
    echo "    cd candor-ai && cargo build --release"
    exit 1
fi
