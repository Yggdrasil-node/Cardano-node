//! Stake-credential registry — `StakeCredentialState` and the
//! `StakeCredentials` map container.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.Shelley.LedgerState::DState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs)'s
//! `dsUnified` map keyed by `Credential Staking`, with the upstream `rdDeposit`
//! captured per registration.
//!
//! Holds per-credential delegation targets (`delegated_pool`,
//! `delegated_drep`) and the deposit paid at registration time. The map
//! supports the GovCert / HardFork / PoolReap rule cleanup flows
//! (clearing DRep / pool delegations on revocation, retirement, or
//! dangling references).
//!
//! Extracted from `state.rs` in R269 eighth slice as part of the strict 1:1
//! filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269h-state-stake-credentials-extraction.md`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Subset of
//! `Cardano.Ledger.Shelley.LedgerState::DState::dsUnified` —
//! specifically the per-credential delegation-target + deposit map.
//! Yggdrasil splits the DState upstream concept into per-component
//! sub-modules; upstream keeps the unified map under one struct.

use super::{
    DrepState, decode_optional_drep, decode_optional_pool_key_hash, encode_optional_drep,
    encode_optional_pool_key_hash, is_builtin_drep,
};
use crate::types::{DRep, PoolKeyHash, StakeCredential};
use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};
use std::collections::BTreeMap;

/// Registered stake-credential state visible from the ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StakeCredentialState {
    pub(super) delegated_pool: Option<PoolKeyHash>,
    pub(super) delegated_drep: Option<DRep>,
    /// The deposit paid at registration time (upstream `rdDeposit` in UMap).
    /// Used to compute the correct refund on unregistration, since the
    /// protocol parameter `keyDeposit` may have changed since registration.
    pub(super) deposit: u64,
}

impl CborEncode for StakeCredentialState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(3);
        encode_optional_pool_key_hash(self.delegated_pool, enc);
        encode_optional_drep(self.delegated_drep.as_ref(), enc);
        enc.unsigned(self.deposit);
    }
}

impl CborDecode for StakeCredentialState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 && len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            });
        }

        let delegated_pool = decode_optional_pool_key_hash(dec)?;
        let delegated_drep = decode_optional_drep(dec)?;
        let deposit = if len >= 3 { dec.unsigned()? } else { 0 };

        Ok(Self {
            delegated_pool,
            delegated_drep,
            deposit,
        })
    }
}

impl StakeCredentialState {
    /// Creates stake-credential state with the given delegation targets and zero deposit.
    pub fn new(delegated_pool: Option<PoolKeyHash>, delegated_drep: Option<DRep>) -> Self {
        Self {
            delegated_pool,
            delegated_drep,
            deposit: 0,
        }
    }

    /// Creates stake-credential state with the given delegation targets and deposit.
    pub fn new_with_deposit(
        delegated_pool: Option<PoolKeyHash>,
        delegated_drep: Option<DRep>,
        deposit: u64,
    ) -> Self {
        Self {
            delegated_pool,
            delegated_drep,
            deposit,
        }
    }

    /// Returns the deposit paid at registration time (upstream `rdDeposit`).
    pub fn deposit(&self) -> u64 {
        self.deposit
    }

    /// Returns the delegated pool, if any.
    pub fn delegated_pool(&self) -> Option<PoolKeyHash> {
        self.delegated_pool
    }

    /// Returns the delegated DRep, if any.
    pub fn delegated_drep(&self) -> Option<DRep> {
        self.delegated_drep
    }

    /// Replaces the delegated pool reference.
    pub fn set_delegated_pool(&mut self, delegated_pool: Option<PoolKeyHash>) {
        self.delegated_pool = delegated_pool;
    }

    /// Replaces the delegated DRep reference.
    pub fn set_delegated_drep(&mut self, delegated_drep: Option<DRep>) {
        self.delegated_drep = delegated_drep;
    }
}

/// Stake-credential map visible from the ledger.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StakeCredentials {
    pub(super) entries: BTreeMap<StakeCredential, StakeCredentialState>,
}

impl CborEncode for StakeCredentials {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(self.entries.len() as u64);
        for (credential, state) in &self.entries {
            enc.array(2);
            credential.encode_cbor(enc);
            state.encode_cbor(enc);
        }
    }
}

impl CborDecode for StakeCredentials {
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

            let credential = StakeCredential::decode_cbor(dec)?;
            let state = StakeCredentialState::decode_cbor(dec)?;
            entries.insert(credential, state);
        }
        Ok(Self { entries })
    }
}

impl StakeCredentials {
    /// Creates an empty stake-credential container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the state for `credential`, if present.
    pub fn get(&self, credential: &StakeCredential) -> Option<&StakeCredentialState> {
        self.entries.get(credential)
    }

    /// Returns mutable state for `credential`, if present.
    pub fn get_mut(&mut self, credential: &StakeCredential) -> Option<&mut StakeCredentialState> {
        self.entries.get_mut(credential)
    }

    /// Iterates over registered stake credentials in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&StakeCredential, &StakeCredentialState)> {
        self.entries.iter()
    }

    /// Returns the number of registered stake credentials.
    ///
    /// O(1) via the underlying `BTreeMap::len`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when no stake credentials are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns true when `credential` is registered.
    pub fn is_registered(&self, credential: &StakeCredential) -> bool {
        self.entries.contains_key(credential)
    }

    /// Registers a stake credential with no delegation target and zero deposit.
    ///
    /// Returns `true` when the credential was freshly registered.
    /// Returns `false` (already registered) **without** modifying the
    /// existing entry — upstream never overwrites an existing
    /// `StakeCredentialState` on duplicate registration.
    pub fn register(&mut self, credential: StakeCredential) -> bool {
        if self.entries.contains_key(&credential) {
            return false;
        }
        self.entries
            .insert(credential, StakeCredentialState::new(None, None));
        true
    }

    /// Registers a stake credential with no delegation target and the given deposit.
    ///
    /// Returns `true` when the credential was freshly registered.
    /// Returns `false` (already registered) **without** overwriting the
    /// existing entry's delegation or deposit state.
    pub fn register_with_deposit(&mut self, credential: StakeCredential, deposit: u64) -> bool {
        if self.entries.contains_key(&credential) {
            return false;
        }
        self.entries.insert(
            credential,
            StakeCredentialState::new_with_deposit(None, None, deposit),
        );
        true
    }

    /// Removes a registered stake credential.
    pub fn unregister(&mut self, credential: &StakeCredential) -> Option<StakeCredentialState> {
        self.entries.remove(credential)
    }

    /// Clears DRep delegation from all stake credentials delegated to `drep`.
    ///
    /// Upstream: `clearDRepDelegations` in `Cardano.Ledger.Conway.Rules.GovCert`.
    pub fn clear_drep_delegation(&mut self, drep: &DRep) {
        for state in self.entries.values_mut() {
            if state.delegated_drep.as_ref() == Some(drep) {
                state.delegated_drep = None;
            }
        }
    }

    /// Clears DRep delegation from stake credentials that point to a
    /// non-existent (unregistered) DRep.
    ///
    /// Upstream: `updateDRepDelegations` in
    /// `Cardano.Ledger.Conway.Rules.HardFork` — called at the PV 9→10
    /// transition (bootstrap → post-bootstrap) to remove dangling
    /// delegations created during bootstrap phase when delegating to
    /// unregistered DReps was allowed.
    pub fn cleanup_dangling_drep_delegations(&mut self, drep_state: &DrepState) -> usize {
        let mut cleaned = 0usize;
        for state in self.entries.values_mut() {
            if let Some(ref drep) = state.delegated_drep {
                if !is_builtin_drep(*drep) && !drep_state.is_registered(drep) {
                    state.delegated_drep = None;
                    cleaned += 1;
                }
            }
        }
        cleaned
    }

    /// Clears pool delegation from all stake credentials delegated to any of
    /// the given retired pools.
    ///
    /// Upstream: `removeStakePoolDelegations` in
    /// `Cardano.Ledger.Shelley.Rules.PoolReap` — called at epoch boundary
    /// to decouple delegators from pools that have just retired.
    pub fn clear_pool_delegations(&mut self, retired: &[PoolKeyHash]) {
        if retired.is_empty() {
            return;
        }
        for state in self.entries.values_mut() {
            if let Some(pool) = state.delegated_pool {
                if retired.contains(&pool) {
                    state.delegated_pool = None;
                }
            }
        }
    }
}
