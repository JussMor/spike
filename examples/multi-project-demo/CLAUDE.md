# vcs — agent change tracking

This project uses **vcs** for structured, conflict-aware change tracking.
vcs tools are available via MCP (loaded from `.mcp.json`).

## The rule: always go through vcs

```
❌  Edit / Write tools          ← writes to disk, bypasses tracking
✅  vcs_edit_from_disk(...)     ← records change in .vcs, never writes disk
```

`vcs_edit_from_disk` is the default tool for all file edits.

## Your workflow on every task

```
0. vcs_session_open({ agent_id: "claude-code-<task-slug>" })
   → save session_id for the whole chat

1. vcs_status()
   → check for open stacks from other sessions
   → if open_stacks non-empty: call vcs_overview() first

2. vcs_stack_open({ agent_id: "...", session_id })
   → save stack_id

3. For each file you create or modify:
   vcs_edit_from_disk({ stack_id, path, content, reason: "precise reason" })
   → after each edit: vcs_touching({ path, stack_id })
   → if other_stacks non-empty: warn the user about the conflict

4. vcs_stack_close({ stack_id })
   vcs_session_close({ session_id })

5. On error or cancellation:
   vcs_stack_abandon({ stack_id })
   vcs_session_close({ session_id })
```

## What is a real collision?

A collision requires ALL THREE:
1. Two stacks touch the **same path**
2. Their content is **different** (different blob hashes)
3. You are about to **merge** them into one view

`vcs_touching` only warns when content ACTUALLY DIFFERS — no false positives.

## Conflicts

If `vcs_view_conflicts` returns conflicts, **stop and report** — never resolve silently.
