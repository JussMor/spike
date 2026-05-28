# vite-overlay — portless multi-agent HMR

A Vite plugin that lets multiple agent sessions share one source tree,
each with their own virtual file overlay — without port conflicts, git
branches, or worktrees.

## The problem it solves

Running N dev servers for N agents normally means:
- Hardcoded ports (3001, 3002, …) → conflicts when anything changes
- Copying the source tree per agent → disk waste, stale copies
- Git worktrees or branches → merge overhead, staging state confusion

## The solution

```
source tree (unchanged on disk)
   ↓
Vite plugin: load() hook
   ↓ checks if overlayDir/relPath exists
   ↓ yes → serve overlay content  (only for THIS session)
   ↓ no  → serve real disk file
   ↓
browser
```

Each agent session gets:
- An **OS-assigned port** (`port: 0` → no conflicts, no config)
- Its own **overlay directory** (`/tmp/vcs-sessions/<agent-id>/`)
- **Targeted HMR**: only that session's browser updates when its overlay changes

## Quickstart

```bash
npm install

# Option A — single session (normal Vite dev server)
npm run dev
# VCS_AGENT_ID=my-agent npm run dev

# Option B — multi-session (portless)
npm run agents
# VCS_AGENTS=agent-a,agent-b npm run agents
```

## How overlays work

Place a file in the agent's overlay directory with the **same relative path**
as the source file you want to override:

```bash
# Source:  examples/vite-overlay/src/App.tsx
# Overlay: /tmp/vcs-sessions/agent-dashboard/src/App.tsx

mkdir -p /tmp/vcs-sessions/agent-dashboard/src
cp src/App.tsx /tmp/vcs-sessions/agent-dashboard/src/App.tsx
# edit it…
```

→ Only `agent-dashboard`'s browser receives the HMR update.
  Other sessions continue to see the real `App.tsx`.

To remove an overlay, delete the file — the session falls back to disk.

## Plugin API

```js
import { sessionOverlay } from './plugins/session-overlay.js'

sessionOverlay({
  sessionId: 'agent-auth',      // shown in session chip + logs
  overlayDir: '/tmp/vcs-sessions/agent-auth', // where overlays live
})
```

### Dev-server endpoints added by the plugin

| Endpoint | Description |
|---|---|
| `GET /__vcs_session` | JSON: `{ sessionId, overlayDir, overlayFiles }` |
| `GET /__vcs_session/events` | SSE stream: overlay file list, 1/sec |

## Connecting to the vcs store

The overlay directory is a natural sink for vcs checkouts:

```bash
# Materialize an agent's stack to its overlay dir
vcs view checkout <view-id> --worktree /tmp/vcs-sessions/agent-auth
# → Vite picks up the changes, HMR fires for agent-auth only
```

Alternatively, use `vcs watch` to auto-commit saves FROM the overlay dir
back into the vcs store:

```bash
vcs watch . --stack <stack-id> --dir /tmp/vcs-sessions/agent-auth
```

## How HMR is modified

Vite's default HMR fires for every file-system write via `chokidar`. This
plugin:

1. Adds the overlay directory to the file watcher
2. On `change`/`add`/`unlink` events from the overlay dir:
   - Finds the corresponding module in Vite's module graph
   - Calls `server.moduleGraph.invalidateModule(mod)`
   - Sends a `js-update` HMR message to connected clients
3. Overlay updates only reach clients connected to THIS Vite instance

```
overlayDir write
  └─ chokidar event
      └─ invalidateModule(projectFile)
          └─ server.ws.send({ type: 'update' })  ← only THIS server's clients
```

Other agent sessions run on separate Vite instances with separate WebSocket
connections — they never receive each other's HMR messages.
