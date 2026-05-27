//! Change — the fundamental append-only event.

use crate::blob::blake3_hex;
use crate::intent::Intent;
use serde::{Deserialize, Serialize};

pub type ChangeId = String;
pub type StackId  = String;
pub type ViewId   = String;
pub type ConflictId = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Create,
    Edit,
    Delete,
    Rename,
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Op::Create => write!(f, "create"),
            Op::Edit   => write!(f, "edit"),
            Op::Delete => write!(f, "delete"),
            Op::Rename => write!(f, "rename"),
        }
    }
}

impl std::str::FromStr for Op {
    type Err = crate::error::VcsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "create" => Ok(Op::Create),
            "edit"   => Ok(Op::Edit),
            "delete" => Ok(Op::Delete),
            "rename" => Ok(Op::Rename),
            _ => Err(crate::error::VcsError::ChangeNotFound(
                format!("unknown op: {s}"),
            )),
        }
    }
}

/// A single recorded event in the change log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub change_id:  ChangeId,
    pub parent_id:  Option<ChangeId>,
    pub path:       String,
    pub op:         Op,
    pub diff_hash:  Option<String>,
    pub agent_id:   String,
    pub intent:     Intent,
    pub created_at: i64,
}

/// Deterministic ID = BLAKE3( parent_id | path | diff_hash | agent_id | ts )
pub fn compute_change_id(
    parent_id: Option<&str>,
    path: &str,
    diff_hash: Option<&str>,
    agent_id: &str,
    ts: i64,
) -> ChangeId {
    let mut buf = String::new();
    buf.push_str(parent_id.unwrap_or("ROOT"));
    buf.push('\x00');
    buf.push_str(path);
    buf.push('\x00');
    buf.push_str(diff_hash.unwrap_or(""));
    buf.push('\x00');
    buf.push_str(agent_id);
    buf.push('\x00');
    buf.push_str(&ts.to_string());
    blake3_hex(buf.as_bytes())
}
