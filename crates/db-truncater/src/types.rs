//! Typed configuration surface for the `db-truncater` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBTruncater/Types.hs.
//!
//! Direct ports:
//!
//! - [`DBTruncaterConfig`] — `data DBTruncaterConfig = DBTruncaterConfig
//!   { dbDir :: FilePath, truncateAfter :: TruncateAfter, verbose :: Bool }`.
//! - [`TruncateAfter`] — `data TruncateAfter = TruncateAfterSlot SlotNo |
//!   TruncateAfterBlock BlockNo`.
//!
//! `SlotNo` and `BlockNo` are reused from `yggdrasil_ledger::types`
//! (the canonical workspace types) rather than defined locally.

use std::path::PathBuf;

use yggdrasil_ledger::{BlockNo, SlotNo};

/// Where to truncate the ImmutableDB.
///
/// Upstream: `data TruncateAfter = TruncateAfterSlot SlotNo | TruncateAfterBlock BlockNo`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TruncateAfter {
    /// Truncate after the given slot number, deleting all blocks with
    /// a higher slot number.
    TruncateAfterSlot(SlotNo),
    /// Truncate after the given block number (such that the new tip
    /// has this block number).
    TruncateAfterBlock(BlockNo),
}

/// Operator-supplied configuration for the `db-truncater` binary.
///
/// Upstream:
/// ```haskell
/// data DBTruncaterConfig = DBTruncaterConfig
///   { dbDir :: FilePath
///   , truncateAfter :: TruncateAfter
///   , verbose :: Bool
///   }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DBTruncaterConfig {
    /// Path to the chain DB. Mirrors upstream `dbDir :: FilePath`.
    pub db_dir: PathBuf,
    /// Truncate target. Mirrors upstream `truncateAfter :: TruncateAfter`.
    pub truncate_after: TruncateAfter,
    /// Verbose logging flag. Mirrors upstream `verbose :: Bool`.
    pub verbose: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_after_slot_round_trips() {
        let after = TruncateAfter::TruncateAfterSlot(SlotNo(42));
        match after {
            TruncateAfter::TruncateAfterSlot(slot) => assert_eq!(slot, SlotNo(42)),
            TruncateAfter::TruncateAfterBlock(_) => panic!("wrong variant"),
        }
    }

    #[test]
    fn truncate_after_block_round_trips() {
        let after = TruncateAfter::TruncateAfterBlock(BlockNo(100));
        match after {
            TruncateAfter::TruncateAfterBlock(block) => assert_eq!(block, BlockNo(100)),
            TruncateAfter::TruncateAfterSlot(_) => panic!("wrong variant"),
        }
    }

    #[test]
    fn db_truncater_config_construction() {
        let config = DBTruncaterConfig {
            db_dir: PathBuf::from("/var/lib/cardano-node/db"),
            truncate_after: TruncateAfter::TruncateAfterSlot(SlotNo(1_000_000)),
            verbose: true,
        };
        assert_eq!(config.db_dir.to_str(), Some("/var/lib/cardano-node/db"));
        assert!(config.verbose);
        assert!(matches!(
            config.truncate_after,
            TruncateAfter::TruncateAfterSlot(SlotNo(1_000_000))
        ));
    }
}
