//! Core protocol-level types shared across ledger, storage, and consensus.
//!
//! These newtypes match upstream Cardano naming from `cardano-slotting` and
//! `ouroboros-network` so that cross-referencing against the official node
//! remains straightforward.

use std::fmt;

// ---------------------------------------------------------------------------
// Slot and block numbering
// ---------------------------------------------------------------------------

/// Absolute slot number on the blockchain.
///
/// Reference: `Cardano.Slotting.Slot` — `SlotNo`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SlotNo(pub u64);

/// Absolute block number (height of the chain).
///
/// Reference: `Cardano.Slotting.Block` — `BlockNo`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlockNo(pub u64);

/// Epoch number.
///
/// Reference: `Cardano.Slotting.Slot` — `EpochNo`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EpochNo(pub u64);

// ---------------------------------------------------------------------------
// Hash-based identifiers
// ---------------------------------------------------------------------------

/// Blake2b-256 hash of a block header, used as the primary block identifier.
///
/// Reference: `Ouroboros.Consensus.Block.Abstract` — `HeaderHash`.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HeaderHash(pub [u8; 32]);

impl fmt::Debug for HeaderHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HeaderHash({})", hex_short(&self.0))
    }
}

impl fmt::Display for HeaderHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex_short(&self.0))
    }
}

/// Blake2b-256 hash of a serialized transaction body.
///
/// Reference: `Cardano.Ledger.TxIn` — `TxId`.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TxId(pub [u8; 32]);

impl fmt::Debug for TxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxId({})", hex_short(&self.0))
    }
}

impl fmt::Display for TxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex_short(&self.0))
    }
}

// ---------------------------------------------------------------------------
// Chain point
// ---------------------------------------------------------------------------

/// A point on the chain, identifying a specific block by its slot and hash.
///
/// `Origin` represents the genesis pseudo-block that precedes slot 0.
///
/// Reference: `Ouroboros.Network.Block` — `Point` (with `GenesisPoint` and
/// `BlockPoint` patterns).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Point {
    /// The genesis pseudo-block (before any real block).
    Origin,
    /// A specific block identified by slot and header hash.
    BlockPoint(SlotNo, HeaderHash),
}

impl Point {
    /// Returns the slot number, or `None` for `Origin`.
    pub fn slot(&self) -> Option<SlotNo> {
        match self {
            Self::Origin => None,
            Self::BlockPoint(s, _) => Some(*s),
        }
    }

    /// Returns the header hash, or `None` for `Origin`.
    pub fn hash(&self) -> Option<HeaderHash> {
        match self {
            Self::Origin => None,
            Self::BlockPoint(_, h) => Some(*h),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Abbreviated hex for display (first 8 bytes).
fn hex_short(bytes: &[u8; 32]) -> String {
    bytes[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        + "…"
}
