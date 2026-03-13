//! Byron-era block envelope types (lightweight extraction only).
//!
//! Byron blocks use a fundamentally different wire format from Shelley+
//! eras. They come in two variants:
//!
//! - **Epoch Boundary Block (EBB)** — era tag 0 in the outer envelope.
//!   Header consensus data: `[epoch, chain_difficulty]`.
//! - **Main Block** — era tag 1 in the outer envelope.
//!   Header consensus data: `[[epoch, slot_in_epoch], pubkey,
//!   difficulty, signature]`.
//!
//! This module provides a lightweight decode that extracts the epoch
//! and slot information without fully parsing the block body or
//! transactions. This is sufficient for multi-era sync to compute
//! absolute slot numbers (`epoch * 21600 + slot_in_epoch`).
//!
//! Reference:
//! <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/byron>

use crate::cbor::Decoder;
use crate::error::LedgerError;

pub const BYRON_NAME: &str = "Byron";

/// Number of slots per Byron epoch on mainnet.
pub const BYRON_SLOTS_PER_EPOCH: u64 = 21600;

// ---------------------------------------------------------------------------
// Byron block variant
// ---------------------------------------------------------------------------

/// A lightweight Byron block envelope with enough information for
/// chain-sync slot tracking.
///
/// Full transaction decode is not modeled; Byron blocks remain opaque
/// beyond the header-level metadata extracted here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ByronBlock {
    /// An Epoch Boundary Block (EBB), marking an epoch transition.
    EpochBoundary {
        /// The epoch number.
        epoch: u64,
        /// Previous block hash (32 bytes).
        prev_hash: [u8; 32],
    },
    /// A regular Byron main block.
    MainBlock {
        /// The epoch number.
        epoch: u64,
        /// The slot index within the epoch (0..21599 on mainnet).
        slot_in_epoch: u64,
        /// Previous block hash (32 bytes).
        prev_hash: [u8; 32],
    },
}

impl ByronBlock {
    /// Returns the epoch number.
    pub fn epoch(&self) -> u64 {
        match self {
            Self::EpochBoundary { epoch, .. } => *epoch,
            Self::MainBlock { epoch, .. } => *epoch,
        }
    }

    /// Computes the absolute slot number.
    ///
    /// EBBs are treated as occupying the first slot of their epoch.
    /// Main blocks use `epoch * slots_per_epoch + slot_in_epoch`.
    pub fn absolute_slot(&self, slots_per_epoch: u64) -> u64 {
        match self {
            Self::EpochBoundary { epoch, .. } => epoch * slots_per_epoch,
            Self::MainBlock {
                epoch,
                slot_in_epoch,
                ..
            } => epoch * slots_per_epoch + slot_in_epoch,
        }
    }

    /// Returns the previous block hash.
    pub fn prev_hash(&self) -> &[u8; 32] {
        match self {
            Self::EpochBoundary { prev_hash, .. } => prev_hash,
            Self::MainBlock { prev_hash, .. } => prev_hash,
        }
    }

    /// Decode a Byron EBB from raw CBOR bytes (the block body inside
    /// the `[0, body]` outer envelope).
    ///
    /// EBB structure: `[header, body, extra]`
    /// Header: `[protocol_magic, prev_hash, body_proof,
    ///   consensus_data, extra_data]`
    /// Consensus data: `[epoch, chain_difficulty]`
    pub fn decode_ebb(raw: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(raw);

        // Outer: [header, body, extra]
        let outer_len = dec.array()?;
        if outer_len < 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: outer_len as usize,
            });
        }

        // Header: [protocol_magic, prev_hash, body_proof,
        //          consensus_data, extra_data]
        let hdr_len = dec.array()?;
        if hdr_len < 5 {
            return Err(LedgerError::CborInvalidLength {
                expected: 5,
                actual: hdr_len as usize,
            });
        }

        // protocol_magic
        dec.skip()?;

        // prev_hash
        let prev_raw = dec.bytes()?;
        let prev_hash: [u8; 32] =
            prev_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: prev_raw.len(),
                })?;

        // body_proof
        dec.skip()?;

        // consensus_data: [epoch, chain_difficulty]
        let cd_len = dec.array()?;
        if cd_len < 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: cd_len as usize,
            });
        }
        let epoch = dec.unsigned()?;
        // Skip remaining consensus data + extra header + body + extra
        // (we only need epoch and prev_hash).

        Ok(Self::EpochBoundary { epoch, prev_hash })
    }

    /// Decode a Byron main block from raw CBOR bytes (the block body
    /// inside the `[1, body]` outer envelope).
    ///
    /// Main block structure: `[header, body, extra]`
    /// Header: `[protocol_magic, prev_hash, body_proof,
    ///   consensus_data, extra_data]`
    /// Consensus data: `[slot_id, pubkey, difficulty, signature]`
    /// Slot id: `[epoch, slot_in_epoch]`
    pub fn decode_main(raw: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(raw);

        // Outer: [header, body, extra]
        let outer_len = dec.array()?;
        if outer_len < 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: outer_len as usize,
            });
        }

        // Header: [protocol_magic, prev_hash, body_proof,
        //          consensus_data, extra_data]
        let hdr_len = dec.array()?;
        if hdr_len < 5 {
            return Err(LedgerError::CborInvalidLength {
                expected: 5,
                actual: hdr_len as usize,
            });
        }

        // protocol_magic
        dec.skip()?;

        // prev_hash
        let prev_raw = dec.bytes()?;
        let prev_hash: [u8; 32] =
            prev_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: prev_raw.len(),
                })?;

        // body_proof
        dec.skip()?;

        // consensus_data: [slot_id, pubkey, difficulty, signature]
        let cd_len = dec.array()?;
        if cd_len < 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: cd_len as usize,
            });
        }

        // slot_id: [epoch, slot_in_epoch]
        let slot_len = dec.array()?;
        if slot_len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: slot_len as usize,
            });
        }
        let epoch = dec.unsigned()?;
        let slot_in_epoch = dec.unsigned()?;

        Ok(Self::MainBlock {
            epoch,
            slot_in_epoch,
            prev_hash,
        })
    }
}
