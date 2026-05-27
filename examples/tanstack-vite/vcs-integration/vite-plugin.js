/**
 * vite-plugin.js
 *
 * A Vite dev-server plugin that exposes the live .vcs/ store over
 * /api/vcs/* HTTP endpoints so the React app can query it with TanStack Query.
 *
 * This is the "git-like" integration:
 *   - The store is in .vcs/ right next to your src/ folder
 *   - The Vite dev server reads it in real-time via the vcs CLI
 *   - TanStack Query polls every 3 seconds and shows live state
 *
 * Endpoints:
 *   GET /api/vcs/status              — store health
 *   GET /api/vcs/changes             — all recorded changes (newest first)
 *   GET /api/vcs/stack/:id/log       — changes in a specific stack
 *   GET /api/vcs/view/:id/files      — files visible in a view
 *   GET /api/vcs/view/:id/conflicts  — conflicts in a view
 *   GET /api/vcs/active-view         — the "current" view (last one opened)
 */

import { spawnSync, execSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { resolve, join } from 'node:path'

const PROJECT_ROOT = resolve(import.meta.dirname ?? process.cwd(), '..')

function findBin() {
  if (process.env.VCS_BIN) return process.env.VCS_BIN
  const ws = resolve(PROJECT_ROOT, '../../target/release/vcs')
  if (existsSync(ws)) return ws
  return 'vcs'
}

const BIN = findBin()
const VCS_STORE = join(PROJECT_ROOT, '.vcs')
const STATE_FILE = join(VCS_STORE, 'active-view.json')

function vcsRun(...args) {
  const r = spawnSync(BIN, ['--json', ...args], {
    cwd: PROJECT_ROOT,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
    env: { ...process.env, VCS_BIN: BIN },
  })
  if (r.status !== 0) throw new Error(r.stderr || `vcs ${args[0]} failed`)
  const out = r.stdout.trim()
  if (!out) return null
  return JSON.parse(out)
}

function isInitialised() {
  return existsSync(join(VCS_STORE, 'vcs.db'))
}

function jsonResponse(res, data, status = 200) {
  res.writeHead(status, { 'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*' })
  res.end(JSON.stringify(data))
}

function errResponse(res, msg, status = 500) {
  jsonResponse(res, { error: msg }, status)
}

// ── Vite plugin ───────────────────────────────────────────────────────────

export function vcsPlugin() {
  return {
    name: 'vcs-api',
    configureServer(server) {
      server.middlewares.use('/api/vcs', async (req, res, next) => {
        const url = new URL(req.url, 'http://localhost')
        const path = url.pathname

        try {
          // GET /status
          if (path === '/status' || path === '/') {
            return jsonResponse(res, {
              storePath: VCS_STORE,
              initialised: isInitialised(),
              binary: BIN,
            })
          }

          if (!isInitialised()) {
            return errResponse(res, 'Store not initialised. Run: npm run vcs:init', 503)
          }

          // GET /changes — query all changes from SQLite directly
          if (path === '/changes') {
            const { Database } = await importSqlite()
            const db = new Database(join(VCS_STORE, 'vcs.db'), { readonly: true })
            const rows = db.prepare(
              'SELECT * FROM changes ORDER BY created_at ASC'
            ).all()
            db.close()
            return jsonResponse(res, rows.map(r => ({
              ...r,
              intent: JSON.parse(r.intent),
            })))
          }

          // GET /stack/:id/log
          const stackLogMatch = path.match(/^\/stack\/([^/]+)\/log$/)
          if (stackLogMatch) {
            const stackId = stackLogMatch[1]
            const log = vcsRun('log', stackId)
            return jsonResponse(res, log ?? [])
          }

          // GET /view/:id/files
          const viewFilesMatch = path.match(/^\/view\/([^/]+)\/files$/)
          if (viewFilesMatch) {
            const viewId = viewFilesMatch[1]
            const result = vcsRun('view', 'ls', viewId)
            return jsonResponse(res, result?.files ?? [])
          }

          // GET /view/:id/conflicts
          const viewConflictsMatch = path.match(/^\/view\/([^/]+)\/conflicts$/)
          if (viewConflictsMatch) {
            const viewId = viewConflictsMatch[1]
            const result = vcsRun('view', 'conflicts', viewId)
            return jsonResponse(res, result ?? [])
          }

          // GET /active-view — reads the state file written by demo scripts
          if (path === '/active-view') {
            if (!existsSync(STATE_FILE)) {
              return jsonResponse(res, null)
            }
            const state = JSON.parse(readFileSync(STATE_FILE, 'utf8'))
            return jsonResponse(res, state)
          }

          return next()
        } catch (e) {
          errResponse(res, e.message)
        }
      })
    },
  }
}

// Better-sqlite3 is optional; fall back to returning empty arrays
async function importSqlite() {
  try {
    return await import('better-sqlite3')
  } catch {
    return {
      Database: class {
        constructor() {}
        prepare() { return { all: () => [] } }
        close() {}
      },
    }
  }
}
