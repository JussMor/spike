<!--
  vcs-overlay instructions for OpenAI Codex.

  Codex reads AGENTS.md from the repository root (and nested directories).
  Copy the section below into your project's root AGENTS.md, or use this file
  as the root AGENTS.md directly. See ./README.md for install options.
-->

# Working with vcs-overlay

This project uses **vcs-overlay**: several agents share one source tree, each
with their own virtual file overlay. Your edits live in *your* overlay dir and
only *your* browser sees them, until you explicitly **promote** them onto the
shared source tree. No git branches, no worktrees, no port conflicts.

## The one rule that matters

**Do not edit the shared source files directly while working in a session.**
Write your modified copy into your overlay dir at the *same relative path*. The
Vite plugin serves it to your browser via HMR; the real file is untouched.

```
Source file:   <project>/src/App.tsx
Your overlay:  /tmp/vcs-sessions/<session-id>/src/App.tsx   ← write here
```

## Establish your session id

Each session has an overlay dir at `/tmp/vcs-sessions/<session-id>/`. Start the
dev server with an explicit id so you know yours:

```bash
VCS_AGENT_ID=codex npm run dev       # → overlay dir /tmp/vcs-sessions/codex/
```

If you did not start the server, list live sessions with
`npx vcs-overlay status`. Use your id wherever `<id>` appears below.

## Make a change

To change `src/Foo.tsx`, write the full new file to your overlay dir:

```bash
mkdir -p /tmp/vcs-sessions/<id>/src
# write the modified file to /tmp/vcs-sessions/<id>/src/Foo.tsx
```

Vite fires HMR for only your browser; other sessions are unaffected. Every
interactive React element needs a `data-testid` named `<feature>-<element>`.

## CLI (run from the project root)

```bash
npx vcs-overlay status                       # all sessions, file counts, collisions
npx vcs-overlay diff <id>                     # unified diff: your overlay vs source
npx vcs-overlay snapshot <id> --reason "msg"  # checkpoint your overlay
npx vcs-overlay log [<id>]                    # commit history
npx vcs-overlay promote <id>                  # copy overlay → real source (the "accept" step)
npx vcs-overlay checkout <id> <commit-id>     # restore overlay from a past snapshot
npx vcs-overlay discard <id>                  # clear the overlay dir
```

Add `--json` to any command for machine-readable output.

## Workflow

1. `snapshot` early and often — each is a content-addressed checkpoint.
2. `diff` to review what you changed against the shared source.
3. `promote` when the change is good — it snapshots, then copies your overlay
   onto the real source tree. The overlay dir is **not** cleared.
4. `checkout <commit-id>` to roll back to any past snapshot (HMR fires).

## This sits on top of git, not instead of it

The overlay handles the ephemeral "N agents editing at once" problem; git owns
permanent history. After `promote`, the change is in the working tree like any
normal edit — then `git commit` and `push` as usual.

```
write to overlay → snapshot → diff → promote → git commit → push
```
