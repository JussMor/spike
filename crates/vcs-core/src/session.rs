//! Session tracking — first-class multi-session support.
//!
//! # Two-server pattern
//!
//! Each session owns an **isolated worktree** and an optional **port**.
//! Neither session's dev-server ever reads from the other's directory.
//!
//! ```text
//! Session A (claude-code-auth)          Session B (claude-code-pay)
//! ─────────────────────────────         ────────────────────────────
//! phase: testing                        phase: working
//! worktree: .vcs/worktrees/<id-a>/      worktree: .vcs/worktrees/<id-b>/
//! port: 5173                            port: 5174
//! dev-server ← own files only           dev-server ← own files only
//! ```
//!
//! Gate rule: a session whose phase is `testing` is the authority.
//! Other sessions MUST NOT merge until the testing session closes or
//! transitions to `done`.
//!
//! # Session lifecycle
//! ```text
//! vcs_session_open()       → session_id, worktree path assigned
//! vcs_stack_open()         → stack auto-linked to session
//! vcs_edit × N             → after each edit: vcs_touching to detect collisions
//! vcs_session_phase testing → mark as "I am now verifying my output"
//! [ run tests / dev server on session.port ]
//! vcs_session_close()      → phase = done; stack stays open for merge
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
    pub phase:        String, // "working" | "testing" | "done"
    pub worktree:     Option<String>, // absolute path to this session's private checkout
    pub port:         Option<u16>,    // reserved dev-server port
}

/// Rich summary of a single session for display in `AgentOverview`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id:     String,
    pub agent_id:       String,
    pub stack_id:       Option<String>,
    pub status:         String,
    pub phase:          String,
    pub worktree:       Option<String>,
    pub port:           Option<u16>,
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
    /// Any session currently in "testing" phase (blocks merges).
    pub testing_session: Option<String>,
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
