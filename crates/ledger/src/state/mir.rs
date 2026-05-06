//! `InstantaneousRewards` — MIR (Move Instantaneous Reward) accumulation state.
//!
//! This module mirrors upstream
//! `Cardano.Ledger.Shelley.LedgerState`'s `InstantaneousRewards` record and the
//! per-epoch processing performed by
//! [`Cardano.Ledger.Shelley.Rules.Mir`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Mir.hs).
//!
//! Extracted from `state.rs` in R269 first slice as part of the strict 1:1
//! filename-mirror refactor — see `docs/operational-runs/2026-05-06-round-269-state-mir-extraction.md`.

use crate::types::StakeCredential;
use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};
use std::collections::BTreeMap;

/// Accumulated instantaneous rewards (MIR) state.
///
/// During Shelley through Babbage block application, `MoveInstantaneousReward`
/// certificates accumulate per-credential reward deltas and pot-to-pot
/// transfer deltas here.  At each epoch boundary the MIR rule processes the
/// accumulated state: per-credential amounts are credited to reward accounts
/// (if still registered), pot transfers adjust reserves/treasury, and the
/// accumulator is cleared.
///
/// Invariant: `delta_reserves + delta_treasury == 0` (transfers are
/// zero-sum between the two pots).
///
/// Reference: `Cardano.Ledger.Shelley.LedgerState` — `InstantaneousRewards`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstantaneousRewards {
    /// Per-credential reward deltas sourced from reserves.
    ///
    /// Accumulated via `StakeCredentials` MIR targets with `MirPot::Reserves`.
    /// Positive values credit; post-Alonzo negative deltas are allowed as
    /// long as the combined map stays non-negative.
    pub ir_reserves: BTreeMap<StakeCredential, i64>,
    /// Per-credential reward deltas sourced from treasury.
    ///
    /// Accumulated via `StakeCredentials` MIR targets with `MirPot::Treasury`.
    pub ir_treasury: BTreeMap<StakeCredential, i64>,
    /// Signed delta applied to reserves for pot-to-pot transfers.
    ///
    /// A negative value means reserves are being transferred out (to
    /// treasury); a positive value means reserves are receiving from
    /// treasury.
    pub delta_reserves: i64,
    /// Signed delta applied to treasury for pot-to-pot transfers.
    ///
    /// Invariant: `delta_treasury == -delta_reserves`.
    pub delta_treasury: i64,
}

impl InstantaneousRewards {
    /// Returns `true` when there are no accumulated MIR entries.
    pub fn is_empty(&self) -> bool {
        self.ir_reserves.is_empty()
            && self.ir_treasury.is_empty()
            && self.delta_reserves == 0
            && self.delta_treasury == 0
    }

    /// Clears all accumulated MIR state.
    pub fn clear(&mut self) {
        self.ir_reserves.clear();
        self.ir_treasury.clear();
        self.delta_reserves = 0;
        self.delta_treasury = 0;
    }
}

impl CborEncode for InstantaneousRewards {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4);
        // ir_reserves: map credential → i64
        enc.map(self.ir_reserves.len() as u64);
        for (cred, &delta) in &self.ir_reserves {
            cred.encode_cbor(enc);
            enc.integer(delta);
        }
        // ir_treasury: map credential → i64
        enc.map(self.ir_treasury.len() as u64);
        for (cred, &delta) in &self.ir_treasury {
            cred.encode_cbor(enc);
            enc.integer(delta);
        }
        // delta_reserves, delta_treasury
        enc.integer(self.delta_reserves);
        enc.integer(self.delta_treasury);
    }
}

impl CborDecode for InstantaneousRewards {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }
        let map_len = dec.map()?;
        let mut ir_reserves = BTreeMap::new();
        for _ in 0..map_len {
            let cred = StakeCredential::decode_cbor(dec)?;
            let delta = dec.integer()?;
            ir_reserves.insert(cred, delta);
        }
        let map_len = dec.map()?;
        let mut ir_treasury = BTreeMap::new();
        for _ in 0..map_len {
            let cred = StakeCredential::decode_cbor(dec)?;
            let delta = dec.integer()?;
            ir_treasury.insert(cred, delta);
        }
        let delta_reserves = dec.integer()?;
        let delta_treasury = dec.integer()?;
        Ok(Self {
            ir_reserves,
            ir_treasury,
            delta_reserves,
            delta_treasury,
        })
    }
}
