import { defineConfig, devices } from '@playwright/test'

/**
 * Playwright config for vcs-spike tanstack-vite example.
 *
 * Design decisions:
 *
 * 1. ALL selectors use data-testid — never CSS classes, never text content,
 *    never DOM structure. This means agent refactors can't break tests.
 *
 * 2. The dev server must be running (npm run dev) before tests run.
 *    In CI: webServer block starts it automatically.
 *
 * 3. vcs-spike must be initialised (npm run vcs:demo) before tests run.
 *    In CI: a setup step runs vcs:demo before the test suite.
 *
 * 4. Tests are independent of each other — each test re-polls the /api/vcs/*
 *    endpoints rather than sharing state.
 */
export default defineConfig({
  testDir: './tests',

  // Fail fast in CI, show all errors locally
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,

  reporter: [
    ['html', { outputFolder: '../playwright-report', open: 'never' }],
    ['list'],
  ],

  use: {
    baseURL: process.env.PLAYWRIGHT_BASE_URL ?? 'http://localhost:5173',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },

  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'mobile',   use: { ...devices['Pixel 5'] } },
  ],

  // Start dev server automatically in CI
  webServer: process.env.CI ? {
    command:   'npm run dev',
    url:       'http://localhost:5173',
    reuseExistingServer: false,
    timeout:   30_000,
  } : undefined,
})
