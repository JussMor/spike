# vcs-overlay — OpenAI Codex integration

Teaches OpenAI Codex how to work in a project that uses
[`vcs-overlay`](../../packages/vcs-overlay).

Codex (and any agent following the [agents.md](https://agents.md) standard)
reads instructions from `AGENTS.md` files — the repository root file applies to
the whole project, and nested files apply to their subtrees.

## Install

Pick whichever fits your project:

**Option A — your project has no AGENTS.md yet**

Copy this file to your project root:

```bash
cp integrations/codex/AGENTS.md /path/to/your-project/AGENTS.md
```

**Option B — your project already has an AGENTS.md**

Append the `# Working with vcs-overlay` section from
[`AGENTS.md`](./AGENTS.md) into your existing root `AGENTS.md`. Codex merges
all instructions in the file.

**Option C — scope it to a subdirectory**

Drop the file into the subtree Codex should apply it to, e.g.
`apps/web/AGENTS.md`. Codex uses the most specific file for a given path.

## What Codex will do

With the instructions in place, Codex:

1. Writes edits to its **session overlay dir** (`/tmp/vcs-sessions/<id>/<relpath>`)
   rather than the shared source tree.
2. Uses `npx vcs-overlay diff/snapshot/promote/checkout` to review, checkpoint,
   land, and roll back changes.
3. Treats `promote` as the step that lands changes on the real source tree —
   after which normal `git commit` / `push` applies.

## Verify Codex picked it up

Ask Codex: *"How should I edit files in this project?"* — it should describe
writing to the overlay dir and using `vcs-overlay promote`, not editing source
files directly.
