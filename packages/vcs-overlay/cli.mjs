#!/usr/bin/env node
/**
 * cli.mjs — vcs-overlay CLI.
 *
 * Run from any project root:
 *   vcs-overlay <command> [args] [--json]
 *
 * Commands:
 *   status                        list live agents, file counts, collisions
 *   snapshot <agent> [--reason]   hash overlay → store blobs → append commit
 *   log [agent]                   commit history
 *   diff <agent> [<commit-id>]    unified diff vs project root
 *   promote <agent>               snapshot + copy overlay onto project root
 *   discard <agent>               clear overlay dir (no snapshot)
 *   checkout <agent> <commit-id>  restore overlay dir from a past commit
 *   watch                         watch all agents, auto-snapshot on change
 *
 * Pass --json for machine-readable output on any command.
 */

import { snapshot, readLog, readBlob, sha256, scanDir } from './store.mjs'
import fs from 'node:fs'
import path from 'node:path'
import os from 'node:os'

// PROJECT_ROOT = the directory where you run the CLI (your project root).
const PROJECT_ROOT = process.cwd()
const SESSIONS_DIR = process.env.VCS_SESSIONS_DIR ?? path.join(os.tmpdir(), 'vcs-sessions')

// ── ANSI ─────────────────────────────────────────────────────────────────────

const TTY = process.stdout.isTTY
const G = TTY ? '\x1b[32m' : ''   // green
const R = TTY ? '\x1b[31m' : ''   // red
const Y = TTY ? '\x1b[33m' : ''   // yellow
const B = TTY ? '\x1b[1m' : ''    // bold
const D = TTY ? '\x1b[2m' : ''    // dim
const X = TTY ? '\x1b[0m' : ''    // reset

// ── Arg parsing ───────────────────────────────────────────────────────────────

const argv = process.argv.slice(2)
const jsonMode = argv.includes('--json')
const reasonIdx = argv.indexOf('--reason')
const reason = reasonIdx >= 0 ? argv[reasonIdx + 1] ?? '' : ''
const positional = argv.filter((a, i) => !a.startsWith('-') && argv[i - 1] !== '--reason')
const [cmd, arg1, arg2] = positional

// ── Helpers ───────────────────────────────────────────────────────────────────

function overlayDir(agentId) {
  return path.join(SESSIONS_DIR, agentId)
}

function listAgents() {
  if (!fs.existsSync(SESSIONS_DIR)) return []
  return fs.readdirSync(SESSIONS_DIR, { withFileTypes: true })
    .filter(e => e.isDirectory())
    .map(e => e.name)
    .sort()
}

function agentFiles(agentId) {
  const dir = overlayDir(agentId)
  const map = {}
  for (const relPath of scanDir(dir)) {
    const content = fs.readFileSync(path.join(dir, relPath))
    map[relPath] = sha256(content)
  }
  return map
}

function readSourceFile(relPath) {
  const p = path.join(PROJECT_ROOT, relPath)
  return fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : ''
}

// ── Myers LCS diff ────────────────────────────────────────────────────────────

/** Compute LCS-based line diff. Returns null if inputs are too large. */
function lcsDiff(a, b) {
  const m = a.length, n = b.length
  if (m > 1500 || n > 1500) return null
  const dp = Array.from({ length: m + 1 }, () => new Int32Array(n + 1))
  for (let i = 1; i <= m; i++)
    for (let j = 1; j <= n; j++)
      dp[i][j] = a[i - 1] === b[j - 1]
        ? dp[i - 1][j - 1] + 1
        : Math.max(dp[i - 1][j], dp[i][j - 1])

  const ops = []
  let i = m, j = n
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && a[i - 1] === b[j - 1]) {
      ops.unshift({ t: '=', v: a[i - 1] }); i--; j--
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      ops.unshift({ t: '+', v: b[j - 1] }); j--
    } else {
      ops.unshift({ t: '-', v: a[i - 1] }); i--
    }
  }
  return ops
}

/** Format a unified diff string between oldText and newText. */
function unifiedDiff(oldText, newText, aLabel, bLabel) {
  const a = splitLines(oldText)
  const b = splitLines(newText)
  const ops = lcsDiff(a, b)
  if (!ops) return `--- a/${aLabel}\n+++ b/${bLabel}\n(file too large for inline diff)\n`
  if (ops.every(o => o.t === '=')) return ''

  // Annotate with line numbers
  const ann = []
  let ai = 1, bi = 1
  for (const o of ops) {
    if (o.t === '=') ann.push({ ...o, ai: ai++, bi: bi++ })
    else if (o.t === '-') ann.push({ ...o, ai: ai++, bi: null })
    else ann.push({ ...o, ai: null, bi: bi++ })
  }

  const CONTEXT = 3
  const changeIdxs = ann.map((e, i) => e.t !== '=' ? i : -1).filter(i => i >= 0)
  if (!changeIdxs.length) return ''

  // Group into hunks
  const hunks = []
  let start = 0
  while (start < changeIdxs.length) {
    const lo = Math.max(0, changeIdxs[start] - CONTEXT)
    let end = start
    while (end + 1 < changeIdxs.length && changeIdxs[end + 1] <= changeIdxs[end] + 2 * CONTEXT + 1) end++
    const hi = Math.min(ann.length - 1, changeIdxs[end] + CONTEXT)
    hunks.push(ann.slice(lo, hi + 1))
    start = end + 1
  }

  const out = [`${B}--- a/${aLabel}${X}`, `${B}+++ b/${bLabel}${X}`]
  for (const hunk of hunks) {
    const oldStart = hunk.find(e => e.ai !== null)?.ai ?? 1
    const newStart = hunk.find(e => e.bi !== null)?.bi ?? 1
    const oldCount = hunk.filter(e => e.t !== '+').length
    const newCount = hunk.filter(e => e.t !== '-').length
    out.push(`${D}@@ -${oldStart},${oldCount} +${newStart},${newCount} @@${X}`)
    for (const e of hunk) {
      if (e.t === '=') out.push(` ${e.v}`)
      else if (e.t === '-') out.push(`${R}-${e.v}${X}`)
      else out.push(`${G}+${e.v}${X}`)
    }
  }
  return out.join('\n')
}

function splitLines(text) {
  if (!text) return []
  const lines = text.split('\n')
  if (lines[lines.length - 1] === '') lines.pop()
  return lines
}

// ── Commands ──────────────────────────────────────────────────────────────────

function cmdStatus() {
  const agents = listAgents()

  if (!agents.length) {
    if (jsonMode) return console.log(JSON.stringify({ agents: [], collisions: [] }))
    return console.log('No active agent sessions.')
  }

  const data = agents.map(id => ({ id, files: agentFiles(id) }))

  // Collision detection: same relPath, different hash across agents
  const pathMap = new Map()
  for (const { id, files } of data) {
    for (const [relPath, hash] of Object.entries(files)) {
      if (!pathMap.has(relPath)) pathMap.set(relPath, [])
      pathMap.get(relPath).push({ id, hash })
    }
  }
  const collisions = []
  for (const [relPath, entries] of pathMap) {
    const hashes = new Set(entries.map(e => e.hash))
    if (hashes.size > 1) collisions.push({ path: relPath, agents: entries.map(e => e.id) })
  }

  if (jsonMode) {
    return console.log(JSON.stringify({
      agents: data.map(({ id, files }) => ({
        agentId: id,
        overlayDir: overlayDir(id),
        fileCount: Object.keys(files).length,
        files: Object.keys(files),
      })),
      collisions,
    }, null, 2))
  }

  const collisionPaths = new Set(collisions.map(c => c.path))
  for (const { id, files } of data) {
    const count = Object.keys(files).length
    const badge = count > 0 ? `${Y}${count} file${count === 1 ? '' : 's'}${X}` : `${D}empty${X}`
    console.log(`${B}${id}${X}  ${badge}`)
    for (const relPath of Object.keys(files)) {
      const col = collisionPaths.has(relPath) ? `  ${R}⚡ collision${X}` : ''
      console.log(`  ${D}${relPath}${X}${col}`)
    }
  }

  if (collisions.length) {
    console.log(`\n${R}${B}Collisions (${collisions.length}):${X}`)
    for (const { path: p, agents } of collisions) {
      console.log(`  ${p}`)
      for (const a of agents) console.log(`    ${D}← ${a}${X}`)
    }
  }
}

function cmdSnapshot(agentId) {
  if (!agentId) return die('Usage: snapshot <agent> [--reason <text>]')
  const oDir = overlayDir(agentId)
  if (!fs.existsSync(oDir)) return die(`No overlay dir for agent: ${agentId}`)
  const commit = snapshot(agentId, oDir, PROJECT_ROOT, 'snapshot', reason)
  if (jsonMode) return console.log(JSON.stringify(commit, null, 2))
  const count = Object.keys(commit.files).length
  console.log(`${G}✓${X} snapshot ${B}${commit.id.slice(0, 12)}${X}  ${Y}${count} file${count === 1 ? '' : 's'}${X}  ${agentId}`)
}

function cmdLog(agentId) {
  const records = readLog(PROJECT_ROOT, agentId ? { agent: agentId } : {})
  if (jsonMode) return console.log(JSON.stringify(records, null, 2))
  if (!records.length) { console.log('No commits yet.'); return }
  for (const r of [...records].reverse()) {
    const count = Object.keys(r.files).length
    const ts = new Date(r.ts).toLocaleString()
    const intent = r.intent === 'promote' ? `${G}promote${X}` : r.intent === 'checkout' ? `${Y}checkout${X}` : `${D}snapshot${X}`
    const reasonStr = r.reason ? `  ${D}"${r.reason}"${X}` : ''
    console.log(`${B}${r.id.slice(0, 12)}${X}  ${ts}  ${B}${r.agent}${X}  ${intent}  ${Y}${count}f${X}${reasonStr}`)
  }
}

function cmdDiff(agentId, commitId) {
  if (!agentId) return die('Usage: diff <agent> [<commit-id>]')

  let filesMap  // relPath → content string

  if (commitId) {
    const record = readLog(PROJECT_ROOT).find(r => r.id === commitId || r.id.startsWith(commitId))
    if (!record) return die(`Commit not found: ${commitId}`)
    filesMap = {}
    for (const [relPath, { hash }] of Object.entries(record.files))
      filesMap[relPath] = readBlob(PROJECT_ROOT, hash).toString('utf8')
  } else {
    const oDir = overlayDir(agentId)
    if (!fs.existsSync(oDir)) return die(`No overlay dir for agent: ${agentId}`)
    filesMap = {}
    for (const relPath of scanDir(oDir))
      filesMap[relPath] = fs.readFileSync(path.join(oDir, relPath), 'utf8')
  }

  if (!Object.keys(filesMap).length) {
    if (jsonMode) return console.log(JSON.stringify({ agent: agentId, diffs: [] }))
    return console.log('Overlay is empty.')
  }

  if (jsonMode) {
    const diffs = []
    for (const [relPath, newContent] of Object.entries(filesMap))
      diffs.push({ path: relPath, changed: readSourceFile(relPath) !== newContent })
    return console.log(JSON.stringify({ agent: agentId, diffs }, null, 2))
  }

  let anyDiff = false
  for (const [relPath, newContent] of Object.entries(filesMap)) {
    const patch = unifiedDiff(readSourceFile(relPath), newContent, relPath, relPath)
    if (patch) { console.log(patch); anyDiff = true }
  }
  if (!anyDiff) console.log('No differences.')
}

function cmdPromote(agentId) {
  if (!agentId) return die('Usage: promote <agent>')
  const oDir = overlayDir(agentId)
  if (!fs.existsSync(oDir)) return die(`No overlay dir for agent: ${agentId}`)
  const files = scanDir(oDir)
  if (!files.length) {
    if (!jsonMode) console.log('Overlay is empty — nothing to promote.')
    return
  }
  const commit = snapshot(agentId, oDir, PROJECT_ROOT, 'promote', reason)
  for (const relPath of files) {
    const dest = path.join(PROJECT_ROOT, relPath)
    fs.mkdirSync(path.dirname(dest), { recursive: true })
    fs.copyFileSync(path.join(oDir, relPath), dest)
  }
  if (jsonMode) return console.log(JSON.stringify(commit, null, 2))
  const count = files.length
  console.log(`${G}✓${X} promoted ${B}${commit.id.slice(0, 12)}${X}  ${Y}${count} file${count === 1 ? '' : 's'}${X} → project root`)
  console.log(`  ${D}overlay dir unchanged — agent can keep working${X}`)
}

function cmdDiscard(agentId) {
  if (!agentId) return die('Usage: discard <agent>')
  const oDir = overlayDir(agentId)
  if (!fs.existsSync(oDir)) return die(`No overlay dir: ${agentId}`)
  const files = scanDir(oDir)
  for (const relPath of files) fs.rmSync(path.join(oDir, relPath))
  if (jsonMode) return console.log(JSON.stringify({ agent: agentId, discarded: files }))
  console.log(`${Y}✓${X} discarded ${files.length} file${files.length === 1 ? '' : 's'} for ${agentId}`)
}

function cmdCheckout(agentId, commitId) {
  if (!agentId || !commitId) return die('Usage: checkout <agent> <commit-id>')
  const record = readLog(PROJECT_ROOT).find(r => r.id === commitId || r.id.startsWith(commitId))
  if (!record) return die(`Commit not found: ${commitId}`)
  const oDir = overlayDir(agentId)
  for (const [relPath, { hash }] of Object.entries(record.files)) {
    const dest = path.join(oDir, relPath)
    fs.mkdirSync(path.dirname(dest), { recursive: true })
    fs.writeFileSync(dest, readBlob(PROJECT_ROOT, hash))
  }
  const commit = snapshot(agentId, oDir, PROJECT_ROOT, 'checkout', `from ${record.id.slice(0, 12)}`)
  if (jsonMode) return console.log(JSON.stringify(commit, null, 2))
  const count = Object.keys(record.files).length
  console.log(`${G}✓${X} checked out ${B}${record.id.slice(0, 12)}${X} → ${agentId}  ${Y}${count} file${count === 1 ? '' : 's'}${X}`)
  console.log(`  ${D}Vite HMR will fire automatically${X}`)
}

async function cmdWatch() {
  if (!jsonMode) console.log(`Watching ${SESSIONS_DIR} for overlay changes…  (Ctrl-C to stop)\n`)
  if (!fs.existsSync(SESSIONS_DIR)) fs.mkdirSync(SESSIONS_DIR, { recursive: true })

  const debounce = new Map()

  function onChange(agentId) {
    clearTimeout(debounce.get(agentId))
    debounce.set(agentId, setTimeout(() => {
      const oDir = overlayDir(agentId)
      if (!fs.existsSync(oDir)) return
      const files = scanDir(oDir)
      if (!files.length) return
      const commit = snapshot(agentId, oDir, PROJECT_ROOT, 'snapshot', 'auto-watch')
      if (jsonMode) {
        console.log(JSON.stringify({ event: 'snapshot', commit }))
      } else {
        const count = Object.keys(commit.files).length
        console.log(`${G}↑${X} ${B}${agentId}${X}  ${commit.id.slice(0, 12)}  ${Y}${count}f${X}`)
      }
    }, 300))
  }

  fs.watch(SESSIONS_DIR, { recursive: true }, (_event, filename) => {
    if (!filename) return
    const agentId = filename.split(path.sep)[0]
    if (agentId) onChange(agentId)
  })

  await new Promise(() => {})
}

// ── Error / dispatch ──────────────────────────────────────────────────────────

function die(msg) {
  console.error(`${R}error:${X} ${msg}`)
  process.exit(1)
}

const USAGE = `
Usage: vcs-overlay <command> [args] [--json]

  status                        list live agents, file counts, collisions
  snapshot <agent>              snapshot overlay to store
  log [agent]                   show commit history
  diff <agent> [<commit-id>]    unified diff vs project root
  promote <agent>               promote overlay → project root
  discard <agent>               clear overlay dir
  checkout <agent> <commit>     restore overlay from a past commit
  watch                         auto-snapshot on overlay changes

Run from your project root. Store lives at <cwd>/.vcs-overlay/
`.trim()

switch (cmd) {
  case 'status':   cmdStatus(); break
  case 'snapshot': cmdSnapshot(arg1); break
  case 'log':      cmdLog(arg1); break
  case 'diff':     cmdDiff(arg1, arg2); break
  case 'promote':  cmdPromote(arg1); break
  case 'discard':  cmdDiscard(arg1); break
  case 'checkout': cmdCheckout(arg1, arg2); break
  case 'watch':    await cmdWatch(); break
  default:
    console.log(USAGE)
    if (cmd) process.exit(1)
}
