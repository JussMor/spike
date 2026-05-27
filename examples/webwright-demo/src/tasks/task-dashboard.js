/**
 * task-dashboard.js
 * Webwright task: implement the dashboard page with vcs change viewer.
 * This agent also touches LoginForm.tsx → will conflict with task-login!
 */

export const TASK_ID = 'task-dashboard'

export async function run(adapter) {
  // ── Step 1: Dashboard component ────────────────────────────────────────
  adapter.write('src/features/dashboard/Dashboard.tsx', `
import { useVcsAllChanges } from '../../hooks/useVcs'

export function Dashboard({ token }: { token: string }) {
  const { data: changes } = useVcsAllChanges()

  return (
    <main data-testid="dashboard">
      <header data-testid="dashboard-header">
        <h1>Dashboard</h1>
        <span data-testid="token-badge">{token.slice(0, 8)}…</span>
      </header>

      <section data-testid="changes-section">
        <h2>Recent Changes ({changes?.length ?? 0})</h2>
        <ul data-testid="changes-list">
          {changes?.slice(-10).reverse().map(c => (
            <li key={c.change_id} data-testid="change-item">
              <code data-testid="change-id">{c.change_id.slice(0, 8)}</code>
              <span data-testid="change-op">{c.op}</span>
              <span data-testid="change-path">{c.path}</span>
            </li>
          ))}
        </ul>
      </section>
    </main>
  )
}
`.trim(), {
    reason: 'implement Dashboard with vcs change viewer and data-testid attributes',
    playwrightCall: { action: 'write_component', component: 'Dashboard' },
  })

  // ── Step 2: CONFLICT — this agent also modifies LoginForm ──────────────
  // (different from what task-login wrote → conflict detected by vcs)
  adapter.write('src/features/auth/LoginForm.tsx', `
import { useState } from 'react'

// dashboard-agent version: adds "remember me" checkbox
export function LoginForm({ onSuccess }: { onSuccess: (token: string) => void }) {
  const [email, setEmail]         = useState('')
  const [password, setPassword]   = useState('')
  const [remember, setRemember]   = useState(false)
  const [error, setError]         = useState<string | null>(null)
  const [loading, setLoading]     = useState(false)

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setLoading(true)
    try {
      const res = await fetch('/api/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password, remember }),
      })
      if (!res.ok) throw new Error('Invalid credentials')
      const { token } = await res.json()
      if (remember) localStorage.setItem('token', token)
      onSuccess(token)
    } catch (err) {
      setError((err as Error).message)
    } finally {
      setLoading(false)
    }
  }

  return (
    <form onSubmit={handleSubmit} data-testid="login-form">
      <input type="email" value={email} onChange={e => setEmail(e.target.value)}
        data-testid="login-email" required />
      <input type="password" value={password} onChange={e => setPassword(e.target.value)}
        data-testid="login-password" required />
      <label>
        <input type="checkbox" checked={remember} onChange={e => setRemember(e.target.checked)}
          data-testid="login-remember" />
        Remember me
      </label>
      {error && <p data-testid="login-error">{error}</p>}
      <button type="submit" disabled={loading} data-testid="login-submit">
        {loading ? 'Signing in…' : 'Sign in'}
      </button>
    </form>
  )
}
`.trim(), {
    reason: 'add "remember me" checkbox to LoginForm — dashboard-agent version',
    playwrightCall: { action: 'modify_component', component: 'LoginForm' },
  })

  // ── Step 3: Dashboard e2e spec ─────────────────────────────────────────
  await adapter.runScript(
    'e2e/tests/dashboard/dashboard.spec.ts',
    `
import { test, expect } from '@playwright/test'

test.describe('Dashboard', () => {
  test.beforeEach(async ({ page }) => {
    await page.route('/api/auth/login', route =>
      route.fulfill({ json: { token: 'test-jwt-123' } })
    )
    await page.goto('/')
    await page.getByTestId('login-email').fill('user@example.com')
    await page.getByTestId('login-password').fill('correct')
    await page.getByTestId('login-submit').click()
    await expect(page.getByTestId('dashboard')).toBeVisible()
  })

  test('shows dashboard header', async ({ page }) => {
    await expect(page.getByTestId('dashboard-header')).toBeVisible()
    await expect(page.getByTestId('token-badge')).toContainText('test-jwt')
  })

  test('shows changes list', async ({ page }) => {
    await expect(page.getByTestId('changes-section')).toBeVisible()
    await expect(page.getByTestId('changes-list')).toBeVisible()
  })
})
`.trim(),
    { reason: 'playwright e2e for dashboard — all selectors via data-testid' },
  )
}
