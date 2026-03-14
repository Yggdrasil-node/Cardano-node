use thiserror::Error;
use yggdrasil_ledger::HeaderHash;

/// Errors returned by storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Attempted to insert a block whose header hash already exists.
    #[error("duplicate block: {0}")]
    DuplicateBlock(HeaderHash),

    /// A requested point could not be found in the current store state.
    #[error("point not found")]
    PointNotFound,

    /// An I/O error occurred during a storage operation.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization or deserialization error occurred.
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl PartialEq for StorageError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::DuplicateBlock(a), Self::DuplicateBlock(b)) => a == b,
            (Self::PointNotFound, Self::PointNotFound) => true,
            (Self::Io(a), Self::Io(b)) => a.kind() == b.kind(),
            (Self::Serialization(a), Self::Serialization(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for StorageError {}
