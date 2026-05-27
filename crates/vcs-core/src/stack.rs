use crate::change::{ChangeId, StackId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StackStatus {
    Open,
    Closed,
    Abandoned,
}

impl std::fmt::Display for StackStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StackStatus::Open      => write!(f, "open"),
            StackStatus::Closed    => write!(f, "closed"),
            StackStatus::Abandoned => write!(f, "abandoned"),
        }
    }
}

impl std::str::FromStr for StackStatus {
    type Err = crate::error::VcsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open"      => Ok(StackStatus::Open),
            "closed"    => Ok(StackStatus::Closed),
            "abandoned" => Ok(StackStatus::Abandoned),
            _ => Err(crate::error::VcsError::StackNotFound(
                format!("unknown status: {s}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stack {
    pub stack_id:       StackId,
    pub agent_id:       String,
    pub base_change_id: Option<ChangeId>,
    pub tip_change_id:  Option<ChangeId>,
    pub status:         StackStatus,
}
