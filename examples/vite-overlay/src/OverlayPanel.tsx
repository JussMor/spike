import { useEffect, useState } from 'react'

interface SessionData {
  sessionId: string
  overlayDir: string
  overlayFiles: string[]
}

/**
 * Shows the live state of this session's overlay via an SSE stream.
 *
 * The plugin exposes /__vcs_session/events — a server-sent event stream
 * that pushes the overlay file list once per second. No polling needed.
 */
export function OverlayPanel() {
  const [data, setData] = useState<SessionData | null>(null)

  useEffect(() => {
    const es = new EventSource('/__vcs_session/events')
    es.onmessage = (e) => {
      try {
        setData(JSON.parse(e.data))
      } catch {}
    }
    return () => es.close()
  }, [])

  if (!data) {
    return (
      <section className="overlay-panel" data-testid="overlay-panel">
        <h2>Overlay</h2>
        <p className="overlay-panel__loading">connecting…</p>
      </section>
    )
  }

  return (
    <section className="overlay-panel" data-testid="overlay-panel">
      <h2>
        Overlay{' '}
        <span className="overlay-panel__count" data-testid="overlay-count">
          {data.overlayFiles.length}
        </span>
      </h2>

      <div className="overlay-panel__meta">
        <code data-testid="overlay-dir">{data.overlayDir}</code>
      </div>

      {data.overlayFiles.length === 0 ? (
        <p className="overlay-panel__empty" data-testid="overlay-empty">
          No overlay files yet — this session sees the real source tree.
        </p>
      ) : (
        <ul className="overlay-panel__files" data-testid="overlay-files">
          {data.overlayFiles.map((f) => (
            <li key={f} className="overlay-panel__file" data-testid="overlay-file">
              <span className="overlay-panel__badge">overlay</span>
              <code>{f}</code>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
