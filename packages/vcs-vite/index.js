/**
 * vcs-vite — Vite plugin for vcs-spike agent workflows.
 *
 * ## Problem it solves
 *
 * Vite (and Next, Astro, Nuxt…) assumes ONE builder, ONE source directory.
 * When two Claude Code sessions both write to src/ they corrupt each other's
 * HMR and the dev-server collapses.
 *
 * ## How this plugin fixes it
 *
 * Each Vite instance is bound to ONE vcs session + ONE vcs stack.
 * Source files are served directly from the vcs store (the view of that stack),
 * not from disk.  Two Vite instances can run simultaneously, each seeing only
 * its own agent's changes.  Disk is never the authority — the store is.
 *
 * ## Usage
 *
 * ```js
 * // vite.config.ts  (Session A — feature-auth, port 5173)
 * import { vcsAgentPlugin } from 'vcs-vite'
 *
 * export default defineConfig({
 *   plugins: [
 *     vcsAgentPlugin({
 *       sessionId:  process.env.VCS_SESSION_ID,   // from vcs_session_open
 *       stackId:    process.env.VCS_STACK_ID,     // from vcs_stack_open
 *       storePath:  '.vcs',                        // default
 *     })
 *   ],
 *   server: { port: 5173 }
 * })
 * ```
 *
 * ```js
 * // vite.config.ts  (Session B — feature-pay, port 5174)
 * export default defineConfig({
 *   plugins: [
 *     vcsAgentPlugin({
 *       sessionId:  process.env.VCS_SESSION_ID,
 *       stackId:    process.env.VCS_STACK_ID,
 *     })
 *   ],
 *   server: { port: 5174 }
 * })
 * ```
 *
 * Both servers read from the SAME .vcs/ store (WAL mode handles concurrency).
 * Each serves only ITS OWN stack's files.  Neither touches disk.
 *
 * ## Phase gate
 *
 * When the dev-server starts the plugin sets phase=testing on the session.
 * vcs_overview shows:
 *   ⛔ feature-auth is TESTING on :5173 — other sessions must not merge yet
 *
 * When the server closes (Ctrl+C or test runner finishes) the plugin sets
 * phase=done automatically.  The gate lifts.  Now another session can merge.
 *
 * ## File serving flow
 *
 *   Browser requests src/auth.ts
 *     → Vite resolveId hook: recognised as project source
 *     → Vite load hook: read from vcs view (store) for this session's stack
 *     → return content  (disk never consulted for tracked files)
 *
 * ## HMR flow
 *
 *   Agent calls vcs_edit(stack, 'src/auth.ts', newContent)
 *     → plugin polls stack tip_change_id every 300 ms
 *     → detects change; identifies WHICH files changed (diff tip→prev)
 *     → invalidates only those modules in Vite's module graph
 *     → sends targeted HMR update — only this session's server reacts
 */

import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { resolve, relative, join } from 'node:path'

// ── Binary resolution ──────────────────────────────────────────────────────

function findBin(storePath) {
  if (process.env.VCS_BIN && existsSync(process.env.VCS_BIN)) return process.env.VCS_BIN
  const siblings = [
    join(storePath, '../../target/release/vcs'),
    resolve(process.cwd(), 'target/release/vcs'),
  ]
  for (const p of siblings) if (existsSync(p)) return p
  return 'vcs'
}

// ── Store access ───────────────────────────────────────────────────────────

function vcsRun(bin, storePath, ...args) {
  const r = spawnSync(bin, ['--json', '--store', storePath, ...args], {
    cwd: process.cwd(),
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  })
  if (r.status !== 0) throw new Error(r.stderr?.trim() || `vcs ${args[0]} failed`)
  const out = r.stdout?.trim()
  if (!out) return null
  try { return JSON.parse(out) } catch { return { text: out } }
}

function vcsRunQuiet(bin, storePath, ...args) {
  try { return vcsRun(bin, storePath, ...args) } catch { return null }
}

function getStackTip(bin, storePath, stackId) {
  const stk = vcsRunQuiet(bin, storePath, 'stack', 'info', stackId)
  return stk?.tip_change_id ?? null
}

function openView(bin, storePath, stackId) {
  const r = vcsRunQuiet(bin, storePath, 'view', 'open', '--base', '', '--stacks', stackId)
  return r?.view_id ?? null
}

function readFileFromView(bin, storePath, viewId, relPath) {
  // Use the binary directly (non-JSON mode) to get raw file bytes
  const r = spawnSync(bin, ['--store', storePath, 'view', 'read', viewId, relPath], {
    cwd: process.cwd(),
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  })
  if (r.status !== 0) return null
  return r.stdout ?? null
}

function getViewFiles(bin, storePath, viewId) {
  const r = vcsRunQuiet(bin, storePath, 'view', 'ls', viewId)
  return r?.files ?? []
}

function setSessionPhase(bin, storePath, sessionId, phase) {
  vcsRunQuiet(bin, storePath, 'session', 'phase', sessionId, phase)
}

function setSessionOutput(bin, storePath, sessionId, outputDir, port) {
  const args = ['session', 'set-output', sessionId, outputDir]
  if (port) args.push('--port', String(port))
  vcsRunQuiet(bin, storePath, ...args)
}

function getChangedPaths(bin, storePath, fromTip, toTip) {
  if (!fromTip || !toTip || fromTip === toTip) return []
  const diff = vcsRunQuiet(bin, storePath, 'diff', fromTip, toTip)
  if (!Array.isArray(diff)) return []
  return diff.map(e => e.path).filter(Boolean)
}

// ── Plugin ─────────────────────────────────────────────────────────────────

/**
 * @param {object} options
 * @param {string}  options.stackId    - vcs stack ID for this session (required)
 * @param {string}  [options.sessionId]  - vcs session ID (for phase management)
 * @param {string}  [options.storePath]  - path to .vcs/ (default: auto-detect)
 * @param {number}  [options.pollMs=300] - HMR poll interval in milliseconds
 * @param {boolean} [options.checkPhaseGate=true] - warn if another session is testing
 */
export function vcsAgentPlugin(options = {}) {
  const {
    stackId,
    sessionId,
    storePath: storePathOpt,
    pollMs = 300,
    checkPhaseGate = true,
  } = options

  if (!stackId) {
    console.warn('[vcs-vite] No stackId provided — plugin will pass through all file reads.')
  }

  const storePath = storePathOpt
    ? resolve(process.cwd(), storePathOpt)
    : resolve(process.cwd(), '.vcs')

  const bin = findBin(storePath)

  let viewId = null
  let lastTip = null
  let trackedPaths = new Set()
  let pollTimer = null
  let devServer = null
  let serverPort = null

  // ── Initialise view ──────────────────────────────────────────────────────

  function initView() {
    if (!stackId || !existsSync(join(storePath, 'vcs.db'))) return
    try {
      viewId = openView(bin, storePath, stackId)
      lastTip = getStackTip(bin, storePath, stackId)
      if (viewId) {
        trackedPaths = new Set(getViewFiles(bin, storePath, viewId))
        console.log(`[vcs-vite] Session stack ${stackId.slice(0, 8)}… — ${trackedPaths.size} file(s) in view`)
      }
    } catch (e) {
      console.warn('[vcs-vite] Could not open view:', e.message)
    }
  }

  // ── HMR polling ──────────────────────────────────────────────────────────

  function startPolling() {
    if (!stackId || !devServer) return

    pollTimer = setInterval(() => {
      try {
        const tip = getStackTip(bin, storePath, stackId)
        if (!tip || tip === lastTip) return

        // Find which paths changed since the last known tip
        const changed = getChangedPaths(bin, storePath, lastTip, tip)
        const prevTip = lastTip
        lastTip = tip

        // Refresh view so next load() returns new content
        viewId = openView(bin, storePath, stackId)
        if (viewId) trackedPaths = new Set(getViewFiles(bin, storePath, viewId))

        if (changed.length === 0) {
          // Fallback: reload everything in the view
          devServer.ws.send({ type: 'full-reload' })
          return
        }

        console.log(`[vcs-vite] ${changed.length} file(s) changed — triggering HMR`)

        // Targeted HMR: invalidate only the changed modules
        const updates = []
        for (const relPath of changed) {
          const absPath = resolve(process.cwd(), relPath)
          const mod = devServer.moduleGraph.getModuleById(absPath)
          if (mod) {
            devServer.moduleGraph.invalidateModule(mod)
            updates.push({
              type: mod.type === 'css' ? 'css-update' : 'js-update',
              path: '/' + relPath,
              acceptedPath: '/' + relPath,
              timestamp: Date.now(),
            })
          }
        }

        if (updates.length > 0) {
          devServer.ws.send({ type: 'update', updates })
        } else {
          // Modules not yet in graph — trigger full reload
          devServer.ws.send({ type: 'full-reload' })
        }
      } catch (_) {
        // Store locked or unavailable — skip this tick
      }
    }, pollMs)
  }

  // ── Phase gate check ─────────────────────────────────────────────────────

  function checkGate() {
    if (!checkPhaseGate || !sessionId) return
    try {
      const ov = vcsRunQuiet(bin, storePath, 'overview')
      if (ov?.testing_session && ov.testing_session !== sessionId) {
        console.warn(
          `[vcs-vite] ⛔ Session "${ov.testing_session}" is in testing phase. ` +
          `Starting anyway — but do NOT merge stacks until that session reaches phase=done.`
        )
      }
    } catch (_) {}
  }

  // ── Vite plugin interface ─────────────────────────────────────────────────

  return {
    name: 'vcs-spike',
    enforce: 'pre', // run before user transforms so we intercept source reads first

    configureServer(server) {
      devServer = server
      serverPort = server.config?.server?.port ?? null

      initView()
      checkGate()

      // Phase: testing — signal to other sessions that this server is live
      if (sessionId) {
        setSessionPhase(bin, storePath, sessionId, 'testing')
        if (serverPort) {
          setSessionOutput(bin, storePath, sessionId, process.cwd(), serverPort)
        }
        console.log(`[vcs-vite] Session ${sessionId.slice(0, 8)}… → phase=testing${serverPort ? ` (port ${serverPort})` : ''}`)
      }

      startPolling()

      // Phase: done — when server closes (Ctrl+C, test runner exit)
      server.httpServer?.on('close', () => {
        if (pollTimer) clearInterval(pollTimer)
        if (sessionId) {
          setSessionPhase(bin, storePath, sessionId, 'done')
          console.log(`[vcs-vite] Session ${sessionId.slice(0, 8)}… → phase=done (gate lifted)`)
        }
      })

      // Add a status endpoint so the browser can see which session is active
      server.middlewares.use('/api/vcs-agent/session', (req, res) => {
        res.setHeader('Content-Type', 'application/json')
        res.end(JSON.stringify({
          sessionId,
          stackId,
          viewId,
          port: serverPort,
          trackedFiles: [...trackedPaths],
          lastTip,
        }))
      })
    },

    // ── Source serving from vcs store ────────────────────────────────────────
    //
    // When Vite tries to load a project source file that is tracked in this
    // session's stack, we return it from the store instead of from disk.
    // This means disk never needs to be the authority.

    load(id) {
      if (!viewId || !stackId) return null
      if (id.includes('node_modules')) return null
      if (id.startsWith('\0')) return null // virtual modules

      // Normalise to a path relative to the project root
      const rel = relative(process.cwd(), id)
      if (rel.startsWith('..') || rel.startsWith('/')) return null // outside project

      // Only intercept files that this stack has touched
      if (!trackedPaths.has(rel)) return null

      const content = readFileFromView(bin, storePath, viewId, rel)
      if (content === null) return null // fall through to normal disk read

      return content
    },

    // Send a custom HMR event with session metadata so browser devtools
    // can show which agent's changes are being hot-reloaded
    handleHotUpdate({ file, modules, server }) {
      const rel = relative(process.cwd(), file)
      if (trackedPaths.has(rel)) {
        server.ws.send({
          type: 'custom',
          event: 'vcs:hot-update',
          data: { path: rel, sessionId, stackId, timestamp: Date.now() },
        })
      }
      return modules // let Vite proceed with normal HMR
    },
  }
}
