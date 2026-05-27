use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Structured provenance attached to every change.
///
/// `reason` is mandatory — if an agent can't give a reason, the spike has
/// already learned something useful: intent is hard to capture.
///
/// `tool_call` is the raw JSON of the tool invocation that produced this edit.
/// `task_ref` links back to an orchestrator task ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Free-text explanation of why this change was made.
    pub reason: String,

    /// The structured tool call that produced this edit, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<Value>,

    /// Opaque reference to an orchestrator task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_ref: Option<String>,
}

impl Intent {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            tool_call: None,
            task_ref: None,
        }
    }

    pub fn with_tool_call(mut self, call: Value) -> Self {
        self.tool_call = Some(call);
        self
    }

    pub fn with_task_ref(mut self, r: impl Into<String>) -> Self {
        self.task_ref = Some(r.into());
        self
    }

    pub fn to_json(&self) -> crate::error::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(s: &str) -> crate::error::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}
