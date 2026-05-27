import { ChangeLog } from './components/ChangeLog'
import { FileTree } from './components/FileTree'
import { ConflictPanel } from './components/ConflictPanel'
import './App.css'

/**
 * The Vite project is tracked by vcs-spike.
 * Run `npm run vcs:demo` to see the full proof.
 * Run `npm run vcs:watch` to track live edits.
 */
function App() {
  return (
    <div className="app">
      <header className="app-header">
        <h1>
          <span className="badge">vcs-spike</span>
          TanStack Demo
        </h1>
        <p className="subtitle">
          This project's own source files are tracked by{' '}
          <code>vcs</code> — an agent-native VCS running inside{' '}
          <code>.vcs/</code> in this directory.
        </p>
      </header>

      <main className="app-main">
        <section className="panel">
          <h2>📄 File Tree</h2>
          <FileTree />
        </section>

        <section className="panel">
          <h2>📋 Change Log</h2>
          <ChangeLog />
        </section>

        <section className="panel">
          <h2>⚡ Conflicts</h2>
          <ConflictPanel />
        </section>
      </main>

      <footer className="app-footer">
        <code>vcs log</code> · <code>vcs view ls</code> ·{' '}
        <code>vcs view conflicts</code> · all from <code>.vcs/</code>
      </footer>
    </div>
  )
}

export default App
