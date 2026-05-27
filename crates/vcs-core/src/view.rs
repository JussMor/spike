//! View computation — merge N stacks on top of a base snapshot.
//!
//! Algorithm:
//!   1. Walk every change in every stack (in stack order, oldest→newest within
//!      each stack) and collect the *final* state of each path per stack.
//!   2. Merge: paths touched by exactly one stack → use that blob.
//!             paths touched by >1 stacks      → conflict.
//!   3. Paths not touched by any stack inherit from base.

use crate::blob::blake3_hex;
use crate::change::{ChangeId, ConflictId, StackId, ViewId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct View {
    pub view_id:        ViewId,
    pub base_change_id: ChangeId,
    /// JSON-encoded list of stack IDs as stored in SQLite.
    pub applied_stacks: String,
    pub state_hash:     String,
}

/// One candidate in a conflict — what a particular stack thinks the file is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub stack_id:  StackId,
    pub change_id: ChangeId,
    /// None = the stack deleted the file
    pub blob_hash: Option<String>,
}

/// A conflict: two or more stacks disagree about a path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub conflict_id: ConflictId,
    pub view_id:     ViewId,
    pub path:        String,
    pub candidates:  Vec<Candidate>,
    pub resolution:  Option<Resolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Resolution {
    /// Pick one of the candidates unchanged.
    Pick { stack_id: StackId },
    /// Provide a merged blob (stored in the blob dir).
    Merge { blob_hash: String },
}

/// The computed file-tree for a view, split into clean and conflicted paths.
#[derive(Debug)]
pub struct MergedTree {
    /// path → blob_hash (None = deleted)
    pub clean:     HashMap<String, Option<String>>,
    /// path → list of candidates
    pub conflicts: HashMap<String, Vec<Candidate>>,
}

/// Compute the merged tree from per-stack snapshots and a base snapshot.
pub fn compute_merge(
    base_snapshot: &HashMap<String, String>,           // path → blob_hash at base
    stack_snapshots: &[(StackId, HashMap<String, Option<String>>)], // per-stack finals
) -> MergedTree {
    let mut clean: HashMap<String, Option<String>>  = HashMap::new();
    let mut conflicts: HashMap<String, Vec<Candidate>> = HashMap::new();
    let mut touched: HashMap<String, Vec<(StackId, Option<String>, ChangeId)>> = HashMap::new();

    for (stack_id, snap) in stack_snapshots {
        for (path, blob) in snap {
            touched
                .entry(path.clone())
                .or_default()
                .push((stack_id.clone(), blob.clone(), String::new()));
        }
    }

    // Paths touched by stacks
    for (path, writers) in &touched {
        if writers.len() == 1 {
            clean.insert(path.clone(), writers[0].1.clone());
        } else {
            // Collect conflict candidates — change_id will be filled by caller
            let candidates: Vec<Candidate> = writers
                .iter()
                .map(|(sid, blob, cid)| Candidate {
                    stack_id:  sid.clone(),
                    change_id: cid.clone(),
                    blob_hash: blob.clone(),
                })
                .collect();
            conflicts.insert(path.clone(), candidates);
        }
    }

    // Paths from base not touched by any stack
    for (path, hash) in base_snapshot {
        if !touched.contains_key(path) {
            clean.insert(path.clone(), Some(hash.clone()));
        }
    }

    MergedTree { clean, conflicts }
}

/// Compute the state hash for a view: BLAKE3 of sorted (path\0blob_hash\n) pairs.
pub fn state_hash(tree: &HashMap<String, Option<String>>) -> String {
    let mut pairs: Vec<(&str, &str)> = tree
        .iter()
        .filter_map(|(p, b)| b.as_deref().map(|h| (p.as_str(), h)))
        .collect();
    pairs.sort_unstable();
    let mut buf = String::new();
    for (path, hash) in pairs {
        buf.push_str(path);
        buf.push('\x00');
        buf.push_str(hash);
        buf.push('\n');
    }
    blake3_hex(buf.as_bytes())
}
