# /vcs-abort — Abandon a vcs stack on error

Call this if the task failed or was cancelled mid-way.
Marks the stack as abandoned — it will not appear in future views.

## Steps

1. Call `vcs_stack_abandon` with the current stack_id
2. Confirm: "✗ Stack abandoned: <stack_id>"
3. No changes from this session will appear in any merged view

## When to use

- Task failed partway through
- User cancelled the operation
- You made a mistake and want to start fresh

After aborting, you can start a new task cleanly with `/vcs-start`.
