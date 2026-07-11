use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("episode not found: {0}")]
    NotFound(String),

    #[error("ambiguous episode id prefix {0:?} matches multiple episodes; use more characters")]
    AmbiguousPrefix(String),

    // Constructed by the MCP delete handler (M4); the store itself never
    // hard-deletes on the agent path.
    #[allow(dead_code)]
    #[error("agents may not hard-delete; demote instead (deletion is tiering, use the operator CLI to purge)")]
    HardDeleteRefused,

    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

// redb 4 surfaces distinct error types per operation; collapse them — callers
// can't act on the distinction, and the message preserves the detail.
macro_rules! from_redb {
    ($($t:ty),+) => {$(
        impl From<$t> for Error {
            fn from(e: $t) -> Self {
                Error::Storage(e.to_string())
            }
        }
    )+};
}

from_redb!(
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError
);

pub type Result<T> = std::result::Result<T, Error>;
