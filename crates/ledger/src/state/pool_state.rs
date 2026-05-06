//! Stake-pool registry state — `RegisteredPool`, `PoolRelayAccessPoint`,
//! and the `PoolState` container.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.State.PoolState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/libs/cardano-ledger-core/src/Cardano/Ledger/State/PoolState.hs)
//! and the `PState` registry from
//! [`Cardano.Ledger.Shelley.LedgerState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs).
//!
//! Tracks `psStakePoolParams` + `psRetiring` together as `entries` and
//! `psFutureStakePoolParams` as `future_params`. The fourth upstream map,
//! `psVRFKeyHashes`, is computed on demand via `find_pool_by_vrf_key`.
//!
//! Extracted from `state.rs` in R269 sixth slice as part of the strict 1:1
//! filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269f-state-pool-state-extraction.md`.

use super::phase1_validation::relay_access_points_from_relays;
use super::{decode_optional_epoch_no, encode_optional_epoch_no};
use crate::types::{EpochNo, PoolKeyHash, PoolParams, VrfKeyHash};
use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};
use std::collections::BTreeMap;

/// Registered stake-pool state carried by the ledger.
///
/// Mirrors upstream `StakePoolState` which carries `spsParams`,
/// `spsDeposit`, and optional retirement epoch.
///
/// Reference: `Cardano.Ledger.State.PoolState` — `spsDeposit`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredPool {
    pub(super) params: PoolParams,
    pub(super) retiring_epoch: Option<EpochNo>,
    /// The deposit paid at registration time (upstream `spsDeposit`).
    ///
    /// Used at retirement to refund the *correct* amount even if
    /// `pp_poolDeposit` changed since registration.
    pub(super) deposit: u64,
}

/// A directly dialable access point extracted from stake-pool relay data.
///
/// This captures only relay forms that can be converted into a concrete
/// host-plus-port endpoint without extra SRV lookup state.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PoolRelayAccessPoint {
    /// DNS name or IP address string.
    pub address: String,
    /// TCP port number.
    pub port: u16,
}

impl CborEncode for RegisteredPool {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(3);
        self.params.encode_cbor(enc);
        encode_optional_epoch_no(self.retiring_epoch, enc);
        enc.unsigned(self.deposit);
    }
}

impl CborDecode for RegisteredPool {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        // Backward-compatible: accept legacy 2-element (no deposit) or
        // new 3-element format.
        if len != 2 && len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            });
        }

        let params = PoolParams::decode_cbor(dec)?;
        let retiring_epoch = decode_optional_epoch_no(dec)?;
        let deposit = if len >= 3 { dec.unsigned()? } else { 0 };

        Ok(Self {
            params,
            retiring_epoch,
            deposit,
        })
    }
}

impl RegisteredPool {
    /// Returns the registered pool parameters.
    pub fn params(&self) -> &PoolParams {
        &self.params
    }

    /// Returns the scheduled retirement epoch, if any.
    pub fn retiring_epoch(&self) -> Option<EpochNo> {
        self.retiring_epoch
    }

    /// Returns the deposit paid at registration time.
    ///
    /// Reference: upstream `spsDeposit` in `StakePoolState`.
    pub fn deposit(&self) -> u64 {
        self.deposit
    }

    /// Returns directly dialable relay access points for the pool.
    ///
    /// This includes single-host address and single-host DNS relays that
    /// declare a port. Multi-host DNS relays and relays without a port are
    /// omitted because they require extra resolution or policy above the
    /// shared ledger layer.
    pub fn relay_access_points(&self) -> Vec<PoolRelayAccessPoint> {
        relay_access_points_from_relays(&self.params.relays)
    }
}

/// Stake-pool registry state visible from the ledger.
///
/// Upstream `PState` carries four maps:
/// - `psStakePoolParams`        — currently effective pool parameters
/// - `psFutureStakePoolParams`  — re-registration params staged for next epoch
/// - `psRetiring`               — pools scheduled for retirement (embedded in our entries)
/// - `psVRFKeyHashes`           — VRF key dedup (derived on the fly in our implementation)
///
/// We model `psStakePoolParams` + `psRetiring` in `entries`, and
/// `psFutureStakePoolParams` in `future_params`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PoolState {
    pub(super) entries: BTreeMap<PoolKeyHash, RegisteredPool>,
    /// Re-registered pool params staged for adoption at the next epoch
    /// boundary. Reference: upstream `psFutureStakePoolParams`.
    pub(super) future_params: BTreeMap<PoolKeyHash, PoolParams>,
}

impl CborEncode for PoolState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        // New format: CBOR map with keys 0 (entries) and 1 (future_params).
        // Key 1 is only emitted when future_params is non-empty.
        let map_len = if self.future_params.is_empty() { 1 } else { 2 };
        enc.map(map_len);
        // Key 0: entries
        enc.unsigned(0);
        enc.array(self.entries.len() as u64);
        for pool in self.entries.values() {
            pool.encode_cbor(enc);
        }
        // Key 1: future_params (only when non-empty)
        if !self.future_params.is_empty() {
            enc.unsigned(1);
            enc.array(self.future_params.len() as u64);
            for params in self.future_params.values() {
                params.encode_cbor(enc);
            }
        }
    }
}

impl CborDecode for PoolState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let major = dec.peek_major()?;
        if major == 5 {
            // New format: CBOR map
            let map_len = dec.map()?;
            let mut entries = BTreeMap::new();
            let mut future_params = BTreeMap::new();
            for _ in 0..map_len {
                let key = dec.unsigned()?;
                match key {
                    0 => {
                        let len = dec.array()?;
                        for _ in 0..len {
                            let pool = RegisteredPool::decode_cbor(dec)?;
                            entries.insert(pool.params.operator, pool);
                        }
                    }
                    1 => {
                        let len = dec.array()?;
                        for _ in 0..len {
                            let params = PoolParams::decode_cbor(dec)?;
                            future_params.insert(params.operator, params);
                        }
                    }
                    _ => {
                        // Skip unknown keys for forward compatibility.
                        dec.skip()?;
                    }
                }
            }
            Ok(Self {
                entries,
                future_params,
            })
        } else {
            // Legacy format: bare array of RegisteredPool (no future_params).
            let len = dec.array()?;
            let mut entries = BTreeMap::new();
            for _ in 0..len {
                let pool = RegisteredPool::decode_cbor(dec)?;
                entries.insert(pool.params.operator, pool);
            }
            Ok(Self {
                entries,
                future_params: BTreeMap::new(),
            })
        }
    }
}

impl PoolState {
    /// Creates an empty pool-state container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the registered state for `operator`, if present.
    pub fn get(&self, operator: &PoolKeyHash) -> Option<&RegisteredPool> {
        self.entries.get(operator)
    }

    /// Returns mutable registered state for `operator`, if present.
    pub fn get_mut(&mut self, operator: &PoolKeyHash) -> Option<&mut RegisteredPool> {
        self.entries.get_mut(operator)
    }

    /// Returns true when `operator` is registered.
    pub fn is_registered(&self, operator: &PoolKeyHash) -> bool {
        self.entries.contains_key(operator)
    }

    /// Iterates over registered pools in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&PoolKeyHash, &RegisteredPool)> {
        self.entries.iter()
    }

    /// Returns the number of registered pools.
    ///
    /// O(1) via the underlying `BTreeMap::len`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when no pools are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns all directly dialable relay access points from registered pools.
    ///
    /// The result is deduplicated in stable pool iteration order.
    pub fn relay_access_points(&self) -> Vec<PoolRelayAccessPoint> {
        let mut access_points = Vec::new();
        for pool in self.entries.values() {
            for access_point in pool.relay_access_points() {
                if !access_points.contains(&access_point) {
                    access_points.push(access_point);
                }
            }
        }
        access_points
    }

    /// Registers a new pool or stages a re-registration for the next epoch.
    ///
    /// **New registration** (`operator` not in `entries`): creates a new
    /// `RegisteredPool` entry with the given `deposit`.
    ///
    /// **Re-registration** (`operator` already in `entries`): stages the new
    /// `params` in `future_params` for adoption at the next epoch boundary.
    /// The original deposit is preserved, and any pending retirement is
    /// cleared.  Reference: upstream `poolTransition` in
    /// `Cardano.Ledger.Shelley.Rules.Pool` — re-registration inserts into
    /// `psFutureStakePoolParams` and deletes from `psRetiring`.
    pub fn register_with_deposit(&mut self, params: PoolParams, deposit: u64) {
        let operator = params.operator;
        if let Some(existing) = self.entries.get_mut(&operator) {
            // Re-registration: stage future params, unretire.
            existing.retiring_epoch = None;
            self.future_params.insert(operator, params);
        } else {
            // New registration.
            self.entries.insert(
                operator,
                RegisteredPool {
                    params,
                    retiring_epoch: None,
                    deposit,
                },
            );
        }
    }

    /// Inserts or replaces the registration for a pool operator
    /// (legacy convenience overload — deposit defaults to 0).
    pub fn register(&mut self, params: PoolParams) {
        self.register_with_deposit(params, 0);
    }

    /// Marks a registered pool as retiring at `epoch`.
    ///
    /// Returns `true` when the pool existed and was updated.
    pub fn retire(&mut self, operator: PoolKeyHash, epoch: EpochNo) -> bool {
        let Some(entry) = self.entries.get_mut(&operator) else {
            return false;
        };

        entry.retiring_epoch = Some(epoch);
        true
    }

    /// Removes all pools whose `retiring_epoch` ≤ `current_epoch`.
    ///
    /// Also clears any staged `future_params` for the retired pools.
    /// Returns the operator keys of the pools that were retired.
    pub fn process_retirements(&mut self, current_epoch: EpochNo) -> Vec<PoolKeyHash> {
        let retiring: Vec<PoolKeyHash> = self
            .entries
            .iter()
            .filter(|(_, pool)| pool.retiring_epoch.is_some_and(|e| e <= current_epoch))
            .map(|(k, _)| *k)
            .collect();
        for key in &retiring {
            self.entries.remove(key);
            self.future_params.remove(key);
        }
        retiring
    }

    /// Returns the operator key of the pool that already uses `vrf_key`, if any.
    ///
    /// Searches both current entries and staged future_params.
    /// This implements the lookup behind upstream `psVRFKeyHashes` for the
    /// `VRFKeyHashAlreadyRegistered` predicate check.
    pub fn find_pool_by_vrf_key(&self, vrf_key: &VrfKeyHash) -> Option<PoolKeyHash> {
        // Check future params first (they represent the latest intent).
        for (operator, params) in &self.future_params {
            if params.vrf_keyhash == *vrf_key {
                return Some(*operator);
            }
        }
        for (operator, pool) in &self.entries {
            // Skip entries that have a future_params override (already checked).
            if self.future_params.contains_key(operator) {
                continue;
            }
            if pool.params.vrf_keyhash == *vrf_key {
                return Some(*operator);
            }
        }
        None
    }

    /// Returns the staged future params map (upstream
    /// `psFutureStakePoolParams`).
    pub fn future_params(&self) -> &BTreeMap<PoolKeyHash, PoolParams> {
        &self.future_params
    }

    /// Adopts staged future pool params into current entries, preserving
    /// each pool's deposit and clearing the future set.
    ///
    /// Upstream: SNAP rule merges `psFutureStakePoolParams` into
    /// `psStakePoolParams` at epoch boundary, carrying forward
    /// `spsDeposit` and resetting the future map.
    pub fn adopt_future_params(&mut self) {
        let staged = std::mem::take(&mut self.future_params);
        for (operator, params) in staged {
            if let Some(entry) = self.entries.get_mut(&operator) {
                entry.params = params;
            }
            // If the pool was removed (retired) between re-registration and
            // epoch boundary, the future params are silently dropped.
        }
    }
}
