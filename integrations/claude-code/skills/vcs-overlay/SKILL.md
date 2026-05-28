---
name: vcs-overlay
description: >-
  How to work in a project that uses vcs-overlay — per-agent file overlays for
  multi-session Vite dev. Use whenever the repo has a .vcs-overlay/ directory, a
  vcs-overlay dependency, or sessionOverlay() in vite.config. Covers editing
  through your session overlay (not the shared source tree), and the
  status/diff/snapshot/promote/checkout CLI workflow.
when_to_use: >-
  Editing files in a repo that runs multiple agent sessions over one source
  tree; the user mentions overlays, sessions, /tmp/vcs-sessions, promote, or
  vcs-overlay; or you see sessionOverlay() in vite.config.
---

# Working with vcs-overlay

`vcs-overlay` lets several agents share **one source tree**, each with their own
virtual file overlay — no git branches, no worktrees, no port conflicts. Your
edits live in *your* overlay dir and only *your* browser sees them, until you
explicitly **promote** them onto the shared source tree.

## The one rule that matters

**Do not edit the shared source files directly when working in a session.**
Write your modified copy into your overlay dir at the *same relative path*. The
Vite plugin serves it to your browser via HMR; the real file is untouched.

```
Source file:   <project>/src/App.tsx
Your overlay:  /tmp/vcs-sessions/<session-id>/src/App.tsx   ← write here
```

## Step 1 — know your session id

Each session has a stable id and an overlay dir at
`/tmp/vcs-sessions/<session-id>/`. Establish it once:

```bash
# Recommended: start the dev server with an explicit id
VCS_AGENT_ID=claude npm run dev      # → overlay dir /tmp/vcs-sessions/claude/
```

If you didn't start the server, discover live sessions:

```bash
npx vcs-overlay status               # lists every session + file counts
```

Throughout this skill, replace `<id>` with your session id.

## Step 2 — make a change

To change `src/Foo.tsx`, write the full new file to your overlay:

```bash
mkdir -p /tmp/vcs-sessions/<id>/src
# write the modified file to /tmp/vcs-sessions/<id>/src/Foo.tsx
```

Vite fires HMR for only your browser. Other sessions are unaffected. Remember:
React components need a `data-testid` on every interactive element
(`<feature>-<element>`).

## Step 3 — the CLI (run from the project root)

```bash
npx vcs-overlay status                       # all sessions, file counts, collisions
npx vcs-overlay diff <id>                     # unified diff: your overlay vs source
npx vcs-overlay snapshot <id> --reason "msg"  # checkpoint your overlay (content-addressed)
npx vcs-overlay log [<id>]                    # commit history
npx vcs-overlay promote <id>                  # copy overlay → real source (the "accept" step)
npx vcs-overlay checkout <id> <commit-id>     # restore overlay from a past snapshot
npx vcs-overlay discard <id>                  # clear the overlay dir
```

Add `--json` to any command for machine-readable output.

## Typical workflow

1. `snapshot` early and often — each one is a verifiable, content-addressed
   checkpoint in `.vcs-overlay/log.jsonl`.
2. `diff` to review exactly what you changed against the shared source.
3. `promote` when the change is good — it snapshots, then copies your overlay
   files onto the real source tree. **The overlay dir is not cleared**, so you
   keep working from the same state.
4. `checkout <commit-id>` to roll back: it re-materializes any past snapshot
   into your overlay dir and HMR fires automatically.

## Mental model: this sits on top of git, not instead of it

The overlay handles the ephemeral "N agents editing at once" problem. Git still
owns permanent history. The flow is:

```
write to overlay → snapshot → diff → promote → (you) git commit → push
```

After `promote`, the change is in the working tree like any normal edit —
commit and push it with git as usual.

## Quick reference

| You want to… | Do this |
|---|---|
| See who's working and on what | `npx vcs-overlay status` |
| Edit a file | write it to `/tmp/vcs-sessions/<id>/<relpath>` |
| Review your changes | `npx vcs-overlay diff <id>` |
| Save a checkpoint | `npx vcs-overlay snapshot <id> --reason "…"` |
| Land changes on the source tree | `npx vcs-overlay promote <id>` |
| Undo / go back to a snapshot | `npx vcs-overlay checkout <id> <commit-id>` |
| Throw away the overlay | `npx vcs-overlay discard <id>` |
