use thiserror::Error;
use yggdrasil_ledger::HeaderHash;

/// Errors returned by storage operations.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum StorageError {
    /// Attempted to insert a block whose header hash already exists.
    #[error("duplicate block: {0}")]
    DuplicateBlock(HeaderHash),
}
