/**
 * store.mjs — content-addressed store for agent overlay snapshots.
 *
 * Layout:
 *   <projectRoot>/.vcs-overlay/
 *     blobs/<first2>/<rest>   — SHA-256 content-addressed file blobs
 *     log.jsonl               — append-only commit records
 *
 * Each commit record:
 *   { id, agent, ts, base, intent, reason, files: { relPath: { hash, size } } }
 *
 * `id` = SHA256 of the rest of the record → immutable, verifiable.
 * `base` chains per-agent history.
 * Blob dedup: same content → same hash → write skipped.
 */

import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

// ── Public: paths ────────────────────────────────────────────────────────────

export function storeDir(projectRoot) {
  return path.join(projectRoot, '.vcs-overlay')
}

// ── Public: hashing ──────────────────────────────────────────────────────────

/** SHA-256 hex of a Buffer or string. */
export function sha256(content) {
  return crypto.createHash('sha256').update(content).digest('hex')
}

// ── Public: blob storage ─────────────────────────────────────────────────────

/** Hash content, persist blob if not already stored, return hex hash. */
export function storeBlob(projectRoot, content) {
  _init(projectRoot)
  const hash = sha256(content)
  const blobPath = _blobPath(projectRoot, hash)
  if (!fs.existsSync(blobPath)) {
    fs.mkdirSync(path.dirname(blobPath), { recursive: true })
    fs.writeFileSync(blobPath, content)
  }
  return hash
}

/** Return raw Buffer for a stored blob. */
export function readBlob(projectRoot, hash) {
  return fs.readFileSync(_blobPath(projectRoot, hash))
}

// ── Public: log ──────────────────────────────────────────────────────────────

/**
 * Read all commit records from log.jsonl.
 * @param {{ agent?: string }} filter
 * @returns {object[]}  ascending by ts
 */
export function readLog(projectRoot, filter = {}) {
  const lp = _logPath(projectRoot)
  if (!fs.existsSync(lp)) return []
  const records = fs.readFileSync(lp, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map(l => JSON.parse(l))
  return filter.agent ? records.filter(r => r.agent === filter.agent) : records
}

// ── Public: snapshot ─────────────────────────────────────────────────────────

/**
 * Hash all files in overlayDir, store blobs, append a commit record.
 * Returns the commit record.
 */
export function snapshot(agentId, overlayDir, projectRoot, intent = 'snapshot', reason = '') {
  _init(projectRoot)
  const files = {}
  for (const relPath of scanDir(overlayDir)) {
    const content = fs.readFileSync(path.join(overlayDir, relPath))
    const hash = storeBlob(projectRoot, content)
    files[relPath] = { hash, size: content.length }
  }
  const base = _lastId(projectRoot, agentId)
  const ts = new Date().toISOString()
  const body = { agent: agentId, ts, base, intent, reason, files }
  const id = sha256(JSON.stringify(body))
  const record = { id, ...body }
  fs.appendFileSync(_logPath(projectRoot), JSON.stringify(record) + '\n')
  return record
}

// ── Internal ─────────────────────────────────────────────────────────────────

function _blobPath(projectRoot, hash) {
  return path.join(storeDir(projectRoot), 'blobs', hash.slice(0, 2), hash.slice(2))
}

function _logPath(projectRoot) {
  return path.join(storeDir(projectRoot), 'log.jsonl')
}

function _init(projectRoot) {
  fs.mkdirSync(path.join(storeDir(projectRoot), 'blobs'), { recursive: true })
  const lp = _logPath(projectRoot)
  if (!fs.existsSync(lp)) fs.writeFileSync(lp, '')
}

function _lastId(projectRoot, agentId) {
  const log = readLog(projectRoot, { agent: agentId })
  return log.length > 0 ? log[log.length - 1].id : null
}

/** Recursively list all file relative paths in a directory. */
export function scanDir(dir, base = dir, out = []) {
  if (!fs.existsSync(dir)) return out
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) scanDir(full, base, out)
    else out.push(path.relative(base, full).replaceAll(path.sep, '/'))
  }
  return out
}
