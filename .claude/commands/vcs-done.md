# /vcs-done — Finish a vcs-tracked task

Closes the current vcs stack and shows a summary.

## Steps

1. Call `vcs_stack_close` with the current stack_id
2. Call `vcs_log` to show what was recorded
3. Print the summary:
   ```
   ✓ Stack closed: <stack_id>
   Changes recorded: <N>
     • edit  src/foo.ts   "add login form"
     • edit  src/bar.ts   "update styles"
   ```

If there was an error or the task was cancelled, use `/vcs-abort` instead.
