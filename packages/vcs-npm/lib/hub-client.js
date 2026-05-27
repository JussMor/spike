/**
 * hub-client.js — bundles a local .vcs/ store for pushing to a remote hub.
 *
 * Usage:
 *   import { buildBundle } from 'vcs-spike/hub'
 *   import { VcsRemoteClient } from 'vcs-spike/remote'
 *
 *   // From project A (frontend):
 *   const bundle = await buildBundle({
 *     projectId:  'frontend',
 *     storePath:  '/path/to/frontend/.vcs',
 *     stackIds:   [stackA1, stackA2],  // stacks to push
 *     vcs,                             // local CLI client (from client.js)
 *   })
 *   const hub = new VcsRemoteClient('http://hub:7474')
 *   await hub.push(bundle)
 *
 *   // From project B (backend):
 *   const bundleB = await buildBundle({ projectId: 'backend', stackIds: [...], vcs })
 *   await hub.push(bundleB)
 *
 *   // Build cross-project view on the hub:
 *   const allStackIds = [...bundle.stacks.map(s => s.stack_id),
 *                        ...bundleB.stacks.map(s => s.stack_id)]
 *   const viewId = await hub.viewOpen('', allStackIds)
 *   const conflicts = await hub.viewConflicts(viewId)
 */

import { readFileSync, readdirSync, existsSync } from 'node:fs'
import { join, resolve } from 'node:path'

/**
 * @typedef {Object} HubBundle
 * @property {string}   project_id
 * @property {Object[]} stacks
 * @property {Object[]} changes
 * @property {Object}   blobs  — { hash: base64string }
 */

/**
 * Build a HubBundle from a local vcs store by reading SQLite directly
 * and bundling the referenced blobs.
 *
 * @param {Object} opts
 * @param {string}   opts.projectId   Human-readable project name
 * @param {string}   opts.storePath   Path to the .vcs/ directory
 * @param {string[]} opts.stackIds    Which stacks to include
 * @returns {HubBundle}
 */
export function buildBundle({ projectId, storePath, stackIds }) {
  // Read the SQLite database directly using better-sqlite3
  let Database
  try {
    Database = (await import('better-sqlite3')).default
  } catch {
    throw new Error(
      'better-sqlite3 is required for buildBundle. Install it: npm install better-sqlite3'
    )
  }

  const dbPath = join(storePath, 'vcs.db')
  if (!existsSync(dbPath)) {
    throw new Error(`vcs store not found at ${dbPath}. Run: vcs init`)
  }

  const db = new Database(dbPath, { readonly: true })

  // ── Collect stacks ───────────────────────────────────────────────────────
  const placeholders = stackIds.map(() => '?').join(',')
  const stacks = db.prepare(
    `SELECT stack_id, agent_id, base_change_id, tip_change_id, status
     FROM stacks WHERE stack_id IN (${placeholders})`
  ).all(...stackIds)

  // ── Collect changes for those stacks ─────────────────────────────────────
  // Walk each stack's change chain tip→base
  const changeIds = new Set()
  for (const stack of stacks) {
    let current = stack.tip_change_id
    while (current && current !== stack.base_change_id) {
      if (changeIds.has(current)) break
      changeIds.add(current)
      const change = db.prepare('SELECT parent_id FROM changes WHERE change_id=?').get(current)
      current = change?.parent_id ?? null
    }
  }

  const changesData = changeIds.size > 0
    ? db.prepare(
        `SELECT change_id, parent_id, path, op, diff_hash, agent_id, intent, created_at
         FROM changes WHERE change_id IN (${[...changeIds].map(() => '?').join(',')})`
      ).all(...changeIds)
    : []

  const changes = changesData.map(r => {
    const intent = JSON.parse(r.intent)
    return {
      change_id:  r.change_id,
      parent_id:  r.parent_id,
      path:       r.path,
      op:         r.op,
      diff_hash:  r.diff_hash,
      agent_id:   r.agent_id,
      reason:     intent.reason,
      task_ref:   intent.task_ref ?? null,
      created_at: r.created_at,
    }
  })

  // ── Collect blob hashes referenced by the changes ─────────────────────────
  const blobHashes = new Set()
  for (const cid of changeIds) {
    const rows = db.prepare(
      'SELECT blob_hash FROM files_at_change WHERE change_id=? AND blob_hash IS NOT NULL'
    ).all(cid)
    for (const r of rows) blobHashes.add(r.blob_hash)
  }

  db.close()

  // ── Read blob bytes from the content-addressed store ──────────────────────
  const blobs = {}
  const blobsDir = join(storePath, 'blobs')
  for (const hash of blobHashes) {
    const blobPath = join(blobsDir, hash.slice(0, 2), hash.slice(2))
    if (existsSync(blobPath)) {
      blobs[hash] = readFileSync(blobPath).toString('base64')
    }
  }

  return {
    project_id: projectId,
    stacks,
    changes,
    blobs,
  }
}
