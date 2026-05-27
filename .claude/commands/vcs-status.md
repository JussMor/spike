# /vcs-status — Show current vcs state

Shows the live state of the vcs store: stacks, recent changes, and any conflicts.

## Steps

1. Call `vcs_status` — confirm initialised
2. For each open stack, call `vcs_log` to show recent changes
3. If a view exists, call `vcs_view_conflicts` and surface any unresolved conflicts
4. Print a summary table:

```
vcs store: .vcs/  ✓ initialised

Open stacks:
  claude-code-add-login   abc12345…  3 changes
  agent-dashboard         def67890…  2 changes

Recent changes:
  edit  src/LoginForm.tsx   "add email/password fields"
  edit  src/App.tsx         "wire up login route"

Conflicts: none ✓
```

## Useful for

- Checking what's been tracked so far
- Seeing what other agents have done
- Spotting conflicts before merging
