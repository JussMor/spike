#!/usr/bin/env node
/**
 * postinstall.js — finds or builds the vcs binary after `npm install`.
 *
 * Resolution order:
 *   1. VCS_BIN env var                       (explicit override)
 *   2. Sibling workspace binary              (monorepo / local dev)
 *   3. System PATH                           (already installed)
 *   4. Build from source via cargo           (Rust toolchain required)
 */

import { existsSync } from 'node:fs'
import { execSync, spawnSync } from 'node:child_process'
import { resolve, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'
import { writeFileSync, chmodSync } from 'node:fs'

const __dir = dirname(fileURLToPath(import.meta.url))
const PKG_ROOT = resolve(__dir, '..')

// ── 1. Env override ───────────────────────────────────────────────────────
if (process.env.VCS_BIN) {
  if (existsSync(process.env.VCS_BIN)) {
    console.log(`[vcs-spike] Using VCS_BIN=${process.env.VCS_BIN}`)
    writeBinShim(process.env.VCS_BIN)
    process.exit(0)
  }
}

// ── 2. Workspace sibling ──────────────────────────────────────────────────
const workspacePaths = [
  resolve(PKG_ROOT, '../../target/release/vcs'),
  resolve(PKG_ROOT, '../../../target/release/vcs'),
]
for (const p of workspacePaths) {
  if (existsSync(p)) {
    console.log(`[vcs-spike] Found workspace binary at ${p}`)
    writeBinShim(p)
    process.exit(0)
  }
}

// ── 3. System PATH ────────────────────────────────────────────────────────
const which = spawnSync('which', ['vcs'], { encoding: 'utf8' })
if (which.status === 0 && which.stdout.trim()) {
  const systemBin = which.stdout.trim()
  // Verify it's our vcs (not some other tool named vcs)
  const ver = spawnSync(systemBin, ['--version'], { encoding: 'utf8' })
  if (ver.status === 0 && ver.stdout.includes('vcs')) {
    console.log(`[vcs-spike] Found system vcs at ${systemBin}`)
    writeBinShim(systemBin)
    process.exit(0)
  }
}

// ── 4. Build from source ──────────────────────────────────────────────────
if (!hasCargo()) {
  console.error('[vcs-spike] ⚠ cargo not found. Options:')
  console.error('  • Install Rust:  https://rustup.rs')
  console.error('  • Set VCS_BIN=/path/to/vcs before npm install')
  console.error('  • Run install.sh:  curl -fsSL https://raw.githubusercontent.com/JussMor/spike/main/install.sh | sh')
  process.exit(1)
}

// Find the repo root (go up from node_modules/vcs-spike or packages/vcs-npm)
const repoRoot = findRepoRoot(__dir)
if (!repoRoot) {
  console.error('[vcs-spike] Could not find vcs-spike Cargo.toml to build from source')
  process.exit(1)
}

console.log('[vcs-spike] Building from source (cargo build --release)…')
console.log('[vcs-spike] This takes ~30s on first build, then is cached.')
try {
  execSync('cargo build --release -p vcs-cli', {
    cwd: repoRoot,
    stdio: 'inherit',
  })
} catch (e) {
  console.error('[vcs-spike] Build failed:', e.message)
  process.exit(1)
}

const built = join(repoRoot, 'target/release/vcs')
if (!existsSync(built)) {
  console.error('[vcs-spike] Build succeeded but binary not found at', built)
  process.exit(1)
}

writeBinShim(built)
console.log('[vcs-spike] ✓ vcs ready!')

// ── Helpers ───────────────────────────────────────────────────────────────

function writeBinShim(binaryPath) {
  const shim = join(PKG_ROOT, 'bin', '.resolved-bin')
  writeFileSync(shim, binaryPath, 'utf8')
}

function hasCargo() {
  return spawnSync('cargo', ['--version'], { encoding: 'utf8' }).status === 0
}

function findRepoRoot(startDir) {
  let dir = startDir
  for (let i = 0; i < 10; i++) {
    if (existsSync(join(dir, 'Cargo.toml'))) {
      const content = require('node:fs').readFileSync(join(dir, 'Cargo.toml'), 'utf8')
      if (content.includes('vcs-cli') || content.includes('vcs-core')) return dir
    }
    const parent = resolve(dir, '..')
    if (parent === dir) break
    dir = parent
  }
  return null
}
