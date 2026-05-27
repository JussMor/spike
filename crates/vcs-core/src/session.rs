//! Session tracking — first-class multi-session support.
//!
//! A **session** is one running Claude Code chat / agent process.
//! Sessions register themselves via `Store::session_open` and send
//! heartbeats via `Store::session_heartbeat`.  On close they call
//! `Store::session_close`.  The store retains history even for dead
//! sessions so audits can see exactly which agent touched what.
//!
//! # Session lifecycle
//! ```text
//! vcs_status()          ← see all active sessions before starting
//! vcs_session_open()    ← register; returns session_id
//!   vcs_stack_open()    ← open a stack, link it to the session
//!   vcs_edit × N        ← each edit carries agent collision info
//! vcs_session_close()   ← mark done; stack may still be open for merge
//! ```

use serde::{Deserialize, Serialize};

/// A live or recently-closed agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id:   String,
    pub agent_id:     String,
    pub stack_id:     Option<String>,
    pub started_at:   i64,
    pub last_seen_at: i64,
    pub status:       String, // "active" | "idle" | "done"
}

/// Rich summary of a single session for display in `AgentOverview`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id:     String,
    pub agent_id:       String,
    pub stack_id:       Option<String>,
    pub status:         String,
    pub files_touched:  Vec<String>,
    pub changes_count:  usize,
    pub started_at:     i64,
    pub last_seen_at:   i64,
}

/// A file currently being modified by more than one open stack.
/// Signals a future conflict before any view is opened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotFile {
    /// File path.
    pub path: String,
    /// agent_ids of all open stacks that have touched this path.
    pub touched_by: Vec<String>,
    /// True when the blobs differ across those stacks (confirmed conflict).
    pub will_conflict: bool,
}

/// Complete multi-agent state snapshot — the tool Claude calls instead of
/// asking the human to open a browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOverview {
    /// All sessions (active and recently closed).
    pub sessions:      Vec<SessionSummary>,
    /// Files touched by 2+ open stacks right now.
    pub hot_files:     Vec<HotFile>,
    /// Number of active sessions.
    pub active_count:  usize,
    /// Human-readable narrative Claude can present directly.
    pub summary:       String,
    pub generated_at:  i64,
}

/// Per-file contention info returned alongside a `vcs_edit` call.
/// Non-blocking — just tells the agent if another session is on the same file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContention {
    pub path:         String,
    /// Other open stacks (not the caller's) that have this file.
    pub other_stacks: Vec<ContentionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentionEntry {
    pub stack_id:  String,
    pub agent_id:  String,
    pub change_id: String,
}
