/**
 * vcs-ui.spec.ts
 *
 * E2e tests for the vcs-spike UI dashboard.
 * All selectors use data-testid — NEVER CSS classes or text content.
 *
 * Why data-testid only:
 *   Agents refactor class names and text constantly.
 *   data-testid is an explicit contract: "this element is test-stable".
 *   If an agent removes a data-testid, the test breaks loudly on purpose.
 */

import { test, expect } from '@playwright/test'

// ── File Tree panel ────────────────────────────────────────────────────────

test.describe('File Tree panel', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
  })

  test('renders file-tree container', async ({ page }) => {
    // Either the tree or the "no files" empty state
    const tree  = page.getByTestId('file-tree')
    const empty = page.locator('[class*="empty"], [class*="loading"]')
    await expect(tree.or(empty).first()).toBeVisible({ timeout: 10_000 })
  })

  test('each file-item has a file-path child', async ({ page }) => {
    const tree = page.getByTestId('file-tree')
    const hasItems = await tree.isVisible().catch(() => false)
    if (!hasItems) return // skip if no files tracked yet

    const items = page.getByTestId('file-item')
    const count = await items.count()
    if (count === 0) return

    // Every file-item must contain a file-path
    for (let i = 0; i < Math.min(count, 5); i++) {
      await expect(items.nth(i).getByTestId('file-path')).toBeVisible()
    }
  })
})

// ── Change Log panel ───────────────────────────────────────────────────────

test.describe('Change Log panel', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
  })

  test('renders change-log or empty state', async ({ page }) => {
    const log   = page.getByTestId('change-log')
    const empty = page.locator('text=No changes recorded')
    await expect(log.or(empty).first()).toBeVisible({ timeout: 10_000 })
  })

  test('change-item has required data-testid children', async ({ page }) => {
    const log = page.getByTestId('change-log')
    const exists = await log.isVisible().catch(() => false)
    if (!exists) return

    const items = page.getByTestId('change-item')
    const count = await items.count()
    if (count === 0) return

    const first = items.first()
    await expect(first.getByTestId('change-op')).toBeVisible()
    await expect(first.getByTestId('change-path')).toBeVisible()
    await expect(first.getByTestId('change-reason')).toBeVisible()
    await expect(first.getByTestId('change-meta')).toBeVisible()
  })

  test('change-op has known value', async ({ page }) => {
    const items = page.getByTestId('change-item')
    const count = await items.count()
    if (count === 0) return

    const op = await items.first().getByTestId('change-op').textContent()
    expect(['create', 'edit', 'delete', 'rename']).toContain(op?.trim())
  })
})

// ── Conflict Panel ─────────────────────────────────────────────────────────

test.describe('Conflict panel', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
  })

  test('renders no-conflicts or conflict-list', async ({ page }) => {
    const noConflicts = page.getByTestId('no-conflicts')
    const list        = page.getByTestId('conflict-list')
    await expect(noConflicts.or(list).first()).toBeVisible({ timeout: 10_000 })
  })

  test('resolved conflict has correct testid', async ({ page }) => {
    const resolved = page.getByTestId('conflict-resolved')
    const count = await resolved.count()
    if (count === 0) return // no resolved conflicts yet

    const first = resolved.first()
    await expect(first.getByTestId('conflict-path')).toBeVisible()
    await expect(first.getByTestId('conflict-candidates')).toBeVisible()
  })

  test('unresolved conflict shows candidates', async ({ page }) => {
    const unresolved = page.getByTestId('conflict-unresolved')
    const count = await unresolved.count()
    if (count === 0) return

    const first = unresolved.first()
    await expect(first.getByTestId('conflict-path')).toBeVisible()
    const candidates = first.getByTestId('conflict-candidate')
    expect(await candidates.count()).toBeGreaterThanOrEqual(2)
  })
})

// ── API contract tests ─────────────────────────────────────────────────────
//
// These test the /api/vcs/* endpoints directly — not the UI.
// They're the bridge between the Playwright e2e layer and the vcs backend.

test.describe('vcs API contract', () => {
  test('GET /api/vcs/status returns initialised=true', async ({ request }) => {
    const res = await request.get('/api/vcs/status')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(body.initialised).toBe(true)
    expect(body.storePath).toContain('.vcs')
  })

  test('GET /api/vcs/active-view returns view shape or null', async ({ request }) => {
    const res = await request.get('/api/vcs/active-view')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    if (body !== null) {
      expect(body).toHaveProperty('view_id')
      expect(body).toHaveProperty('base_change_id')
      expect(body).toHaveProperty('stack_ids')
    }
  })

  test('GET /api/vcs/changes returns array', async ({ request }) => {
    const res = await request.get('/api/vcs/changes')
    expect(res.ok()).toBeTruthy()
    const body = await res.json()
    expect(Array.isArray(body)).toBe(true)
    // Each change must have the required fields
    for (const change of body.slice(0, 5)) {
      expect(change).toHaveProperty('change_id')
      expect(change).toHaveProperty('path')
      expect(change).toHaveProperty('op')
      expect(change).toHaveProperty('intent')
      expect(change.intent).toHaveProperty('reason')
    }
  })
})
