/**
 * ConflictPanel — interactive conflict resolution (P2.2).
 *
 * Shows all conflicts in the active view with:
 *  - Pick A / Pick B buttons for each candidate
 *  - Custom merge textarea + submit
 *  - Live resolved / unresolved state
 *
 * All resolution actions call POST /api/vcs/conflicts/:id/resolve through
 * the useResolveConflict() mutation, which auto-invalidates the conflict list.
 */

import { useState } from 'react'
import { useActiveView, useVcsConflicts, useResolveConflict, type Conflict } from '../hooks/useVcs'

// ── Sub-component: single conflict card ───────────────────────────────────

function ConflictCard({ conflict }: { conflict: Conflict }) {
  const [showMerge, setShowMerge] = useState(false)
  const [mergeContent, setMergeContent] = useState('')
  const resolve = useResolveConflict()

  const isResolved = !!conflict.resolution
  const isPending = resolve.isPending

  function pick(stackId: string) {
    resolve.mutate({ conflictId: conflict.conflict_id, resolution: { type: 'pick', stack_id: stackId } })
  }

  function submitMerge() {
    if (!mergeContent.trim()) return
    resolve.mutate({
      conflictId: conflict.conflict_id,
      resolution: { type: 'merge', content: mergeContent },
    })
    setShowMerge(false)
    setMergeContent('')
  }

  return (
    <li
      className={`conflict-item ${isResolved ? 'conflict-resolved' : 'conflict-unresolved'}`}
      data-testid={isResolved ? 'conflict-resolved' : 'conflict-unresolved'}
    >
      {/* ── Header row ─────────────────────────────────────────────────── */}
      <div className="conflict-header">
        <span className="conflict-icon">{isResolved ? '✓' : '⚡'}</span>
        <span className="conflict-path" data-testid="conflict-path">
          {conflict.path}
        </span>
        <span className={`conflict-badge ${isResolved ? 'badge-resolved' : 'badge-conflict'}`}>
          {isResolved ? 'resolved' : 'unresolved'}
        </span>
      </div>

      {/* ── Candidates + Pick buttons ───────────────────────────────────── */}
      {!isResolved && (
        <ul className="conflict-candidates" data-testid="conflict-candidates">
          {conflict.candidates.map((cand, i) => (
            <li key={cand.stack_id} className="conflict-candidate" data-testid="conflict-candidate">
              <div className="candidate-info">
                <span className="candidate-label">
                  {String.fromCharCode(65 + i)} {/* A, B, C… */}
                </span>
                <code className="candidate-stack">stack {cand.stack_id.slice(0, 8)}</code>
                <code className="candidate-blob">
                  blob {(cand.blob_hash ?? 'deleted').slice(0, 8)}
                </code>
              </div>
              <button
                className="btn btn-pick"
                data-testid={`conflict-pick-${i}`}
                disabled={isPending}
                onClick={() => pick(cand.stack_id)}
              >
                {isPending ? '…' : `Pick ${String.fromCharCode(65 + i)}`}
              </button>
            </li>
          ))}
        </ul>
      )}

      {/* ── Custom merge ────────────────────────────────────────────────── */}
      {!isResolved && (
        <div className="conflict-merge-zone">
          {!showMerge ? (
            <button
              className="btn btn-merge-toggle"
              data-testid="conflict-merge-toggle"
              onClick={() => setShowMerge(true)}
            >
              ✏ Custom merge…
            </button>
          ) : (
            <div className="conflict-merge-editor" data-testid="conflict-merge-editor">
              <textarea
                className="merge-textarea"
                data-testid="conflict-merge-content"
                rows={8}
                placeholder="Paste or type the merged content here…"
                value={mergeContent}
                onChange={e => setMergeContent(e.target.value)}
              />
              <div className="merge-actions">
                <button
                  className="btn btn-merge-submit"
                  data-testid="conflict-merge-submit"
                  disabled={!mergeContent.trim() || isPending}
                  onClick={submitMerge}
                >
                  {isPending ? '…' : 'Apply merge'}
                </button>
                <button
                  className="btn btn-merge-cancel"
                  data-testid="conflict-merge-cancel"
                  onClick={() => { setShowMerge(false); setMergeContent('') }}
                >
                  Cancel
                </button>
              </div>
            </div>
          )}
        </div>
      )}

      {/* ── Error feedback ───────────────────────────────────────────────── */}
      {resolve.isError && (
        <p className="conflict-error" data-testid="conflict-error">
          ⚠ {(resolve.error as Error).message}
        </p>
      )}
    </li>
  )
}

// ── Main panel ─────────────────────────────────────────────────────────────

export function ConflictPanel() {
  const { data: activeView } = useActiveView()
  const { data: conflicts, isLoading, error } = useVcsConflicts(activeView?.view_id ?? null)

  if (isLoading) return <p className="loading">Loading…</p>
  if (error) return <p className="error">⚠ {(error as Error).message}</p>
  if (!activeView) return <p className="empty">No active view.</p>

  if (!conflicts?.length) {
    return (
      <p className="no-conflicts" data-testid="no-conflicts">
        <span>✓</span> No conflicts — all stacks merge cleanly.
      </p>
    )
  }

  const unresolved = conflicts.filter(c => !c.resolution)
  const resolved   = conflicts.filter(c =>  c.resolution)

  return (
    <div data-testid="conflict-panel">
      {unresolved.length > 0 && (
        <p className="conflict-summary conflict-summary-warn" data-testid="conflict-summary-unresolved">
          ⚡ {unresolved.length} unresolved conflict{unresolved.length > 1 ? 's' : ''} — pick a winner or provide a custom merge
        </p>
      )}
      {resolved.length > 0 && unresolved.length === 0 && (
        <p className="conflict-summary conflict-summary-ok" data-testid="conflict-summary-resolved">
          ✓ All {resolved.length} conflict{resolved.length > 1 ? 's' : ''} resolved
        </p>
      )}

      <ul className="conflict-list" data-testid="conflict-list">
        {/* Unresolved first */}
        {unresolved.map(c => <ConflictCard key={c.conflict_id} conflict={c} />)}
        {resolved.map(c => <ConflictCard key={c.conflict_id} conflict={c} />)}
      </ul>
    </div>
  )
}
