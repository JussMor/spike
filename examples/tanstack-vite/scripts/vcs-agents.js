/**
 * vcs-agents.js
 *
 * Parallel agents demo — N worker_threads simultaneously edit this project's
 * files through the shared .vcs/ store, then the orchestrator opens a view.
 *
 * This is the parallel-servers proof applied to a real Vite project:
 *   - Each worker represents an agent working on a different feature
 *   - All share the .vcs/ SQLite store
 *   - The orchestrator merges their stacks and detects conflicts
 *
 * Run: npm run vcs:agents [N]
 */

import { Worker, isMainThread, workerData, parentPort } from 'node:worker_threads'
import { fileURLToPath } from 'node:url'
import { readFileSync, writeFileSync, existsSync } from 'node:fs'
import { join, resolve, dirname } from 'node:path'
import { spawnSync } from 'node:child_process'

const __dir = dirname(fileURLToPath(import.meta.url))
const PROJECT_ROOT = resolve(__dir, '..')
const VCS_STORE = join(PROJECT_ROOT, '.vcs')
const STATE_FILE = join(VCS_STORE, 'active-view.json')

function findBin() {
  if (process.env.VCS_BIN) return process.env.VCS_BIN
  const ws = resolve(PROJECT_ROOT, '../../target/release/vcs')
  if (existsSync(ws)) return ws
  return 'vcs'
}

const BIN = findBin()

// ── Worker ────────────────────────────────────────────────────────────────

if (!isMainThread) {
  const { agentId, baseTip, edits } = workerData

  function vcsSyncRun(...args) {
    const r = spawnSync(BIN, ['--json', ...args], {
      cwd: PROJECT_ROOT,
      encoding: 'utf8',
      maxBuffer: 8 * 1024 * 1024,
      env: { ...process.env, VCS_BIN: BIN },
    })
    if (r.status !== 0) throw new Error(`vcs ${args[0]} (${agentId}): ${r.stderr}`)
    return JSON.parse(r.stdout.trim())
  }

  function editFile(stackId, path, content, reason) {
    const tmp = join(import.meta.dirname, '..', '.vcs', `tmp-${agentId}-${Date.now()}`)
    writeFileSync(tmp, content)
    const r = spawnSync(BIN, ['--json', 'edit', stackId, path, '--content-file', tmp, '--reason', reason], {
      cwd: PROJECT_ROOT,
      encoding: 'utf8',
      env: { ...process.env, VCS_BIN: BIN },
    })
    if (r.status !== 0) throw new Error(`vcs edit (${agentId}): ${r.stderr}`)
    return JSON.parse(r.stdout.trim()).change_id
  }

  const stackId = vcsSyncRun('stack', 'open', '--agent', agentId, '--base', baseTip).stack_id
  const changeIds = []

  for (const { path, content, reason } of edits) {
    const cid = editFile(stackId, path, content, reason)
    changeIds.push(cid)
  }

  vcsSyncRun('stack', 'close', stackId)
  parentPort.postMessage({ agentId, stackId, changeIds })
}

// ── Orchestrator ──────────────────────────────────────────────────────────

if (isMainThread) {
  const N = parseInt(process.argv[2] ?? '4', 10)
  const { vcs } = await import('../vcs-integration/client.js')

  const log = (...a) => console.log(...a)
  const sep = (t) => log(`\n${'─'.repeat(58)}\n  ${t}\n${'─'.repeat(58)}`)

  sep(`Parallel Agents Demo — ${N} workers on the Vite project`)
  log(`  Store: ${VCS_STORE}\n`)

  // Seed base from real source files
  const devStack = vcs.stackOpen('seed')
  const seedFiles = ['src/main.tsx', 'src/App.tsx', 'package.json']
  let baseTip
  for (const f of seedFiles) {
    const abs = join(PROJECT_ROOT, f)
    const content = existsSync(abs) ? readFileSync(abs, 'utf8') : `// ${f}\n`
    baseTip = vcs.edit(devStack, f, content, { reason: `seed ${f}` })
  }
  vcs.stackClose(devStack)
  log(`  base tip: ${baseTip.slice(0, 12)}…\n`)

  // Build per-agent workloads
  const agentWork = Array.from({ length: N }, (_, i) => ({
    agentId: `agent-${String.fromCharCode(65 + i)}`,
    baseTip,
    edits: [
      {
        path: `src/features/feature-${i}/index.tsx`,
        content: `import { useQuery } from '@tanstack/react-query'\n\nexport function Feature${i}() {\n  const { data } = useQuery({ queryKey: ['feature', ${i}], queryFn: async () => ${i} })\n  return <div>Feature ${i}: {data}</div>\n}\n`,
        reason: `agent-${String.fromCharCode(65 + i)} implements feature ${i}`,
      },
      {
        path: `src/features/feature-${i}/styles.css`,
        content: `.feature-${i} { color: hsl(${i * 60}, 70%, 60%); }\n`,
        reason: `add styles for feature ${i}`,
      },
    ],
  }))

  log(`Spawning ${N} agent workers…`)
  const t0 = Date.now()

  const results = await Promise.all(
    agentWork.map(work =>
      new Promise((resolve, reject) => {
        const w = new Worker(fileURLToPath(import.meta.url), { workerData: work })
        w.on('message', resolve)
        w.on('error', reject)
        w.on('exit', code => { if (code !== 0) reject(new Error(`worker exited ${code}`)) })
      })
    )
  )

  const elapsed = Date.now() - t0
  log(`  ✓ ${N} workers done in ${elapsed}ms\n`)
  results.forEach(r => log(`    ${r.agentId}: stack=${r.stackId.slice(0, 8)}  changes=${r.changeIds.length}`))

  // Open merged view
  sep('Orchestrator opens merged view')
  const stackIds = [devStack, ...results.map(r => r.stackId)]
  const viewId = vcs.viewOpen(baseTip, stackIds)
  log(`  view: ${viewId.slice(0, 8)}…`)

  const files = vcs.viewLs(viewId)
  log(`  files in merged view: ${files.length}`)
  files.forEach(f => log(`    ${f}`))

  const conflicts = vcs.viewConflicts(viewId)
  log(`\n  conflicts: ${conflicts.length} (expected 0 — all private files)`)

  // Save for dev server
  writeFileSync(STATE_FILE, JSON.stringify({ view_id: viewId, base_change_id: baseTip, stack_ids: stackIds }, null, 2))
  log(`\n  saved to .vcs/active-view.json → npm run dev to see in browser`)

  sep('Results')
  log(`  ${N} agents × ${agentWork[0].edits.length} edits = ${N * agentWork[0].edits.length} total edits`)
  log(`  Wall time: ${elapsed}ms  (${(elapsed / (N * 2)).toFixed(1)}ms/edit)`)
  log(`  SQLite WAL handled concurrent writes: ✓`)
  log(`  View merged all stacks: ✓`)
  log()
}
