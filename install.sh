#!/usr/bin/env bash
# Candor AI — Install script
# Usage: curl -fsSL https://raw.githubusercontent.com/iknowkungfubar/candor-ai/main/install.sh | bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${CYAN}${BOLD}"
echo "   ___                _              ___    ___ "
echo "  / __|__ _ _ _  __ _| |___ _ _     /_\ \  |_ _|"
echo " | (__/ _\` | ' \/ _\` | / _ \ '_|   / _ \  | | "
echo "  \___\__,_|_||_\__,_|_\___/_|    /_/ \_\|___|"
echo -e "${NC}"
echo -e "${BOLD}Candor AI — Lawful Good Agentic Operating System${NC}"
echo -e "Version: 1.0.0"
echo ""

# ── Install methods ──
INSTALL_METHOD="${1:-auto}"
PREFIX="${CANDOR_PREFIX:-/usr/local}"

install_via_cargo() {
    echo -e "${CYAN}Building from source with cargo...${NC}"
    cargo install --git https://github.com/iknowkungfubar/candor-ai.git candor-ai 2>&1 | tail -3
    echo -e "${GREEN}✓${NC} Installed via cargo"
}

install_via_homebrew() {
    if command -v brew &>/dev/null; then
        echo -e "${CYAN}Installing via Homebrew...${NC}"
        brew tap iknowkungfubar/candor-ai
        brew install candor-ai
        echo -e "${GREEN}✓${NC} Installed via Homebrew"
    else
        echo -e "${YELLOW}Homebrew not found, falling back to cargo...${NC}"
        install_via_cargo
    fi
}

install_via_source() {
    local dir="${CANDOR_DIR:-$HOME/.candor-ai}"
    echo -e "${CYAN}Building from source in $dir...${NC}"

    if [ -d "$dir/.git" ]; then
        cd "$dir" && git pull --ff-only origin main
    else
        git clone https://github.com/iknowkungfubar/candor-ai.git "$dir"
        cd "$dir"
    fi

    cargo build --release 2>&1 | tail -3

    local bin_dir="${CANDOR_BIN_DIR:-$HOME/.local/bin}"
    mkdir -p "$bin_dir"
    ln -sf "$dir/target/release/candor" "$bin_dir/candor"
    echo -e "${GREEN}✓${NC} Binary linked to $bin_dir/candor"

    if [[ ":$PATH:" != *":$bin_dir:"* ]]; then
        echo ""
        echo -e "${YELLOW}Add to your shell profile:${NC}"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
}

# ── Select install method ──
case "$INSTALL_METHOD" in
    brew|homebrew) install_via_homebrew ;;
    cargo) install_via_cargo ;;
    source|local) install_via_source ;;
    auto)
        if command -v brew &>/dev/null; then
            install_via_homebrew
        elif command -v cargo &>/dev/null; then
            install_via_source
        else
            echo -e "${RED}No Rust toolchain found. Install it first:${NC}"
            echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
            exit 1
        fi
        ;;
esac

# ── Verify ──
echo ""
if command -v candor &>/dev/null; then
    echo -e "${GREEN}${BOLD}Candor AI installed successfully!${NC}"
    echo ""
    echo "  ${CYAN}Quick start:${NC}"
    echo "    candor --health                    # Check subsystems"
    echo "    candor --chat                      # Interactive chat mode"
    echo "    candor --task \"build a CLI tool\"   # Run agent task"
    echo "    candor --voice-task                # Voice-activated"
    echo "    candor --init my-project           # Bootstrap project"
    echo ""
    echo "  ${CYAN}LLM setup (pick one):${NC}"
    echo "    export LM_STUDIO_URL=\"http://localhost:1234/v1\""
    echo "    export OPENAI_API_KEY=\"sk-...\""
    echo "    export ANTHROPIC_API_KEY=\"sk-ant-...\""
    echo ""
    echo "  ${CYAN}Desktop UI:${NC}"
    echo "    cd $dir/desktop && npm run tauri dev"
    echo ""
    echo "  ${CYAN}Tests:${NC}"
    echo "    cd $dir && cargo test"
else
    echo -e "${RED}Installation issue — 'candor' not found on PATH.${NC}"
    echo "Add ~/.local/bin to your PATH or restart your shell."
    exit 1
fi
