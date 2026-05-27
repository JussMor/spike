//! vcs-core — agent-native VCS data model.
//!
//! # Overview
//!
//! The model has five concepts:
//!
//! * **Blobs** — raw content, content-addressed by BLAKE3.
//! * **Changes** — append-only events (create/edit/delete/rename) that each record
//!   *who* made the change, *why* (Intent), and *what* the content hash is.
//! * **Stacks** — an ordered list of changes produced by one agent.  Agents never
//!   share a stack; conflicts arise when two stacks disagree.
//! * **Views** — a virtual merge of N stacks on top of a base snapshot.  Reading
//!   always goes through a view.
//! * **Conflicts** — first-class data objects, not error states.  The orchestrator
//!   decides; agents surface them and stop.
//!
//! # Invariants
//!
//! * The `changes` table is append-only.
//! * A stack may only be mutated while `status = 'open'`.
//! * Intent is always required on edits (enforced by the type system).
//! * Views are immutable once opened; re-open to refresh.

pub mod blob;
pub mod change;
pub mod error;
pub mod hub;
pub mod intent;
pub mod stack;
pub mod store;
pub mod view;

// Re-export the main surface area
pub use blob::BlobStore;
pub use change::{Change, ChangeId, ConflictId, Op, StackId, ViewId};
pub use error::{Result, VcsError};
pub use hub::{HubBundle, HubChange, HubFileEntry, HubStack};
pub use intent::Intent;
pub use stack::{Stack, StackStatus};
pub use store::{DiffEntry, Store};
pub use view::{Candidate, Conflict, Resolution, View};
