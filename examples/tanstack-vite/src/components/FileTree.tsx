import { useActiveView, useVcsFiles } from '../hooks/useVcs'

function fileIcon(path: string) {
  if (path.endsWith('.tsx') || path.endsWith('.ts')) return '📘'
  if (path.endsWith('.css')) return '🎨'
  if (path.endsWith('.json')) return '📋'
  if (path.endsWith('.md')) return '📝'
  if (path.endsWith('.js') || path.endsWith('.mjs')) return '📜'
  if (path.endsWith('.html')) return '🌐'
  if (path.includes('/')) return '📁'
  return '📄'
}

export function FileTree() {
  const { data: activeView, isLoading: viewLoading } = useActiveView()
  const { data: files, isLoading, error } = useVcsFiles(activeView?.view_id ?? null)

  if (viewLoading || isLoading) return <p className="loading">Loading…</p>
  if (error) return <p className="error">⚠ {(error as Error).message}</p>
  if (!activeView) return <p className="empty">No active view. Run <code>npm run vcs:demo</code></p>
  if (!files?.length) return <p className="empty">No files tracked yet.</p>

  return (
    <ul className="file-list">
      {files.map(f => (
        <li key={f} className="file-item">
          <span className="file-icon">{fileIcon(f)}</span>
          {f}
        </li>
      ))}
    </ul>
  )
}
