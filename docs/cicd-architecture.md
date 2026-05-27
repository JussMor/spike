# CI/CD Architecture

## The problem vcs-spike solves in CI/CD

Without vcs-spike, parallel agents write to the filesystem directly.
Two agents edit the same file → last writer wins → silent data loss.
In CI you only find out after the tests fail on a garbled file.

With vcs-spike, conflicts are surfaced as data **before** anything is
written to disk.  CI can gate on "zero unresolved conflicts" before
running e2e tests.

---

## The testid contract

Every UI element that must survive agent refactoring gets a `data-testid`.
This is the contract between agents and tests:

```
Agent writes component  →  must include data-testid
Test selects element    →  must use getByTestId(), never CSS class or text
```

**Why this matters for multi-agent CI:**
- Agent A refactors className from `.btn-login` to `.auth-submit` → test breaks
- Agent A removes `data-testid="login-submit"` → test breaks loudly on purpose
- The testid is the stable interface.  The implementation can change freely.

**Convention:**  `<feature>-<element>`
```
login-form     login-email      login-password   login-submit    login-error
dashboard      dashboard-header changes-list     change-item
register-form  register-name    register-submit
```

---

## Pipeline design

```
┌─────────────────────────────────────────────────────────────┐
│  PR opened / push to branch                                 │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │  Job 1       │  │  Job 2       │  │  Job 3           │  │
│  │  vcs checks  │  │  unit tests  │  │  build           │  │
│  │              │  │  (cargo test)│  │  (vite build)    │  │
│  │  ─────────── │  │              │  │                  │  │
│  │  vcs:demo    │  │  11 tests    │  │  tsc + rollup    │  │
│  │  zero        │  │  must pass   │  │  no type errors  │  │
│  │  conflicts   │  │              │  │                  │  │
│  └──────┬───────┘  └──────┬───────┘  └────────┬─────────┘  │
│         └─────────────────┴───────────────────┘            │
│                           │ all green                       │
│                           ▼                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Job 4 — e2e (Playwright, separate job)             │   │
│  │                                                     │   │
│  │  1. npm run vcs:demo   (seed store with demo state) │   │
│  │  2. npm run dev &      (start Vite dev server)      │   │
│  │  3. npm run e2e        (Playwright against :5173)   │   │
│  │                                                     │   │
│  │  Tests use ONLY data-testid selectors               │   │
│  │  Retries: 2 (CI flakiness buffer)                   │   │
│  └─────────────────────────────────────────────────────┘   │
│                           │ all green                       │
│                           ▼                                 │
│                        merge allowed                        │
└─────────────────────────────────────────────────────────────┘
```

**Why e2e is a separate job:**
- Browser tests are slow (30–60s per suite)
- They need a running server (Vite dev server)
- They can be flaky on slow CI machines → retries=2
- They should not block unit tests from running in parallel
- A separate job can be re-triggered without re-running cargo test

---

## GitHub Actions example

```yaml
# .github/workflows/ci.yml
name: CI

on: [push, pull_request]

jobs:
  # ── Fast jobs (parallel) ────────────────────────────────────────────────

  rust-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test

  vcs-checks:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
      - uses: actions/setup-node@v4
        with: { node-version: '22' }
      - run: cd examples/webwright-demo && VCS_BIN=../../target/release/vcs node src/orchestrator.js
        # Fails if any conflict is left unresolved

  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: '22' }
      - run: cd examples/tanstack-vite && npm ci && npm run build

  # ── E2e (depends on all fast jobs) ─────────────────────────────────────

  e2e:
    needs: [rust-tests, vcs-checks, build]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release

      - uses: actions/setup-node@v4
        with: { node-version: '22' }
      - run: cd examples/tanstack-vite && npm ci

      - name: Install Playwright browsers
        run: cd examples/tanstack-vite && npx playwright install --with-deps chromium

      - name: Seed vcs store
        run: |
          cd examples/tanstack-vite
          VCS_BIN=../../target/release/vcs npm run vcs:init
          VCS_BIN=../../target/release/vcs npm run vcs:demo

      - name: Run e2e tests
        run: |
          cd examples/tanstack-vite
          VCS_BIN=../../target/release/vcs npm run e2e
        env:
          CI: true
          PLAYWRIGHT_BASE_URL: http://localhost:5173

      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: playwright-report
          path: examples/tanstack-vite/playwright-report/
```

---

## The vcs gate

The `vcs-checks` job acts as a conflict gate:

```bash
# This command exits non-zero if any unresolved conflicts remain
VCS_BIN=./target/release/vcs node examples/webwright-demo/src/orchestrator.js

# CI sees exit code 1 → job fails → e2e doesn't run → merge blocked
```

This means: **agents can never silently overwrite each other's work in a PR.**
The conflict must be resolved (by an orchestrator or manually) before e2e runs.

---

## What stays in same CI vs separate

| Check | Same CI | Why |
|---|---|---|
| `cargo test` | ✓ | Fast, no browser |
| `vcs conflicts == 0` | ✓ | Fast, just SQLite |
| `tsc --noEmit` | ✓ | Fast, no browser |
| Playwright e2e | separate job | Slow, needs browser, needs server |
| Visual regression | separate job | Needs screenshots, slow |
| Load / perf tests | separate pipeline | Very slow, different triggers |

---

## Future: vcs as CI artifact

Once vcs has remotes (post-spike), the `.vcs/` store itself becomes a CI
artifact — the full audit trail of what every agent did in the PR:

```yaml
- uses: actions/upload-artifact@v4
  with:
    name: vcs-store
    path: .vcs/
```

Then the reviewer can download the store, open a view, read every change,
see every conflict and resolution — a structured audit trail that git's
commit graph can't provide.
