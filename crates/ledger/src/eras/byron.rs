//! Byron-era block envelope types.
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
//! This module extracts the epoch, slot, `chain_difficulty` (block
//! number), previous hash, and raw header byte ranges needed for
//! header hash computation.
//!
//! ## Header hash computation
//!
//! Byron header hashes are **not** computed over the full wire block.
//! Instead they are `Blake2b-256(prefix ++ raw_header_cbor_bytes)` where
//! the prefix encodes the variant discriminator:
//!
//! - EBB:  `0x82 0x00` (`[2-array, 0]`)
//! - Main: `0x82 0x01` (`[2-array, 1]`)
//!
//! Reference:
//! <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/byron>

use crate::cbor::Decoder;
use crate::error::LedgerError;
use crate::types::HeaderHash;

pub const BYRON_NAME: &str = "Byron";

/// Number of slots per Byron epoch on mainnet.
pub const BYRON_SLOTS_PER_EPOCH: u64 = 21600;

/// Prefix prepended to EBB header bytes before hashing.
///
/// `0x82 0x00` = CBOR 2-element array, tag 0.
const EBB_HASH_PREFIX: [u8; 2] = [0x82, 0x00];

/// Prefix prepended to main-block header bytes before hashing.
///
/// `0x82 0x01` = CBOR 2-element array, tag 1.
const MAIN_HASH_PREFIX: [u8; 2] = [0x82, 0x01];

// ---------------------------------------------------------------------------
// Byron block variant
// ---------------------------------------------------------------------------

/// A decoded Byron block envelope carrying header-level metadata and the
/// raw header annotation bytes needed for correct header hash computation.
///
/// Full transaction decode is not modeled; Byron blocks remain opaque
/// beyond the header-level metadata extracted here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ByronBlock {
    /// An Epoch Boundary Block (EBB), marking an epoch transition.
    EpochBoundary {
        /// The epoch number.
        epoch: u64,
        /// `ChainDifficulty` value (serves as block number).
        chain_difficulty: u64,
        /// Previous block hash (32 bytes).
        prev_hash: [u8; 32],
        /// Raw CBOR bytes of the header element (for hash computation).
        raw_header: Vec<u8>,
    },
    /// A regular Byron main block.
    MainBlock {
        /// The epoch number.
        epoch: u64,
        /// The slot index within the epoch (0..21599 on mainnet).
        slot_in_epoch: u64,
        /// `ChainDifficulty` value (serves as block number).
        chain_difficulty: u64,
        /// Previous block hash (32 bytes).
        prev_hash: [u8; 32],
        /// Raw CBOR bytes of the header element (for hash computation).
        raw_header: Vec<u8>,
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

    /// Returns the `ChainDifficulty` value, which serves as the block
    /// number in Byron.
    ///
    /// EBBs share the block number of their predecessor (they do **not**
    /// increment the difficulty counter).
    pub fn chain_difficulty(&self) -> u64 {
        match self {
            Self::EpochBoundary {
                chain_difficulty, ..
            } => *chain_difficulty,
            Self::MainBlock {
                chain_difficulty, ..
            } => *chain_difficulty,
        }
    }

    /// Returns `true` for Epoch Boundary Blocks.
    pub fn is_ebb(&self) -> bool {
        matches!(self, Self::EpochBoundary { .. })
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

    /// Computes the Byron header hash.
    ///
    /// The hash is `Blake2b-256(prefix ++ raw_header_cbor)` where:
    /// - EBB prefix  = `0x82 0x00`
    /// - Main prefix = `0x82 0x01`
    ///
    /// This matches the upstream `headerHashAnnotated` /
    /// `boundaryHeaderHashAnnotated` from `cardano-ledger-byron`.
    pub fn header_hash(&self) -> HeaderHash {
        let (prefix, raw_header) = match self {
            Self::EpochBoundary { raw_header, .. } => (&EBB_HASH_PREFIX[..], raw_header),
            Self::MainBlock { raw_header, .. } => (&MAIN_HASH_PREFIX[..], raw_header),
        };
        let mut buf = Vec::with_capacity(prefix.len() + raw_header.len());
        buf.extend_from_slice(prefix);
        buf.extend_from_slice(raw_header);
        let digest = yggdrasil_crypto::hash_bytes_256(&buf);
        HeaderHash(digest.0)
    }

    /// Decode a Byron EBB from raw CBOR bytes (the block body inside
    /// the `[0, body]` outer envelope).
    ///
    /// EBB structure: `[header, body, extra]`
    /// Header: `[protocol_magic, prev_hash, body_proof,
    ///   consensus_data, extra_data]`
    /// Consensus data: `[epoch, chain_difficulty]`
    ///
    /// `chain_difficulty` is CBOR-encoded as `[Word64]` (a 1-element
    /// array wrapping the difficulty value).
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

        // Capture the header byte range for hash computation.
        let hdr_start = dec.position();

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

        // chain_difficulty: CBOR [Word64] — 1-element array.
        let diff_len = dec.array()?;
        if diff_len < 1 {
            return Err(LedgerError::CborInvalidLength {
                expected: 1,
                actual: diff_len as usize,
            });
        }
        let chain_difficulty = dec.unsigned()?;

        // Skip remaining header fields (extra_data).
        dec.skip()?;

        let hdr_end = dec.position();
        let raw_header = raw[hdr_start..hdr_end].to_vec();

        Ok(Self::EpochBoundary {
            epoch,
            chain_difficulty,
            prev_hash,
            raw_header,
        })
    }

    /// Decode a Byron main block from raw CBOR bytes (the block body
    /// inside the `[1, body]` outer envelope).
    ///
    /// Main block structure: `[header, body, extra]`
    /// Header: `[protocol_magic, prev_hash, body_proof,
    ///   consensus_data, extra_data]`
    /// Consensus data: `[slot_id, pubkey, difficulty, signature]`
    /// Slot id: `[epoch, slot_in_epoch]`
    ///
    /// `difficulty` is CBOR-encoded as `[Word64]` (a 1-element array
    /// wrapping the difficulty value).
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

        // Capture the header byte range for hash computation.
        let hdr_start = dec.position();

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

        // pubkey
        dec.skip()?;

        // difficulty: [Word64] — 1-element array.
        let diff_len = dec.array()?;
        if diff_len < 1 {
            return Err(LedgerError::CborInvalidLength {
                expected: 1,
                actual: diff_len as usize,
            });
        }
        let chain_difficulty = dec.unsigned()?;

        // Skip remaining consensus fields (signature) + extra_data.
        dec.skip()?;
        dec.skip()?;

        let hdr_end = dec.position();
        let raw_header = raw[hdr_start..hdr_end].to_vec();

        Ok(Self::MainBlock {
            epoch,
            slot_in_epoch,
            chain_difficulty,
            prev_hash,
            raw_header,
        })
    }
}
