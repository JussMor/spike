# vcs-spike skill

When working on a code task, use `vcs` to track your edits instead of
writing files directly.  This gives the orchestrator structured change
events and makes your work composable with other parallel agents.

## Prerequisites

The `vcs` binary must be on PATH, or set `VCS_BIN` to its absolute path.
The store path defaults to `~/.vcs-spike/`; override with `VCS_STORE_PATH`
or the `--store` flag.

## Lifecycle

```bash
# 1. Open a stack at the start of every task
STACK=$(vcs stack open --agent <your-agent-id> [--base <base_change_id>])

# 2. For every file change, record it with a clear reason
CHANGE=$(vcs edit $STACK <path> --content-file <file> --reason "what and why" \
                [--task-ref <orchestrator-task-id>])

# 3. When done, close the stack
vcs stack close $STACK
```

Never abandon a stack unless the work is definitively thrown away.

## Reading files

Never read files from the filesystem directly.  Open a view and read
through it so you see the merged state including your own edits:

```bash
VIEW=$(vcs view open --base <base_change_id> --stacks <your_stack_id>)
vcs view read $VIEW <path>
vcs view ls $VIEW
```

If other agents' stacks should be visible in your view, list them all:

```bash
VIEW=$(vcs view open --base <base> --stacks <stack-a>,<stack-b>,<your-stack>)
```

## Intents

Always pass `--reason`.  If your edit came from a specific orchestrator
task, also pass `--task-ref`.  The orchestrator routes follow-up work by
querying these fields.

```bash
vcs edit $STACK src/api.rs \
  --content-file /tmp/api.rs \
  --reason "add /users endpoint for task auth-42" \
  --task-ref "task-auth-42"
```

## Conflicts

If `vcs view conflicts $VIEW` returns anything, **stop and report**.
Do not try to resolve conflicts yourself.  The orchestrator decides.

```bash
# Check for conflicts before reading through a view
CONFLICTS=$(vcs view conflicts $VIEW --json)
if [ "$(echo $CONFLICTS | jq 'length')" -gt 0 ]; then
  echo "CONFLICTS:" && echo $CONFLICTS | jq .
  exit 1   # surface to orchestrator
fi
```

## JSON mode

Every command supports `--json` for machine-readable output.  Use it when
driving from Node.js, Python, or another agent layer:

```bash
vcs --json stack open --agent my-agent | jq -r .stack_id
vcs --json view conflicts $VIEW | jq '.[].path'
```

## Worked example: single agent

```bash
# Initialise (idempotent)
vcs init

# Open a stack
STACK=$(vcs --json stack open --agent claude-sub-01 | jq -r .stack_id)

# Write a file
echo 'fn main() {}' > /tmp/main.rs
CHANGE=$(vcs --json edit $STACK src/main.rs \
  --content-file /tmp/main.rs \
  --reason "scaffold main" \
  --task-ref task-001 | jq -r .change_id)

# Open a view at tip
VIEW=$(vcs --json view open --base $CHANGE --stacks $STACK | jq -r .view_id)

# Read back
vcs view read $VIEW src/main.rs

# Done
vcs stack close $STACK
```

## Worked example: parallel agents (orchestrator side)

```bash
# Seed base
SEED_STACK=$(vcs --json stack open --agent seed | jq -r .stack_id)
BASE=$(echo '{}' | vcs --json edit $SEED_STACK config.json \
  --stdin --reason "initial" | jq -r .change_id)
vcs stack close $SEED_STACK

# (Agent A and Agent B work in parallel, each with their own stack)
# Agent A opens: STACK_A=$(vcs stack open --agent agent-a --base $BASE)
# Agent B opens: STACK_B=$(vcs stack open --agent agent-b --base $BASE)
# ... both make edits ...

# Orchestrator merges
VIEW=$(vcs --json view open --base $BASE --stacks "$STACK_A,$STACK_B" | jq -r .view_id)
vcs view conflicts $VIEW --json   # resolve any conflicts here
vcs view ls $VIEW
```

## Worked example: resolving a conflict

```bash
CONFLICTS=$(vcs --json view conflicts $VIEW)
CONFLICT_ID=$(echo $CONFLICTS | jq -r '.[0].conflict_id')
WINNING_STACK=$(echo $CONFLICTS | jq -r '.[0].candidates[0].stack_id')

# Pick a winner
vcs view resolve $CONFLICT_ID --pick $WINNING_STACK

# Or supply merged content
vcs view resolve $CONFLICT_ID --merge-file /tmp/merged.json
```

## Quick reference

| Command | Description |
|---|---|
| `vcs init` | Initialise store |
| `vcs stack open --agent <id>` | Open a new stack |
| `vcs stack close <id>` | Close a stack (work done) |
| `vcs stack abandon <id>` | Discard a stack |
| `vcs edit <stack> <path> --content-file <f> --reason <r>` | Record edit |
| `vcs delete <stack> <path> --reason <r>` | Record deletion |
| `vcs view open --base <cid> --stacks <s1,s2>` | Open a view |
| `vcs view read <view> <path>` | Read file through view |
| `vcs view ls <view>` | List files in view |
| `vcs view conflicts <view>` | List conflicts |
| `vcs view resolve <cid> --pick <stack>` | Resolve by picking |
| `vcs log <stack>` | Change log for stack |
| `vcs history` | Full change history across stacks |
| `vcs diff <from> <to>` | Diff two change IDs |
| `vcs checkout <change_id> --worktree <dir>` | Materialize tracked state for replay/testing |
| `vcs push <remote-url-or-name> --project-id <id>` | Push structured agent store to hub |
| `vcs pull <remote-url-or-name>` | Pull structured agent store from hub |
