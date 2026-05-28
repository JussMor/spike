import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { sessionOverlay } from './plugins/session-overlay.js'
import path from 'node:path'
import os from 'node:os'

/**
 * Single-session dev config.
 *
 * Set VCS_AGENT_ID to customise the session identity.
 * Set VCS_OVERLAY_DIR to customise where overlays live.
 *
 * For multi-session (portless), run:
 *   npm run agents
 */
const agentId = process.env.VCS_AGENT_ID ?? 'default'
const overlayDir =
  process.env.VCS_OVERLAY_DIR ??
  path.join(os.tmpdir(), 'vcs-sessions', agentId)

export default defineConfig({
  plugins: [
    react(),
    sessionOverlay({ sessionId: agentId, overlayDir }),
  ],
})
