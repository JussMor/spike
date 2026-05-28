#!/bin/bash
# session-start hook — installs vite-overlay deps

set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(pwd)}"
OVERLAY_DIR="$PROJECT_DIR/examples/vite-overlay"

if [ ! -d "$OVERLAY_DIR/node_modules" ]; then
  echo "[session-start] Installing vite-overlay deps..."
  npm install --prefix "$OVERLAY_DIR"
else
  echo "[session-start] vite-overlay deps already installed"
fi
