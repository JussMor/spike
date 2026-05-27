#!/usr/bin/env node
/**
 * vcs-dev-server — framework-neutral store-backed dev-server runtime.
 *
 * Vite has a deep adapter because Vite exposes module graph and HMR hooks.
 * Other stacks should share this runtime instead of inventing per-framework
 * store polling, view reads, session phase tracking, and status endpoints.
 */

import { createServer } from 'node:http'
import { spawnSync } from 'node:child_process'
import { existsSync, createReadStream, statSync, realpathSync } from 'node:fs'
import { extname, join, relative, resolve, normalize } from 'node:path'
import { fileURLToPath } from 'node:url'

export function findVcsBin({ root = process.cwd(), storePath = '.vcs' } = {}) {
  if (process.env.VCS_BIN && existsSync(process.env.VCS_BIN)) return process.env.VCS_BIN
  const resolvedStore = resolve(root, storePath)
  const candidates = [
    resolve(root, 'target/release/vcs'),
    resolve(resolvedStore, '../../target/release/vcs'),
    resolve(fileURLToPath(import.meta.url), '../../../target/release/vcs'),
    '/Users/jussmor/.cargo/bin/vcs',
    '/opt/homebrew/bin/vcs',
  ]
  for (const candidate of candidates) if (existsSync(candidate)) return candidate
  return 'vcs'
}

export function vcsRun(bin, storePath, args, { cwd = process.cwd(), json = true } = {}) {
  const fullArgs = json
    ? ['--json', '--store', storePath, ...args]
    : ['--store', storePath, ...args]
  const r = spawnSync(bin, fullArgs, {
    cwd,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  })
  if (r.status !== 0) throw new Error(r.stderr?.trim() || `vcs ${args[0]} failed`)
  if (!json) return r.stdout ?? ''
  const out = r.stdout?.trim()
  if (!out) return null
  try { return JSON.parse(out) } catch { return { text: out } }
}

function vcsRunQuiet(bin, storePath, args, options) {
  try { return vcsRun(bin, storePath, args, options) } catch { return null }
}

function safeRelative(root, requestPath) {
  const decoded = decodeURIComponent(requestPath.split('?')[0] || '/')
  const clean = normalize(decoded).replace(/^(\.\.[/\\])+/, '').replace(/^[/\\]+/, '')
  const absolute = resolve(root, clean || 'index.html')
  const rel = relative(root, absolute)
  if (rel.startsWith('..') || rel.startsWith('/')) return null
  return rel
}

function contentType(path) {
  switch (extname(path)) {
    case '.html': return 'text/html; charset=utf-8'
    case '.js':
    case '.mjs':
    case '.ts':
    case '.tsx':
    case '.jsx': return 'text/javascript; charset=utf-8'
    case '.css': return 'text/css; charset=utf-8'
    case '.json': return 'application/json; charset=utf-8'
    case '.svg': return 'image/svg+xml'
    case '.png': return 'image/png'
    case '.jpg':
    case '.jpeg': return 'image/jpeg'
    case '.webp': return 'image/webp'
    default: return 'application/octet-stream'
  }
}

function escapeHtml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;')
}

export function createVcsAgentRuntime(options = {}) {
  const root = resolve(options.root ?? process.cwd())
  const storePath = resolve(root, options.storePath ?? '.vcs')
  const stackId = options.stackId ?? process.env.VCS_STACK_ID
  const sessionId = options.sessionId ?? process.env.VCS_SESSION_ID
  const pollMs = options.pollMs ?? 300
  const bin = options.bin ?? findVcsBin({ root, storePath })

  let viewId = null
  let lastTip = null
  let trackedFiles = new Set()
  let timer = null
  const listeners = new Set()

  function run(args, opts) {
    return vcsRun(bin, storePath, args, { cwd: root, ...opts })
  }

  function runQuiet(args, opts) {
    return vcsRunQuiet(bin, storePath, args, { cwd: root, ...opts })
  }

  function refreshView() {
    if (!stackId || !existsSync(join(storePath, 'vcs.db'))) return null
    const opened = runQuiet(['view', 'open', '--base', '', '--stacks', stackId])
    viewId = opened?.view_id ?? viewId
    const info = runQuiet(['stack', 'info', stackId])
    lastTip = info?.tip_change_id ?? lastTip
    const files = viewId ? runQuiet(['view', 'ls', viewId])?.files : []
    trackedFiles = new Set(files ?? [])
    return viewId
  }

  function refreshViewStrict() {
    if (!stackId) return null
    if (!existsSync(join(storePath, 'vcs.db'))) {
      throw new Error(`vcs store not found at ${storePath}`)
    }
    run(['stack', 'info', stackId])
    const opened = run(['view', 'open', '--base', '', '--stacks', stackId])
    viewId = opened?.view_id ?? null
    if (!viewId) throw new Error(`could not open vcs view for stack ${stackId}`)
    const info = run(['stack', 'info', stackId])
    lastTip = info?.tip_change_id ?? null
    const files = run(['view', 'ls', viewId])?.files ?? []
    trackedFiles = new Set(files)
    return viewId
  }

  function read(relPath) {
    if (!viewId) refreshView()
    if (!viewId || !trackedFiles.has(relPath)) return null
    return runQuiet(['view', 'read', viewId, relPath], { json: false })
  }

  function changedSince(fromTip, toTip) {
    if (!fromTip || !toTip || fromTip === toTip) return { changed: [], deleted: [] }
    const diff = runQuiet(['diff', fromTip, toTip])
    if (!Array.isArray(diff)) return { changed: [], deleted: [] }
    return {
      changed: diff.filter(e => e.op !== 'delete').map(e => e.path).filter(Boolean),
      deleted: diff.filter(e => e.op === 'delete').map(e => e.path).filter(Boolean),
    }
  }

  function pollOnce() {
    if (!stackId) return null
    const previous = lastTip
    const info = runQuiet(['stack', 'info', stackId])
    const next = info?.tip_change_id ?? null
    if (!next || next === previous) return null
    const changes = changedSince(previous, next)
    refreshView()
    const event = { previousTip: previous, tip: next, ...changes }
    for (const listener of listeners) listener(event)
    return event
  }

  function start() {
    refreshViewStrict()
    if (sessionId) runQuiet(['session', 'phase', sessionId, 'testing'])
    if (!timer) timer = setInterval(pollOnce, pollMs)
    return runtime
  }

  function stop() {
    if (timer) clearInterval(timer)
    timer = null
    if (sessionId) runQuiet(['session', 'phase', sessionId, 'done'])
  }

  function onChange(listener) {
    listeners.add(listener)
    return () => listeners.delete(listener)
  }

  function status() {
    return {
      root,
      storePath,
      sessionId,
      stackId,
      viewId,
      lastTip,
      trackedFiles: [...trackedFiles],
    }
  }

  const runtime = {
    root,
    storePath,
    stackId,
    sessionId,
    bin,
    start,
    stop,
    refreshView,
    read,
    files: () => [...trackedFiles],
    status,
    pollOnce,
    onChange,
    run,
  }

  return runtime
}

export function vcsSourceMiddleware(options = {}) {
  const runtime = options.runtime ?? createVcsAgentRuntime(options)
  if (options.autoStart !== false) runtime.start()

  return function handleVcsSource(req, res, next) {
    if (req.url === '/api/vcs-agent/session') {
      res.setHeader('Content-Type', 'application/json')
      res.end(JSON.stringify(runtime.status()))
      return
    }

    if (req.url === '/api/vcs-agent/events') {
      res.writeHead(200, {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        Connection: 'keep-alive',
      })
      const off = runtime.onChange(event => {
        res.write('event: vcs-change\n')
        res.write(`data: ${JSON.stringify(event)}\n\n`)
      })
      req.on('close', off)
      return
    }

    const rel = safeRelative(runtime.root, req.url ?? '/')
    if (!rel) {
      res.statusCode = 400
      res.end('invalid path')
      return
    }

    const content = runtime.read(rel)
    if (content !== null) {
      res.setHeader('Content-Type', contentType(rel))
      res.end(content)
      return
    }

    if (next) next()
    else {
      res.statusCode = 404
      res.end('not found')
    }
  }
}

export function createVcsStaticServer(options = {}) {
  const root = resolve(options.root ?? process.cwd())
  const runtime = options.runtime ?? createVcsAgentRuntime({ ...options, root })
  const middleware = vcsSourceMiddleware({ runtime })

  const server = createServer((req, res) => {
    middleware(req, res, () => {
      const rel = safeRelative(root, req.url ?? '/')
      if (!rel) {
        res.statusCode = 400
        res.end('invalid path')
        return
      }
      const diskPath = resolve(root, rel)
      if (!existsSync(diskPath) || !statSync(diskPath).isFile()) {
        const status = runtime.status()
        const body = `<!doctype html>
<meta charset="utf-8">
<title>vcs-dev-server: not found</title>
<style>
body { font: 14px/1.45 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; margin: 24px; color: #202124; }
code, pre { background: #f5f5f5; padding: 2px 4px; border-radius: 4px; }
pre { padding: 12px; overflow: auto; }
</style>
<h1>not found: /${rel}</h1>
<p>The server is running, but this path is not present in the active vcs stack view or on disk.</p>
<pre>${escapeHtml(JSON.stringify({
  requested: req.url,
  resolvedPath: rel,
  root: status.root,
  storePath: status.storePath,
  storeExists: existsSync(join(status.storePath, 'vcs.db')),
  stackId: status.stackId,
  viewId: status.viewId,
  trackedFiles: status.trackedFiles,
}, null, 2))}</pre>
<p>Check <a href="/api/vcs-agent/session">/api/vcs-agent/session</a>.</p>
`
        res.statusCode = 404
        res.setHeader('Content-Type', 'text/html; charset=utf-8')
        res.end(body)
        return
      }
      res.setHeader('Content-Type', contentType(diskPath))
      createReadStream(diskPath).pipe(res)
    })
  })
  server.__vcsRuntime = runtime
  return server
}

export function startVcsStaticServer(options = {}) {
  const port = Number(options.port ?? process.env.PORT ?? 8787)
  const host = options.host ?? process.env.HOST ?? '127.0.0.1'
  const server = createVcsStaticServer(options)
  server.listen(port, host, () => {
    const runtime = server.__vcsRuntime
    if (runtime) {
      const status = runtime.status()
      console.log(`[vcs-dev-server] root=${status.root}`)
      console.log(`[vcs-dev-server] store=${status.storePath}`)
      console.log(`[vcs-dev-server] stack=${status.stackId ?? '(none)'}`)
      console.log(`[vcs-dev-server] view=${status.viewId ?? '(none)'}`)
      console.log(`[vcs-dev-server] tracked=${status.trackedFiles.length}`)
    }
    console.log(`[vcs-dev-server] http://${host}:${port}`)
  })
  return server
}

function parseArgs(argv) {
  const out = {}
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]
    if (!arg.startsWith('--')) continue
    const key = arg.slice(2).replace(/-([a-z])/g, (_, c) => c.toUpperCase())
    const next = argv[i + 1]
    out[key] = next && !next.startsWith('--') ? argv[++i] : true
  }
  return out
}

function isDirectRun() {
  if (!process.argv[1]) return false
  try {
    return realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))
  } catch {
    return process.argv[1] === fileURLToPath(import.meta.url)
  }
}

if (isDirectRun()) {
  const args = parseArgs(process.argv.slice(2))
  if (args.help || args.h) {
    console.log(`Usage: vcs-dev-server [options]

Options:
  --root <dir>       Project root to serve. Defaults to current directory.
  --dir <dir>        Alias for --root.
  --store <dir>      vcs store directory. Defaults to .vcs under root.
  --stack <id>       vcs stack id to serve.
  --session <id>     vcs session id for phase/status tracking.
  --port <port>      Port to listen on. Defaults to 8787.
  --host <host>      Host to listen on. Defaults to 127.0.0.1.
  --help             Show this help.
`)
    process.exit(0)
  }
  const root = args.root ?? args.dir
  try {
    startVcsStaticServer({
      root,
      storePath: args.store,
      stackId: args.stack,
      sessionId: args.session,
      port: args.port,
      host: args.host,
    })
  } catch (error) {
    console.error(`[vcs-dev-server] ${error.message}`)
    console.error('[vcs-dev-server] Refusing to serve an empty/wrong stack. Verify --root, --store, and --stack.')
    process.exit(1)
  }
}
