# vcs-spike

**Agent-native version control — the spike that answers: can agents produce structured, conflict-aware changes at scale, across multiple projects?**

The answer is yes. This repo proves it end-to-end: Rust data model → CLI you install in one command → parallel agents writing structured changes → a hub server that connects separate codebases → live TanStack Query dashboard showing everything in the browser.

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
│   ├── vcs-core/               Rust library — data model, store, view engine
│   └── vcs-cli/                vcs binary — git-like CLI + HTTP hub server
├── examples/
│   ├── webwright-demo/         Single-project: parallel agents, conflict detected
│   ├── tanstack-vite/          Real Vite + TanStack project tracked by vcs
│   │   └── e2e/                Playwright tests — all selectors via data-testid
│   └── multi-project-demo/     Two separate codebases → one hub → cross-project conflict
├── packages/
│   ├── vcs-npm/                npm package  (npm install -g vcs-spike)
│   ├── vcs-mcp/                Claude Code MCP server — vcs as native Claude tools
│   └── vcs-openai/             OpenAI Codex functions + OpenAPI spec + plugin manifest
├── .mcp.json                   Claude Code project config — auto-loads vcs MCP server
├── CLAUDE.md                   Claude Code instructions — how Claude uses vcs
├── .claude/commands/           Custom slash commands: /vcs-start /vcs-done /vcs-abort
├── docs/
│   └── cicd-architecture.md   Pipeline design, conflict gate, e2e strategy
├── install.sh                  One-command curl installer
└── skill/
    └── SKILL.md                Skill manifest — teaches any agent to drive vcs
```

---

## Install

### Option 1 — curl (any machine with Rust)

```bash
curl -fsSL https://raw.githubusercontent.com/JussMor/spike/main/install.sh | sh
```

Clones the repo into `~/.vcs-spike-src/`, builds the release binary, installs to `/usr/local/bin/vcs`. If Rust isn't installed it installs that first via rustup.

### Option 2 — npm (Node.js projects)

```bash
npm install -g vcs-spike      # installs vcs to PATH
# or per-project:
npm install vcs-spike
```

`postinstall` auto-finds the binary: `VCS_BIN` env → workspace sibling → system PATH → build from source.

### Option 3 — build from source

```bash
cargo build --release
cp target/release/vcs /usr/local/bin/vcs
```

### Start using it

```bash
cd your-project
vcs init                # creates .vcs/  (like git init)
vcs --help              # full command reference
```

The binary auto-detects `.vcs/` by walking up from CWD — **exactly like git finds `.git/`**.

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

### Efficient edit records

`vcs edit` stores two layers of data:

1. A result blob for deterministic checkout and view reads.
2. A compact structured patch blob plus `edit_metadata` row describing the base blob, result blob, edit kind, and changed line range.

That keeps materialization reliable while giving agents a small, inspectable operation for review, sync, and conflict reasoning. When two stacks edit different line ranges of the same base blob, `vcs view open` can auto-resolve that path by applying both patches and storing the merged result as a resolved conflict record.

---

## Examples

### 1. Webwright-style parallel agents (single project)

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

### 2. Multi-project hub (frontend + backend, separated codebases)

The core new capability: two projects that live in separate repos, each with their own `.vcs/` store, push to a shared hub. The hub detects cross-project conflicts before anything is deployed.

```bash
cd examples/multi-project-demo
VCS_BIN=../../target/release/vcs node orchestrator.js
```

```
Setup — init three isolated stores
  hub store:       .vcs-hub/
  frontend store:  .vcs-frontend/
  backend store:   .vcs-backend/

Project A: Frontend agent defines API contract
  ✓ frontend agent done (2 changes)
    → shared/api-contract.md:  POST /auth/login

Project B: Backend agent defines API contract (DIFFERENT endpoint!)
  ✓ backend agent done (2 changes)
    → shared/api-contract.md:  POST /auth/signin

Hub — cross-project view
  Files: backend/src/routes/auth.ts, frontend/src/api-client.ts, shared/api-contract.md

  Conflicts (1):
    ⚡ UNRESOLVED  shared/api-contract.md
      └─ frontend/agent-ui  (POST /auth/login)
      └─ backend/agent-api  (POST /auth/signin)

→ resolved in 9ms  (backend wins — they own the contract)
  Action: agent-ui must update fetch call from /auth/login → /auth/signin
```

The conflict is surfaced **before any code is merged or deployed**. Without the hub, the mismatch only appears when the e2e tests fail in production.

### 3. TanStack Vite — live dashboard

```bash
cd examples/tanstack-vite
VCS_BIN=../../target/release/vcs npm run vcs:init   # vcs init in this project
VCS_BIN=../../target/release/vcs npm run vcs:demo   # seed store with demo state
VCS_BIN=../../target/release/vcs npm run vcs:agents # 4 parallel workers
npm run dev                                          # dashboard at :5173
```

### 4. E2e tests (Playwright, all data-testid)

```bash
cd examples/tanstack-vite
npm run e2e          # run against running dev server
npm run e2e:ui       # Playwright UI mode
npm run e2e:report   # open HTML report
```

---

## `vcs serve` — the hub server

`vcs serve` turns any vcs store into an HTTP API server. Projects in other repos push their stacks to it; the hub builds a cross-project view and surfaces conflicts.

```bash
# On a shared machine (or localhost):
vcs init --store /tmp/hub
vcs serve --store /tmp/hub --port 7474
```

```
vcs hub listening on http://0.0.0.0:7474
  Dashboard:  point the tanstack-vite UI at http://localhost:7474
  Push URL:   POST http://localhost:7474/api/vcs/push
```

### Read endpoints (same shape as Vite plugin — existing UI works against hub)

```
GET  /api/vcs/status
GET  /api/vcs/changes
GET  /api/vcs/edits
GET  /api/vcs/stacks
GET  /api/vcs/views
GET  /api/vcs/active-view
GET  /api/vcs/view/:id/files
GET  /api/vcs/view/:id/conflicts
GET  /api/vcs/export
GET  /api/vcs/blobs/:hash
```

### Write endpoints (for agents and push protocol)

```
POST /api/vcs/stacks/open           { agent_id, base_change_id? }
POST /api/vcs/stacks/:id/close
POST /api/vcs/edit                  { stack_id, path, content_b64, intent }
POST /api/vcs/delete                { stack_id, path, intent }
POST /api/vcs/views/open            { base_change_id, stack_ids: [] }
POST /api/vcs/conflicts/:id/resolve { pick?: stack_id, merge_content_b64? }
POST /api/vcs/push                  { project_id, stacks[], changes[], files[], blobs{} }
```

### Remote sync between stores

`vcs serve` is bidirectional: a local store can push its complete structured history to a hub, and another store can pull that bundle back down.

```bash
vcs serve --store /tmp/hub --port 7474

vcs remote add hub http://localhost:7474
vcs push hub --project-id frontend
vcs pull hub
```

Bundles include stacks, changes, structured edit metadata, per-change file-state rows, and content-addressed blobs, so pulled stores can open views, detect conflicts, inspect compact edit operations, and checkout historical states without needing the original project directory.

### History checkout

Agents can materialize the file tree at any recorded change:

```bash
vcs history
vcs checkout <change_id>
vcs checkout <change_id> --worktree /tmp/replay
```

Checkout composes the parent chain for `<change_id>`, writes the tracked files in that snapshot, and removes tracked files that are absent from that point in history.

### Connecting a project to the hub (Node.js)

```js
import { VcsRemoteClient } from 'vcs-spike/remote'

const hub = new VcsRemoteClient('http://hub.internal:7474')

// Agent in Project A (frontend):
const stackId = await hub.stackOpen('agent-ui')
await hub.edit(stackId, 'shared/api-contract.md', content, {
  reason: 'define POST /auth/login',
  task_ref: 'FE-101',
})
await hub.stackClose(stackId)

// After all agents are done — build cross-project view:
const viewId = await hub.viewOpen('', [frontendStackId, backendStackId])
const conflicts = await hub.viewConflicts(viewId)
// [{ path: 'shared/api-contract.md', candidates: [...] }]
```

---

## AI integrations

### Claude Code (MCP server)

The `.mcp.json` at the repo root tells Claude Code to auto-load the vcs MCP server.
Once connected, Claude has these tools natively — no CLI calls needed:

```
vcs_init        vcs_stack_open   vcs_stack_close  vcs_stack_abandon
vcs_edit        vcs_delete       vcs_rename
vcs_view_open   vcs_view_files   vcs_view_conflicts   vcs_resolve
vcs_log
```

**What changes for Claude:**
```
Before:  Write("src/foo.ts", content)          ← direct write, invisible to other agents
After:   vcs_edit(stackId, "src/foo.ts", ...)  ← tracked, intent-documented, conflict-aware
```

Custom slash commands available in Claude Code:
```
/vcs-start <task>   open a stack, begin tracking
/vcs-done           close stack, show summary
/vcs-abort          abandon stack on error
/vcs-status         show what's tracked so far
```

**For a new project:**

```json
// .mcp.json
{
  "mcpServers": {
    "vcs": {
      "command": "npx",
      "args": ["vcs-mcp"],
      "env": { "VCS_BIN": "/usr/local/bin/vcs" }
    }
  }
}
```

See `packages/vcs-mcp/README.md` and `CLAUDE.md` for full setup.

---

### OpenAI Codex / GPT-4o

Drop-in function definitions for the Chat Completions and Assistants APIs:

```js
import { vcsTools, vcsSystemPrompt, handleVcsTool } from 'vcs-openai'

const response = await openai.chat.completions.create({
  model: 'gpt-4o',
  messages: [
    { role: 'system', content: vcsSystemPrompt },
    { role: 'user',   content: 'Add a LoginForm component' },
  ],
  tools: vcsTools,   // ← all 12 vcs functions, with JSON schemas
})

// Dispatch tool calls back to vcs binary:
for (const call of response.choices[0].message.tool_calls ?? []) {
  const result = await handleVcsTool(call.function.name, JSON.parse(call.function.arguments))
}
```

Also includes:
- `openapi.yaml` — full OpenAPI 3.1 spec for the `vcs serve` hub API
- `ai-plugin.json` — ChatGPT plugin manifest (points at `GET /openapi.yaml`)

See `packages/vcs-openai/README.md` for full docs.

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
├── cargo test            (11 tests, fast)
├── vcs conflict gate     single-project: webwright demo — zero unresolved conflicts
│                         multi-project:  hub demo        — cross-project conflicts resolved
└── vite build            (tsc + rollup, no type errors)
         │ all green
         ▼
    Playwright e2e         (separate job — browser, Vite dev server, retries=2)
         │ green
         ▼
    merge allowed
```

The **vcs conflict gate** is the key: agents can never silently overwrite each other's work — in the same project or across projects. Both orchestrators must exit zero before e2e runs.

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
| Can separate projects share change awareness? | ✓ vcs serve hub — POST /api/vcs/push from any project, cross-project view |
| One install command? | ✓ curl install.sh \| sh  or  npm install -g vcs-spike |

---

## What's built vs what's next

| Feature | Status |
|---|---|
| `vcs init / edit / delete / rename` | ✅ built |
| `vcs stack open / close / abandon` | ✅ built |
| `vcs view open / ls / conflicts / resolve` | ✅ built |
| `vcs serve --port` (HTTP hub) | ✅ built |
| curl one-command installer | ✅ built |
| npm package (`vcs-spike`) | ✅ built |
| Webwright integration demo | ✅ built |
| Multi-project hub demo | ✅ built |
| Playwright e2e with data-testid | ✅ built |
| GitHub Actions CI (4-job pipeline) | ✅ built |
| Claude Code MCP server (`vcs-mcp`) | ✅ built |
| Claude Code slash commands | ✅ built |
| OpenAI function definitions (`vcs-openai`) | ✅ built |
| OpenAPI 3.1 spec for hub API | ✅ built |
| Remote push/pull between stores | ✅ built |
| History navigation / filesystem materializer (`vcs checkout`) | ✅ built |
| `vcs watch` — filesystem watcher (human dev UX) | ✅ built |
| Conflict resolution UI (interactive Pick A/B + custom merge) | ✅ built |
| Multi-token ACL (`vcs token add/ls/remove`) | ✅ built |
| Binary releases (GitHub Actions release workflow) | ✅ built |
