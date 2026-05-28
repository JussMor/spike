# vcs-overlay — Claude Code plugin

Teaches Claude Code how to work in a project that uses
[`vcs-overlay`](../../packages/vcs-overlay): per-agent file overlays with
content-addressed history for multi-session Vite development.

## What it adds

| Component | Name | Purpose |
|---|---|---|
| Skill | `/vcs-overlay:vcs-overlay` | Full workflow: edit through your overlay, snapshot, diff, promote, checkout. Auto-loads when Claude detects an overlay project. |
| Command | `/vcs-overlay:status` | Quick live view of active sessions and collisions. |

## Install

From this git repo (recommended):

```
/plugin marketplace add JussMor/spike
/plugin install vcs-overlay@spike
```

For local development against a checkout:

```bash
claude --plugin-dir ./integrations/claude-code
```

Then `/reload-plugins` to activate. Verify with `claude plugin validate ./integrations/claude-code`.

## How Claude uses it

Once installed, when Claude is working in a repo that has a `.vcs-overlay/`
directory, a `vcs-overlay` dependency, or `sessionOverlay()` in its Vite config,
the skill activates and Claude will:

1. Write edits to its **session overlay dir** (`/tmp/vcs-sessions/<id>/<relpath>`)
   instead of the shared source tree.
2. Use `npx vcs-overlay diff/snapshot/promote/checkout` to review, checkpoint,
   land, and roll back changes.
3. Treat `promote` as the step that lands changes on the real source tree —
   after which normal `git commit` / `push` applies.
