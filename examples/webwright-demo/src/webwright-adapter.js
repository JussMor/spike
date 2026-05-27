/**
 * webwright-adapter.js
 *
 * Wraps vcs-spike around a Webwright-style agent so every script the agent
 * writes, every file it produces, and every decision it makes is recorded
 * as a structured change event.
 *
 * This is the integration layer — NOT the full Webwright runtime.
 * It follows the same pattern: agent writes code → code runs → output recorded.
 *
 * Key principle:
 *   Agent doesn't write to disk directly.
 *   Agent calls adapter.write() which records via vcs then writes to disk.
 *   This gives you: audit trail + conflict detection + intent metadata.
 */

import { spawnSync } from 'node:child_process'
import { writeFileSync, mkdirSync, existsSync, readFileSync } from 'node:fs'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dir = dirname(fileURLToPath(import.meta.url))
const PROJECT_ROOT = resolve(__dir, '..')
const VCS_STORE    = join(PROJECT_ROOT, '.vcs')

function findBin() {
  if (process.env.VCS_BIN) return process.env.VCS_BIN
  const ws = resolve(PROJECT_ROOT, '../../target/release/vcs')
  if (existsSync(ws)) return ws
  return 'vcs'
}

const BIN = findBin()

// ── vcs runner ────────────────────────────────────────────────────────────

function vcsRun(...args) {
  const r = spawnSync(BIN, ['--json', ...args], {
    cwd: PROJECT_ROOT,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
    env: { ...process.env, VCS_BIN: BIN },
  })
  if (r.status !== 0) throw new Error(`vcs ${args[0]}: ${r.stderr}`)
  const out = r.stdout.trim()
  return out ? JSON.parse(out) : null
}

function vcsEdit(stackId, path, content, reason, taskRef, toolCall) {
  const tmp = join(VCS_STORE, `tmp-${Date.now()}-${Math.random().toString(36).slice(2)}`)
  writeFileSync(tmp, content)
  try {
    const args = ['edit', stackId, path, '--content-file', tmp, '--reason', reason]
    if (taskRef) args.push('--task-ref', taskRef)
    if (toolCall) args.push('--tool-call', JSON.stringify(toolCall))
    return vcsRun(...args).change_id
  } finally {
    try { require('fs').unlinkSync(tmp) } catch {}
  }
}

// ── WebwrightAdapter ──────────────────────────────────────────────────────

/**
 * One adapter instance = one agent session.
 * The agent opens a stack, does work via adapter methods, closes the stack.
 * The orchestrator then opens a view over all stacks.
 */
export class WebwrightAdapter {
  constructor({ agentId, taskId, baseChangeId }) {
    this.agentId = agentId
    this.taskId  = taskId
    this.stackId = null
    this.steps   = []
    this.baseChangeId = baseChangeId
  }

  // ── Lifecycle ─────────────────────────────────────────────────────────

  begin() {
    const args = ['stack', 'open', '--agent', this.agentId]
    if (this.baseChangeId) args.push('--base', this.baseChangeId)
    this.stackId = vcsRun(...args).stack_id
    console.log(`  [${this.agentId}] stack=${this.stackId.slice(0, 8)}…`)
    return this
  }

  done() {
    vcsRun('stack', 'close', this.stackId)
    console.log(`  [${this.agentId}] stack closed (${this.steps.length} steps)`)
    return this.stackId
  }

  abandon() {
    vcsRun('stack', 'abandon', this.stackId)
  }

  // ── Core action: write a file ─────────────────────────────────────────
  //
  // This is what Webwright calls when the agent produces a Playwright script,
  // a fixture, a helper, or any output file.

  write(path, content, { reason, playwrightCall } = {}) {
    if (!reason) throw new Error('reason required — agent must say why')

    const changeId = vcsEdit(
      this.stackId,
      path,
      typeof content === 'string' ? content : JSON.stringify(content, null, 2),
      reason,
      this.taskId,
      playwrightCall ? { name: 'playwright', call: playwrightCall } : undefined,
    )

    this.steps.push({ path, reason, changeId })
    console.log(`  [${this.agentId}] ${changeId.slice(0, 8)}… write ${path}`)
    return changeId
  }

  // ── Simulate Playwright execution ─────────────────────────────────────
  //
  // In real Webwright the agent runs the script and inspects the result.
  // Here we simulate: write the script, "run" it, write the result artifact.

  async runScript(scriptPath, scriptContent, { reason } = {}) {
    // 1. Record the script itself
    this.write(scriptPath, scriptContent, {
      reason: reason ?? `playwright script for ${this.taskId}`,
      playwrightCall: { script: scriptPath, action: 'write' },
    })

    // 2. Simulate execution result (in real Webwright: actually runs Playwright)
    const result = {
      status:     'passed',
      duration:   Math.floor(Math.random() * 800) + 200,
      screenshot: `trajectories/${this.taskId}/${Date.now()}.png`,
      assertions: Math.floor(Math.random() * 5) + 1,
    }

    // 3. Record the result artifact
    this.write(
      `trajectories/${this.taskId}/result.json`,
      result,
      {
        reason: `execution result for ${scriptPath} — ${result.status}`,
        playwrightCall: { script: scriptPath, action: 'execute', result },
      },
    )

    return result
  }

  get log() {
    return vcsRun('log', this.stackId) ?? []
  }
}

// ── Orchestrator helpers ──────────────────────────────────────────────────

export function openView(baseChangeId, stackIds) {
  return vcsRun('view', 'open', '--base', baseChangeId, '--stacks', stackIds.join(',')).view_id
}

export function viewFiles(viewId) {
  return vcsRun('view', 'ls', viewId)?.files ?? []
}

export function viewConflicts(viewId) {
  return vcsRun('view', 'conflicts', viewId) ?? []
}

export function resolveConflict(conflictId, winnerStackId) {
  return vcsRun('view', 'resolve', conflictId, '--pick', winnerStackId)
}

export function initStore() {
  if (!existsSync(join(VCS_STORE, 'vcs.db'))) {
    vcsRun('init')
  }
}

export function seedBase(files) {
  const seedStack = vcsRun('stack', 'open', '--agent', 'seed').stack_id
  let tip
  for (const [path, content] of files) {
    const tmp = join(VCS_STORE, `tmp-seed-${Date.now()}`)
    writeFileSync(tmp, content)
    tip = vcsRun('edit', seedStack, path, '--content-file', tmp, '--reason', `seed ${path}`).change_id
  }
  vcsRun('stack', 'close', seedStack)
  return tip
}

export { VCS_STORE, PROJECT_ROOT, BIN }
