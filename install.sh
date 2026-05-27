#!/usr/bin/env bash
# vcs — one-command installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/JussMor/spike/main/install.sh | sh
#
# What it does:
#   1. Detects OS + arch
#   2. Downloads the latest pre-built binary from GitHub Releases (fast, ~2s)
#   3. Falls back to building from source if no binary is available or Rust
#      is already installed and the user prefers it
#
# After install:
#   vcs init            — initialise .vcs/ in the current project
#   vcs serve           — start a hub server on :7474
#   vcs --help          — full command reference

set -euo pipefail

REPO_OWNER="JussMor"
REPO_NAME="spike"
REPO="https://github.com/${REPO_OWNER}/${REPO_NAME}.git"
API="https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}"
SRC_DIR="${HOME}/.vcs-spike-src"
BIN_NAME="vcs"
FORCE_BUILD="${VCS_FORCE_BUILD:-0}"

# ── Colours ───────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { printf "${GREEN}[vcs-install]${NC} %s\n" "$*"; }
warn()  { printf "${YELLOW}[vcs-install]${NC} %s\n" "$*"; }
error() { printf "${RED}[vcs-install]${NC} %s\n" "$*"; exit 1; }
step()  { printf "${CYAN}[vcs-install]${NC} %s\n" "$*"; }

# ── Detect platform ───────────────────────────────────────────────────────
detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)
      case "$arch" in
        x86_64)  echo "linux-x86_64"  ;;
        aarch64) echo "linux-aarch64" ;;
        arm64)   echo "linux-aarch64" ;;
        *)       echo ""              ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64)  echo "macos-x86_64"  ;;
        arm64)   echo "macos-aarch64" ;;
        *)       echo ""              ;;
      esac
      ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "windows-x86_64"
      ;;
    *)
      echo ""
      ;;
  esac
}

# ── Install the binary ────────────────────────────────────────────────────
install_bin() {
  local src="$1"
  local dest

  if [ -w "/usr/local/bin" ]; then
    dest="/usr/local/bin/${BIN_NAME}"
  elif sudo -n true 2>/dev/null; then
    dest="/usr/local/bin/${BIN_NAME}"
    sudo cp "${src}" "${dest}"
    sudo chmod +x "${dest}"
    info "Installed to ${dest}"
    return
  else
    warn "/usr/local/bin not writable — installing to ~/bin/${BIN_NAME}"
    dest="${HOME}/bin/${BIN_NAME}"
    warn "Add ~/bin to PATH: export PATH=\"\$HOME/bin:\$PATH\""
  fi

  mkdir -p "$(dirname "${dest}")"
  cp "${src}" "${dest}"
  chmod +x "${dest}"
  info "Installed to ${dest}"
}

# ── Try binary release download ───────────────────────────────────────────
try_download() {
  local platform="$1"

  step "Checking GitHub Releases for pre-built binary…"

  # Get latest release tag
  local release_info
  if ! release_info="$(curl -fsSL "${API}/releases/latest" 2>/dev/null)"; then
    warn "Could not reach GitHub API — will build from source"
    return 1
  fi

  local tag
  tag="$(echo "${release_info}" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": "\(.*\)".*/\1/')"
  if [ -z "${tag}" ]; then
    warn "No releases found — will build from source"
    return 1
  fi

  info "Latest release: ${tag}"

  # Construct download URL
  local ext="tar.gz"
  [[ "${platform}" == windows* ]] && ext="zip"
  local filename="vcs-${tag}-${platform}.${ext}"
  local url="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${tag}/${filename}"

  step "Downloading ${filename}…"
  local tmpdir
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "${tmpdir}"' EXIT

  if ! curl -fsSL -o "${tmpdir}/${filename}" "${url}"; then
    warn "Pre-built binary not available for ${platform} — will build from source"
    return 1
  fi

  # Extract
  if [ "${ext}" = "tar.gz" ]; then
    tar -xzf "${tmpdir}/${filename}" -C "${tmpdir}"
    install_bin "${tmpdir}/vcs"
  else
    # Windows (zip)
    unzip -q "${tmpdir}/${filename}" -d "${tmpdir}"
    install_bin "${tmpdir}/vcs.exe"
  fi

  return 0
}

# ── Build from source ─────────────────────────────────────────────────────
build_from_source() {
  if ! command -v cargo &>/dev/null; then
    warn "Rust/cargo not found. Installing via rustup…"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    # shellcheck disable=SC1090
    source "${HOME}/.cargo/env"
  fi

  if [ -d "${SRC_DIR}/.git" ]; then
    info "Updating existing source at ${SRC_DIR}"
    git -C "${SRC_DIR}" pull --ff-only
  else
    info "Cloning vcs-spike into ${SRC_DIR}"
    git clone --depth=1 "${REPO}" "${SRC_DIR}"
  fi

  step "Building release binary (this takes ~30s first time)…"
  cargo build --release --manifest-path "${SRC_DIR}/Cargo.toml" -p vcs-cli

  local built="${SRC_DIR}/target/release/${BIN_NAME}"
  [ -f "${built}" ] || error "Build succeeded but binary not found at ${built}"
  install_bin "${built}"
}

# ── Main ──────────────────────────────────────────────────────────────────
main() {
  echo ""
  echo "  vcs-spike installer"
  echo "  ─────────────────────────────────────────"
  echo ""

  local platform
  platform="$(detect_platform)"

  if [ -z "${platform}" ]; then
    warn "Unknown platform — falling back to build from source"
    build_from_source
  elif [ "${FORCE_BUILD}" = "1" ]; then
    step "VCS_FORCE_BUILD=1 — skipping binary download"
    build_from_source
  else
    if ! try_download "${platform}"; then
      step "Falling back to build from source…"
      build_from_source
    fi
  fi

  # ── Verify ──────────────────────────────────────────────────────────────
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
  echo "  vcs init                       # initialise .vcs/ (like git init)"
  echo "  vcs stack open --agent me      # open a work stack"
  echo "  vcs watch . --stack <id>       # auto-track file saves (human dev UX)"
  echo "  vcs serve                      # start hub server on :7474"
  echo "  npm install vcs-spike          # optional Node.js MCP/OpenAI client"
  echo ""
}

main "$@"
