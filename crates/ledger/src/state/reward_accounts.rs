//! Reward-account state — `RewardAccountState` (per-account balance +
//! delegated pool) and the `RewardAccounts` map container.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.State.AccountState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/libs/cardano-ledger-core/src/Cardano/Ledger/State/AccountState.hs)
//! and the `dsUnified` reward-account portion of
//! [`Cardano.Ledger.Shelley.LedgerState::DState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs).
//!
//! Extracted from `state.rs` in R269 seventh slice as part of the strict 1:1
//! filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269g-state-reward-accounts-extraction.md`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Combines `AccountState` (per-account
//! balance, defined inline in `Cardano.Ledger.State.CertState`) with
//! the `dsUnified` reward-account portion of
//! `Cardano.Ledger.Shelley.LedgerState::DState`. Yggdrasil splits the
//! DState upstream concept into per-component sub-modules
//! (`reward_accounts.rs`, `stake_credentials.rs`) for cohesion;
//! upstream keeps the unified map under one struct.

use super::{decode_optional_pool_key_hash, encode_optional_pool_key_hash};
use crate::types::{PoolKeyHash, RewardAccount, StakeCredential};
use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};
use std::collections::BTreeMap;

/// Reward-account state visible from the ledger.
///
/// This container tracks the current reward balance and the delegated pool, if
/// one has been recorded for the account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardAccountState {
    pub(super) balance: u64,
    pub(super) delegated_pool: Option<PoolKeyHash>,
}

impl CborEncode for RewardAccountState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2).unsigned(self.balance);
        encode_optional_pool_key_hash(self.delegated_pool, enc);
    }
}

impl CborDecode for RewardAccountState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }

        Ok(Self {
            balance: dec.unsigned()?,
            delegated_pool: decode_optional_pool_key_hash(dec)?,
        })
    }
}

impl RewardAccountState {
    /// Creates reward-account state with the given balance and delegation.
    pub fn new(balance: u64, delegated_pool: Option<PoolKeyHash>) -> Self {
        Self {
            balance,
            delegated_pool,
        }
    }

    /// Returns the current reward balance.
    pub fn balance(&self) -> u64 {
        self.balance
    }

    /// Returns the delegated pool, if any.
    pub fn delegated_pool(&self) -> Option<PoolKeyHash> {
        self.delegated_pool
    }

    /// Replaces the reward balance.
    pub fn set_balance(&mut self, balance: u64) {
        self.balance = balance;
    }

    /// Replaces the delegated pool reference.
    pub fn set_delegated_pool(&mut self, delegated_pool: Option<PoolKeyHash>) {
        self.delegated_pool = delegated_pool;
    }
}

/// Reward-account map visible from the ledger.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RewardAccounts {
    pub(super) entries: BTreeMap<RewardAccount, RewardAccountState>,
}

impl CborEncode for RewardAccounts {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(self.entries.len() as u64);
        for (account, state) in &self.entries {
            enc.array(2);
            account.encode_cbor(enc);
            state.encode_cbor(enc);
        }
    }
}

impl CborDecode for RewardAccounts {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        let mut entries = BTreeMap::new();
        for _ in 0..len {
            let pair_len = dec.array()?;
            if pair_len != 2 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 2,
                    actual: pair_len as usize,
                });
            }

            let account = RewardAccount::decode_cbor(dec)?;
            let state = RewardAccountState::decode_cbor(dec)?;
            entries.insert(account, state);
        }
        Ok(Self { entries })
    }
}

impl RewardAccounts {
    /// Creates an empty reward-account container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the state for `account`, if present.
    pub fn get(&self, account: &RewardAccount) -> Option<&RewardAccountState> {
        self.entries.get(account)
    }

    /// Returns mutable state for `account`, if present.
    pub fn get_mut(&mut self, account: &RewardAccount) -> Option<&mut RewardAccountState> {
        self.entries.get_mut(account)
    }

    /// Iterates over reward-account entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&RewardAccount, &RewardAccountState)> {
        self.entries.iter()
    }

    /// Returns the number of known reward accounts.
    ///
    /// O(1) via the underlying `BTreeMap::len`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when no reward accounts are known.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Inserts or replaces reward-account state.
    pub fn insert(
        &mut self,
        account: RewardAccount,
        state: RewardAccountState,
    ) -> Option<RewardAccountState> {
        self.entries.insert(account, state)
    }

    /// Returns the reward balance for `account`, defaulting to zero.
    pub fn balance(&self, account: &RewardAccount) -> u64 {
        self.entries
            .get(account)
            .map(RewardAccountState::balance)
            .unwrap_or(0)
    }

    /// Looks up the first registered reward account matching `credential`
    /// (any network byte).
    ///
    /// Upstream keys member rewards by `Credential Staking` and resolves
    /// the `RewardAccount` (including network byte) from the DState at
    /// application time.  This method provides the same lookup.
    pub fn find_account_by_credential(&self, cred: &StakeCredential) -> Option<&RewardAccount> {
        self.entries.keys().find(|acct| acct.credential == *cred)
    }

    /// Credits `amount` to the registered reward account matching
    /// `credential`.  Returns `true` if the account was found and
    /// credited, `false` if no matching account is registered.
    pub fn credit_by_credential(&mut self, cred: &StakeCredential, amount: u64) -> bool {
        // Find the matching RewardAccount key.
        if let Some(key) = self.entries.keys().find(|a| a.credential == *cred).copied() {
            if let Some(state) = self.entries.get_mut(&key) {
                state.set_balance(state.balance().saturating_add(amount));
                return true;
            }
        }
        false
    }
}
