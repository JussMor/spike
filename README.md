# vcs-spike

**Agent-native version control — the spike that answers: can agents produce structured, conflict-aware changes at scale?**

The answer is yes. This repo proves it end-to-end: from the Rust data model through a CLI you install like a binary, to parallel Webwright-style agents writing Playwright tests with stable `data-testid` selectors, to a live TanStack Query dashboard showing everything in the browser.

---

## The one-line pitch

```
git is for humans who edit files.
vcs-spike is for agents that produce changes.
```

Agents know exactly what they touched and why. They don't need a watcher or a staging area — they call `vcs edit` directly, with intent. The orchestrator opens a view, sees conflicts as data, and decides. No silent overwrites. Ever.

---

## Repository layout

```
vcs-spike/
├── crates/
│   ├── vcs-core/          Rust library — data model, store, view engine
│   └── vcs-cli/           vcs binary — git-like CLI, auto-detects .vcs/
├── examples/
│   ├── webwright-demo/    Webwright-style parallel agents writing Playwright specs
│   └── tanstack-vite/     Real Vite + TanStack project tracked by vcs
│       └── e2e/           Playwright e2e tests — all selectors via data-testid
├── docs/
│   └── cicd-architecture.md  Pipeline design, conflict gate, e2e strategy
└── skill/
    └── SKILL.md           Skill manifest — teaches any agent to drive vcs
```

---

## Install (two files, that's it)

```bash
# Build the binary
cargo build --release

# Put it on PATH — or just reference it directly
cp target/release/vcs /usr/local/bin/vcs

# In any project:
vcs init         # creates .vcs/ here — like git init
vcs stack open --agent me --json
vcs edit <stack> src/App.tsx --content-file src/App.tsx --reason "why"
vcs stack close <stack>
```

The binary auto-detects `.vcs/` by walking up from CWD — **exactly like git finds `.git/`**. No config file, no env var needed.

---

## The data model (five tables, one blob dir)

```
changes          append-only event log
  change_id    = BLAKE3(parent_id | path | diff_hash | agent_id | ts)
  intent       = { reason (required), tool_call?, task_ref? }

stacks           one per agent session
  base → tip   ordered chain of change_ids

views            virtual merge of N stacks on a base
  conflicts    = paths touched by >1 stacks → first-class data objects

files_at_change  derived index: file tree state at each change
blobs/           content-addressed storage (like git objects, no zlib)
```

---

## How it works for agents

```
Orchestrator
│
├── spawn Agent A ──→ open stack → edit files → close stack
├── spawn Agent B ──→ open stack → edit files → close stack  (parallel)
│
└── open view(base, [stack_A, stack_B])
    ├── conflicts() → [] or [{path, candidates}]   ← data, not errors
    ├── list_files() → merged file tree
    └── read_file(path) → merged content (after resolution)
```

No agent coordinates with another agent. The orchestrator sees everything after.

---

## Examples

### Webwright-style parallel agents

Two agents run simultaneously. Both write Playwright specs. Both touch `LoginForm.tsx` → conflict detected automatically.

```bash
cd examples/webwright-demo
VCS_BIN=../../target/release/vcs node src/orchestrator.js
```

```
agent-login    4 steps  [LoginForm.tsx, login.spec.ts, result.json, auth.ts]
agent-dashboard 4 steps [Dashboard.tsx, LoginForm.tsx, dashboard.spec.ts, result.json]

Files in merged view: 8
Conflicts: 1  ⚡ src/features/auth/LoginForm.tsx (2 candidates)
→ resolved by orchestrator (169ms total)
```

### TanStack Vite — real project tracked by vcs

```bash
cd examples/tanstack-vite
VCS_BIN=../../target/release/vcs npm run vcs:init   # vcs init in this project
VCS_BIN=../../target/release/vcs npm run vcs:demo   # track real source files
VCS_BIN=../../target/release/vcs npm run vcs:agents # 4 parallel workers
npm run dev                                          # live dashboard at :5173
```

### E2e tests (Playwright, all data-testid)

```bash
cd examples/tanstack-vite
npm run e2e          # run against running dev server
npm run e2e:ui       # Playwright UI mode
npm run e2e:report   # open HTML report
```

---

## The data-testid contract

Every component an agent writes must have `data-testid` on interactive elements. Every Playwright test must select via `getByTestId()` only. This is the contract that survives agent refactors:

```tsx
// ✓ agent writes this
<form data-testid="login-form">
  <input data-testid="login-email" />
  <button data-testid="login-submit">Sign in</button>
  <p data-testid="login-error">{error}</p>
</form>

// ✓ test selects this — survives any CSS or structure refactor
await page.getByTestId('login-submit').click()
await expect(page.getByTestId('login-error')).toBeVisible()
```

Convention: `<feature>-<element>` — `login-form`, `login-email`, `dashboard-header`, `change-item`.

---

## CI/CD pipeline

```
push / PR
│
├── cargo test          (11 tests, fast)
├── vcs conflict gate   (webwright demo: zero unresolved conflicts)
└── vite build          (tsc + rollup)
         │ all green
         ▼
    Playwright e2e      (separate job — browser, slow, retries=2)
         │ green
         ▼
    merge allowed
```

The **vcs conflict gate** is the key: agents can never silently overwrite each other's work. The orchestrator must resolve all conflicts before the e2e job runs.

See `docs/cicd-architecture.md` for the full GitHub Actions workflow.

---

## Spike questions — answered

| Question | Answer |
|---|---|
| Is the change-event model expressive enough? | ✓ create/edit/delete/rename covers all cases |
| Is SQLite fast enough for parallel agents? | ✓ WAL mode, 6 workers × 3 edits in 300ms, zero loss |
| Does intent metadata earn its weight? | ✓ task_ref links changes to tasks; tool_call captures Webwright context |
| What's the view computation cost? | O(changes in stacks) — negligible at <1000 changes |
| Can this integrate with Webwright? | ✓ adapter wraps agent writes; skill manifest teaches the agent |
| How do e2e tests survive agent refactors? | data-testid contract — explicit, stable, breaks loudly if removed |

---

## What's not built (intentional)

| Missing | Why |
|---|---|
| Filesystem materializer | Post-spike — `vcs checkout <view> <dir>` |
| Filesystem watcher | Not needed for agents; needed for human dev UX |
| Remotes / push / pull | Post-spike — views already define the merge protocol |
| Conflict resolution UI | Conflicts are data; the orchestrator resolves |
| ACL / secrets | Hook point is obvious; don't build what you don't need yet |
