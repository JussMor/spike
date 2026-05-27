# vcs-vite

Vite plugin for **vcs-spike** agent workflows.

Lets two (or more) Claude Code sessions run **separate Vite dev-servers simultaneously** without collapsing each other's HMR or corrupting shared source files.

---

## The problem

Vite assumes one builder, one source directory.  
When two Claude Code sessions both write to `src/` they corrupt each other's HMR and the dev-server collapses.

## The solution

Each Vite instance is bound to **one vcs session + one vcs stack**.  
Source files are served directly from the **vcs store** (the view of that stack), not from disk.  
Two Vite instances can run simultaneously, each seeing only its own agent's changes.  
**Disk is never the authority — the store is.**

---

## Installation

```bash
npm install vcs-vite --save-dev
# peer dep: vite >= 4
```

---

## Usage — two-server pattern

```js
// vite.config.ts  (Session A — feature-auth, port 5173)
import { defineConfig } from 'vite'
import { vcsAgentPlugin } from 'vcs-vite'

export default defineConfig({
  plugins: [
    vcsAgentPlugin({
      sessionId: process.env.VCS_SESSION_ID,  // from vcs_session_open
      stackId:   process.env.VCS_STACK_ID,    // from vcs_stack_open
      storePath: '.vcs',                       // default
    }),
  ],
  server: { port: 5173 },
})
```

```js
// vite.config.ts  (Session B — feature-pay, port 5174)
export default defineConfig({
  plugins: [
    vcsAgentPlugin({
      sessionId: process.env.VCS_SESSION_ID,
      stackId:   process.env.VCS_STACK_ID,
    }),
  ],
  server: { port: 5174 },
})
```

Both servers read from the **same** `.vcs/` store (SQLite WAL mode handles concurrency).  
Each serves only **its own** stack's files.  
Neither touches disk.

---

## Options

| Option | Type | Default | Description |
|---|---|---|---|
| `stackId` | `string` | *required* | vcs stack ID for this session |
| `sessionId` | `string` | — | vcs session ID (enables phase management) |
| `storePath` | `string` | `.vcs` | path to the vcs store directory |
| `pollMs` | `number` | `300` | HMR polling interval in milliseconds |
| `checkPhaseGate` | `boolean` | `true` | warn if another session is in testing phase |

---

## How it works

### File serving

```
Browser requests src/auth.ts
  → Vite resolveId hook: recognised as project source
  → Vite load hook: read from vcs view (store) for this session's stack
  → return content  (disk never consulted for tracked files)
```

Files **not** tracked by this stack fall through to normal disk reads — so your `node_modules`, `public/`, and untracked assets all work normally.

### HMR

```
Agent calls vcs_edit(stack, 'src/auth.ts', newContent)
  → plugin polls stack tip_change_id every 300 ms
  → detects change; identifies WHICH files changed (diff tip→prev)
  → invalidates only those modules in Vite's module graph
  → sends targeted HMR update — only this session's server reacts
```

The other session's Vite instance **never sees the change** because it's polling a different stack.

### Phase gate

When the dev-server starts the plugin sets `phase=testing` on the session.  
`vcs_overview` shows:

```
⛔ feature-auth is TESTING on :5173 — other sessions must not merge yet
```

When the server closes (Ctrl+C or test runner finishes) the plugin sets `phase=done` automatically. The gate lifts. Now another session can merge.

---

## Status endpoint

The plugin exposes a status endpoint so the browser (or your CI runner) can inspect which session is active:

```
GET /api/vcs-agent/session
```

Response:
```json
{
  "sessionId": "sess_abc12345",
  "stackId":   "stk_def67890",
  "viewId":    "view_xyz",
  "port":      5173,
  "trackedFiles": ["src/auth.ts", "src/LoginForm.tsx"],
  "lastTip":   "ch_deadbeef"
}
```

---

## Agent workflow

```js
// 1. Register session
const { session_id } = await vcs_session_open({ agent_id: 'claude-code-feature-auth' })

// 2. Open stack
const { stack_id } = await vcs_stack_open({ agent_id: 'claude-code-feature-auth', session_id })

// 3. Set env vars and start Vite
process.env.VCS_SESSION_ID = session_id
process.env.VCS_STACK_ID   = stack_id
// → vite dev (plugin picks up env vars, sets phase=testing)

// 4. Make changes — each edit triggers HMR only on this session's server
await vcs_edit({ stack_id, path: 'src/auth.ts', content: '...', reason: 'add login form' })

// 5. Done — Ctrl+C or test runner exit → plugin sets phase=done automatically
await vcs_stack_close({ stack_id })
await vcs_session_close({ session_id })
```

---

## Custom HMR events

The plugin emits a `vcs:hot-update` custom WebSocket event on every HMR cycle.  
Connect in your browser code to show agent activity in devtools or a status overlay:

```ts
if (import.meta.hot) {
  import.meta.hot.on('vcs:hot-update', ({ path, sessionId, stackId, timestamp }) => {
    console.log(`[vcs] ${sessionId.slice(0, 8)}… updated ${path} at ${new Date(timestamp).toISOString()}`)
  })
}
```

---

## Philosophy

> **Store is truth. Disk is explicit.**

vcs-vite enforces this at the framework level.  
The vcs store (`.vcs/vcs.db`) is the single source of truth.  
Disk is only written when you explicitly call `vcs checkout`.  
Two agents, two servers, one store — no collisions.
