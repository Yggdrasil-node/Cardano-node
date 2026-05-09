//! `LedgerStateCheckpoint` — restorable checkpoint of full ledger state.
//!
//! Companion sidecar to [`super::LedgerState`] used by storage and node
//! orchestration as a rollback / recovery seam: unlike
//! [`super::LedgerStateSnapshot`] (a read-only LSQ-friendly capture
//! covering only the visible query surface), `LedgerStateCheckpoint`
//! preserves a full restorable copy of the entire mutable ledger state.
//!
//! Used by `crates/storage/src/chain_db.rs` (recovery + checkpoint
//! persistence) and `crates/storage/src/ocert_sidecar.rs` (file-backed
//! checkpoint storage).
//!
//! Extracted from `state.rs` in R269 fourteenth slice as part of the strict
//! 1:1 filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269n-state-checkpoint-extraction.md`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-specific restorable-checkpoint
//! concept used by `crates/storage/src/chain_db.rs` for rollback and
//! recovery. Upstream's on-disk snapshot codec lives in
//! `Ouroboros.Consensus.Storage.LedgerDB.Snapshots` and serves a
//! different role (file format, not in-memory restore copy).

use super::LedgerState;
use crate::types::Point;
use crate::{CborDecode, CborEncode, Decoder, Encoder, Era, LedgerError};

/// Restorable checkpoint of full ledger state.
///
/// This checkpoint is intended as a rollback and recovery seam for higher
/// layers such as storage and node orchestration. Unlike
/// [`super::LedgerStateSnapshot`], it preserves a restorable copy of the entire
/// mutable ledger state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerStateCheckpoint {
    pub(super) state: LedgerState,
}

impl CborEncode for LedgerStateCheckpoint {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(1);
        self.state.encode_cbor(enc);
    }
}

impl CborDecode for LedgerStateCheckpoint {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 1 {
            return Err(LedgerError::CborInvalidLength {
                expected: 1,
                actual: len as usize,
            });
        }

        Ok(Self {
            state: LedgerState::decode_cbor(dec)?,
        })
    }
}

impl LedgerStateCheckpoint {
    /// Returns the era captured by the checkpoint.
    pub fn current_era(&self) -> Era {
        self.state.current_era
    }

    /// Returns the tip captured by the checkpoint.
    pub fn tip(&self) -> &Point {
        &self.state.tip
    }

    /// Restores the captured ledger state by cloning it out of the checkpoint.
    pub fn restore(&self) -> LedgerState {
        self.state.clone()
    }
}
