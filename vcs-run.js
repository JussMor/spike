#!/usr/bin/env node
/**
 * vcs-run.js — zero-config orchestrator.
 *
 * Reads vcs.run.json, opens one vcs stack per agent, starts one
 * vcs-dev-server per stack, and writes vcs.run.lock.json so every
 * agent knows its stack_id without manual wiring.
 *
 * Usage:
 *   node vcs-run.js                 # reads ./vcs.run.json
 *   node vcs-run.js --manifest path/to/vcs.run.json
 *   node vcs-run.js --store .vcs    # override store path
 *
 * Agents read their stack_id from vcs.run.lock.json:
 *   cat vcs.run.lock.json | node -e \
 *     "const d=JSON.parse(require('fs').readFileSync('/dev/stdin','utf8')); \
 *      console.log(d.agents.find(a=>a.id==='agent-ui').stack_id)"
 *
 * On SIGINT / SIGTERM: closes all stacks cleanly and kills servers.
 */

import { readFileSync, writeFileSync, existsSync } from 'node:fs'
import { resolve, join, dirname } from 'node:path'
import { spawnSync, spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const __dir = dirname(fileURLToPath(import.meta.url))

// ── Argument parsing ──────────────────────────────────────────────────────

function parseArgs(argv) {
  const out = {}
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (!a.startsWith('--')) continue
    const key = a.slice(2).replace(/-([a-z])/g, (_, c) => c.toUpperCase())
    const next = argv[i + 1]
    out[key] = next && !next.startsWith('--') ? argv[++i] : true
  }
  return out
}

const args = parseArgs(process.argv.slice(2))

// ── Manifest ───────────────────────────────────────────────────────────────

const manifestPath = resolve(args.manifest ?? 'vcs.run.json')
if (!existsSync(manifestPath)) {
  console.error(`[vcs-run] manifest not found: ${manifestPath}`)
  console.error('[vcs-run] Create vcs.run.json or pass --manifest <path>')
  process.exit(1)
}

const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
const root      = resolve(dirname(manifestPath), manifest.root ?? '.')
const storePath = resolve(root, args.store ?? manifest.store ?? '.vcs')
const lockPath  = join(dirname(manifestPath), 'vcs.run.lock.json')

// ── Binary resolution ──────────────────────────────────────────────────────

function findBin() {
  if (process.env.VCS_BIN && existsSync(process.env.VCS_BIN)) return process.env.VCS_BIN
  const candidates = [
    join(root, 'target/release/vcs'),
    join(__dir, 'target/release/vcs'),
    '/usr/local/bin/vcs',
  ]
  for (const c of candidates) if (existsSync(c)) return c
  return 'vcs'
}

const VCS = findBin()

function vcs(args_) {
  const r = spawnSync(VCS, ['--json', '--store', storePath, ...args_], {
    cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024,
  })
  if (r.status !== 0) throw new Error(r.stderr?.trim() || `vcs ${args_[0]} failed`)
  const out = r.stdout?.trim()
  if (!out) return null
  try { return JSON.parse(out) } catch { return { text: out } }
}

function vcsQuiet(args_) {
  try { return vcs(args_) } catch { return null }
}

// ── Store init ─────────────────────────────────────────────────────────────

if (!existsSync(join(storePath, 'vcs.db'))) {
  console.log('[vcs-run] Initialising vcs store...')
  vcs(['init'])
}

// ── Open stacks ────────────────────────────────────────────────────────────

const agentDefs = manifest.agents ?? []
if (agentDefs.length === 0) {
  console.error('[vcs-run] No agents defined in manifest. Add at least one entry to "agents".')
  process.exit(1)
}

console.log(`[vcs-run] Opening ${agentDefs.length} stack(s)...`)
const openedStacks = []
for (const agent of agentDefs) {
  const result = vcs(['stack', 'open', '--agent', agent.id])
  const stackId = result?.stack_id
  if (!stackId) throw new Error(`failed to open stack for agent ${agent.id}`)
  console.log(`[vcs-run]   ${agent.id} → stack ${stackId} on :${agent.port}`)
  openedStacks.push({ id: agent.id, port: agent.port, stack_id: stackId })
}

// ── Write lock file ────────────────────────────────────────────────────────

const lockData = { started_at: Date.now(), root, store: storePath, agents: openedStacks }
writeFileSync(lockPath, JSON.stringify(lockData, null, 2) + '\n')
console.log(`[vcs-run] Lock written → ${lockPath}`)

// ── Spawn dev-servers ──────────────────────────────────────────────────────

const devServerBin = join(__dir, 'packages/vcs-dev-server/index.js')
const children = []

for (const agent of openedStacks) {
  const child = spawn(process.execPath, [
    devServerBin,
    '--root',    root,
    '--store',   storePath,
    '--stack',   agent.stack_id,
    '--port',    String(agent.port),
  ], {
    env: { ...process.env, VCS_BIN: VCS },
    stdio: 'inherit',
  })
  child.__agentId  = agent.id
  child.__stackId  = agent.stack_id
  child.__port     = agent.port
  children.push(child)
  child.on('exit', (code) => {
    if (code !== 0 && code !== null) {
      console.error(`[vcs-run] server for ${agent.id} exited with code ${code}`)
    }
  })
}

// ── Spawn dashboard (optional) ─────────────────────────────────────────────

if (manifest.dashboard_port) {
  const dashboardBin = join(__dir, 'packages/vcs-dev-server/dashboard.js')
  if (existsSync(dashboardBin)) {
    const dash = spawn(process.execPath, [
      dashboardBin,
      '--store',  storePath,
      '--lock',   lockPath,
      '--port',   String(manifest.dashboard_port),
    ], {
      env: { ...process.env, VCS_BIN: VCS },
      stdio: 'inherit',
    })
    children.push(dash)
  }
}

// ── Status ─────────────────────────────────────────────────────────────────

console.log('\n[vcs-run] Running — Ctrl+C to stop and close stacks\n')
for (const a of openedStacks) {
  console.log(`  ${a.id.padEnd(20)} http://localhost:${a.port}   stack: ${a.stack_id}`)
}
if (manifest.dashboard_port) {
  console.log(`  ${'dashboard'.padEnd(20)} http://localhost:${manifest.dashboard_port}`)
}
console.log()
console.log('  Agents read their stack_id from vcs.run.lock.json')
console.log()

// ── Shutdown ───────────────────────────────────────────────────────────────

function shutdown(signal) {
  console.log(`\n[vcs-run] ${signal} — closing stacks and stopping servers...`)
  for (const child of children) {
    try { child.kill('SIGTERM') } catch {}
  }
  for (const agent of openedStacks) {
    const r = vcsQuiet(['stack', 'close', agent.stack_id])
    const status = r?.status ?? 'closed'
    console.log(`[vcs-run]   closed stack ${agent.stack_id} (${agent.id}) → ${status}`)
  }
  try { writeFileSync(lockPath, JSON.stringify({ ...lockData, stopped_at: Date.now() }, null, 2) + '\n') } catch {}
  process.exit(0)
}

process.on('SIGINT',  () => shutdown('SIGINT'))
process.on('SIGTERM', () => shutdown('SIGTERM'))
