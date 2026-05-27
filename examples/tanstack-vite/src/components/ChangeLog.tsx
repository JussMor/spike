import { useVcsAllChanges } from '../hooks/useVcs'

function opClass(op: string) {
  return `change-op op-${op}`
}

function formatTs(ms: number) {
  return new Date(ms).toLocaleTimeString()
}

export function ChangeLog() {
  const { data: changes, isLoading, error } = useVcsAllChanges()

  if (isLoading) return <p className="loading">Loading…</p>
  if (error) return <p className="error">⚠ {(error as Error).message}</p>
  if (!changes?.length) return (
    <p className="empty">
      No changes recorded yet.<br />
      Run <code>npm run vcs:demo</code> to track some.
    </p>
  )

  return (
    <ul className="change-list">
      {[...changes].reverse().map(c => (
        <li key={c.change_id} className="change-item">
          <span className={opClass(c.op)}>{c.op}</span>
          <span className="change-path">{c.path}</span>
          <span className="change-reason">{c.intent.reason}</span>
          <span className="change-meta">
            {c.change_id.slice(0, 10)} · {c.agent_id} · {formatTs(c.created_at)}
            {c.intent.task_ref ? ` · task:${c.intent.task_ref}` : ''}
          </span>
        </li>
      ))}
    </ul>
  )
}
