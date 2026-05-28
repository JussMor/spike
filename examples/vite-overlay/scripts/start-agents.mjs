#!/usr/bin/env node
/**
 * start-agents.mjs — portless multi-session launcher.
 *
 * Starts one Vite dev server per agent. Each server:
 *   - Binds to port 0  →  OS picks a free port (no conflicts, no config)
 *   - Gets its own overlayDir  →  agent-specific file overrides
 *   - Shares the same source tree  →  no copying, no branches, no worktrees
 *
 * Usage:
 *   node scripts/start-agents.mjs
 *   VCS_AGENTS=agent-a,agent-b node scripts/start-agents.mjs
 *
 * To apply a change for a specific agent:
 *   echo '<h1>Hello from agent-a</h1>' > /tmp/vcs-sessions/agent-a/src/greeting.tsx
 *   # → only agent-a's browser reloads. Others are unaffected.
 */

import { createServer } from 'vite'
import react from '@vitejs/plugin-react'
import { sessionOverlay } from '../plugins/session-overlay.js'
import path from 'node:path'
import os from 'node:os'
import fs from 'node:fs'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

// ── Agent list ──────────────────────────────────────────────────────────────

const agentIds = (process.env.VCS_AGENTS ?? 'agent-dashboard,agent-auth')
  .split(',')
  .map((s) => s.trim())
  .filter(Boolean)

// ── Launcher ────────────────────────────────────────────────────────────────

/**
 * Start one Vite instance for a single agent session.
 * Returns { agentId, port, overlayDir, server }.
 */
async function startSession(agentId) {
  const overlayDir = path.join(os.tmpdir(), 'vcs-sessions', agentId)
  fs.mkdirSync(overlayDir, { recursive: true })

  const server = await createServer({
    root: ROOT,
    configFile: false, // do not load vite.config.ts — we configure inline
    plugins: [
      react(),
      sessionOverlay({ sessionId: agentId, overlayDir }),
    ],
    server: {
      port: 0,         // ← portless: OS assigns any free port
      strictPort: false,
      host: 'localhost',
    },
    logLevel: 'warn',  // suppress Vite's per-request logs
  })

  await server.listen()

  const addr = /** @type {import('node:net').AddressInfo} */ (
    server.httpServer.address()
  )

  return { agentId, port: addr.port, overlayDir, server }
}

// ── Main ────────────────────────────────────────────────────────────────────

console.log(`[vcs-overlay] Starting ${agentIds.length} agent session(s)...`)

const sessions = await Promise.all(agentIds.map(startSession))

console.log('\n  Agent sessions:\n')
for (const s of sessions) {
  console.log(`  ${s.agentId.padEnd(22)} http://localhost:${s.port}`)
  console.log(`  ${'overlay dir:'.padEnd(22)} ${s.overlayDir}\n`)
}

console.log(
  '  Write files into an overlay dir to trigger session-specific HMR.',
)
console.log('  Example:')
console.log()

const example = sessions[0]
console.log(
  `    mkdir -p ${example.overlayDir}/src`,
)
console.log(
  `    cp ${ROOT}/src/App.tsx ${example.overlayDir}/src/App.tsx`,
)
console.log(`    # edit ${example.overlayDir}/src/App.tsx`)
console.log(`    # → only http://localhost:${example.port} reloads\n`)

// ── Demo: seed an overlay for the first agent after 4 seconds ──────────────

setTimeout(() => {
  const s = sessions[0]
  const dir = path.join(s.overlayDir, 'src')
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(
    path.join(dir, 'AgentBadge.tsx'),
    `// Auto-generated overlay for ${s.agentId}
import './AgentBadge.css'

export function AgentBadge() {
  return (
    <div className="agent-badge" data-testid="agent-badge">
      <span className="agent-badge__label">active session</span>
      <strong className="agent-badge__id">${s.agentId}</strong>
    </div>
  )
}
`,
  )
  fs.writeFileSync(
    path.join(dir, 'AgentBadge.css'),
    `.agent-badge {
  display: inline-flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  padding: 6px 12px;
  background: #1a1a2e;
  border: 1px solid #4ecca3;
  border-radius: 6px;
  font-family: ui-monospace, monospace;
}
.agent-badge__label { font-size: 10px; color: #888; text-transform: uppercase; letter-spacing: 1px; }
.agent-badge__id    { font-size: 14px; color: #4ecca3; }
`,
  )
  console.log(
    `[vcs-overlay] Demo: seeded AgentBadge overlay for ${s.agentId}`,
  )
  console.log(
    `             → check http://localhost:${s.port} — HMR should fire\n`,
  )
}, 4000)

// ── Shutdown ─────────────────────────────────────────────────────────────────

process.on('SIGINT', async () => {
  console.log('\n[vcs-overlay] Shutting down...')
  await Promise.all(sessions.map((s) => s.server.close()))
  process.exit(0)
})
