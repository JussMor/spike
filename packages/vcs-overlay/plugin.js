/**
 * plugin.js — Vite plugin for per-agent file overlays.
 *
 * How it works:
 *   1. Each agent session gets an `overlayDir` (e.g. /tmp/vcs-sessions/s1234/).
 *   2. The plugin's `load()` hook fires for every module request.
 *      If overlayDir/relPath exists → serve that instead of the real disk file.
 *      Otherwise → Vite loads normally (no change to the source tree).
 *   3. `configureServer` watches overlayDir and pushes targeted HMR:
 *      only the agent's browser connections receive the update.
 *
 * "Portless" means: each agent's Vite instance binds to port 0 so the OS
 * picks a free port at startup. No hardcoded ports, no conflicts.
 *
 * The overlay is purely virtual — the real source files are never modified.
 */

import fs from 'node:fs'
import path from 'node:path'

/**
 * @param {{ sessionId: string, overlayDir: string }} options
 * @returns {import('vite').Plugin}
 */
export function sessionOverlay(options) {
  const { sessionId, overlayDir } = options
  let projectRoot = ''

  return {
    name: 'vcs-session-overlay',
    enforce: 'pre',

    configResolved(config) {
      projectRoot = config.root
    },

    /**
     * Intercept every module load. If the agent has an overlay version of
     * this file, return that content instead of what's on disk.
     *
     * Vite calls this with the resolved absolute path, so we map:
     *   /project/src/App.tsx → overlayDir/src/App.tsx
     */
    load(id) {
      // Skip virtual modules and node_modules
      if (id.includes('\0') || id.includes('node_modules')) return

      // Only intercept files inside the project root
      if (!id.startsWith(projectRoot)) return

      const relPath = path.relative(projectRoot, id)
      const overlayPath = path.join(overlayDir, relPath)

      if (fs.existsSync(overlayPath)) {
        // Tell Vite to re-run this loader if the overlay file changes
        this.addWatchFile(overlayPath)
        return {
          code: fs.readFileSync(overlayPath, 'utf8'),
          map: null,
        }
      }
    },

    configureServer(server) {
      // Ensure the overlay directory exists
      fs.mkdirSync(overlayDir, { recursive: true })

      // Add overlay dir to Vite's file watcher
      server.watcher.add(overlayDir)

      // When an overlay file changes → trigger HMR for the corresponding
      // source module (as if the source file itself had changed).
      const onOverlayChange = (file) => {
        if (!file.startsWith(overlayDir)) return

        const relPath = path.relative(overlayDir, file)
        const projectFile = path.join(projectRoot, relPath)
        const urlPath = '/' + relPath.replaceAll(path.sep, '/')

        // Try precise HMR first (only invalidates this one module)
        const mod = server.moduleGraph.getModuleById(projectFile)
        if (mod) {
          server.moduleGraph.invalidateModule(mod)
          server.ws.send({
            type: 'update',
            updates: [
              {
                type: 'js-update',
                path: urlPath,
                acceptedPath: urlPath,
                timestamp: Date.now(),
              },
            ],
          })
          console.log(`[vcs-overlay:${sessionId}] HMR → ${relPath}`)
        } else {
          // Module not yet in graph (first overlay write) — full reload
          server.ws.send({ type: 'full-reload' })
          console.log(`[vcs-overlay:${sessionId}] full-reload → ${relPath}`)
        }
      }

      server.watcher.on('change', onOverlayChange)
      server.watcher.on('add', onOverlayChange)
      server.watcher.on('unlink', onOverlayChange)

      // Expose session metadata for the in-browser session panel
      server.middlewares.use('/__vcs_session', (_req, res) => {
        const overlayFiles = scanOverlay(overlayDir)
        res.setHeader('Content-Type', 'application/json')
        res.setHeader('Cache-Control', 'no-store')
        res.end(
          JSON.stringify({ sessionId, overlayDir, projectRoot, overlayFiles }),
        )
      })

      // SSE stream: push overlay file list updates to the browser
      server.middlewares.use('/__vcs_session/events', (req, res) => {
        res.writeHead(200, {
          'Content-Type': 'text/event-stream',
          'Cache-Control': 'no-cache',
          Connection: 'keep-alive',
          'Access-Control-Allow-Origin': '*',
        })

        const send = () => {
          const data = JSON.stringify({
            sessionId,
            overlayFiles: scanOverlay(overlayDir),
          })
          res.write(`data: ${data}\n\n`)
        }

        send()
        const interval = setInterval(send, 1000)
        req.on('close', () => clearInterval(interval))
      })
    },
  }
}

/** List all files currently in the overlay directory (relative paths). */
function scanOverlay(dir, base = dir, out = []) {
  if (!fs.existsSync(dir)) return out
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      scanOverlay(full, base, out)
    } else {
      out.push(path.relative(base, full).replaceAll(path.sep, '/'))
    }
  }
  return out
}
