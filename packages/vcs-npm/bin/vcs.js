#!/usr/bin/env node
/**
 * vcs — npm bin shim.
 *
 * Finds the real binary (set by postinstall) and execs it,
 * forwarding all args and stdio transparently.
 */

import { existsSync, readFileSync } from 'node:fs'
import { resolve, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const __dir = dirname(fileURLToPath(import.meta.url))
const PKG_ROOT = resolve(__dir, '..')

function findBin() {
  // 1. Env override
  if (process.env.VCS_BIN && existsSync(process.env.VCS_BIN)) {
    return process.env.VCS_BIN
  }
  // 2. Resolved by postinstall
  const resolvedFile = join(PKG_ROOT, 'bin', '.resolved-bin')
  if (existsSync(resolvedFile)) {
    const p = readFileSync(resolvedFile, 'utf8').trim()
    if (existsSync(p)) return p
  }
  // 3. Workspace sibling
  for (const rel of ['../../target/release/vcs', '../../../target/release/vcs']) {
    const p = resolve(PKG_ROOT, rel)
    if (existsSync(p)) return p
  }
  // 4. System PATH fallback
  return 'vcs'
}

const bin = findBin()
const result = spawnSync(bin, process.argv.slice(2), { stdio: 'inherit' })
process.exit(result.status ?? 1)
