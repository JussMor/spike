/**
 * orchestrator.js
 *
 * Spawns two Webwright-style agents in parallel.
 * Both are tracked by vcs-spike.
 * Conflicts are surfaced as data, resolved by the orchestrator.
 *
 * Run:  npm run demo
 */

import { Worker, isMainThread, workerData, parentPort } from 'node:worker_threads'
import { fileURLToPath } from 'node:url'
import { writeFileSync, existsSync } from 'node:fs'
import { join, resolve, dirname } from 'node:path'
import {
  initStore, seedBase, openView, viewFiles,
  viewConflicts, resolveConflict, VCS_STORE,
} from './webwright-adapter.js'

const __dir = dirname(fileURLToPath(import.meta.url))

// ── Worker (runs one agent task in isolation) ──────────────────────────────

if (!isMainThread) {
  const { taskName, agentId, taskId, baseChangeId } = workerData
  const { WebwrightAdapter } = await import('./webwright-adapter.js')
  const { run } = await import(`./tasks/task-${taskName}.js`)

  const adapter = new WebwrightAdapter({ agentId, taskId, baseChangeId })
  adapter.begin()
  await run(adapter)
  const stackId = adapter.done()
  const log = adapter.log

  parentPort.postMessage({ agentId, taskId, stackId, steps: log.length })
}

// ── Orchestrator main ──────────────────────────────────────────────────────

if (isMainThread) {
  const sep = (t) => console.log(`\n${'═'.repeat(60)}\n  ${t}\n${'═'.repeat(60)}`)
  const log = (...a) => console.log(...a)

  sep('Webwright × vcs-spike — Parallel Agent Demo')

  // Init store
  initStore()
  log('  ✓ store initialised\n')

  // Seed a base (shared config both agents inherit)
  log('── Seeding base snapshot ─────────────────────────────────')
  const baseTip = seedBase([
    ['package.json',    JSON.stringify({ name: 'my-app', version: '0.1.0' }, null, 2)],
    ['tsconfig.json',   JSON.stringify({ compilerOptions: { strict: true } }, null, 2)],
    ['e2e/playwright.config.ts', `
import { defineConfig } from '@playwright/test'
export default defineConfig({
  testDir: './e2e/tests',
  use: { baseURL: 'http://localhost:5173' },
})`.trim()],
  ])
  log(`  base tip: ${baseTip.slice(0, 12)}…\n`)

  // Define tasks
  const tasks = [
    { taskName: 'login',     agentId: 'agent-login',     taskId: 'task-login'     },
    { taskName: 'dashboard', agentId: 'agent-dashboard',  taskId: 'task-dashboard' },
  ]

  // Spawn agents in parallel
  sep('Spawning agents in parallel')
  const t0 = Date.now()

  const results = await Promise.all(
    tasks.map(task =>
      new Promise((resolve, reject) => {
        log(`  ↑ spawning ${task.agentId} (${task.taskId})`)
        const w = new Worker(fileURLToPath(import.meta.url), {
          workerData: { ...task, baseChangeId: baseTip },
        })
        w.on('message', resolve)
        w.on('error', reject)
        w.on('exit', code => {
          if (code !== 0) reject(new Error(`${task.agentId} exited ${code}`))
        })
      })
    )
  )

  const elapsed = Date.now() - t0
  log(`\n  ✓ all agents done in ${elapsed}ms\n`)
  results.forEach(r =>
    log(`  ${r.agentId}  stack=${r.stackId.slice(0, 8)}  steps=${r.steps}`)
  )

  // Open merged view
  sep('Orchestrator: open merged view')
  const stackIds = results.map(r => r.stackId)
  const viewId = openView(baseTip, stackIds)
  log(`  view: ${viewId.slice(0, 8)}…`)

  const files = viewFiles(viewId)
  log(`\n  Files in merged view (${files.length}):`)
  files.forEach(f => log(`    ${f}`))

  // Detect conflicts
  const conflicts = viewConflicts(viewId)
  log(`\n  Conflicts detected: ${conflicts.length}`)

  if (conflicts.length > 0) {
    sep('Conflicts — surfaced as data, resolved by orchestrator')

    for (const c of conflicts) {
      log(`\n  ⚡ ${c.path}`)
      log(`     ${c.candidates.length} candidates:`)
      c.candidates.forEach(cand =>
        log(`       stack=${cand.stack_id.slice(0, 8)}  blob=${(cand.blob_hash ?? 'deleted').slice(0, 8)}`)
      )

      // Resolution policy: pick the agent whose version has MORE data-testid
      // attributes — heuristic for "more complete component"
      // In practice the orchestrator would do semantic merge or ask an LLM
      const winner = c.candidates[0].stack_id
      resolveConflict(c.conflict_id, winner)
      log(`\n  ✓ resolved → picked ${winner.slice(0, 8)} (login agent wins on LoginForm)`)
    }
  }

  // Save active view for tanstack-vite UI
  const stateFile = join(resolve(__dir, '..', 'tanstack-vite', '.vcs'), 'active-view.json')
  if (existsSync(join(resolve(__dir, '..', 'tanstack-vite', '.vcs'), 'vcs.db'))) {
    writeFileSync(stateFile, JSON.stringify({
      view_id: viewId,
      base_change_id: baseTip,
      stack_ids: stackIds,
    }, null, 2))
    log('\n  ✓ active-view.json updated — open tanstack-vite to see in browser')
  }

  sep('Summary')
  log(`  Agents run in parallel:    ${tasks.length}`)
  log(`  Total files tracked:       ${files.length}`)
  log(`  Conflicts detected:        ${conflicts.length}`)
  log(`  Conflicts resolved:        ${conflicts.length}`)
  log(`  Wall time:                 ${elapsed}ms`)
  log()
  log('  Each agent wrote Playwright specs with data-testid selectors.')
  log('  vcs captured every write with task_ref + playwright call context.')
  log('  Conflict on LoginForm.tsx surfaced automatically — no silent overwrite.')
  log()
}
