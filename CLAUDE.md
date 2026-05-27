# vcs-spike — Claude Code instructions

This project uses **vcs-spike** for structured, conflict-aware change tracking.
You have vcs tools available via MCP. Use them for every file modification.

## The rule: always go through vcs

```
❌  Write("src/foo.ts", content)         ← writes to disk, bypasses change tracking
✅  vcs_edit_from_disk({ stack_id, path: "src/foo.ts", content, reason: "why" })
    ← reads disk as base, records change in .vcs, never writes disk
```

`vcs_edit_from_disk` is the **default tool for all edits**.  It is safe for
existing files because it seeds the current disk content as the base before
recording your new content.  This means the 3-way merge at conflict resolution
time has all the context it needs.

## What is a collision?

A **collision** (real conflict) requires ALL THREE:
1. Two stacks touch the **same path**
2. Their content is **different** (different blob hashes)
3. You are about to **merge** them into one view

```
NOT a collision ✓:
  • Two agents write identical content to the same file  (same blob hash)
  • One agent edits a file, the other never touched it  (auto-resolves)
  • Two agents working on totally different files        (no overlap)
  • Same session, same stack, multiple edits to one file (just history)

IS a collision ⚠:
  • Two open stacks, same file, different content, being merged
```

`vcs_touching` (pre-merge) only warns when content ACTUALLY DIFFERS.
The final verdict is `vcs_view_conflicts` — which only fires at merge time.

## Source of truth

**The `.vcs/` store is the single source of truth.  Disk is a read-only
materialisation of the store, not an authority.**

- Agents read/write through the store (never through disk for tracked files)
- Dev servers serve files from the store (vcs-vite `load()` hook)
- Disk state only matters for files not yet tracked in any stack

## Your workflow on every task (multi-session aware)

```
0. vcs_session_open({ agent_id: "claude-code-<task-slug>" })
   → register THIS session. Save session_id for the whole chat.

1. vcs_status()                          → check store + existing open stacks
   If not initialised: vcs_init()

   ⚠️  CHECK open_stacks in the response.
   If open_stacks is non-empty, other sessions have work in progress:

   a) Call vcs_overview() to see the full picture — which agents, which files
   b) Only act if files OVERLAP (same path, different content).
      If different files: ignore — no conflict possible.
      If same file, same content: ignore — idempotent, no real conflict.
      If same file, different content: ask the user what to do.

   c) If resolving: vcs_view_open({ stack_ids: [existing..., myStack] })
             then vcs_view_conflicts and REPORT — never resolve silently
   d) If abandoning orphaned session: vcs_stack_abandon({ stack_id: orphanedId })

2. vcs_stack_open({ agent_id: "...", session_id })
   → stack auto-linked to the session

3. For each file you create or modify:
   vcs_edit_from_disk({ stack_id, path, content, reason: "precise reason" })
   → after EACH edit, check vcs_touching({ path, stack_id })
   → if other_stacks is non-empty (content DIFFERS from others), tell the user:
     "⚡ <other-agent> has a DIFFERENT version of <path> — will conflict on merge"
   → if other_stacks is empty: safe, continue

4. vcs_stack_close({ stack_id })         → when task is done
   vcs_session_close({ session_id })     → deregister this session

5. On cancellation or error:
   vcs_stack_abandon({ stack_id })       → mark stack dead
   vcs_session_close({ session_id })     → always close the session
```

## Sessions are independent by default

Two sessions working on **different tasks** are independent — they share the
store but do not need to coordinate unless their stacks are merged.

```
Session A (agent-auth)  edits: src/auth.ts, src/login.tsx
Session B (agent-api)   edits: src/api.ts, src/types.ts
→ Zero collision risk.  Do NOT report warnings for this pattern.

Session A (agent-auth)  edits: src/types.ts  (adds AuthUser type)
Session B (agent-api)   edits: src/types.ts  (adds ApiResponse type)
→ POTENTIAL collision. Check content with vcs_touching.
   If different content → warn. If they arrived at the same content → fine.
```

## Conflicts

If `vcs_view_conflicts` returns conflicts, **stop and report** — do not silently pick a winner.
Conflicts are data. The human or orchestrator decides.

```
⚡ CONFLICT: src/features/auth/LoginForm.tsx
  candidate A: stack abc12345 (agent-ui)   blob: a1b2c3d4
  candidate B: stack def67890 (agent-api)  blob: e5f6a7b8

→ Report to user. Do NOT call vcs_resolve without explicit instruction.
```

## History Navigation

Do not create checkout directories just to inspect old state. For agent rollback
analysis, open a view at the target change and read files from the store:

```js
const { view_id } = await vcs_view_open({ base_change_id: changeId, stack_ids: [] })
const { files } = await vcs_view_files({ view_id })
const file = await vcs_view_read({ view_id, path: "src/file.ts" })
```

## Multi-agent tasks

When you spawn sub-agents for parallel work:
- Each sub-agent gets its own `vcs_stack_open` call
- All stacks share the same store (`.vcs/` on this machine)
- After all agents complete, open a merged view:

```js
const viewId = vcs_view_open({ stack_ids: [stackA, stackB, stackC] })
const conflicts = vcs_view_conflicts({ view_id: viewId })
// Only report if conflicts.length > 0
```

## React components — data-testid required

Every interactive element in React components must have a `data-testid`:

```tsx
// ✅ correct — survives any refactor
<button data-testid="login-submit">Sign in</button>

// ❌ wrong — breaks on rename
<button className="btn-submit">Sign in</button>
```

Convention: `<feature>-<element>` → `login-submit`, `dashboard-header`, `change-item`

## Running tests

```bash
cargo test --workspace           # Rust unit tests (25 tests)
cd examples/tanstack-vite && npm run e2e   # Playwright e2e
```

## Package roles

| Package | What it does | When to use |
|---------|-------------|-------------|
| `vcs-mcp` | MCP server for Claude Code | AI agents via Claude Code |
| `vcs-openai` | OpenAI function definitions | AI agents via OpenAI/Codex |
| `vcs-dev-server` | Framework-neutral dev-server runtime | Wraps any dev server |
| `vcs-vite` | Vite-specific plugin (uses vcs-dev-server) | Vite projects |
| `vcs-npm` | npm wrapper for the `vcs` CLI binary | Node.js project installs |

## Useful vcs commands (CLI reference)

```bash
# Session lifecycle (multi-session)
vcs session open --agent my-agent --json    # register session → session_id
vcs session close <session_id>             # done
vcs session ls --json                      # list all sessions

# Overview — see every agent, every file, every collision
vcs overview --json                        # full multi-agent picture

# Per-file collision check (after each edit — content-aware)
# Returns empty other_stacks if content is identical — no false positives
vcs touching src/foo.ts --stack <stack_id> --json

# Stack and edit
vcs init
vcs stack open --agent my-agent --json
vcs edit <stack> src/foo.ts \
  --content-file /tmp/foo.ts \
  --reason "why"
vcs stack ls --status open --json         # list open stacks

# Human dev: auto-track file saves
vcs watch . --stack <id>                  # watch directory, commit on save

# Merge and conflicts
vcs view open --base "" --stacks a,b --json
vcs view conflicts <view-id> --json

# Remote hub (multi-machine, NOT git)
vcs serve --port 7474                      # hub for multi-project
vcs remote add hub http://localhost:7474
vcs push hub && vcs pull hub

# Maintenance
vcs gc                                     # free unreferenced blobs
```

## .vcs/ is gitignored — never commit it

`.vcs/` is your local agent store. Like `.git/`, it is in `.gitignore`.
To share state between machines: use `vcs push/pull` to a hub server.
Never `git add .vcs/`.
