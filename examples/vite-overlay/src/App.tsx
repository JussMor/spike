import { useEffect, useState } from 'react'
import { OverlayPanel } from './OverlayPanel'
import './App.css'

/**
 * Root component — served from disk normally.
 *
 * When an agent writes a modified version to their overlay dir, only THAT
 * agent's browser gets the HMR update. All other sessions stay on this file.
 */
function App() {
  return (
    <div className="app" data-testid="app-root">
      <Header />
      <main className="app-main">
        <OverlayPanel />
        <Instructions />
      </main>
    </div>
  )
}

function Header() {
  const [sessionId, setSessionId] = useState<string | null>(null)

  useEffect(() => {
    fetch('/__vcs_session')
      .then((r) => r.json())
      .then((d) => setSessionId(d.sessionId))
      .catch(() => setSessionId('default'))
  }, [])

  return (
    <header className="app-header" data-testid="app-header">
      <h1>vcs overlay</h1>
      {sessionId && (
        <span className="session-chip" data-testid="session-chip">
          {sessionId}
        </span>
      )}
    </header>
  )
}

function Instructions() {
  return (
    <section className="instructions" data-testid="instructions">
      <h2>How it works</h2>
      <ol>
        <li>
          Each agent runs its own Vite server on an OS-assigned port
          (no hardcoded ports, no conflicts).
        </li>
        <li>
          The <code>session-overlay</code> plugin intercepts Vite's{' '}
          <code>load()</code> hook. If the agent's overlay directory has a
          version of a file, that version is served instead of the real one.
        </li>
        <li>
          When a file in the overlay changes, only that agent's browser gets
          the HMR update. Other agents are unaffected.
        </li>
        <li>
          The source tree is never modified. Overlays live in{' '}
          <code>/tmp/vcs-sessions/&lt;agent-id&gt;/</code>.
        </li>
      </ol>
    </section>
  )
}

export default App
