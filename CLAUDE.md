# vcs-spike — Claude Code instructions

This project uses **vcs-spike** for structured, conflict-aware change tracking.
You have vcs tools available via MCP. Use them for every file modification.

## The rule: always go through vcs

```
❌  Write("src/foo.ts", content)         ← writes to disk, invisible to other agents
✅  vcs_edit(stackId, "src/foo.ts", content, { reason: "why" })  ← tracked
```

## Your workflow on every task

```
1. vcs_status()                          → confirm store is initialised
   If not: vcs_init()

   ⚠️  CHECK open_stacks in the response.
   If open_stacks is non-empty, another session left work in progress:

   a) Ask the user: "Session <agent_id> has an open stack with unfinished work.
      Should I (A) include it in a conflict check, (B) abandon it, or (C) ignore it?"

   b) If A — open a view over all open stacks first:
        viewId = vcs_view_open({ stack_ids: [existing..., newStack] })
        check vcs_view_conflicts({ view_id: viewId })
        report any conflicts before proceeding

   c) If B — abandon the orphaned stack:
        vcs_stack_abandon({ stack_id: orphanedId })

   d) If C — proceed, but warn the user their previous work won't be merged.

2. vcs_stack_open({ agent_id: "claude-code-<task-slug>" })
   → save the returned stack_id for this entire task

3. For each file you create or modify:
   vcs_edit({ stack_id, path, content, reason: "precise reason" })
   vcs_delete({ stack_id, path, reason })
   vcs_rename({ stack_id, from, to, content, reason })

4. vcs_stack_close({ stack_id })         → when task is done

5. On cancellation or error:
   vcs_stack_abandon({ stack_id })       → mark stack dead, never close it
```

## Intent (the "reason" field)

Every edit requires a `reason`. Make it precise:

```
✅  "add login form component with email/password fields"
✅  "fix: handle null user in dashboard — FE-203"
✅  "refactor: extract AuthContext from App.tsx to reduce coupling"
❌  "update file"
❌  "make changes"
```

If the task came from an issue or PR, include `task_ref`:
```
vcs_edit({ ..., reason: "...", task_ref: "JIRA-123" })
```

## Conflicts

If `vcs_view_conflicts` returns conflicts, **stop and report** — do not silently pick a winner.
Conflicts are data. The human or orchestrator decides.

```
⚡ CONFLICT: src/features/auth/LoginForm.tsx
  candidate A: stack abc12345 (agent-ui)
  candidate B: stack def67890 (agent-api)

→ Report to user. Do NOT call vcs_resolve without explicit instruction.
```

## Multi-agent tasks

When you spawn sub-agents for parallel work:
- Each sub-agent gets its own `vcs_stack_open` call
- All stacks use the same store (`.vcs/` in this project)
- After all agents are done, open a merged view:

```js
const viewId = vcs_view_open({ stack_ids: [stackA, stackB, stackC] })
const conflicts = vcs_view_conflicts({ view_id: viewId })
// Report conflicts before proceeding
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
cargo test --workspace           # Rust unit tests (11 tests)
cd examples/tanstack-vite && npm run e2e   # Playwright e2e
```

## Useful vcs commands (CLI reference)

```bash
vcs init                                    # initialise .vcs/
vcs stack open --agent my-agent --json      # open stack
vcs edit <stack> src/foo.ts \
  --content-file /tmp/foo.ts \
  --reason "why"                            # record edit
vcs view open --base "" --stacks a,b --json # merge two stacks
vcs view conflicts <view-id> --json         # check conflicts
vcs serve --port 7474                       # start hub for multi-project
```
