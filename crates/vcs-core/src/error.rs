use thiserror::Error;

#[derive(Debug, Error)]
pub enum VcsError {
    #[error("store not initialised at {0}")]
    NotInitialised(String),

    #[error("stack {0} not found")]
    StackNotFound(String),

    #[error("stack {0} is not open (status={1})")]
    StackNotOpen(String, String),

    #[error("change {0} not found")]
    ChangeNotFound(String),

    #[error("view {0} not found")]
    ViewNotFound(String),

    #[error("conflict {0} not found")]
    ConflictNotFound(String),

    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("unresolved conflicts in view {0} — resolve before reading")]
    UnresolvedConflicts(String),

    #[error("blob not found: {0}")]
    BlobNotFound(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = VcsError> = std::result::Result<T, E>;
