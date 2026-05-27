#!/bin/bash
# vcs-spike session-start hook
#
# Runs when a Claude Code web session opens. Ensures:
#   1. The vcs binary is built (Rust)
#   2. The MCP server deps are installed (Node.js)
#   3. The TanStack Vite example deps are installed (for build/e2e)
#   4. Playwright browsers are ready (for e2e tests)
#   5. The vcs store is initialised in the project root
#
# Idempotent — safe to run multiple times. Skips steps already done.

set -euo pipefail

# Only run in remote Claude Code environments
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(pwd)}"
cd "$PROJECT_DIR"

echo "[vcs-spike/session-start] project: $PROJECT_DIR"

# ── 1. Build the vcs binary ───────────────────────────────────────────────
VCS_BIN="$PROJECT_DIR/target/release/vcs"
if [ ! -f "$VCS_BIN" ]; then
  echo "[vcs-spike/session-start] Building vcs binary (first run — may take ~60s)..."
  cargo build --release -p vcs-cli
else
  echo "[vcs-spike/session-start] vcs binary already built: $VCS_BIN"
fi

# Export VCS_BIN and store path for the session
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
  echo "export VCS_BIN=\"$VCS_BIN\""           >> "$CLAUDE_ENV_FILE"
  echo "export VCS_STORE_PATH=\"$PROJECT_DIR/.vcs\"" >> "$CLAUDE_ENV_FILE"
fi

# ── 2. MCP server deps ────────────────────────────────────────────────────
if [ ! -d "$PROJECT_DIR/packages/vcs-mcp/node_modules" ]; then
  echo "[vcs-spike/session-start] Installing vcs-mcp deps..."
  npm install --prefix "$PROJECT_DIR/packages/vcs-mcp"
else
  echo "[vcs-spike/session-start] vcs-mcp deps already installed"
fi

# ── 3. TanStack Vite deps ─────────────────────────────────────────────────
VITE_DIR="$PROJECT_DIR/examples/tanstack-vite"
if [ ! -d "$VITE_DIR/node_modules" ]; then
  echo "[vcs-spike/session-start] Installing tanstack-vite deps..."
  npm install --prefix "$VITE_DIR"
else
  echo "[vcs-spike/session-start] tanstack-vite deps already installed"
fi

# ── 4. Playwright browsers ────────────────────────────────────────────────
PLAYWRIGHT_SENTINEL="$VITE_DIR/node_modules/.playwright-installed"
if [ ! -f "$PLAYWRIGHT_SENTINEL" ]; then
  echo "[vcs-spike/session-start] Installing Playwright browsers..."
  cd "$VITE_DIR"
  # Try --with-deps first; fall back to browser-only; warn but don't abort
  if npx playwright install --with-deps chromium 2>&1; then
    touch "$PLAYWRIGHT_SENTINEL"
  elif npx playwright install chromium 2>&1; then
    touch "$PLAYWRIGHT_SENTINEL"
  else
    echo "[vcs-spike/session-start] ⚠️  Playwright browser install failed — e2e tests may not run"
    echo "[vcs-spike/session-start]    Run manually: cd $VITE_DIR && npx playwright install chromium"
  fi
  cd "$PROJECT_DIR"
else
  echo "[vcs-spike/session-start] Playwright browsers already installed"
fi

# ── 5. Initialise vcs store ───────────────────────────────────────────────
VCS_DB="$PROJECT_DIR/.vcs/vcs.db"
if [ ! -f "$VCS_DB" ]; then
  echo "[vcs-spike/session-start] Initialising vcs store at .vcs/"
  "$VCS_BIN" init
else
  echo "[vcs-spike/session-start] vcs store already initialised"
fi

echo "[vcs-spike/session-start] ✓ ready — $(\"$VCS_BIN\" --version 2>/dev/null || echo 'vcs binary ok')"
