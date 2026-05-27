/**
 * remote-client.js — HTTP client for a remote `vcs serve` hub.
 *
 * Drop-in replacement for the local CLI client when you want to connect
 * multiple projects to a shared hub instead of each having its own .vcs/.
 *
 * Usage:
 *   import { VcsRemoteClient } from 'vcs-spike/remote'
 *
 *   const vcs = new VcsRemoteClient('http://hub.internal:7474')
 *
 *   const stackId = await vcs.stackOpen('agent-frontend')
 *   await vcs.edit(stackId, 'src/api-client.ts', content, { reason: 'add login endpoint' })
 *   await vcs.stackClose(stackId)
 *
 *   // Build a cross-project view with stacks from BOTH projects
 *   const viewId = await vcs.viewOpen('', [frontendStackId, backendStackId])
 *   const conflicts = await vcs.viewConflicts(viewId)
 *
 * The API shape is identical to the local vcs client (client.js),
 * so you can swap them with a single import change.
 */

export class VcsRemoteClient {
  /**
   * @param {string} hubUrl  Base URL of the hub server, e.g. 'http://localhost:7474'
   */
  constructor(hubUrl = 'http://localhost:7474') {
    this.hubUrl = hubUrl.replace(/\/$/, '')
  }

  // ── Internal ─────────────────────────────────────────────────────────────

  async _get(path) {
    const res = await fetch(`${this.hubUrl}${path}`)
    if (!res.ok) {
      const body = await res.text()
      throw new Error(`GET ${path} → ${res.status}: ${body}`)
    }
    return res.json()
  }

  async _post(path, body) {
    const res = await fetch(`${this.hubUrl}${path}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!res.ok) {
      const text = await res.text()
      throw new Error(`POST ${path} → ${res.status}: ${text}`)
    }
    return res.json()
  }

  _toB64(content) {
    if (typeof content === 'string') {
      return Buffer.from(content, 'utf8').toString('base64')
    }
    return Buffer.from(content).toString('base64')
  }

  // ── Store ─────────────────────────────────────────────────────────────────

  async status() {
    return this._get('/api/vcs/status')
  }

  // ── Stacks ────────────────────────────────────────────────────────────────

  async stackOpen(agentId, baseChangeId) {
    const r = await this._post('/api/vcs/stacks/open', {
      agent_id:       agentId,
      base_change_id: baseChangeId ?? null,
    })
    return r.stack_id
  }

  async stackClose(stackId) {
    return this._post(`/api/vcs/stacks/${stackId}/close`, {})
  }

  async stackAbandon(stackId) {
    return this._post(`/api/vcs/stacks/${stackId}/abandon`, {})
  }

  async listStacks() {
    return this._get('/api/vcs/stacks')
  }

  // ── Edits ─────────────────────────────────────────────────────────────────

  /**
   * Record a file edit on the hub.
   * @param {string} stackId
   * @param {string} path
   * @param {string|Buffer} content
   * @param {{ reason: string, task_ref?: string }} intent
   */
  async edit(stackId, path, content, { reason, task_ref } = {}) {
    if (!reason) throw new Error('intent.reason is required')
    const r = await this._post('/api/vcs/edit', {
      stack_id:    stackId,
      path,
      content_b64: this._toB64(content),
      intent:      { reason, task_ref: task_ref ?? null },
    })
    return r.change_id
  }

  async delete(stackId, path, { reason, task_ref } = {}) {
    if (!reason) throw new Error('intent.reason is required')
    const r = await this._post('/api/vcs/delete', {
      stack_id: stackId,
      path,
      intent:   { reason, task_ref: task_ref ?? null },
    })
    return r.change_id
  }

  // ── Views ─────────────────────────────────────────────────────────────────

  async viewOpen(baseChangeId, stackIds) {
    const r = await this._post('/api/vcs/views/open', {
      base_change_id: baseChangeId,
      stack_ids:      stackIds,
    })
    return r.view_id
  }

  async viewFiles(viewId) {
    return this._get(`/api/vcs/view/${viewId}/files`)
  }

  async viewConflicts(viewId) {
    return this._get(`/api/vcs/view/${viewId}/conflicts`)
  }

  async listViews() {
    return this._get('/api/vcs/views')
  }

  async activeView() {
    return this._get('/api/vcs/active-view')
  }

  // ── Conflicts ─────────────────────────────────────────────────────────────

  async resolveByPick(conflictId, stackId) {
    return this._post(`/api/vcs/conflicts/${conflictId}/resolve`, {
      pick: stackId,
    })
  }

  async resolveByMerge(conflictId, content) {
    return this._post(`/api/vcs/conflicts/${conflictId}/resolve`, {
      merge_content_b64: this._toB64(content),
    })
  }

  // ── Push (inter-project) ──────────────────────────────────────────────────

  /**
   * Push a local project's stacks+changes+blobs to this hub.
   *
   * The bundle is collected from a local `vcs` client (CLI wrapper)
   * and sent over HTTP so the hub can build a cross-project view.
   *
   * @param {import('./hub-client.js').HubBundle} bundle
   */
  async push(bundle) {
    return this._post('/api/vcs/push', bundle)
  }
}
