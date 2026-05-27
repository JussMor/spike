-- vcs-core SQLite schema
-- All tables are append-only by convention. Updates only on views/conflicts.

PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

-- ── append-only event log ──────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS changes (
    change_id  TEXT PRIMARY KEY,
    parent_id  TEXT,                       -- previous change in this stack; NULL = root
    path       TEXT NOT NULL,
    op         TEXT NOT NULL,              -- 'create' | 'edit' | 'delete' | 'rename'
    diff_hash  TEXT,                       -- BLAKE3 of diff blob; NULL for deletes
    agent_id   TEXT NOT NULL,
    intent     TEXT,                       -- JSON: {reason, tool_call?, task_ref?}
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_changes_parent   ON changes(parent_id);
CREATE INDEX IF NOT EXISTS idx_changes_path     ON changes(path);
CREATE INDEX IF NOT EXISTS idx_changes_agent    ON changes(agent_id);
CREATE INDEX IF NOT EXISTS idx_changes_created  ON changes(created_at);

-- ── stacks ─────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS stacks (
    stack_id       TEXT PRIMARY KEY,
    agent_id       TEXT NOT NULL,
    base_change_id TEXT,                   -- NULL = fresh repo root
    tip_change_id  TEXT,                   -- current HEAD of this stack
    status         TEXT NOT NULL DEFAULT 'open'   -- 'open'|'closed'|'abandoned'
);

CREATE INDEX IF NOT EXISTS idx_stacks_agent  ON stacks(agent_id);
CREATE INDEX IF NOT EXISTS idx_stacks_status ON stacks(status);

-- ── views (cached materialised snapshots) ─────────────────────────────────
CREATE TABLE IF NOT EXISTS views (
    view_id        TEXT PRIMARY KEY,
    base_change_id TEXT NOT NULL,
    applied_stacks TEXT NOT NULL,          -- JSON array of stack_ids
    state_hash     TEXT NOT NULL           -- BLAKE3 of sorted (path,blob_hash) pairs
);

-- ── derived file-tree index ───────────────────────────────────────────────
-- One row per (change, path) pair, recording what the file looks like
-- immediately after that change lands.  NULL blob_hash = deleted.
CREATE TABLE IF NOT EXISTS files_at_change (
    change_id TEXT NOT NULL,
    path      TEXT NOT NULL,
    blob_hash TEXT,
    PRIMARY KEY (change_id, path)
);

CREATE INDEX IF NOT EXISTS idx_fac_path ON files_at_change(path, change_id);

-- ── structured edit metadata ──────────────────────────────────────────────
-- Keeps agent edits compact and inspectable. The result blob remains in
-- files_at_change for deterministic checkout, while patch_blob_hash stores a
-- line-range operation describing how the agent changed the previous content.
CREATE TABLE IF NOT EXISTS edit_metadata (
    change_id        TEXT PRIMARY KEY,
    path             TEXT NOT NULL,
    base_blob_hash   TEXT,
    result_blob_hash TEXT,
    patch_blob_hash  TEXT,
    edit_kind        TEXT NOT NULL,
    start_line       INTEGER,
    end_line         INTEGER,
    inserted_lines   INTEGER NOT NULL DEFAULT 0,
    deleted_lines    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_edit_metadata_path ON edit_metadata(path);

-- ── sessions (first-class multi-session tracking) ───────────────────────
-- One row per active Claude Code / agent session.
-- Heartbeated by the agent while alive; status set to 'done' on close.
CREATE TABLE IF NOT EXISTS sessions (
    session_id   TEXT PRIMARY KEY,
    agent_id     TEXT NOT NULL,
    stack_id     TEXT,                        -- NULL until vcs_stack_open is called
    started_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    status       TEXT NOT NULL DEFAULT 'active'  -- 'active' | 'idle' | 'done'
);

CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_stack  ON sessions(stack_id);

-- ── conflicts (first-class data, not error states) ────────────────────────
CREATE TABLE IF NOT EXISTS conflicts (
    conflict_id TEXT PRIMARY KEY,
    view_id     TEXT NOT NULL,
    path        TEXT NOT NULL,
    candidates  TEXT NOT NULL,             -- JSON: [{stack_id, change_id, blob_hash}, ...]
    resolution  TEXT                       -- JSON when resolved; NULL = unresolved
);

CREATE INDEX IF NOT EXISTS idx_conflicts_view ON conflicts(view_id);
