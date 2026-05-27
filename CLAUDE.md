# vcs-spike — Claude Code instructions

This project uses **vcs-spike** for structured, conflict-aware change tracking.
You have vcs tools available via MCP. Use them for every file modification.

## The rule: always go through vcs

```
❌  Write("src/foo.ts", content)         ← writes to disk, invisible to other agents
✅  vcs_edit_from_disk({ stack_id, path: "src/foo.ts", content, reason: "why" })
    ← reads disk as base, stores touched file in .vcs, never writes disk
```

## Your workflow on every task (multi-session aware)

```
0. vcs_session_open({ agent_id: "claude-code-<task-slug>" })
   → register THIS session. Save session_id for the whole chat.

1. vcs_status()                          → check store + existing open stacks
   If not initialised: vcs_init()

   ⚠️  CHECK open_stacks in the response.
   If open_stacks is non-empty, other sessions have work in progress:

   a) Call vcs_overview() to see the full picture — which agents, which files
   b) Ask the user: "Session <agent_id> has an open stack touching <files>.
      Should I (A) merge/check for conflicts, (B) abandon it, or (C) ignore it?"
   c) If A — vcs_view_open({ stack_ids: [existing..., myStack] })
             then vcs_view_conflicts and REPORT — never resolve silently
   d) If B — vcs_stack_abandon({ stack_id: orphanedId })

2. vcs_stack_open({ agent_id: "...", session_id })
   → stack auto-linked to the session

3. For each file you create or modify:
   vcs_edit_from_disk({ stack_id, path, content, reason: "precise reason" })
   → after EACH edit, check vcs_touching({ path, stack_id })
   → if other_stacks is non-empty, IMMEDIATELY tell the user:
     "⚡ <other-agent> is also editing <path> — conflict likely on merge"

4. vcs_stack_close({ stack_id })         → when task is done
   vcs_session_close({ session_id })     → deregister this session

5. On cancellation or error:
   vcs_stack_abandon({ stack_id })       → mark stack dead
   vcs_session_close({ session_id })     → always close the session
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

## History Navigation

Do not create checkout directories just to inspect old state. Disk materialization is a
human/export operation. For agent rollback analysis, open a view at the target change and
read files from the store:

```js
const { view_id } = await vcs_view_open({ base_change_id: changeId, stack_ids: [] })
const { files } = await vcs_view_files({ view_id })
const file = await vcs_view_read({ view_id, path: "src/file.ts" })
```

Checkout/materialization is not exposed through the agent MCP. If the user asks
for an export, tell them it must be done explicitly through the CLI.

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
# Session lifecycle (multi-session)
vcs session open --agent my-agent --json    # register session → session_id
vcs session close <session_id>             # done
vcs session ls --json                      # list all sessions

# Overview — see every agent, every file, every collision
vcs overview --json                        # full multi-agent picture

# Per-file collision check (after each edit)
vcs touching src/foo.ts --stack <stack_id> --json

# Stack and edit
vcs init
vcs stack open --agent my-agent --json
vcs edit <stack> src/foo.ts \
  --content-file /tmp/foo.ts \
  --reason "why"
vcs stack ls --status open --json         # list open stacks

# Merge and conflicts
vcs view open --base "" --stacks a,b --json
vcs view conflicts <view-id> --json

# Remote
vcs serve --port 7474                      # hub for multi-project
vcs remote add hub http://localhost:7474
vcs push hub && vcs pull hub
```
