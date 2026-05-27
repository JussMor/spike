#!/usr/bin/env bash
# vcs — one-command installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/JussMor/spike/main/install.sh | sh
#
# What it does:
#   1. Clones (or updates) the repo into ~/.vcs-spike-src/
#   2. Builds the release binary with `cargo build --release`
#   3. Copies the binary to /usr/local/bin/vcs  (or ~/bin/vcs if no sudo)
#
# After install:
#   vcs init            — initialise .vcs/ in the current project
#   vcs serve           — start a hub server on :7474
#   vcs --help          — full command reference

set -euo pipefail

REPO="https://github.com/JussMor/spike.git"
SRC_DIR="${HOME}/.vcs-spike-src"
BIN_NAME="vcs"

# ── Colours ───────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { printf "${GREEN}[vcs-install]${NC} %s\n" "$*"; }
warn()  { printf "${YELLOW}[vcs-install]${NC} %s\n" "$*"; }
error() { printf "${RED}[vcs-install]${NC} %s\n" "$*"; exit 1; }

# ── Check Rust ────────────────────────────────────────────────────────────
if ! command -v cargo &>/dev/null; then
  warn "Rust/cargo not found. Installing via rustup…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

# ── Clone or update repo ──────────────────────────────────────────────────
if [ -d "${SRC_DIR}/.git" ]; then
  info "Updating existing source at ${SRC_DIR}"
  git -C "${SRC_DIR}" pull --ff-only
else
  info "Cloning vcs-spike into ${SRC_DIR}"
  git clone --depth=1 "${REPO}" "${SRC_DIR}"
fi

# ── Build ─────────────────────────────────────────────────────────────────
info "Building release binary (this takes ~30s first time)…"
cargo build --release --manifest-path "${SRC_DIR}/Cargo.toml" -p vcs-cli

BUILT="${SRC_DIR}/target/release/${BIN_NAME}"
[ -f "${BUILT}" ] || error "Build succeeded but binary not found at ${BUILT}"

# ── Install ───────────────────────────────────────────────────────────────
install_to() {
  local dest="$1"
  mkdir -p "$(dirname "${dest}")"
  cp "${BUILT}" "${dest}"
  chmod +x "${dest}"
  info "Installed to ${dest}"
}

if [ -w "/usr/local/bin" ]; then
  install_to "/usr/local/bin/${BIN_NAME}"
elif sudo -n true 2>/dev/null; then
  sudo install_to "/usr/local/bin/${BIN_NAME}"
else
  warn "/usr/local/bin not writable — installing to ~/bin/${BIN_NAME}"
  install_to "${HOME}/bin/${BIN_NAME}"
  echo ""
  warn "Add ~/bin to PATH: export PATH=\"\$HOME/bin:\$PATH\""
fi

# ── Verify ────────────────────────────────────────────────────────────────
echo ""
if command -v vcs &>/dev/null; then
  info "✓ vcs installed successfully!"
  vcs --version
else
  warn "Binary installed but 'vcs' not in PATH yet."
  warn "Restart your shell or run: export PATH=\"\${HOME}/bin:\${PATH}\""
fi

echo ""
echo "Quick start:"
echo "  cd your-project"
echo "  vcs init                     # initialise .vcs/ (like git init)"
echo "  vcs serve                    # start hub server on :7474"
echo "  npm install vcs-client       # optional Node.js client"
