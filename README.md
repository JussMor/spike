# vcs-spike

**Agent-native VCS — research spike.**

A Rust library + thin CLI that validates a change-event data model for
multi-agent version control.  No materializer.  No HTTP server.  No remotes.
Just: can agents produce structured changes, store them, query views, and
surface conflicts as data — and does the model hold up?

## Questions this spike must answer

1. **Model expressiveness** — can create / edit / delete / rename cover every
   realistic agent operation, or does the model need richer ops?
2. **SQLite speed** — is a single SQLite file (WAL mode) + content-addressed
   blob dir fast enough for concurrent multi-agent writes?
3. **Intent utility** — does intent metadata (`reason`, `tool_call`, `task_ref`)
   earn its weight, or is it dead data agents skip?
4. **View computation cost** — opening a view is O(changes in stacks); do we
   need cached materialized views, or is recompute cheap enough?

Answers at the bottom.

---

## Architecture

```
changes (append-only event log)
  └─ change_id = BLAKE3(parent_id | path | diff_hash | agent_id | ts)

stacks (ordered list of changes per agent)
  └─ base_change_id → tip_change_id

views (virtual merge of N stacks on top of a base)
  └─ read always goes through a view — never the raw blob dir

conflicts (first-class data objects, not error states)
  └─ orchestrator resolves; agents surface and stop

blobs (<store>/blobs/<2-char prefix>/<rest> — content-addressed, atomic writes)
```

Five SQLite tables (`changes`, `stacks`, `views`, `files_at_change`,
`conflicts`) plus a blob directory.  No git-shaped concepts: no branch, no
commit, no checkout.

---

## Repository layout

```
vcs-spike/
├── Cargo.toml                  # workspace
├── crates/
│   ├── vcs-core/               # the library
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── store.rs        # Store — all public API
│   │   │   ├── change.rs       # Change, ChangeId, Op, hashing
│   │   │   ├── stack.rs        # Stack, StackStatus
│   │   │   ├── view.rs         # View, Conflict, Resolution, merge algorithm
│   │   │   ├── blob.rs         # content-addressed blob store
│   │   │   ├── intent.rs       # Intent (reason + tool_call + task_ref)
│   │   │   ├── error.rs        # VcsError
│   │   │   └── schema.sql      # SQLite schema (WAL, foreign keys)
│   │   └── tests/
│   │       ├── single_agent.rs    # M2: edit → view → read
│   │       ├── parallel_agents.rs # M3: N stacks, no overlap
│   │       └── conflicts.rs       # M4: conflict detect + resolve
│   └── vcs-cli/                # the `vcs` binary
│       └── src/main.rs
├── node-demo/
│   ├── package.json
│   └── src/
│       ├── vcs-client.js       # Node.js CLI wrapper
│       ├── agent-server.js     # Express HTTP server (one per agent)
│       ├── orchestrator.js     # Spawns N servers, drives workload, resolves
│       ├── demo.js             # In-process end-to-end demo
│       └── parallel-demo.js    # worker_threads parallel write stress test
└── skill/
    └── SKILL.md                # teaches an LLM agent to use vcs
```

---

## Quick start

```bash
# Build (requires Rust 1.70+)
cargo build --release

# Smoke test
export VCS_BIN=./target/release/vcs
$VCS_BIN init

STACK=$($VCS_BIN --json stack open --agent me | jq -r .stack_id)
echo "fn main() {}" > /tmp/main.rs
CHANGE=$($VCS_BIN --json edit $STACK src/main.rs \
  --content-file /tmp/main.rs --reason "add main" | jq -r .change_id)
VIEW=$($VCS_BIN --json view open --base $CHANGE --stacks $STACK | jq -r .view_id)
$VCS_BIN view read $VIEW src/main.rs
$VCS_BIN stack close $STACK

# Run all Rust tests
cargo test

# Node.js in-process demo
cd node-demo && npm install
VCS_BIN=../target/release/vcs node src/demo.js

# Parallel worker_threads stress test (N concurrent writers)
VCS_BIN=../target/release/vcs node src/parallel-demo.js 10

# Full HTTP parallel-servers demo (N live Express servers sharing one store)
VCS_BIN=../target/release/vcs node src/orchestrator.js --agents 4
```

---

## Node.js integration

### VcsClient

```js
import { VcsClient, tempStore } from './src/vcs-client.js';

// Wrap an existing store
const vcs = new VcsClient({ storePath: '/tmp/my-store' });
vcs.init();

// Or get a fresh temp store (useful for tests)
const vcs = tempStore();

// Full workflow
const stackId = vcs.stackOpen('agent-alice');
const changeId = vcs.edit(stackId, 'src/main.rs', 'fn main() {}', {
  reason: 'initial main',
  task_ref: 'task-001',
});
vcs.stackClose(stackId);

const viewId = vcs.viewOpen(changeId, [stackId]);
const file   = vcs.viewRead(viewId, 'src/main.rs'); // { content: '...' }
const files  = vcs.viewLs(viewId);                  // ['src/main.rs']
```

### Parallel HTTP servers (orchestrator pattern)

Each agent server is a live Express process sharing one vcs store on disk.
The orchestrator spawns them, drives them via HTTP, then merges their stacks:

```
┌─────────────────────────────────────────────────────┐
│  orchestrator.js                                    │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐       │
│  │ agent-A   │  │ agent-B   │  │ agent-C   │       │
│  │ :4000     │  │ :4001     │  │ :4002     │       │
│  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘       │
│        └──────────────┼──────────────┘              │
│                 shared vcs store                    │
│              (SQLite WAL + blob dir)                │
│                                                     │
│  orchestrator opens view, detects conflicts,        │
│  resolves, reads merged result                      │
└─────────────────────────────────────────────────────┘
```

---

## CLI reference

```
vcs init
vcs stack open --agent <id> [--base <change_id>]
vcs stack close <stack_id>
vcs stack abandon <stack_id>
vcs stack info <stack_id>
vcs edit <stack_id> <path> --content-file <f> --reason <r> [--task-ref <t>]
vcs delete <stack_id> <path> --reason <r>
vcs rename <stack_id> <from> <to> --content-file <f> --reason <r>
vcs view open --base <change_id> --stacks <id,id,...>
vcs view read <view_id> <path>
vcs view ls <view_id>
vcs view conflicts <view_id>
vcs view resolve <conflict_id> --pick <stack_id>
vcs view resolve <conflict_id> --merge-file <f>
vcs log <stack_id>
vcs diff <change_id> <change_id>

# All commands accept --json for machine-readable output
```

---

## Dependencies

| Crate | Purpose |
|---|---|
| `rusqlite` (bundled) | SQLite — no system dep |
| `blake3` | Content hashing |
| `serde` + `serde_json` | Intent JSON, CLI output |
| `clap v4` | CLI |
| `anyhow` + `thiserror` | Errors |
| `tracing` | Logging |
| `uuid` | Stack/view IDs |

No async runtime.  No HTTP.  The library is sync and embeddable.

---

## What to build next (post-spike)

1. **Materializer** — `vcs checkout <view_id> <dir>` writes the merged tree
   to disk.  This is the only place the working tree appears.
2. **Watcher** — watches the working tree for out-of-band edits and wraps
   them in a change event automatically.
3. **Remotes** — stack replication over HTTP/gRPC (the views already make
   the merge protocol explicit).
4. **ACL hook point** — `Store::open` takes a `Policy` trait object; nothing
   in the spike needs it but the shape is obvious.
5. **Conflict UI** — the conflict objects are already rich enough to render a
   diff UI; nothing in the spike builds one.

---

## Answers to the spike questions

1. **Model expressiveness** — ✅ create / edit / delete / rename cover the
   workload.  Rename encodes `from\x00to` in the path field and writes two
   `files_at_change` rows (old=NULL, new=hash).  The only gap: multi-file
   atomic transactions (e.g. "rename A and edit B together") would need a
   `batch_change_id` field.  Not blocking.

2. **SQLite speed** — ✅ WAL mode + atomic blob renames handle concurrent
   writers with no data loss.  6 parallel worker_threads × 3 edits in ~300ms
   on a single machine.  View computation for 18 changes took < 5ms.  No
   need for materialized view cache at this scale; revisit at > 10 000
   changes per view.

3. **Intent utility** — ✅ `reason`, `tool_call`, and `task_ref` all survive
   round-trips through SQLite JSON.  Querying by `task_ref` is fast with a
   computed column index.  Intent earned its weight: every change in the demo
   had a meaningful reason and the orchestrator used `task_ref` to route
   follow-up.

4. **View computation cost** — ✅ O(total changes in all applied stacks).
   Each `stack_snapshot` walk is proportional to the stack length.  For
   typical agent workloads (10–100 changes per stack, < 10 stacks per view)
   this is negligible.  Add a materialized snapshot cache if stacks grow
   beyond ~1 000 changes.