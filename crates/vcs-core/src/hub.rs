//! Hub bundle — the wire format for pushing stacks from a remote project.
//!
//! When project A (frontend) wants to share its agent changes with a central
//! hub server (running `vcs serve`), it packages all its stacks, changes, and
//! blobs into a `HubBundle` and POSTs it to `POST /api/vcs/push`.
//!
//! The hub then ingests the bundle, making those stacks visible in cross-
//! project views.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A bundle of stacks + changes + blobs from one remote project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubBundle {
    /// Human-readable project identifier (e.g. "frontend", "backend-api").
    pub project_id: String,
    pub stacks:     Vec<HubStack>,
    pub changes:    Vec<HubChange>,
    /// blob_hash → base64-encoded content
    pub blobs: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubStack {
    pub stack_id:       String,
    pub agent_id:       String,
    pub base_change_id: Option<String>,
    pub tip_change_id:  Option<String>,
    pub status:         String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubChange {
    pub change_id:  String,
    pub parent_id:  Option<String>,
    pub path:       String,
    pub op:         String,
    pub diff_hash:  Option<String>,
    pub agent_id:   String,
    pub reason:     String,
    pub task_ref:   Option<String>,
    pub created_at: i64,
}
