/**
 * vcs-integration/client.js
 *
 * Node.js wrapper for the vcs CLI, scoped to this project.
 * Reads/writes the .vcs/ store in the project root.
 *
 * Because this project uses vcs like git — you ran `vcs init` in this
 * directory and the store lives in .vcs/ — there is no --store flag needed.
 * The binary auto-detects it by walking up from CWD.
 *
 * Usage (from scripts/):
 *   import { vcs } from '../vcs-integration/client.js'
 *   const stack = vcs.stackOpen('my-agent')
 *   vcs.edit(stack, 'src/App.tsx', newContent, { reason: 'add header' })
 */

import { spawnSync } from 'node:child_process'
import { writeFileSync, mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { existsSync } from 'node:fs'

const __dir = dirname(fileURLToPath(import.meta.url))
const PROJECT_ROOT = resolve(__dir, '..')

// ── Binary resolution ─────────────────────────────────────────────────────

function findBin() {
  if (process.env.VCS_BIN) return process.env.VCS_BIN
  // Look for sibling workspace binary
  const ws = resolve(PROJECT_ROOT, '../../target/release/vcs')
  if (existsSync(ws)) return ws
  return 'vcs'
}

const BIN = findBin()

// ── Core runner ───────────────────────────────────────────────────────────

/**
 * Run vcs with the given args, returning parsed JSON output.
 * The CWD is set to the project root so the CLI auto-finds .vcs/.
 */
function run(args, input) {
  const result = spawnSync(BIN, ['--json', ...args], {
    cwd: PROJECT_ROOT,
    input,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
    env: { ...process.env, VCS_BIN: BIN },
  })

  if (result.status !== 0) {
    throw new Error(`vcs ${args[0]} failed:\n${result.stderr}`)
  }

  const out = result.stdout.trim()
  if (!out) return null
  try { return JSON.parse(out) } catch { return out }
}

function tmpWrite(content) {
  const p = join(tmpdir(), `vcs-ts-${Date.now()}-${Math.random().toString(36).slice(2)}`)
  writeFileSync(p, typeof content === 'string' ? content : Buffer.from(content))
  return p
}

// ── Public API ────────────────────────────────────────────────────────────

export const vcs = {
  // ── Store ───────────────────────────────────────────────────────────────

  /** Initialise the .vcs/ store in the project root. */
  init() {
    return run(['init'])
  },

  // ── Stacks ──────────────────────────────────────────────────────────────

  /** Open a stack for agentId, optionally branching from baseChangeId. */
  stackOpen(agentId, baseChangeId) {
    const args = ['stack', 'open', '--agent', agentId]
    if (baseChangeId) args.push('--base', baseChangeId)
    return run(args).stack_id
  },

  stackClose(stackId) {
    return run(['stack', 'close', stackId])
  },

  stackAbandon(stackId) {
    return run(['stack', 'abandon', stackId])
  },

  stackInfo(stackId) {
    return run(['stack', 'info', stackId])
  },

  // ── Edits ───────────────────────────────────────────────────────────────

  /**
   * Record an edit.
   * @param {string} stackId
   * @param {string} path           – path as stored in vcs (relative to project)
   * @param {string|Buffer} content
   * @param {{ reason: string, task_ref?: string, tool_call?: object }} intent
   */
  edit(stackId, path, content, { reason, task_ref, tool_call } = {}) {
    if (!reason) throw new Error('intent.reason is required')
    const tmp = tmpWrite(content)
    const args = ['edit', stackId, path, '--content-file', tmp, '--reason', reason]
    if (task_ref) args.push('--task-ref', task_ref)
    if (tool_call) args.push('--tool-call', JSON.stringify(tool_call))
    return run(args).change_id
  },

  delete(stackId, path, { reason, task_ref } = {}) {
    if (!reason) throw new Error('intent.reason is required')
    const args = ['delete', stackId, path, '--reason', reason]
    if (task_ref) args.push('--task-ref', task_ref)
    return run(args).change_id
  },

  rename(stackId, from, to, content, { reason, task_ref } = {}) {
    if (!reason) throw new Error('intent.reason is required')
    const tmp = tmpWrite(content)
    const args = ['rename', stackId, from, to, '--content-file', tmp, '--reason', reason]
    if (task_ref) args.push('--task-ref', task_ref)
    return run(args).change_id
  },

  // ── Views ────────────────────────────────────────────────────────────────

  viewOpen(baseChangeId, stackIds) {
    return run(['view', 'open', '--base', baseChangeId, '--stacks', stackIds.join(',')]).view_id
  },

  viewRead(viewId, path) {
    return run(['view', 'read', viewId, path])
  },

  viewLs(viewId) {
    return run(['view', 'ls', viewId]).files
  },

  viewConflicts(viewId) {
    return run(['view', 'conflicts', viewId])
  },

  resolveByPick(conflictId, stackId) {
    return run(['view', 'resolve', conflictId, '--pick', stackId])
  },

  resolveByMerge(conflictId, content) {
    const tmp = tmpWrite(content)
    return run(['view', 'resolve', conflictId, '--merge-file', tmp])
  },

  // ── Inspection ───────────────────────────────────────────────────────────

  log(stackId) {
    return run(['log', stackId])
  },

  diff(from, to) {
    return run(['diff', from, to])
  },
}

export { BIN, PROJECT_ROOT }
