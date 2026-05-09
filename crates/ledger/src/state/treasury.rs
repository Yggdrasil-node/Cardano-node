//! `AccountingState` — treasury and reserves accounting tracked by the ledger.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.Shelley.LedgerState::esAccountState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs).
//!
//! `treasury` and `reserves` are the two pots the protocol moves lovelace
//! between (rewards distribution, MIR transfers, treasury withdrawals,
//! monetary expansion via ρ).
//!
//! Extracted from `state.rs` in R269 twelfth slice as part of the strict 1:1
//! filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269l-state-treasury-chaindep-extraction.md`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Mirrors the `esAccountState` field of
//! upstream `Cardano.Ledger.Shelley.LedgerState::EpochState`
//! (treasury + reserves accounting). Defined inline upstream;
//! Yggdrasil isolates for cohesion since the two pots are operated
//! as a unit by rewards distribution + MIR + treasury withdrawals.

use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};

/// Treasury and reserves accounting tracked by the ledger.
///
/// Reference: `Cardano.Ledger.Shelley.LedgerState` — `esAccountState`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccountingState {
    /// Total lovelace in the treasury.
    pub treasury: u64,
    /// Total lovelace in the reserves.
    pub reserves: u64,
}

impl CborEncode for AccountingState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        enc.unsigned(self.treasury);
        enc.unsigned(self.reserves);
    }
}

impl CborDecode for AccountingState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        Ok(Self {
            treasury: dec.unsigned()?,
            reserves: dec.unsigned()?,
        })
    }
}
