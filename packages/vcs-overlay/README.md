# vcs-overlay

Per-agent file overlays for Vite, with a content-addressed history CLI. Lets
several agents share **one source tree** — each with their own virtual file
overlay — without git branches, worktrees, or port conflicts.

- **Plugin** (`vcs-overlay/plugin`) — a Vite plugin whose `load()` hook serves a
  session's overlay file if it exists, otherwise the real disk file.
- **Store** (`vcs-overlay/store`) — content-addressed snapshot store (SHA-256
  blobs + append-only `log.jsonl` in `.vcs-overlay/`).
- **CLI** (`vcs-overlay`) — `status` / `diff` / `snapshot` / `promote` /
  `checkout` over the store. Zero runtime dependencies beyond Vite.

## Install

```bash
npm install vcs-overlay        # published
# or, from a local checkout:
npm install /path/to/packages/vcs-overlay
```

`vite` is a peer dependency (>=5), so your project supplies it.

## 1. Wire the plugin into `vite.config.ts`

```ts
import { defineConfig } from 'vite'
import { sessionOverlay } from 'vcs-overlay/plugin'
import path from 'node:path'
import os from 'node:os'

// Stable, known id per session. Set VCS_AGENT_ID when you start the server
// (e.g. VCS_AGENT_ID=claude npm run dev); falls back to the process PID.
const agentId = process.env.VCS_AGENT_ID ?? `s${process.pid}`
const overlayDir =
  process.env.VCS_OVERLAY_DIR ?? path.join(os.tmpdir(), 'vcs-sessions', agentId)

export default defineConfig({
  plugins: [
    sessionOverlay({ sessionId: agentId, overlayDir }),
    // ...your other plugins (react(), etc.)
  ],
})
```

Start a session with an explicit id so you know its overlay dir:

```bash
VCS_AGENT_ID=claude npm run dev      # → overlay dir /tmp/vcs-sessions/claude/
```

## 2. Make a change through the overlay

To override `src/App.tsx` for your session, write the modified file into your
overlay dir at the **same relative path** — the real source file is untouched:

```bash
mkdir -p /tmp/vcs-sessions/claude/src
# write the new file to /tmp/vcs-sessions/claude/src/App.tsx
```

Only your session's browser hot-reloads. Delete the overlay file to fall back to
disk.

## 3. Track, review, land, and roll back — the CLI

Run from your project root (the store lives at `<cwd>/.vcs-overlay/`):

```bash
npx vcs-overlay status                        # sessions, file counts, collisions
npx vcs-overlay diff <id>                      # unified diff: overlay vs source
npx vcs-overlay snapshot <id> --reason "msg"   # content-addressed checkpoint
npx vcs-overlay log [<id>]                      # commit history
npx vcs-overlay promote <id>                    # copy overlay → real source ("accept")
npx vcs-overlay checkout <id> <commit-id>       # restore overlay from a snapshot
npx vcs-overlay discard <id>                    # clear the overlay dir
```

Add `--json` to any command for machine-readable output. `promote` does **not**
clear the overlay — you keep working from the same state.

## 4. Multi-session launcher (optional)

To run N sessions at once, each on an OS-assigned port, create one Vite server
per session with `configFile: false` and an inline `sessionOverlay()`. See
[`examples/vite-overlay/scripts/start-agents.mjs`](../../examples/vite-overlay/scripts/start-agents.mjs)
for a complete launcher (`VCS_N=3 node start-agents.mjs`).

## 5. Ignore the store in git

```gitignore
.vcs-overlay/
```

## How it relates to git

The overlay handles the ephemeral "N agents editing at once" problem; git owns
permanent history. The flow is:

```
write to overlay → snapshot → diff → promote → git commit → push
```

After `promote`, the change is in your working tree like any normal edit.

## Teaching agents to use it

Drop-in agent instructions live in
[`integrations/`](../../integrations): a Claude Code plugin and an OpenAI Codex
`AGENTS.md`. Both teach an agent to edit through its overlay and use this CLI.

## API

```ts
import { sessionOverlay } from 'vcs-overlay/plugin'
import { snapshot, readLog, readBlob, scanDir, storeDir } from 'vcs-overlay/store'
```

| Export | Purpose |
|---|---|
| `sessionOverlay({ sessionId, overlayDir })` | Vite plugin |
| `snapshot(agentId, overlayDir, projectRoot, intent?, reason?)` | hash overlay → store blobs → append commit |
| `readLog(projectRoot, { agent? })` | commit records, ascending by ts |
| `readBlob(projectRoot, hash)` | raw blob bytes |
| `scanDir(dir)` | recursive relative file list |
| `storeDir(projectRoot)` | path to `.vcs-overlay/` |
