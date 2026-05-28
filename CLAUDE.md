# vite-overlay — Claude Code instructions

## What this repo is

A Vite plugin (`examples/vite-overlay/plugins/session-overlay.js`) that lets multiple agent sessions share one source tree, each with their own virtual file overlay — without port conflicts, git branches, or worktrees.

## Running

```bash
cd examples/vite-overlay
npm install
npm run dev      # single session
npm run agents   # multi-session (OS-assigned ports)
```

## Key files

| File | Role |
|------|------|
| `plugins/session-overlay.js` | Core plugin — `load()` hook + HMR + SSE |
| `scripts/start-agents.mjs` | Portless multi-session launcher |
| `src/OverlayPanel.tsx` | Live overlay state via SSE |
| `vite.config.ts` | Single-session config using env vars |

## Plugin API

```js
import { sessionOverlay } from './plugins/session-overlay.js'

sessionOverlay({
  sessionId: 'agent-auth',
  overlayDir: '/tmp/vcs-sessions/agent-auth',
})
```

## Overlay mechanics

- Only modified files live in the overlay dir
- `load()` returns `undefined` for files not in the overlay → Vite reads from disk
- Each session runs a separate Vite instance → separate WebSocket → HMR cannot bleed between sessions
- `port: 0` → OS assigns any free port → no conflicts

## React components — data-testid required

Every interactive element must have a `data-testid`:

```tsx
<button data-testid="login-submit">Sign in</button>
```

Convention: `<feature>-<element>`

## Running tests

```bash
cd examples/vite-overlay && npm run build   # tsc + rollup
```
