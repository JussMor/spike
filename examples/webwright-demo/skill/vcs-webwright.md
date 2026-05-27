# vcs-spike skill — Webwright edition

Use this skill when you are a Webwright agent working on a code task.
Track every file you write through vcs so the orchestrator has a structured
change log, conflict detection, and full audit trail.

## Rule 1 — Never write to disk directly

Wrong:
```python
with open('src/LoginForm.tsx', 'w') as f:
    f.write(content)
```

Right:
```python
adapter.write('src/LoginForm.tsx', content, reason="implement login form")
```

Or from the CLI:
```bash
vcs edit $STACK src/LoginForm.tsx --content-file /tmp/loginform.tsx \
  --reason "implement login form" \
  --task-ref task-login \
  --tool-call '{"name":"write_file","path":"src/LoginForm.tsx"}'
```

## Rule 2 — Every component needs data-testid

Every interactive element you write MUST have a `data-testid` attribute.
This is what makes e2e tests stable across agent refactors.

```tsx
// ✓ correct
<form data-testid="login-form">
  <input data-testid="login-email" />
  <input data-testid="login-password" />
  <button data-testid="login-submit">Sign in</button>
  <p data-testid="login-error">{error}</p>
</form>

// ✗ wrong — no testid, brittle selector
<form className="login">
  <input type="email" />
```

**Naming convention**: `<feature>-<element>`, e.g.:
- `login-form`, `login-email`, `login-submit`, `login-error`
- `dashboard-header`, `dashboard-changes-list`, `change-item`
- `register-form`, `register-name`, `register-submit`

## Rule 3 — Playwright specs use ONLY data-testid

```typescript
// ✓ stable across refactors
page.getByTestId('login-submit')
page.getByTestId('login-error')

// ✗ brittle — breaks when agent refactors CSS or structure
page.locator('.login-btn')
page.locator('button[type=submit]')
page.locator('form > button:last-child')
```

## Rule 4 — task_ref links everything

Every `vcs edit` must include `--task-ref <task-id>`.
This links your changes to the orchestrator task so conflicts
can be routed back to the right agent.

## Rule 5 — Stop on conflict, report to orchestrator

```bash
CONFLICTS=$(vcs view conflicts $VIEW --json)
if [ $(echo $CONFLICTS | jq 'length') -gt 0 ]; then
  echo "CONFLICT_DETECTED"
  echo $CONFLICTS | jq .
  exit 1  # orchestrator handles resolution
fi
```

Never resolve conflicts yourself. You don't have the full picture.

## Lifecycle template

```bash
# 1. Begin task
STACK=$(vcs stack open --agent $AGENT_ID --base $BASE_TIP --json | jq -r .stack_id)

# 2. Write files (repeat for each file you produce)
vcs edit $STACK src/features/auth/LoginForm.tsx \
  --content-file /tmp/loginform.tsx \
  --reason "implement login form with email+password" \
  --task-ref $TASK_ID

# 3. Write Playwright spec (always with data-testid selectors)
vcs edit $STACK e2e/tests/auth/login.spec.ts \
  --content-file /tmp/login.spec.ts \
  --reason "e2e spec for login form — stable data-testid selectors" \
  --task-ref $TASK_ID

# 4. Done
vcs stack close $STACK
echo "STACK_ID=$STACK"
```
