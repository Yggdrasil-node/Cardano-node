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

    /// Coordinated recovery failed because the stored chain state was
    /// internally inconsistent.
    #[error("recovery error: {0}")]
    Recovery(String),
}

impl PartialEq for StorageError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::DuplicateBlock(a), Self::DuplicateBlock(b)) => a == b,
            (Self::PointNotFound, Self::PointNotFound) => true,
            (Self::Io(a), Self::Io(b)) => a.kind() == b.kind(),
            (Self::Serialization(a), Self::Serialization(b)) => a == b,
            (Self::Recovery(a), Self::Recovery(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for StorageError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_duplicate_block_names_hash_prefix() {
        let e = StorageError::DuplicateBlock(HeaderHash([0xAB; 32]));
        let s = format!("{e}");
        assert!(
            s.to_lowercase().contains("duplicate"),
            "message must identify the rule: {s}",
        );
        // `HeaderHash` Display emits the hex_short form; assert the hex is
        // present so a future refactor dropping the `{0}` placeholder fails.
        assert!(
            s.contains("ab") || s.contains("AB"),
            "message must surface the hash bytes: {s}",
        );
    }

    #[test]
    fn display_point_not_found() {
        let s = format!("{}", StorageError::PointNotFound);
        assert!(s.to_lowercase().contains("point"));
        assert!(s.to_lowercase().contains("not found"));
    }

    #[test]
    fn display_io_propagates_inner_error() {
        let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access");
        let e = StorageError::Io(inner);
        let s = format!("{e}");
        assert!(s.to_lowercase().contains("i/o"));
        assert!(
            s.contains("no access"),
            "Display must propagate the inner I/O error message: {s}",
        );
    }

    #[test]
    fn display_serialization_propagates_message() {
        let e = StorageError::Serialization("bad CBOR at offset 17".to_owned());
        let s = format!("{e}");
        assert!(s.to_lowercase().contains("serial"));
        assert!(
            s.contains("bad CBOR at offset 17"),
            "Display must surface the serialization diagnostic: {s}",
        );
    }

    #[test]
    fn display_recovery_propagates_message() {
        let e = StorageError::Recovery("volatile tip missing from immutable DB".to_owned());
        let s = format!("{e}");
        assert!(s.to_lowercase().contains("recovery"));
        assert!(
            s.contains("volatile tip missing"),
            "Display must surface the recovery diagnostic: {s}",
        );
    }

    #[test]
    fn partial_eq_ignores_io_inner_message_uses_kind() {
        let a = StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "a",
        ));
        let b = StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "b",
        ));
        let c = StorageError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "c"));
        // Same ErrorKind compares equal even with different messages.
        assert_eq!(a, b);
        // Different ErrorKind compares unequal.
        assert_ne!(a, c);
    }
}
