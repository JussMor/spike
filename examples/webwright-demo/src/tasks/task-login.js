/**
 * task-login.js
 * Webwright task: implement and test a login form.
 * The agent writes a Playwright spec + a React component.
 */

export const TASK_ID = 'task-login'

export async function run(adapter) {
  // ── Step 1: write the React component ──────────────────────────────────
  adapter.write('src/features/auth/LoginForm.tsx', `
import { useState } from 'react'

export function LoginForm({ onSuccess }: { onSuccess: (token: string) => void }) {
  const [email, setEmail]       = useState('')
  const [password, setPassword] = useState('')
  const [error, setError]       = useState<string | null>(null)
  const [loading, setLoading]   = useState(false)

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setLoading(true)
    setError(null)
    try {
      const res = await fetch('/api/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password }),
      })
      if (!res.ok) throw new Error('Invalid credentials')
      const { token } = await res.json()
      onSuccess(token)
    } catch (err) {
      setError((err as Error).message)
    } finally {
      setLoading(false)
    }
  }

  return (
    <form onSubmit={handleSubmit} data-testid="login-form">
      <div>
        <label htmlFor="email">Email</label>
        <input
          id="email"
          type="email"
          value={email}
          onChange={e => setEmail(e.target.value)}
          data-testid="login-email"
          required
        />
      </div>
      <div>
        <label htmlFor="password">Password</label>
        <input
          id="password"
          type="password"
          value={password}
          onChange={e => setPassword(e.target.value)}
          data-testid="login-password"
          required
        />
      </div>
      {error && <p data-testid="login-error">{error}</p>}
      <button type="submit" disabled={loading} data-testid="login-submit">
        {loading ? 'Signing in…' : 'Sign in'}
      </button>
    </form>
  )
}
`.trim(), {
    reason: 'implement LoginForm component with data-testid attributes for e2e stability',
    playwrightCall: { action: 'write_component', component: 'LoginForm' },
  })

  // ── Step 2: write the Playwright spec ──────────────────────────────────
  await adapter.runScript(
    'e2e/tests/auth/login.spec.ts',
    `
import { test, expect } from '@playwright/test'

test.describe('Login form', () => {
  test('renders with correct test IDs', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByTestId('login-form')).toBeVisible()
    await expect(page.getByTestId('login-email')).toBeVisible()
    await expect(page.getByTestId('login-password')).toBeVisible()
    await expect(page.getByTestId('login-submit')).toBeVisible()
  })

  test('shows error on bad credentials', async ({ page }) => {
    await page.goto('/')
    await page.getByTestId('login-email').fill('bad@example.com')
    await page.getByTestId('login-password').fill('wrong')
    await page.getByTestId('login-submit').click()
    await expect(page.getByTestId('login-error')).toBeVisible()
  })

  test('calls onSuccess with token on valid login', async ({ page }) => {
    await page.route('/api/auth/login', route =>
      route.fulfill({ json: { token: 'test-jwt' } })
    )
    await page.goto('/')
    await page.getByTestId('login-email').fill('user@example.com')
    await page.getByTestId('login-password').fill('correct')
    await page.getByTestId('login-submit').click()
    // onSuccess called — token stored
    await expect(page.locator('[data-testid="dashboard"]')).toBeVisible()
  })
})
`.trim(),
    { reason: 'playwright e2e spec for login — all selectors via data-testid' },
  )

  // ── Step 3: write route handler ────────────────────────────────────────
  adapter.write('src/api/auth.ts', `
export async function loginHandler(req: Request): Promise<Response> {
  const { email, password } = await req.json()
  if (email === 'user@example.com' && password === 'correct') {
    return Response.json({ token: 'jwt-' + btoa(email) })
  }
  return Response.json({ error: 'Invalid credentials' }, { status: 401 })
}
`.trim(), {
    reason: 'stub login API handler',
  })
}
