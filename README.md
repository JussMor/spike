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

## Tracking & CLI

Snapshots, diffs, promotion, and rollback are handled by the `vcs-overlay`
CLI (content-addressed store in `.vcs-overlay/`, zero npm deps):

```bash
npx vcs-overlay status                       # sessions, file counts, collisions
npx vcs-overlay diff <id>                     # unified diff vs source
npx vcs-overlay snapshot <id> --reason "msg"  # content-addressed checkpoint
npx vcs-overlay promote <id>                  # land overlay onto the source tree
npx vcs-overlay checkout <id> <commit-id>     # restore overlay from a snapshot
```

## Agent integrations

Drop-in skills so coding agents know to edit through their overlay (not the
shared source tree) and use the CLI workflow:

- **Claude Code** — `integrations/claude-code/` (installable plugin):
  ```
  /plugin marketplace add JussMor/spike
  /plugin install vcs-overlay@spike
  ```
- **OpenAI Codex** — `integrations/codex/AGENTS.md` (copy into your project root).

## Repository layout

```
packages/vcs-overlay/         npm package: Vite plugin + content-addressed store + CLI
├── plugin.js                 sessionOverlay() — load() hook + HMR + SSE endpoint
├── store.mjs                 content-addressed snapshot store
└── cli.mjs                   vcs-overlay binary

examples/vite-overlay/        demo app
└── scripts/start-agents.mjs  portless multi-session launcher

integrations/
├── claude-code/              Claude Code plugin (skill + /status command)
└── codex/                    OpenAI Codex AGENTS.md instructions
```
