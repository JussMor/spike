# /vcs-start — Begin a vcs-tracked task

Opens a vcs stack for this task. Call at the start of any file-editing session.

## Steps

1. Call `vcs_status` to confirm the store is ready
2. Call `vcs_stack_open` with `agent_id: "claude-code-$ARGUMENTS"` (use the task description)
3. Save the `stack_id` — pass it to every edit in this session
4. Confirm to the user: "✓ vcs stack opened: <stack_id>"

If the store is not initialised, run `vcs_init` first then retry.

## Example

```
/vcs-start add-login-form
```

Opens a stack with agent_id `claude-code-add-login-form`. Every file you edit
goes through `vcs_edit` with this stack_id until you call `/vcs-done`.
