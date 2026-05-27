import { useActiveView, useVcsConflicts } from '../hooks/useVcs'

export function ConflictPanel() {
  const { data: activeView } = useActiveView()
  const { data: conflicts, isLoading, error } = useVcsConflicts(activeView?.view_id ?? null)

  if (isLoading) return <p className="loading">Loading…</p>
  if (error) return <p className="error">⚠ {(error as Error).message}</p>
  if (!activeView) return <p className="empty">No active view.</p>

  if (!conflicts?.length) {
    return (
      <p className="no-conflicts">
        <span>✓</span> No conflicts — all stacks merge cleanly.
      </p>
    )
  }

  return (
    <ul className="conflict-list">
      {conflicts.map(c => (
        <li
          key={c.conflict_id}
          className={`conflict-item ${c.resolution ? 'conflict-resolved' : ''}`}
        >
          <div className="conflict-path">
            {c.resolution ? '✓ ' : '⚡ '}
            {c.path}
            {c.resolution ? ' (resolved)' : ' (unresolved)'}
          </div>
          <ul className="conflict-candidates">
            {c.candidates.map((cand, i) => (
              <li key={i} className="conflict-candidate">
                stack {cand.stack_id.slice(0, 8)} · blob {(cand.blob_hash ?? 'deleted').slice(0, 8)}
              </li>
            ))}
          </ul>
        </li>
      ))}
    </ul>
  )
}
