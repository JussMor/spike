import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { sessionOverlay } from './plugins/session-overlay.js'
import path from 'node:path'
import os from 'node:os'

// Session ID: use VCS_AGENT_ID if set, otherwise derive from PID.
// Stable for the server's lifetime — no pre-registration needed.
const agentId = process.env.VCS_AGENT_ID ?? `s${process.pid}`
const overlayDir =
  process.env.VCS_OVERLAY_DIR ??
  path.join(os.tmpdir(), 'vcs-sessions', agentId)

export default defineConfig({
  plugins: [
    react(),
    sessionOverlay({ sessionId: agentId, overlayDir }),
  ],
})
