use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("episode not found: {0}")]
    NotFound(String),

    #[error("ambiguous episode id prefix {0:?} matches multiple episodes; use more characters")]
    AmbiguousPrefix(String),

    #[error("archived version {version:?} not found for episode {episode_id}")]
    VersionNotFound { episode_id: String, version: String },

    #[error(
        "archived version prefix {version:?} matches multiple versions of episode {episode_id}; use more characters"
    )]
    AmbiguousVersionPrefix { episode_id: String, version: String },

    // Constructed by the MCP delete handler (M4); the store itself never
    // hard-deletes on the agent path.
    #[allow(dead_code)]
    #[error(
        "agents may not hard-delete; demote instead (deletion is tiering, use the operator CLI to purge)"
    )]
    HardDeleteRefused,

    // Two-phase delete guard: purge destroys only what demote already hid.
    // There is deliberately no --force path around this.
    #[error(
        "episode {0} is not demoted; purge only destroys demoted episodes (run `ecphory demote {0}` first)"
    )]
    NotDemoted(String),

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
