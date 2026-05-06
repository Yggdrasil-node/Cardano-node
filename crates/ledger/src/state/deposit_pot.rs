//! `DepositPot` — aggregate deposit accounting tracked by the ledger.
//!
//! Mirrors upstream `Obligations` (`oblStake`, `oblPool`, `oblDRep`,
//! `oblProposal`) from
//! [`Cardano.Ledger.State.CertState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/libs/cardano-ledger-core/src/Cardano/Ledger/State/CertState.hs)
//! and the `utxosDeposited` field of
//! [`Cardano.Ledger.Shelley.LedgerState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs).
//!
//! At epoch boundaries, deposit refunds (from unregistrations, pool
//! retirements, and expired or enacted proposals) are paid out and deducted
//! from this pot.
//!
//! Extracted from `state.rs` in R269 fourth slice as part of the strict 1:1
//! filename-mirror refactor — see `docs/operational-runs/2026-05-06-round-269d-state-deposit-pot-extraction.md`.

use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};

/// Aggregate deposit accounting tracked by the ledger.
///
/// Tracks the total lovelace locked in key deposits, pool deposits, DRep
/// deposits, and governance action (proposal) deposits.  At epoch boundaries
/// deposit refunds (from unregistrations, pool retirements, and expired or
/// enacted proposals) are paid out and deducted from this pot.
///
/// Reference: upstream `Obligations` (`oblStake`, `oblPool`, `oblDRep`,
/// `oblProposal`) from `Cardano.Ledger.State.CertState`, and
/// `utxosDeposited` from `Cardano.Ledger.Shelley.LedgerState`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DepositPot {
    /// Total lovelace deposited for key registrations.
    pub key_deposits: u64,
    /// Total lovelace deposited for pool registrations.
    pub pool_deposits: u64,
    /// Total lovelace deposited for DRep registrations (Conway+).
    pub drep_deposits: u64,
    /// Total lovelace deposited for governance action proposals (Conway+).
    ///
    /// Reference: upstream `oblProposal` from `Obligations`.
    pub proposal_deposits: u64,
}

impl DepositPot {
    /// Returns the total deposits across all categories (upstream `sumObligation`).
    pub fn total(&self) -> u64 {
        self.key_deposits
            .saturating_add(self.pool_deposits)
            .saturating_add(self.drep_deposits)
            .saturating_add(self.proposal_deposits)
    }

    /// Adds a key deposit.
    pub fn add_key_deposit(&mut self, amount: u64) {
        self.key_deposits = self.key_deposits.saturating_add(amount);
    }

    /// Returns a key deposit.
    pub fn return_key_deposit(&mut self, amount: u64) {
        self.key_deposits = self.key_deposits.saturating_sub(amount);
    }

    /// Adds a pool deposit.
    pub fn add_pool_deposit(&mut self, amount: u64) {
        self.pool_deposits = self.pool_deposits.saturating_add(amount);
    }

    /// Returns a pool deposit.
    pub fn return_pool_deposit(&mut self, amount: u64) {
        self.pool_deposits = self.pool_deposits.saturating_sub(amount);
    }

    /// Adds a DRep deposit.
    pub fn add_drep_deposit(&mut self, amount: u64) {
        self.drep_deposits = self.drep_deposits.saturating_add(amount);
    }

    /// Returns a DRep deposit.
    pub fn return_drep_deposit(&mut self, amount: u64) {
        self.drep_deposits = self.drep_deposits.saturating_sub(amount);
    }

    /// Adds a governance action proposal deposit (Conway+).
    pub fn add_proposal_deposit(&mut self, amount: u64) {
        self.proposal_deposits = self.proposal_deposits.saturating_add(amount);
    }

    /// Returns a governance action proposal deposit (Conway+).
    pub fn return_proposal_deposit(&mut self, amount: u64) {
        self.proposal_deposits = self.proposal_deposits.saturating_sub(amount);
    }
}

impl CborEncode for DepositPot {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4);
        enc.unsigned(self.key_deposits);
        enc.unsigned(self.pool_deposits);
        enc.unsigned(self.drep_deposits);
        enc.unsigned(self.proposal_deposits);
    }
}

impl CborDecode for DepositPot {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        match len {
            3 => Ok(Self {
                key_deposits: dec.unsigned()?,
                pool_deposits: dec.unsigned()?,
                drep_deposits: dec.unsigned()?,
                proposal_deposits: 0,
            }),
            4 => Ok(Self {
                key_deposits: dec.unsigned()?,
                pool_deposits: dec.unsigned()?,
                drep_deposits: dec.unsigned()?,
                proposal_deposits: dec.unsigned()?,
            }),
            _ => Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            }),
        }
    }
}
