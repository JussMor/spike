# vite-overlay

Portless multi-agent HMR via per-session file overlays — no port conflicts, no repo copies, no git branches.

## The problem

Running N dev servers for N agents normally means hardcoded ports, full repo copies per worktree, or git branch overhead.

## The solution

Each agent session gets:
- An **OS-assigned port** (`port: 0` — no conflicts, no config)
- Its own **overlay directory** (`/tmp/vcs-sessions/<agent-id>/`)
- **Targeted HMR**: only that session's browser updates when its overlay changes

The overlay dir holds **only the files the agent changed**. Everything else is served from the real source tree via Vite's `load()` hook fallthrough.

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

## Quickstart

```bash
cd examples/vite-overlay
npm install

# Single session
npm run dev

# Multi-session (2 agents, OS-assigned ports)
npm run agents
```

## How overlays work

Place a file in the agent's overlay directory with the same relative path as the source file you want to override:

```bash
mkdir -p /tmp/vcs-sessions/agent-dashboard/src
cp examples/vite-overlay/src/App.tsx /tmp/vcs-sessions/agent-dashboard/src/App.tsx
# edit it — only agent-dashboard's browser updates
```

To remove an overlay, delete the file — the session falls back to disk.

## Repository layout

```
examples/vite-overlay/
├── plugins/
│   └── session-overlay.js    Vite plugin — load() hook + HMR + SSE endpoint
├── scripts/
│   └── start-agents.mjs      Portless multi-session launcher
└── src/
    ├── App.tsx
    └── OverlayPanel.tsx      Live overlay file list via SSE
```
