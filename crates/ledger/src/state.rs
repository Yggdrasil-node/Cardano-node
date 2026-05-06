use crate::eras::allegra::AllegraTxBody;
use crate::eras::alonzo::AlonzoTxBody;
use crate::eras::babbage::BabbageTxBody;
use crate::eras::byron::ByronTx;
use crate::eras::conway::ConwayTxBody;
use crate::eras::mary::{MultiAsset, Value};
use crate::eras::shelley::{ShelleyTxBody, ShelleyTxIn, ShelleyUtxo};
use crate::types::{
    Address, Anchor, BlockNo, DCert, DRep, EpochNo, GenesisDelegateHash, GenesisHash, MirPot,
    MirTarget, Nonce, Point, PoolKeyHash, PoolParams, Relay, RewardAccount, StakeCredential,
    UnitInterval, VrfKeyHash,
};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use crate::{CborDecode, CborEncode, Decoder, Encoder, Era, LedgerError};
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};

type FutureGenesisDelegKey = (u64, GenesisHash);

// ---------------------------------------------------------------------------
// PPUP (Protocol Parameter Update Proposal) helpers
// ---------------------------------------------------------------------------

/// Slot-based context for full upstream PPUP epoch validation.
///
/// When provided to [`LedgerState::validate_ppup_proposal`], enables the
/// exact `getTheSlotOfNoReturn` check from upstream `Ppup.hs`:
///
/// * `too_late = first_slot(current_epoch + 1) - stability_window`
/// * If `slot < too_late`: target must equal `current_epoch` (VoteForThisEpoch).
/// * If `slot >= too_late`: target must equal `current_epoch + 1` (VoteForNextEpoch).
///
/// Reference: `Cardano.Ledger.Slot.getTheSlotOfNoReturn`.
#[derive(Clone, Debug)]
pub struct PpupSlotContext {
    /// Current slot of the transaction or block being applied.
    pub slot: u64,
    /// First slot of the next epoch — pre-resolved era-aware (mirrors
    /// upstream `epochInfoFirst (currentEpoch + 1)`). Caller must
    /// compute via [`LedgerState::epoch_first_slot`] so any chain
    /// with a Byron prefix uses the correct boundary, not the
    /// `(current + 1) * epoch_size` fixed-length math anchored at
    /// slot 0 (R263/R264 bug class).
    pub first_slot_next_epoch: u64,
    /// Stability window in slots (upstream `stabilityWindow`, typically `3k/f`).
    pub stability_window: u64,
}

/// Upstream `pvCanFollow` — check whether a proposed protocol version is a
/// legal successor to the current one.
///
/// Rules (from `Cardano.Ledger.Shelley.PParams`):
/// * `(succVersion curMajor, 0) == (Just newMajor, newMinor)` — major+1 with minor=0, OR
/// * `(curMajor, curMinor + 1) == (newMajor, newMinor)` — same major with minor+1.
pub fn pv_can_follow(cur_major: u64, cur_minor: u64, new_major: u64, new_minor: u64) -> bool {
    // Increment major by 1 and set minor to 0.
    let major_bump = new_major == cur_major + 1 && new_minor == 0;
    // Keep major, increment minor by 1.
    let minor_bump = new_major == cur_major && new_minor == cur_minor + 1;
    major_bump || minor_bump
}

fn overlay_step(offset_from_epoch_start: u64, d: UnitInterval) -> u128 {
    let denominator = d.denominator as u128;
    if denominator == 0 {
        return 0;
    }
    (offset_from_epoch_start as u128)
        .saturating_mul(d.numerator as u128)
        .div_ceil(denominator)
}

fn is_overlay_slot_for_blocks_made(first_slot: u64, d: UnitInterval, slot: u64) -> bool {
    if d.numerator == 0 || d.denominator == 0 || slot < first_slot {
        return false;
    }

    let offset = slot - first_slot;
    overlay_step(offset, d) < overlay_step(offset.saturating_add(1), d)
}

fn encode_optional_epoch_no(value: Option<EpochNo>, enc: &mut Encoder) {
    match value {
        Some(epoch) => epoch.encode_cbor(enc),
        None => {
            enc.null();
        }
    }
}

fn decode_optional_epoch_no(dec: &mut Decoder<'_>) -> Result<Option<EpochNo>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(EpochNo::decode_cbor(dec)?))
    }
}

fn encode_optional_pool_key_hash(value: Option<PoolKeyHash>, enc: &mut Encoder) {
    match value {
        Some(hash) => {
            enc.bytes(&hash);
        }
        None => {
            enc.null();
        }
    }
}

fn decode_optional_pool_key_hash(
    dec: &mut Decoder<'_>,
) -> Result<Option<PoolKeyHash>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        return Ok(None);
    }

    let raw = dec.bytes()?;
    let hash: [u8; 28] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
        expected: 28,
        actual: raw.len(),
    })?;
    Ok(Some(hash))
}

fn encode_optional_anchor(value: Option<&Anchor>, enc: &mut Encoder) {
    match value {
        Some(anchor) => anchor.encode_cbor(enc),
        None => {
            enc.null();
        }
    }
}

fn decode_optional_anchor(dec: &mut Decoder<'_>) -> Result<Option<Anchor>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(Anchor::decode_cbor(dec)?))
    }
}

fn encode_optional_drep(value: Option<&DRep>, enc: &mut Encoder) {
    match value {
        Some(drep) => drep.encode_cbor(enc),
        None => {
            enc.null();
        }
    }
}

fn decode_optional_drep(dec: &mut Decoder<'_>) -> Result<Option<DRep>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(DRep::decode_cbor(dec)?))
    }
}

fn encode_optional_gov_action_id(
    value: Option<&crate::eras::conway::GovActionId>,
    enc: &mut Encoder,
) {
    match value {
        Some(id) => id.encode_cbor(enc),
        None => {
            enc.null();
        }
    }
}

fn decode_optional_gov_action_id(
    dec: &mut Decoder<'_>,
) -> Result<Option<crate::eras::conway::GovActionId>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(crate::eras::conway::GovActionId::decode_cbor(dec)?))
    }
}

/// Registered stake-pool state carried by the ledger.
///
/// Mirrors upstream `StakePoolState` which carries `spsParams`,
/// `spsDeposit`, and optional retirement epoch.
///
/// Reference: `Cardano.Ledger.State.PoolState` — `spsDeposit`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredPool {
    params: PoolParams,
    retiring_epoch: Option<EpochNo>,
    /// The deposit paid at registration time (upstream `spsDeposit`).
    ///
    /// Used at retirement to refund the *correct* amount even if
    /// `pp_poolDeposit` changed since registration.
    deposit: u64,
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
    entries: BTreeMap<PoolKeyHash, RegisteredPool>,
    /// Re-registered pool params staged for adoption at the next epoch
    /// boundary. Reference: upstream `psFutureStakePoolParams`.
    future_params: BTreeMap<PoolKeyHash, PoolParams>,
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

/// Reward-account state visible from the ledger.
///
/// This container tracks the current reward balance and the delegated pool, if
/// one has been recorded for the account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardAccountState {
    balance: u64,
    delegated_pool: Option<PoolKeyHash>,
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
    entries: BTreeMap<RewardAccount, RewardAccountState>,
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

/// Genesis delegation entry: maps a genesis key to a delegate key and VRF
/// key, as found in the `genDelegs` section of the Shelley genesis file
/// and updatable via `GenesisDelegation` certificates.
///
/// Reference: `Cardano.Ledger.Shelley.Genesis` — `GenDelegs`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenesisDelegationState {
    pub delegate: GenesisDelegateHash,
    pub vrf: VrfKeyHash,
}

/// Registered stake-credential state visible from the ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StakeCredentialState {
    delegated_pool: Option<PoolKeyHash>,
    delegated_drep: Option<DRep>,
    /// The deposit paid at registration time (upstream `rdDeposit` in UMap).
    /// Used to compute the correct refund on unregistration, since the
    /// protocol parameter `keyDeposit` may have changed since registration.
    deposit: u64,
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
    entries: BTreeMap<StakeCredential, StakeCredentialState>,
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

/// Registered DRep state visible from the ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredDrep {
    anchor: Option<Anchor>,
    deposit: u64,
    /// The most recent epoch in which this DRep was considered active
    /// (registration, vote cast, or update).  `None` for legacy entries
    /// that predate activity tracking.
    last_active_epoch: Option<EpochNo>,
}

impl CborEncode for RegisteredDrep {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(3);
        encode_optional_anchor(self.anchor.as_ref(), enc);
        enc.unsigned(self.deposit);
        match self.last_active_epoch {
            Some(e) => enc.unsigned(e.0),
            None => enc.null(),
        };
    }
}

impl CborDecode for RegisteredDrep {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 && len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            });
        }

        let anchor = decode_optional_anchor(dec)?;
        let deposit = dec.unsigned()?;
        let last_active_epoch = if len >= 3 {
            if dec.peek_is_null() {
                dec.null()?;
                None
            } else {
                Some(EpochNo(dec.unsigned()?))
            }
        } else {
            None
        };

        Ok(Self {
            anchor,
            deposit,
            last_active_epoch,
        })
    }
}

impl RegisteredDrep {
    /// Creates registered DRep state.
    pub fn new(deposit: u64, anchor: Option<Anchor>) -> Self {
        Self {
            anchor,
            deposit,
            last_active_epoch: None,
        }
    }

    /// Creates registered DRep state with an initial activity epoch.
    pub fn new_active(deposit: u64, anchor: Option<Anchor>, epoch: EpochNo) -> Self {
        Self {
            anchor,
            deposit,
            last_active_epoch: Some(epoch),
        }
    }

    /// Returns the current DRep anchor, if any.
    pub fn anchor(&self) -> Option<&Anchor> {
        self.anchor.as_ref()
    }

    /// Returns the current DRep deposit value.
    pub fn deposit(&self) -> u64 {
        self.deposit
    }

    /// Returns the last epoch in which this DRep was active.
    pub fn last_active_epoch(&self) -> Option<EpochNo> {
        self.last_active_epoch
    }

    /// Records that this DRep was active in `epoch`.
    pub fn touch_activity(&mut self, epoch: EpochNo) {
        self.last_active_epoch = Some(epoch);
    }

    /// Replaces the current DRep anchor.
    pub fn set_anchor(&mut self, anchor: Option<Anchor>) {
        self.anchor = anchor;
    }
}

/// DRep registry visible from the ledger.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DrepState {
    entries: BTreeMap<DRep, RegisteredDrep>,
}

impl CborEncode for DrepState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(self.entries.len() as u64);
        for (drep, state) in &self.entries {
            enc.array(2);
            drep.encode_cbor(enc);
            state.encode_cbor(enc);
        }
    }
}

impl CborDecode for DrepState {
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

            let drep = DRep::decode_cbor(dec)?;
            let state = RegisteredDrep::decode_cbor(dec)?;
            entries.insert(drep, state);
        }
        Ok(Self { entries })
    }
}

impl DrepState {
    /// Creates an empty DRep registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the registered state for `drep`, if present.
    pub fn get(&self, drep: &DRep) -> Option<&RegisteredDrep> {
        self.entries.get(drep)
    }

    /// Returns mutable registered state for `drep`, if present.
    pub fn get_mut(&mut self, drep: &DRep) -> Option<&mut RegisteredDrep> {
        self.entries.get_mut(drep)
    }

    /// Returns true when `drep` is registered.
    pub fn is_registered(&self, drep: &DRep) -> bool {
        self.entries.contains_key(drep)
    }

    /// Iterates over registered DReps in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&DRep, &RegisteredDrep)> {
        self.entries.iter()
    }

    /// Returns a mutable iterator over registered DRep entries.
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut RegisteredDrep> {
        self.entries.values_mut()
    }

    /// Registers a DRep.
    ///
    /// Returns `true` when the DRep was freshly registered.
    /// Returns `false` (already registered) **without** overwriting the
    /// existing `RegisteredDrep` entry — upstream never destroys the
    /// existing deposit / anchor / activity state on duplicate registration.
    pub fn register(&mut self, drep: DRep, state: RegisteredDrep) -> bool {
        if self.entries.contains_key(&drep) {
            return false;
        }
        self.entries.insert(drep, state);
        true
    }

    /// Unregisters a DRep.
    pub fn unregister(&mut self, drep: &DRep) -> Option<RegisteredDrep> {
        self.entries.remove(drep)
    }

    /// Returns the number of registered DReps.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if there are no registered DReps.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the set of DReps that are inactive according to the
    /// upstream Conway `drepExpiry` rule.
    ///
    /// A DRep is inactive if its `last_active_epoch + drep_activity < epoch`.
    /// DReps without a recorded `last_active_epoch` (legacy entries) are
    /// treated as active to avoid false expiry.
    ///
    /// Upstream reference: `Cardano.Ledger.Conway.Rules.Epoch` — the
    /// `drepExpiry` function used when computing the active voting stake.
    pub fn inactive_dreps(&self, epoch: EpochNo, drep_activity: u64) -> Vec<DRep> {
        self.entries
            .iter()
            .filter(|(_, state)| {
                state
                    .last_active_epoch
                    .is_some_and(|e| e.0.saturating_add(drep_activity) < epoch.0)
            })
            .map(|(drep, _)| *drep)
            .collect()
    }
}

/// Committee-member authorization state visible from the ledger.
///
/// This mirrors the Conway cert-state split where a known cold credential may
/// have no hot key, an authorized hot key, or a recorded resignation anchor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitteeAuthorization {
    /// The member has an authorized hot credential.
    CommitteeHotCredential(StakeCredential),
    /// The member has resigned, optionally carrying an anchor.
    CommitteeMemberResigned(Option<Anchor>),
}

impl CborEncode for CommitteeAuthorization {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::CommitteeHotCredential(credential) => {
                enc.array(2).unsigned(0);
                credential.encode_cbor(enc);
            }
            Self::CommitteeMemberResigned(anchor) => {
                enc.array(2).unsigned(1);
                encode_optional_anchor(anchor.as_ref(), enc);
            }
        }
    }
}

impl CborDecode for CommitteeAuthorization {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }

        match dec.unsigned()? {
            0 => Ok(Self::CommitteeHotCredential(StakeCredential::decode_cbor(
                dec,
            )?)),
            1 => Ok(Self::CommitteeMemberResigned(decode_optional_anchor(dec)?)),
            tag => Err(LedgerError::CborInvalidAdditionalInfo(tag as u8)),
        }
    }
}

/// State for a known constitutional-committee cold credential.
///
/// Upstream reference: `Cardano.Ledger.Conway.Governance.Committee`
/// — members are stored as `Map Credential EpochNo` where the epoch
/// is the term expiry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitteeMemberState {
    authorization: Option<CommitteeAuthorization>,
    /// The epoch at which this member's term expires (inclusive).
    /// Upstream: the per-member `EpochNo` value in `committeeMembers`.
    expires_at: Option<u64>,
}

impl CborEncode for CommitteeMemberState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        // New format: 3-element array [version=2, authorization_or_null, expires_at_or_null].
        enc.array(3).unsigned(2);
        match self.authorization.as_ref() {
            Some(authorization) => authorization.encode_cbor(enc),
            None => {
                enc.null();
            }
        }
        match self.expires_at {
            Some(epoch) => {
                enc.unsigned(epoch);
            }
            None => {
                enc.null();
            }
        }
    }
}

impl CborDecode for CommitteeMemberState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let major = dec.peek_major()?;
        if major == 7 {
            // Legacy format: bare null → no authorization, no term.
            dec.null()?;
            return Ok(Self {
                authorization: None,
                expires_at: None,
            });
        }
        // Must be an array.
        let len = dec.array()?;
        match len {
            2 => {
                // Legacy format: CommitteeAuthorization [tag, data].
                let tag = dec.unsigned()?;
                let auth = match tag {
                    0 => CommitteeAuthorization::CommitteeHotCredential(
                        StakeCredential::decode_cbor(dec)?,
                    ),
                    1 => CommitteeAuthorization::CommitteeMemberResigned(decode_optional_anchor(
                        dec,
                    )?),
                    _ => return Err(LedgerError::CborInvalidAdditionalInfo(tag as u8)),
                };
                Ok(Self {
                    authorization: Some(auth),
                    expires_at: None,
                })
            }
            3 => {
                // New format: [version=2, authorization_or_null, expires_at_or_null].
                let _version = dec.unsigned()?;
                let authorization = if dec.peek_major()? == 7 {
                    dec.null()?;
                    None
                } else {
                    Some(CommitteeAuthorization::decode_cbor(dec)?)
                };
                let expires_at = if dec.peek_major()? == 7 {
                    dec.null()?;
                    None
                } else {
                    Some(dec.unsigned()?)
                };
                Ok(Self {
                    authorization,
                    expires_at,
                })
            }
            _ => Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            }),
        }
    }
}

impl CommitteeMemberState {
    /// Creates member state with no authorized hot credential.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates member state with a term expiry epoch.
    pub fn with_term(expires_at: u64) -> Self {
        Self {
            authorization: None,
            expires_at: Some(expires_at),
        }
    }

    /// Returns the epoch at which this member's term expires, if known.
    pub fn expires_at(&self) -> Option<u64> {
        self.expires_at
    }

    /// Returns `true` when the member's term has expired at the given epoch.
    ///
    /// Upstream: `currentEpoch <= expirationEpoch` means active.
    /// So expired means `current_epoch > expires_at`.
    pub fn is_expired(&self, current_epoch: EpochNo) -> bool {
        self.expires_at.is_some_and(|term| current_epoch.0 > term)
    }

    /// Returns the member authorization state, if any.
    pub fn authorization(&self) -> Option<&CommitteeAuthorization> {
        self.authorization.as_ref()
    }

    /// Returns the authorized hot credential, if present.
    pub fn hot_credential(&self) -> Option<StakeCredential> {
        match self.authorization.as_ref() {
            Some(CommitteeAuthorization::CommitteeHotCredential(credential)) => Some(*credential),
            _ => None,
        }
    }

    /// Returns the resignation anchor, if the member has resigned.
    pub fn resignation_anchor(&self) -> Option<&Anchor> {
        match self.authorization.as_ref() {
            Some(CommitteeAuthorization::CommitteeMemberResigned(anchor)) => anchor.as_ref(),
            _ => None,
        }
    }

    /// Returns true when the member has a recorded resignation.
    pub fn is_resigned(&self) -> bool {
        matches!(
            self.authorization,
            Some(CommitteeAuthorization::CommitteeMemberResigned(_))
        )
    }

    /// Returns true when this credential is an enacted committee member.
    ///
    /// Upstream: `committeeMembers` stores `Map Credential EpochNo`.
    /// A credential is an enacted member if and only if it has a term
    /// epoch (set by `register_with_term` during `UpdateCommittee`
    /// enactment).  Credentials that only have authorization/resignation
    /// state but no term (e.g., auto-registered via `isPotentialFutureMember`
    /// or membership-cleared via `NoConfidence`) are NOT enacted members.
    pub fn is_enacted_member(&self) -> bool {
        self.expires_at.is_some()
    }

    pub(crate) fn set_authorization(&mut self, authorization: Option<CommitteeAuthorization>) {
        self.authorization = authorization;
    }
}

/// Known constitutional-committee members visible from the ledger.
///
/// Membership itself is governed elsewhere in Conway state. This narrow local
/// container tracks known cold credentials plus their hot-key authorization or
/// resignation status so committee certificates can be applied atomically.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitteeState {
    entries: BTreeMap<StakeCredential, CommitteeMemberState>,
}

/// Stored Conway governance action state visible from the ledger.
///
/// This is a reduced local analogue of the upstream Conway `GovActionState`.
/// It preserves the submitted proposal body plus the currently recorded votes
/// keyed by Conway `Voter`, which is enough for proposal lookup and vote
/// replacement semantics in this ledger slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceActionState {
    proposal: crate::eras::conway::ProposalProcedure,
    votes: BTreeMap<crate::eras::conway::Voter, crate::eras::conway::Vote>,
    proposed_in: Option<EpochNo>,
    expires_after: Option<EpochNo>,
}

impl CborEncode for GovernanceActionState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4);
        self.proposal.encode_cbor(enc);
        enc.map(self.votes.len() as u64);
        for (voter, vote) in &self.votes {
            voter.encode_cbor(enc);
            vote.encode_cbor(enc);
        }
        encode_optional_epoch_no(self.proposed_in, enc);
        encode_optional_epoch_no(self.expires_after, enc);
    }
}

impl CborDecode for GovernanceActionState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 && len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }

        let proposal = crate::eras::conway::ProposalProcedure::decode_cbor(dec)?;
        let votes_len = dec.map()?;
        let mut votes = BTreeMap::new();
        for _ in 0..votes_len {
            let voter = crate::eras::conway::Voter::decode_cbor(dec)?;
            let vote = crate::eras::conway::Vote::decode_cbor(dec)?;
            votes.insert(voter, vote);
        }

        let proposed_in = if len == 4 {
            decode_optional_epoch_no(dec)?
        } else {
            None
        };
        let expires_after = if len == 4 {
            decode_optional_epoch_no(dec)?
        } else {
            None
        };

        Ok(Self {
            proposal,
            votes,
            proposed_in,
            expires_after,
        })
    }
}

impl GovernanceActionState {
    /// Creates stored governance action state for a newly submitted proposal.
    pub fn new(proposal: crate::eras::conway::ProposalProcedure) -> Self {
        Self {
            proposal,
            votes: BTreeMap::new(),
            proposed_in: None,
            expires_after: None,
        }
    }

    pub(crate) fn new_with_lifetime(
        proposal: crate::eras::conway::ProposalProcedure,
        proposed_in: EpochNo,
        gov_action_lifetime: Option<u64>,
    ) -> Self {
        Self {
            proposal,
            votes: BTreeMap::new(),
            proposed_in: Some(proposed_in),
            expires_after: gov_action_lifetime
                .map(|lifetime| EpochNo(proposed_in.0.saturating_add(lifetime))),
        }
    }

    /// Returns the submitted proposal procedure.
    pub fn proposal(&self) -> &crate::eras::conway::ProposalProcedure {
        &self.proposal
    }

    /// Returns the recorded votes keyed by voter.
    pub fn votes(&self) -> &BTreeMap<crate::eras::conway::Voter, crate::eras::conway::Vote> {
        &self.votes
    }

    /// Returns the epoch in which the proposal was introduced, when tracked.
    pub fn proposed_in(&self) -> Option<EpochNo> {
        self.proposed_in
    }

    /// Returns the last epoch in which votes are accepted, when tracked.
    pub fn expires_after(&self) -> Option<EpochNo> {
        self.expires_after
    }

    /// Records a vote from `voter`, replacing any previous vote.
    pub fn record_vote(
        &mut self,
        voter: crate::eras::conway::Voter,
        vote: crate::eras::conway::Vote,
    ) {
        self.votes.insert(voter, vote);
    }
}

impl CborEncode for CommitteeState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(self.entries.len() as u64);
        for (credential, state) in &self.entries {
            enc.array(2);
            credential.encode_cbor(enc);
            state.encode_cbor(enc);
        }
    }
}

impl CborDecode for CommitteeState {
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
            let state = CommitteeMemberState::decode_cbor(dec)?;
            entries.insert(credential, state);
        }
        Ok(Self { entries })
    }
}

impl CommitteeState {
    /// Creates an empty committee-state container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the state for `credential`, if present.
    pub fn get(&self, credential: &StakeCredential) -> Option<&CommitteeMemberState> {
        self.entries.get(credential)
    }

    /// Returns mutable state for `credential`, if present.
    pub fn get_mut(&mut self, credential: &StakeCredential) -> Option<&mut CommitteeMemberState> {
        self.entries.get_mut(credential)
    }

    /// Returns true when `credential` is a known committee member.
    pub fn is_member(&self, credential: &StakeCredential) -> bool {
        self.entries.contains_key(credential)
    }

    /// Iterates over known committee members in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&StakeCredential, &CommitteeMemberState)> {
        self.entries.iter()
    }

    /// Inserts a known committee member with no authorized hot credential.
    pub fn register(&mut self, credential: StakeCredential) -> bool {
        self.entries
            .insert(credential, CommitteeMemberState::new())
            .is_none()
    }

    /// Sets the term expiry epoch for a committee member, preserving any
    /// existing authorization/resignation state.
    ///
    /// Upstream: `committeeMembers` stores `Map Credential EpochNo` which
    /// is separate from `csCommitteeCreds` (authorization state).  When
    /// `UpdateCommittee` is enacted, only `committeeMembers` is modified —
    /// `csCommitteeCreds` is untouched.  In our combined model we preserve
    /// the existing authorization when the entry already exists.
    pub fn register_with_term(&mut self, credential: StakeCredential, expires_at: u64) -> bool {
        use std::collections::btree_map::Entry;
        match self.entries.entry(credential) {
            Entry::Occupied(mut entry) => {
                // Preserve authorization/resignation — only update term.
                entry.get_mut().expires_at = Some(expires_at);
                false
            }
            Entry::Vacant(entry) => {
                entry.insert(CommitteeMemberState::with_term(expires_at));
                true
            }
        }
    }

    /// Removes a known committee member entirely (entry + authorization).
    pub fn unregister(&mut self, credential: &StakeCredential) -> Option<CommitteeMemberState> {
        self.entries.remove(credential)
    }

    /// Clears enacted membership for a single credential by setting
    /// `expires_at = None`, while preserving its authorization/resignation
    /// state.
    ///
    /// Upstream: removing from `committeeMembers` does not touch
    /// `csCommitteeCreds`.
    pub fn clear_membership(&mut self, credential: &StakeCredential) {
        if let Some(member) = self.entries.get_mut(credential) {
            member.expires_at = None;
        }
    }

    /// Clears enacted membership for all credentials by setting every
    /// entry's `expires_at = None`, preserving authorization/resignation
    /// state.
    ///
    /// Upstream: `NoConfidence` sets `ensCommittee = SNothing` which removes
    /// all `committeeMembers` but leaves `csCommitteeCreds` untouched.
    pub fn clear_all_membership(&mut self) {
        for member in self.entries.values_mut() {
            member.expires_at = None;
        }
    }

    /// Removes all entries whose credential is not a current committee
    /// member (i.e., `expires_at` is `None`).
    ///
    /// This implements upstream `updateCommitteeState` from
    /// `Cardano.Ledger.Conway.Rules.Epoch`:
    ///
    /// ```haskell
    /// updateCommitteeState committee (CommitteeState creds) =
    ///   CommitteeState $ Map.intersection creds members
    ///   where members = foldMap' committeeMembers committee
    /// ```
    ///
    /// Must be called at each epoch boundary after governance enactment
    /// so that hot-key authorizations for removed committee members are
    /// cleaned up.  Without this, re-elected members would retain their
    /// old authorization instead of having to re-register.
    pub fn prune_non_members(&mut self) {
        self.entries.retain(|_, m| m.expires_at.is_some());
    }

    /// Returns the number of known committee members.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if there are no known committee members.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Enacted governance state (Conway)
// ---------------------------------------------------------------------------

/// Enacted governance state tracking the current constitution, committee
/// quorum, and the most recently enacted action ID per governance purpose
/// group.
///
/// Upstream reference: `Cardano.Ledger.Conway.Governance.EnactState`.
///
/// The purpose groups mirror the upstream `GovRelation`:
/// * **PParamUpdate** — `ParameterChange` actions.
/// * **HardFork** — `HardForkInitiation` actions.
/// * **Committee** — `NoConfidence` and `UpdateCommittee` actions.
/// * **Constitution** — `NewConstitution` actions.
///
/// `TreasuryWithdrawals` and `InfoAction` have no lineage tracking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnactState {
    /// The current enacted constitution.
    pub constitution: crate::eras::conway::Constitution,
    /// Committee quorum threshold (ratio of yes-votes needed).
    pub committee_quorum: UnitInterval,
    /// Whether a committee currently exists.
    ///
    /// After `NoConfidence`, upstream sets `ensCommitteeL = SNothing`,
    /// causing `committeeAccepted` to return `False` for all
    /// committee-requiring actions.  `UpdateCommittee` re-establishes
    /// the committee (`SJust`).
    pub has_committee: bool,
    /// Most recently enacted `ParameterChange` action ID.
    pub prev_pparams_update: Option<crate::eras::conway::GovActionId>,
    /// Most recently enacted `HardForkInitiation` action ID.
    pub prev_hard_fork: Option<crate::eras::conway::GovActionId>,
    /// Most recently enacted `NoConfidence` or `UpdateCommittee` action ID.
    pub prev_committee: Option<crate::eras::conway::GovActionId>,
    /// Most recently enacted `NewConstitution` action ID.
    pub prev_constitution: Option<crate::eras::conway::GovActionId>,
}

impl Default for EnactState {
    fn default() -> Self {
        Self {
            constitution: crate::eras::conway::Constitution {
                anchor: crate::types::Anchor {
                    url: String::new(),
                    data_hash: [0u8; 32],
                },
                guardrails_script_hash: None,
            },
            committee_quorum: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            has_committee: true,
            prev_pparams_update: None,
            prev_hard_fork: None,
            prev_committee: None,
            prev_constitution: None,
        }
    }
}

impl CborEncode for EnactState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(7);
        self.constitution.encode_cbor(enc);
        self.committee_quorum.encode_cbor(enc);
        enc.bool(self.has_committee);
        encode_optional_gov_action_id(self.prev_pparams_update.as_ref(), enc);
        encode_optional_gov_action_id(self.prev_hard_fork.as_ref(), enc);
        encode_optional_gov_action_id(self.prev_committee.as_ref(), enc);
        encode_optional_gov_action_id(self.prev_constitution.as_ref(), enc);
    }
}

impl CborDecode for EnactState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 6 && len != 7 {
            return Err(LedgerError::CborInvalidLength {
                expected: 7,
                actual: len as usize,
            });
        }
        let constitution = crate::eras::conway::Constitution::decode_cbor(dec)?;
        let committee_quorum = UnitInterval::decode_cbor(dec)?;
        let has_committee = if len >= 7 { dec.bool()? } else { true };
        let prev_pparams_update = decode_optional_gov_action_id(dec)?;
        let prev_hard_fork = decode_optional_gov_action_id(dec)?;
        let prev_committee = decode_optional_gov_action_id(dec)?;
        let prev_constitution = decode_optional_gov_action_id(dec)?;
        Ok(Self {
            constitution,
            committee_quorum,
            has_committee,
            prev_pparams_update,
            prev_hard_fork,
            prev_committee,
            prev_constitution,
        })
    }
}

impl EnactState {
    /// Creates a default `EnactState` with empty constitution and no lineage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the currently enacted constitution.
    pub fn constitution(&self) -> &crate::eras::conway::Constitution {
        &self.constitution
    }

    /// Returns the current committee quorum threshold.
    pub fn committee_quorum(&self) -> &UnitInterval {
        &self.committee_quorum
    }

    /// Returns the most recently enacted action ID for each purpose group.
    pub fn prev_pparams_update(&self) -> Option<&crate::eras::conway::GovActionId> {
        self.prev_pparams_update.as_ref()
    }

    pub fn prev_hard_fork(&self) -> Option<&crate::eras::conway::GovActionId> {
        self.prev_hard_fork.as_ref()
    }

    pub fn prev_committee(&self) -> Option<&crate::eras::conway::GovActionId> {
        self.prev_committee.as_ref()
    }

    pub fn prev_constitution(&self) -> Option<&crate::eras::conway::GovActionId> {
        self.prev_constitution.as_ref()
    }

    /// Returns the enacted root for the given governance purpose group.
    ///
    /// This is used during Conway proposal validation to check whether a
    /// proposal's `prev_action_id` correctly references the most recently
    /// enacted action of its purpose family.
    ///
    /// Upstream reference: `Cardano.Ledger.Conway.Governance.prevGovActionIds`.
    pub(crate) fn enacted_root(
        &self,
        purpose: ConwayGovActionPurpose,
    ) -> Option<&crate::eras::conway::GovActionId> {
        match purpose {
            ConwayGovActionPurpose::ParameterChange => self.prev_pparams_update.as_ref(),
            ConwayGovActionPurpose::HardFork => self.prev_hard_fork.as_ref(),
            ConwayGovActionPurpose::Committee => self.prev_committee.as_ref(),
            ConwayGovActionPurpose::Constitution => self.prev_constitution.as_ref(),
            // TreasuryWithdrawals and Info have no lineage.
            ConwayGovActionPurpose::TreasuryWithdrawals | ConwayGovActionPurpose::Info => None,
        }
    }
}

/// Outcome of enacting a single governance action.
///
/// Callers inspect this to determine what side-effects to apply to
/// `LedgerState` (committee, treasury, protocol params, etc.).
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Enact`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnactOutcome {
    /// No on-chain effect (InfoAction).
    NoEffect,
    /// The constitution was updated.
    ConstitutionUpdated,
    /// All committee members were removed (no-confidence motion).
    CommitteeRemoved,
    /// Committee membership was updated and quorum changed.
    CommitteeUpdated {
        members_removed: usize,
        members_added: usize,
    },
    /// A hard fork was enacted — the protocol version was updated.
    HardForkEnacted { new_version: (u64, u64) },
    /// Treasury withdrawals were enacted — lovelace credited to reward
    /// accounts from the treasury.
    TreasuryWithdrawn { total_withdrawn: u64 },
    /// A parameter change was enacted and applied to protocol parameters.
    ParameterChangeRecorded,
}

/// Enacts a single ratified governance action, updating the `EnactState`
/// lineage and applying side-effects to the mutable ledger components.
///
/// This function implements the Conway `ENACT` rule for each governance
/// action variant. Side-effects are applied directly to the provided
/// mutable references so callers do not need to interpret the outcome
/// for state updates — the `EnactOutcome` is purely informational.
///
/// # Parameters
///
/// * `enact` — Enacted governance state (constitution, quorum, lineage).
/// * `action_id` — The `GovActionId` of the action being enacted.
/// * `action` — The `GovAction` body to enact.
/// * `committee` — Mutable committee-member state.
/// * `protocol_params` — Mutable protocol parameters (for hard-fork version).
/// * `reward_accounts` — Mutable reward-account balances (for treasury withdrawal).
/// * `accounting` — Mutable treasury/reserves accounting.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Enact`.
pub fn enact_gov_action(
    enact: &mut EnactState,
    action_id: crate::eras::conway::GovActionId,
    action: &crate::eras::conway::GovAction,
    committee: &mut CommitteeState,
    protocol_params: &mut Option<crate::protocol_params::ProtocolParameters>,
    reward_accounts: &mut RewardAccounts,
    accounting: &mut AccountingState,
) -> EnactOutcome {
    enact_gov_action_at_epoch(
        enact,
        EpochNo(0),
        action_id,
        action,
        committee,
        protocol_params,
        reward_accounts,
        accounting,
    )
}

fn enact_gov_action_at_epoch(
    enact: &mut EnactState,
    _current_epoch: EpochNo,
    action_id: crate::eras::conway::GovActionId,
    action: &crate::eras::conway::GovAction,
    committee: &mut CommitteeState,
    protocol_params: &mut Option<crate::protocol_params::ProtocolParameters>,
    reward_accounts: &mut RewardAccounts,
    accounting: &mut AccountingState,
) -> EnactOutcome {
    use crate::eras::conway::GovAction;

    match action {
        GovAction::InfoAction => EnactOutcome::NoEffect,

        GovAction::NewConstitution { constitution, .. } => {
            enact.constitution = constitution.clone();
            enact.prev_constitution = Some(action_id);
            EnactOutcome::ConstitutionUpdated
        }

        GovAction::NoConfidence { .. } => {
            // Upstream sets `ensCommittee = SNothing` which removes all
            // members from committeeMembers, but csCommitteeCreds (authorization
            // and resignation state) is preserved in VState.
            // In our combined model, we clear expires_at (membership) while
            // preserving authorization/resignation state.
            let count = committee.len();
            committee.clear_all_membership();
            enact.committee_quorum = UnitInterval {
                numerator: 0,
                denominator: 1,
            };
            enact.has_committee = false;
            enact.prev_committee = Some(action_id);
            let _ = count; // suppress unused; count is informational
            EnactOutcome::CommitteeRemoved
        }

        GovAction::UpdateCommittee {
            members_to_remove,
            members_to_add,
            quorum,
            ..
        } => {
            let mut removed = 0usize;
            for cred in members_to_remove {
                // Upstream: removes from committeeMembers only — does not
                // touch csCommitteeCreds (authorization/resignation state).
                if committee
                    .get(cred)
                    .is_some_and(|m| m.expires_at().is_some())
                {
                    committee.clear_membership(cred);
                    removed += 1;
                }
            }
            let mut added = 0usize;
            for (cred, term_epoch) in members_to_add {
                // Register the new member with no hot-key authorization
                // but with a term expiry epoch (upstream committeeMembers).
                if committee.register_with_term(*cred, *term_epoch) {
                    added += 1;
                }
            }
            enact.committee_quorum = *quorum;
            enact.has_committee = true;
            enact.prev_committee = Some(action_id);
            EnactOutcome::CommitteeUpdated {
                members_removed: removed,
                members_added: added,
            }
        }

        GovAction::HardForkInitiation {
            protocol_version, ..
        } => {
            let params = protocol_params.get_or_insert_with(Default::default);
            params.protocol_version = Some(*protocol_version);
            enact.prev_hard_fork = Some(action_id);
            EnactOutcome::HardForkEnacted {
                new_version: *protocol_version,
            }
        }

        GovAction::TreasuryWithdrawals { withdrawals, .. } => {
            let mut total = 0u64;
            for (ra, &amount) in withdrawals {
                if amount == 0 {
                    continue;
                }
                if let Some(ra_state) = reward_accounts.get_mut(ra) {
                    // Only credit registered reward accounts.
                    ra_state.set_balance(ra_state.balance().saturating_add(amount));
                    accounting.treasury = accounting.treasury.saturating_sub(amount);
                    total = total.saturating_add(amount);
                }
                // Unregistered reward accounts: withdrawal is lost (matching
                // upstream behavior where uncredited amounts remain in treasury).
            }
            EnactOutcome::TreasuryWithdrawn {
                total_withdrawn: total,
            }
        }

        GovAction::ParameterChange {
            protocol_param_update,
            ..
        } => {
            let params = protocol_params.get_or_insert_with(Default::default);
            params.apply_update(protocol_param_update);
            enact.prev_pparams_update = Some(action_id);
            EnactOutcome::ParameterChangeRecorded
        }
    }
}

/// Round 192 — Companion `ChainDepState` snapshot data attached to
/// [`LedgerStateSnapshot`] so LSQ dispatchers can serve live nonces
/// and OCert counters in `query protocol-state`.
///
/// `crates/consensus` owns the canonical
/// `NonceEvolutionState`/`OcertCounters` types but cannot be imported
/// here without inverting the dependency direction. The runtime
/// translates from those types into this snapshot-side mirror at
/// snapshot capture time.
///
/// Reference: `Ouroboros.Consensus.Protocol.Praos.PraosState`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainDepStateContext {
    /// `praosStateEvolvingNonce` (η_v) — combines every block's VRF
    /// nonce contribution within an epoch.
    pub evolving_nonce: Nonce,
    /// `praosStateCandidateNonce` (η_c) — frozen at the stability
    /// window inside an epoch.
    pub candidate_nonce: Nonce,
    /// `praosStateEpochNonce` — the active epoch nonce used for VRF
    /// verification.
    pub epoch_nonce: Nonce,
    /// `praosStatePreviousEpochNonce` — previous epoch's nonce.
    /// Yggdrasil does not yet track this distinctly from the epoch
    /// nonce; emits Neutral until plumbed.
    pub previous_epoch_nonce: Nonce,
    /// `praosStateLabNonce` — the "last applied block" nonce derived
    /// from the most recent block's prev-hash.
    pub lab_nonce: Nonce,
    /// `praosStateLastEpochBlockNonce` — the nonce derived from the
    /// last block of the previous epoch (yggdrasil's
    /// `NonceEvolutionState::prev_hash_nonce`).
    pub last_epoch_block_nonce: Nonce,
    /// `praosStateOCertCounters` — per-pool monotonic OpCert
    /// sequence-number tracker keyed by 28-byte cold-key hash.
    pub opcert_counters: BTreeMap<[u8; 28], u64>,
}

impl Default for ChainDepStateContext {
    fn default() -> Self {
        Self {
            evolving_nonce: Nonce::Neutral,
            candidate_nonce: Nonce::Neutral,
            epoch_nonce: Nonce::Neutral,
            previous_epoch_nonce: Nonce::Neutral,
            lab_nonce: Nonce::Neutral,
            last_epoch_block_nonce: Nonce::Neutral,
            opcert_counters: BTreeMap::new(),
        }
    }
}

/// Read-only snapshot of ledger-visible state.
///
/// This snapshot preserves the current era, tip, stake-pool state,
/// reward-account state, and both UTxO views so callers can query
/// ledger-visible data without mutating `LedgerState`. The dual UTxO
/// representation is retained because Shelley-only state is still stored
/// separately for backward compatibility.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerStateSnapshot {
    current_era: Era,
    tip: Point,
    latest_block_protocol_version: Option<(u64, u64)>,
    tip_block_no: Option<BlockNo>,
    current_epoch: EpochNo,
    expected_network_id: Option<u8>,
    governance_actions: BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    pool_state: PoolState,
    stake_credentials: StakeCredentials,
    committee_state: CommitteeState,
    drep_state: DrepState,
    reward_accounts: RewardAccounts,
    shelley_utxo: ShelleyUtxo,
    multi_era_utxo: MultiEraUtxo,
    protocol_params: Option<crate::protocol_params::ProtocolParameters>,
    deposit_pot: DepositPot,
    accounting: AccountingState,
    enact_state: EnactState,
    gen_delegs: BTreeMap<GenesisHash, GenesisDelegationState>,
    stability_window: Option<u64>,
    num_dormant_epochs: u64,
    /// Round 192 — optional consensus-side `ChainDepState` mirror
    /// for serving `query protocol-state` with live nonces + OCert
    /// counters.  `None` when the runtime hasn't populated it yet
    /// (test fakes, very early bootstrap); LSQ dispatchers fall back
    /// to neutral placeholders in that case.
    chain_dep_state: Option<ChainDepStateContext>,
    /// Round 202 — optional active stake-snapshot rotation
    /// (`mark`/`set`/`go`) for serving `query stake-snapshot` and
    /// stake-distribution queries with live per-pool totals.
    /// `None` when the runtime hasn't populated it (test fakes,
    /// pre-epoch-boundary bootstrap, or sync paths without a
    /// stake-snapshot tracker).
    stake_snapshots: Option<crate::stake::StakeSnapshots>,
}

impl LedgerStateSnapshot {
    /// Returns the era active at the time this snapshot was captured.
    pub fn current_era(&self) -> Era {
        self.current_era
    }

    /// Round 192 — attach a [`ChainDepStateContext`] from the consensus
    /// runtime so LSQ `query protocol-state` can serve live nonces +
    /// OCert counters.
    pub fn with_chain_dep_state(mut self, ctx: ChainDepStateContext) -> Self {
        self.chain_dep_state = Some(ctx);
        self
    }

    /// Round 192 — read-only access to the attached
    /// [`ChainDepStateContext`].  `None` until the runtime calls
    /// [`Self::with_chain_dep_state`] (e.g. during early bootstrap or
    /// in test fakes).
    pub fn chain_dep_state(&self) -> Option<&ChainDepStateContext> {
        self.chain_dep_state.as_ref()
    }

    /// Round 202 — attach the active mark/set/go stake snapshot
    /// rotation from the consensus runtime so LSQ stake-related
    /// queries can serve live per-pool totals.
    pub fn with_stake_snapshots(mut self, snapshots: crate::stake::StakeSnapshots) -> Self {
        self.stake_snapshots = Some(snapshots);
        self
    }

    /// Round 202 — read-only access to the attached active stake
    /// snapshots.
    pub fn stake_snapshots(&self) -> Option<&crate::stake::StakeSnapshots> {
        self.stake_snapshots.as_ref()
    }

    /// Returns the chain tip captured in this snapshot.
    pub fn tip(&self) -> &Point {
        &self.tip
    }

    /// Returns the current epoch captured in this snapshot.
    pub fn current_epoch(&self) -> EpochNo {
        self.current_epoch
    }

    /// Returns the protocol version `(major, minor)` declared in the
    /// most recently applied block's header.  See
    /// [`LedgerState::latest_block_protocol_version`] for rationale.
    pub fn latest_block_protocol_version(&self) -> Option<(u64, u64)> {
        self.latest_block_protocol_version
    }

    /// Returns the chain-tip block number captured in this snapshot.
    ///
    /// Mirrors upstream `Ouroboros.Network.Block.Tip.tipBlockNo`.  `None`
    /// when no block has been applied yet (`tip == Origin`).
    pub fn tip_block_no(&self) -> Option<BlockNo> {
        self.tip_block_no
    }

    /// Returns the expected reward-account network id, if configured.
    pub fn expected_network_id(&self) -> Option<u8> {
        self.expected_network_id
    }

    /// Returns the stored governance action state for `id`, if present.
    pub fn governance_action(
        &self,
        id: &crate::eras::conway::GovActionId,
    ) -> Option<&GovernanceActionState> {
        self.governance_actions.get(id)
    }

    /// Returns all stored governance actions keyed by governance action id.
    pub fn governance_actions(
        &self,
    ) -> &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState> {
        &self.governance_actions
    }

    /// Returns a mutable reference to stored governance actions.
    pub fn governance_actions_mut(
        &mut self,
    ) -> &mut BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState> {
        &mut self.governance_actions
    }

    /// Returns the registered stake-pool state captured in this snapshot.
    pub fn pool_state(&self) -> &PoolState {
        &self.pool_state
    }

    /// Returns the registered stake-credential state captured in this snapshot.
    pub fn stake_credentials(&self) -> &StakeCredentials {
        &self.stake_credentials
    }

    /// Returns the committee-member state captured in this snapshot.
    pub fn committee_state(&self) -> &CommitteeState {
        &self.committee_state
    }

    /// Returns the registered DRep state captured in this snapshot.
    pub fn drep_state(&self) -> &DrepState {
        &self.drep_state
    }

    /// Returns the reward-account state captured in this snapshot.
    pub fn reward_accounts(&self) -> &RewardAccounts {
        &self.reward_accounts
    }

    /// Returns the registered state for `operator`, if present.
    pub fn registered_pool(&self, operator: &PoolKeyHash) -> Option<&RegisteredPool> {
        self.pool_state.get(operator)
    }

    /// Returns the stake-credential state for `credential`, if present.
    pub fn stake_credential_state(
        &self,
        credential: &StakeCredential,
    ) -> Option<&StakeCredentialState> {
        self.stake_credentials.get(credential)
    }

    /// Returns the committee-member state for `credential`, if present.
    pub fn committee_member_state(
        &self,
        credential: &StakeCredential,
    ) -> Option<&CommitteeMemberState> {
        self.committee_state.get(credential)
    }

    /// Returns the registered DRep state for `drep`, if present.
    pub fn registered_drep(&self, drep: &DRep) -> Option<&RegisteredDrep> {
        self.drep_state.get(drep)
    }

    /// Returns the reward-account state for `account`, if present.
    pub fn reward_account_state(&self, account: &RewardAccount) -> Option<&RewardAccountState> {
        self.reward_accounts.get(account)
    }

    /// Returns the visible reward balance for `account`.
    pub fn query_reward_balance(&self, account: &RewardAccount) -> u64 {
        self.reward_accounts.balance(account)
    }

    /// Returns the multi-era UTxO set captured in this snapshot.
    pub fn multi_era_utxo(&self) -> &MultiEraUtxo {
        &self.multi_era_utxo
    }

    /// Returns the legacy Shelley-only UTxO set captured in this snapshot.
    pub fn utxo(&self) -> &ShelleyUtxo {
        &self.shelley_utxo
    }

    /// Returns the protocol parameters captured in this snapshot.
    pub fn protocol_params(&self) -> Option<&crate::protocol_params::ProtocolParameters> {
        self.protocol_params.as_ref()
    }

    /// Returns the deposit pot captured in this snapshot.
    pub fn deposit_pot(&self) -> &DepositPot {
        &self.deposit_pot
    }

    /// Returns the treasury/reserves accounting captured in this snapshot.
    pub fn accounting(&self) -> &AccountingState {
        &self.accounting
    }

    /// Returns the Conway enactment state captured in this snapshot.
    pub fn enact_state(&self) -> &EnactState {
        &self.enact_state
    }

    /// Returns the active genesis-delegation map captured in this snapshot.
    /// Mirrors upstream `dsGenDelegs` from
    /// `Cardano.Ledger.Shelley.LedgerState.DPState`.
    pub fn gen_delegs(&self) -> &BTreeMap<GenesisHash, GenesisDelegationState> {
        &self.gen_delegs
    }

    /// Returns the configured stability window (`3k/f`) captured in this
    /// snapshot, if known.
    pub fn stability_window(&self) -> Option<u64> {
        self.stability_window
    }

    /// Returns the consecutive dormant-epoch count captured in this
    /// snapshot.  Mirrors upstream `csNumDormantEpochs` from
    /// `Cardano.Ledger.Conway.Governance.DRepPulser`.
    pub fn num_dormant_epochs(&self) -> u64 {
        self.num_dormant_epochs
    }

    /// Returns UTxO entries for the given transaction inputs.
    ///
    /// For each requested `ShelleyTxIn`, if the entry exists in either
    /// the multi-era or legacy Shelley UTxO set it is included in the
    /// result.  Multi-era entries take precedence.
    ///
    /// Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
    /// `GetUTxOByTxIn`.
    pub fn query_utxos_by_txin(
        &self,
        txins: &[crate::eras::shelley::ShelleyTxIn],
    ) -> Vec<(crate::eras::shelley::ShelleyTxIn, MultiEraTxOut)> {
        let mut matched = BTreeMap::new();
        for txin in txins {
            if let Some(txout) = self.multi_era_utxo.get(txin) {
                matched.insert(txin.clone(), txout.clone());
            } else if let Some(txout) = self.shelley_utxo.get(txin) {
                matched.insert(txin.clone(), MultiEraTxOut::Shelley(txout.clone()));
            }
        }
        matched.into_iter().collect()
    }

    /// Returns the set of all registered pool operator key hashes.
    ///
    /// Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
    /// `GetStakePools`.
    pub fn query_stake_pool_ids(&self) -> Vec<PoolKeyHash> {
        self.pool_state.iter().map(|(k, _)| *k).collect()
    }

    /// Returns delegations and reward balances for the given stake credentials.
    ///
    /// For each credential present in the ledger, the result includes the
    /// delegated pool (if any) and the reward balance of the corresponding
    /// reward account.
    ///
    /// Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
    /// `GetFilteredDelegationsAndRewardAccounts`.
    pub fn query_delegations_and_rewards(
        &self,
        credentials: &[StakeCredential],
    ) -> Vec<(StakeCredential, Option<PoolKeyHash>, u64)> {
        let mut results = Vec::new();
        for cred in credentials {
            if let Some(state) = self.stake_credentials.get(cred) {
                // Look up reward balance via the reward account built from
                // network id 1 (mainnet) or 0, then fall back to iterating
                // reward accounts to find a match by credential.
                let balance = self.find_reward_balance_for_credential(cred);
                results.push((*cred, state.delegated_pool(), balance));
            }
        }
        results
    }

    /// Returns DRep stake distribution: each registered DRep mapped to its
    /// total delegated stake (sum of reward-account balances of credentials
    /// that delegate to that DRep).
    ///
    /// Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
    /// `GetDRepStakeDistr`.
    pub fn query_drep_stake_distribution(&self) -> BTreeMap<DRep, u64> {
        let mut drep_stake: BTreeMap<DRep, u64> = BTreeMap::new();
        for (cred, cred_state) in self.stake_credentials.iter() {
            if let Some(drep) = cred_state.delegated_drep() {
                let balance = self.find_reward_balance_for_credential(cred);
                *drep_stake.entry(drep).or_insert(0) += balance;
            }
        }
        drep_stake
    }

    /// Finds the reward balance for a stake credential by scanning reward
    /// accounts.  Returns 0 if no matching account is found.
    fn find_reward_balance_for_credential(&self, credential: &StakeCredential) -> u64 {
        for (account, state) in self.reward_accounts.iter() {
            if &account.credential == credential {
                return state.balance();
            }
        }
        0
    }

    /// Returns all UTxO entries paying to `address`.
    ///
    /// Entries from the multi-era UTxO set take precedence when the same
    /// `ShelleyTxIn` is visible through both backing stores.
    pub fn query_utxos_by_address(
        &self,
        address: &Address,
    ) -> Vec<(crate::eras::shelley::ShelleyTxIn, MultiEraTxOut)> {
        let address_bytes = address.to_bytes();
        let mut matched = BTreeMap::new();

        for (txin, txout) in self.shelley_utxo.iter() {
            if txout.address == address_bytes {
                matched.insert(txin.clone(), MultiEraTxOut::Shelley(txout.clone()));
            }
        }

        for (txin, txout) in self.multi_era_utxo.iter() {
            if txout.address() == address_bytes.as_slice() {
                matched.insert(txin.clone(), txout.clone());
            }
        }

        matched.into_iter().collect()
    }

    /// Returns the aggregate balance for `address` across visible UTxO entries.
    pub fn query_balance(&self, address: &Address) -> Value {
        let mut coin_total = 0u64;
        let mut asset_total: MultiAsset = BTreeMap::new();

        for (_, txout) in self.query_utxos_by_address(address) {
            let value = txout.value();
            coin_total = coin_total.saturating_add(value.coin());
            if let Some(assets) = value.multi_asset() {
                accumulate_multi_asset(&mut asset_total, assets);
            }
        }

        if asset_total.is_empty() {
            Value::Coin(coin_total)
        } else {
            Value::CoinAndAssets(coin_total, asset_total)
        }
    }
}

// ---------------------------------------------------------------------------
// InstantaneousRewards — MIR accumulation state
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// DepositPot — aggregate deposit tracking
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// TreasuryState — treasury and reserves
// ---------------------------------------------------------------------------

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

/// Ledger state tracking the current era, chain tip, and UTxO set.
///
/// `apply_block` decodes each transaction body according to the block's
/// era and applies the UTxO transition rules via `MultiEraUtxo`.
/// The state also carries stake-pool and reward-account containers for
/// pool-certificate and withdrawal work. A legacy `ShelleyUtxO`
/// accessor is retained for backward compatibility with existing tests
/// that seed and inspect Shelley-only entries.
///
/// Reference: `Ouroboros.Consensus.Ledger.Abstract` — `LedgerState`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerState {
    /// The ledger era currently in effect.
    pub current_era: Era,
    /// Chain tip as a point (slot + header hash).
    pub tip: Point,
    /// Protocol version `(major, minor)` declared in the most
    /// recently applied block's header.
    ///
    /// This tracks the chain's *active* protocol — distinct from
    /// `protocol_params.protocol_version` (which is the
    /// genesis/PPUP-managed PP field).  When the chain is in a
    /// hard-fork transition state (e.g. Alonzo era with PV major
    /// bumped to 7 to signal Babbage), the header PV is the
    /// canonical source for "what era is this chain effectively
    /// in", used by upstream's hard-fork combinator and surfaced
    /// to LSQ clients via the era-promotion logic in the local
    /// server.
    ///
    /// `None` until the first non-Byron block is applied (Byron
    /// blocks have no header PV).
    pub latest_block_protocol_version: Option<(u64, u64)>,
    /// Block number of the most recently applied block, mirrors
    /// upstream `nesEs.esLState.lsTip.blockNo` at the chain tip.
    ///
    /// Updated by every successful `apply_block_validated`; flows into
    /// `LedgerStateSnapshot::tip_block_no` so LSQ `GetChainBlockNo`
    /// (upstream `[2]`) can return the actual block height instead of
    /// falling back to `Origin`.
    ///
    /// `None` before any block is applied.  Reference:
    /// `Ouroboros.Network.Block.Tip.tipBlockNo`.
    pub tip_block_no: Option<BlockNo>,
    /// Current epoch known to the ledger state.
    pub current_epoch: EpochNo,
    /// Expected network id for reward-account validation.
    expected_network_id: Option<u8>,
    /// Persisted Conway governance actions keyed by `GovActionId`.
    governance_actions: BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    /// Registered stake-pool state.
    pool_state: PoolState,
    /// Registered stake-credential state.
    stake_credentials: StakeCredentials,
    /// Known committee-member state.
    committee_state: CommitteeState,
    /// Registered DRep state.
    drep_state: DrepState,
    /// Reward-account balances and delegation pointers.
    reward_accounts: RewardAccounts,
    /// Multi-era UTxO set.
    multi_era_utxo: MultiEraUtxo,
    /// Legacy Shelley-only UTxO set kept in sync for backward compatibility.
    shelley_utxo: ShelleyUtxo,
    /// Protocol parameters governing validation rules.
    protocol_params: Option<crate::protocol_params::ProtocolParameters>,
    /// Protocol parameters from the previous UPEC update — i.e. the
    /// `curPParams` value just before the most recent UPEC fired at an
    /// epoch boundary.
    ///
    /// Upstream `Cardano.Ledger.Shelley.LedgerState.PulsingReward.startStep`
    /// reads `esPrevPParams.d` (NOT `esCurPParams.d`) when computing
    /// `η = min(1, blocksMade/expectedBlocks)`.  Since UPEC at the
    /// start of each new epoch shifts `curPParams → prevPParams` then
    /// applies any due update, `prevPParams` always lags one epoch behind.
    /// Reading `curPParams.d` instead causes the RUPD applied at the
    /// boundary entering the d=1 → d=0 transition (preview B(3) at
    /// slot 172,800) to compute `eta = blocks_made / expected_blocks`
    /// when upstream uses `eta = 1` (because pre-UPEC `d` was still ≥ 0.8),
    /// dropping the entire monetary expansion at that boundary and
    /// drifting our reserves from the upstream chain forever after.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerState.PulsingReward` —
    /// `startStep` reads `pr ^. ppDL` where `pr = esPrevPParams`.
    previous_protocol_params: Option<crate::protocol_params::ProtocolParameters>,
    /// Aggregate deposit accounting.
    deposit_pot: DepositPot,
    /// Treasury and reserves accounting.
    accounting: AccountingState,
    /// Conway governance enactment state (constitution, quorum, lineage).
    enact_state: EnactState,
    /// Shelley genesis UTxO entries to activate when replay first reaches a
    /// Shelley-family block.
    pending_shelley_genesis_utxo: Option<
        Vec<(
            crate::eras::shelley::ShelleyTxIn,
            crate::eras::shelley::ShelleyTxOut,
        )>,
    >,
    /// Shelley genesis stake delegations to activate when replay first
    /// reaches a Shelley-family block.
    pending_shelley_genesis_stake: Option<Vec<(StakeCredential, PoolKeyHash)>>,
    /// Genesis delegation entries awaiting activation on the first
    /// Shelley-family block.
    pending_shelley_genesis_delegs: Option<BTreeMap<GenesisHash, GenesisDelegationState>>,
    /// Active genesis delegation mapping (genesis key → delegate + VRF).
    ///
    /// Populated from the `genDelegs` section of the Shelley genesis file
    /// and updated by `GenesisDelegation` certificates.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerState` — `GenDelegs`.
    gen_delegs: BTreeMap<GenesisHash, GenesisDelegationState>,
    /// Future genesis delegations scheduled by `GenesisDelegation`
    /// certificates.
    ///
    /// Keyed by `(activation_slot, genesis_hash)` and adopted into
    /// `gen_delegs` when the current slot reaches `activation_slot`.
    ///
    /// Reference: `Cardano.Ledger.Shelley.State` — `dsFutureGenDelegs`.
    future_gen_delegs: BTreeMap<FutureGenesisDelegKey, GenesisDelegationState>,
    /// Pending Shelley-era protocol parameter update proposals keyed by
    /// target epoch and genesis delegate key hash.
    ///
    /// Each transaction carrying a `ShelleyUpdate` (CDDL key 6) adds its
    /// per-genesis-hash proposals here.  At the epoch boundary when the
    /// target epoch arrives, proposals that reach a quorum (> 50% of
    /// `gen_delegs`) are merged and applied to `protocol_params`.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Ppup` — PPUP rule.
    pending_pparam_updates:
        BTreeMap<EpochNo, BTreeMap<GenesisHash, crate::protocol_params::ProtocolParameterUpdate>>,
    /// Accumulated per-transaction treasury donations (Conway `treasuryDonation`).
    ///
    /// Each valid Conway transaction's `treasury_donation` field is added
    /// here during block application.  At the epoch boundary the total is
    /// credited to the treasury and this field is reset to zero.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerState` — `utxosDonation`.
    utxos_donation: u64,
    /// Accumulated instantaneous rewards (MIR) state.
    ///
    /// MIR certificates (DCert tag 6, Shelley through Babbage) accumulate
    /// per-credential reward deltas and pot-to-pot transfer deltas here.
    /// At the epoch boundary the MIR rule applies accumulated rewards and
    /// clears this state.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerState` — `dsIRewards`.
    instantaneous_rewards: InstantaneousRewards,
    /// Number of genesis delegate key signatures required to authorise a
    /// MIR certificate.  Loaded from `ShelleyGenesis.updateQuorum` (mainnet: 5).
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxow` — `validateMIRInsufficientGenesisSigs`.
    genesis_update_quorum: u64,
    /// Number of consecutive epochs with no active governance proposals.
    ///
    /// Incremented at each epoch boundary when no non-expired proposals
    /// remain.  Reset to zero when a transaction contains new proposals.
    /// Used to extend DRep expiry so dormant epochs don't count against
    /// DRep activity.
    ///
    /// Reference: `Cardano.Ledger.Conway.State` — `vsNumDormantEpochs`;
    /// `Cardano.Ledger.Conway.Rules.Epoch` — `updateNumDormantEpochs`;
    /// `Cardano.Ledger.Conway.Rules.Certs` — `updateDormantDRepExpiry`.
    pub(crate) num_dormant_epochs: u64,
    /// Per-pool block production counts for the current epoch.
    ///
    /// Each non-Byron, non-overlay block applied via
    /// [`apply_block_validated`] increments the count for the block's
    /// issuer pool (identified by `Blake2b-224(issuer_vkey)`).  At the
    /// epoch boundary, these counts are used to derive per-pool
    /// performance ratios which modulate the reward calculation, then
    /// cleared for the new epoch.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerState` — `NewEpochState.nesBcur`;
    /// `BlocksMade (EraCrypto era)`.
    blocks_made: BTreeMap<PoolKeyHash, u64>,
    /// Per-pool block production counts from the previous epoch.
    ///
    /// Upstream reward pulsing uses `nesBprev` when starting/completing a
    /// reward update, not the just-ending `nesBcur` counts. This delayed map
    /// is rotated from [`Self::blocks_made`] at epoch boundaries after any
    /// currently eligible rewards have been applied.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerState` —
    /// `NewEpochState.nesBprev`.
    blocks_made_prev: BTreeMap<PoolKeyHash, u64>,

    /// Maximum lovelace supply from genesis (mainnet: 45 000 000 000 000 000).
    ///
    /// Used to compute `circulation = max_lovelace_supply - reserves` for the
    /// upstream `maxPool` sigma/pledge denominator.  Not CBOR-serialized —
    /// re-set from genesis loading on every node startup.  When zero, the
    /// reward formula falls back to total active stake.
    ///
    /// Reference: `ShelleyGenesis.sgMaxLovelaceSupply`.
    max_lovelace_supply: u64,

    /// Slots per epoch from genesis (mainnet Shelley: 432000).
    ///
    /// Used to compute `eta` (monetary expansion efficiency factor) at
    /// epoch boundaries.  Not CBOR-serialized — set from genesis.
    ///
    /// Reference: `ShelleyGenesis.sgEpochLength`.
    slots_per_epoch: u64,

    /// Active slot coefficient from genesis (mainnet: 0.05, as numerator/denominator).
    ///
    /// Used to compute `expectedBlocks` for the `eta` monetary expansion
    /// factor.  Not CBOR-serialized — set from genesis.
    ///
    /// Reference: `ShelleyGenesis.sgActiveSlotsCoeff`.
    active_slot_coeff: UnitInterval,

    /// Stability window in slots (`3k/f` for Praos); used for PPUP
    /// slot-of-no-return calculations.  Not CBOR-serialized — set from
    /// genesis.
    ///
    /// When `Some`, block-apply paths construct a `PpupSlotContext` so
    /// the PPUP validator can enforce the exact upstream epoch-targeting
    /// rule (`getTheSlotOfNoReturn`).  When `None` the relaxed fallback
    /// (current or current+1) is used.
    ///
    /// Reference: `Cardano.Ledger.Slot.getTheSlotOfNoReturn`.
    stability_window: Option<u64>,

    /// Byron→Shelley transition `(boundary_slot, first_shelley_epoch)`.
    /// `None` for Shelley-only chains (preview); `Some` for chains with
    /// a Byron prefix (mainnet, preprod). Not CBOR-serialized — set from
    /// genesis.
    ///
    /// When `Some`, all `epoch → first_slot` math in the ledger
    /// (PPUP slot-of-no-return, MIR deadline, blocks_made overlay
    /// classification) uses the era-aware schedule. Without this,
    /// fixed-length math anchored at slot 0 produces a divergent
    /// `first_slot_next_epoch` for any chain with a Byron prefix
    /// (R263/R264 bug class).
    ///
    /// Reference: `Cardano.Slotting.EpochInfo` /
    /// `Cardano.Ledger.Slot::epochInfoFirst` — era-aware via the
    /// `EpochInfo` interpreter.
    byron_shelley_transition: Option<(u64, u64)>,
}

/// Restorable checkpoint of full ledger state.
///
/// This checkpoint is intended as a rollback and recovery seam for higher
/// layers such as storage and node orchestration. Unlike
/// [`LedgerStateSnapshot`], it preserves a restorable copy of the entire
/// mutable ledger state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerStateCheckpoint {
    state: LedgerState,
}

impl CborEncode for LedgerState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(24);
        self.current_era.encode_cbor(enc);
        self.tip.encode_cbor(enc);
        match self.expected_network_id {
            Some(network_id) => {
                enc.unsigned(u64::from(network_id));
            }
            None => {
                enc.null();
            }
        }
        enc.map(self.governance_actions.len() as u64);
        for (gov_action_id, state) in &self.governance_actions {
            gov_action_id.encode_cbor(enc);
            state.encode_cbor(enc);
        }
        self.pool_state.encode_cbor(enc);
        self.stake_credentials.encode_cbor(enc);
        self.committee_state.encode_cbor(enc);
        self.drep_state.encode_cbor(enc);
        self.reward_accounts.encode_cbor(enc);
        self.multi_era_utxo.encode_cbor(enc);
        self.shelley_utxo.encode_cbor(enc);
        // Encode protocol_params as either the params map or CBOR null.
        match &self.protocol_params {
            Some(pp) => pp.encode_cbor(enc),
            None => {
                enc.null();
            }
        }
        self.deposit_pot.encode_cbor(enc);
        self.accounting.encode_cbor(enc);
        self.current_epoch.encode_cbor(enc);
        self.enact_state.encode_cbor(enc);
        // gen_delegs: map of genesis-hash → (delegate, vrf)
        enc.map(self.gen_delegs.len() as u64);
        for (genesis_hash, deleg) in &self.gen_delegs {
            enc.bytes(genesis_hash);
            enc.array(2);
            enc.bytes(&deleg.delegate);
            enc.bytes(&deleg.vrf);
        }
        // pending_pparam_updates: map epoch → map genesis-hash → update
        enc.map(self.pending_pparam_updates.len() as u64);
        for (epoch, proposals) in &self.pending_pparam_updates {
            epoch.encode_cbor(enc);
            enc.map(proposals.len() as u64);
            for (genesis_hash, update) in proposals {
                enc.bytes(genesis_hash);
                update.encode_cbor(enc);
            }
        }
        // utxos_donation: accumulated treasury donations (Conway).
        enc.unsigned(self.utxos_donation);
        // instantaneous_rewards: accumulated MIR state (Shelley–Babbage).
        self.instantaneous_rewards.encode_cbor(enc);
        // genesis_update_quorum: MIR cert signature threshold.
        enc.unsigned(self.genesis_update_quorum);
        // num_dormant_epochs: consecutive dormant epoch count (Conway).
        enc.unsigned(self.num_dormant_epochs);
        // blocks_made: per-pool block production counts (current epoch).
        // Reference: NewEpochState.nesBcur.
        enc.map(self.blocks_made.len() as u64);
        for (pool_hash, &count) in &self.blocks_made {
            enc.bytes(pool_hash);
            enc.unsigned(count);
        }
        // blocks_made_prev: delayed per-pool block counts used by rewards.
        // Reference: NewEpochState.nesBprev.
        enc.map(self.blocks_made_prev.len() as u64);
        for (pool_hash, &count) in &self.blocks_made_prev {
            enc.bytes(pool_hash);
            enc.unsigned(count);
        }
    }
}

impl CborDecode for LedgerState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        // Accept legacy 9/10-element arrays and current 12-24-element arrays.
        if len != 9 && len != 10 && !(12..=24).contains(&len) {
            return Err(LedgerError::CborInvalidLength {
                expected: 24,
                actual: len as usize,
            });
        }

        let current_era = Era::decode_cbor(dec)?;
        let tip = Point::decode_cbor(dec)?;
        let expected_network_id = if len >= 13 {
            if dec.peek_is_null() {
                dec.skip()?;
                None
            } else {
                Some(dec.unsigned()? as u8)
            }
        } else {
            None
        };
        let governance_actions = if len >= 14 {
            let map_len = dec.map()?;
            let mut governance_actions = BTreeMap::new();
            for _ in 0..map_len {
                let gov_action_id = crate::eras::conway::GovActionId::decode_cbor(dec)?;
                let state = GovernanceActionState::decode_cbor(dec)?;
                governance_actions.insert(gov_action_id, state);
            }
            governance_actions
        } else {
            BTreeMap::new()
        };
        let pool_state = PoolState::decode_cbor(dec)?;
        let stake_credentials = StakeCredentials::decode_cbor(dec)?;
        let committee_state = CommitteeState::decode_cbor(dec)?;
        let drep_state = DrepState::decode_cbor(dec)?;
        let reward_accounts = RewardAccounts::decode_cbor(dec)?;
        let multi_era_utxo = MultiEraUtxo::decode_cbor(dec)?;
        let shelley_utxo = ShelleyUtxo::decode_cbor(dec)?;

        let protocol_params = if len >= 10 {
            if dec.peek_is_null() {
                dec.skip()?;
                None
            } else {
                Some(crate::protocol_params::ProtocolParameters::decode_cbor(
                    dec,
                )?)
            }
        } else {
            None
        };

        let deposit_pot = if len >= 12 {
            DepositPot::decode_cbor(dec)?
        } else {
            DepositPot::default()
        };

        let accounting = if len >= 12 {
            AccountingState::decode_cbor(dec)?
        } else {
            AccountingState::default()
        };

        let current_epoch = if len >= 15 {
            EpochNo::decode_cbor(dec)?
        } else {
            EpochNo(0)
        };

        let enact_state = if len >= 16 {
            EnactState::decode_cbor(dec)?
        } else {
            EnactState::default()
        };

        let gen_delegs = if len >= 17 {
            let map_len = dec.map()?;
            let mut delegs = BTreeMap::new();
            for _ in 0..map_len {
                let genesis_hash: GenesisHash = {
                    let bytes = dec.bytes()?;
                    let mut arr = [0u8; 28];
                    arr.copy_from_slice(bytes);
                    arr
                };
                let inner_len = dec.array()?;
                if inner_len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: inner_len as usize,
                    });
                }
                let delegate: GenesisDelegateHash = {
                    let bytes = dec.bytes()?;
                    let mut arr = [0u8; 28];
                    arr.copy_from_slice(bytes);
                    arr
                };
                let vrf: VrfKeyHash = {
                    let bytes = dec.bytes()?;
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(bytes);
                    arr
                };
                delegs.insert(genesis_hash, GenesisDelegationState { delegate, vrf });
            }
            delegs
        } else {
            BTreeMap::new()
        };

        let pending_pparam_updates = if len >= 18 {
            let outer_len = dec.map()?;
            let mut updates = BTreeMap::new();
            for _ in 0..outer_len {
                let epoch = EpochNo::decode_cbor(dec)?;
                let inner_len = dec.map()?;
                let mut proposals = BTreeMap::new();
                for _ in 0..inner_len {
                    let genesis_hash: GenesisHash = {
                        let bytes = dec.bytes()?;
                        let mut arr = [0u8; 28];
                        arr.copy_from_slice(bytes);
                        arr
                    };
                    let update = crate::protocol_params::ProtocolParameterUpdate::decode_cbor(dec)?;
                    proposals.insert(genesis_hash, update);
                }
                updates.insert(epoch, proposals);
            }
            updates
        } else {
            BTreeMap::new()
        };

        let utxos_donation = if len >= 19 { dec.unsigned()? } else { 0 };

        let instantaneous_rewards = if len >= 20 {
            InstantaneousRewards::decode_cbor(dec)?
        } else {
            InstantaneousRewards::default()
        };

        let genesis_update_quorum = if len >= 21 {
            dec.unsigned()?
        } else {
            5 // upstream default (mainnet)
        };

        let num_dormant_epochs = if len >= 22 { dec.unsigned()? } else { 0 };

        let blocks_made = if len >= 23 {
            let map_len = dec.map()?;
            let mut bm = BTreeMap::new();
            for _ in 0..map_len {
                let bytes = dec.bytes()?;
                let mut arr = [0u8; 28];
                arr.copy_from_slice(bytes);
                let count = dec.unsigned()?;
                bm.insert(arr, count);
            }
            bm
        } else {
            BTreeMap::new()
        };

        let blocks_made_prev = if len >= 24 {
            let map_len = dec.map()?;
            let mut bm = BTreeMap::new();
            for _ in 0..map_len {
                let bytes = dec.bytes()?;
                let mut arr = [0u8; 28];
                arr.copy_from_slice(bytes);
                let count = dec.unsigned()?;
                bm.insert(arr, count);
            }
            bm
        } else {
            BTreeMap::new()
        };

        Ok(Self {
            current_era,
            tip,
            latest_block_protocol_version: None,
            tip_block_no: None,
            current_epoch,
            expected_network_id,
            governance_actions,
            pool_state,
            stake_credentials,
            committee_state,
            drep_state,
            reward_accounts,
            multi_era_utxo,
            shelley_utxo,
            protocol_params,
            // Reconstructing from a checkpoint: the snapshot only
            // captured curPParams, not prevPParams.  Initialise prev = cur
            // so the first reward calc after checkpoint resume falls back
            // to current params; once UPEC fires again the field will
            // hold the proper pre-update value.
            previous_protocol_params: None,
            deposit_pot,
            accounting,
            enact_state,
            gen_delegs,
            future_gen_delegs: BTreeMap::new(),
            pending_pparam_updates,
            utxos_donation,
            instantaneous_rewards,
            genesis_update_quorum,
            num_dormant_epochs,
            blocks_made,
            blocks_made_prev,
            pending_shelley_genesis_utxo: None,
            pending_shelley_genesis_stake: None,
            pending_shelley_genesis_delegs: None,
            // Runtime-only fields — not serialized, re-set from genesis.
            max_lovelace_supply: 0,
            slots_per_epoch: 0,
            active_slot_coeff: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            stability_window: None,
            byron_shelley_transition: None,
        })
    }
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

impl LedgerState {
    /// Creates a new ledger state rooted at the given era with an `Origin`
    /// tip and an empty UTxO set.
    pub fn new(current_era: Era) -> Self {
        Self {
            current_era,
            tip: Point::Origin,
            latest_block_protocol_version: None,
            tip_block_no: None,
            current_epoch: EpochNo(0),
            expected_network_id: None,
            governance_actions: BTreeMap::new(),
            pool_state: PoolState::new(),
            stake_credentials: StakeCredentials::new(),
            committee_state: CommitteeState::new(),
            drep_state: DrepState::new(),
            reward_accounts: RewardAccounts::new(),
            multi_era_utxo: MultiEraUtxo::new(),
            shelley_utxo: ShelleyUtxo::new(),
            protocol_params: None,
            previous_protocol_params: None,
            deposit_pot: DepositPot::default(),
            accounting: AccountingState::default(),
            enact_state: EnactState::default(),
            pending_shelley_genesis_utxo: None,
            pending_shelley_genesis_stake: None,
            pending_shelley_genesis_delegs: None,
            gen_delegs: BTreeMap::new(),
            future_gen_delegs: BTreeMap::new(),
            pending_pparam_updates: BTreeMap::new(),
            utxos_donation: 0,
            instantaneous_rewards: InstantaneousRewards::default(),
            genesis_update_quorum: 5,
            num_dormant_epochs: 0,
            blocks_made: BTreeMap::new(),
            blocks_made_prev: BTreeMap::new(),
            max_lovelace_supply: 0,
            slots_per_epoch: 0,
            active_slot_coeff: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            stability_window: None,
            byron_shelley_transition: None,
        }
    }

    /// Returns the era currently active in this ledger state.
    pub fn current_era(&self) -> Era {
        self.current_era
    }

    /// Configures Shelley genesis UTxO entries that should become visible
    /// only when replay first reaches a Shelley-family block.
    pub fn configure_pending_shelley_genesis_utxo(
        &mut self,
        entries: Vec<(
            crate::eras::shelley::ShelleyTxIn,
            crate::eras::shelley::ShelleyTxOut,
        )>,
    ) {
        self.pending_shelley_genesis_utxo = if entries.is_empty() {
            None
        } else {
            Some(entries)
        };
    }

    /// Seeds the multi-era UTxO with Byron genesis UTxO entries.
    ///
    /// Byron genesis distributes initial Ada via two channels:
    /// `avvmDistr` (ADA Voucher Vending Machine) and `nonAvvmBalances`.
    /// For each non-zero entry the upstream `genesisUtxo` /
    /// `fromTxOut` formula computes:
    ///
    /// ```text
    ///     tx_id = Blake2b-256( CBOR(address) )
    ///     utxo[ TxIn(tx_id, 0) ] = TxOut(address, amount)
    /// ```
    ///
    /// where `address` is the canonical CBOR encoding of the Byron
    /// `Address` (already preserved as raw bytes in `address`).  The
    /// amount is part of the produced `TxOut`, not the pseudo transaction
    /// id. The resulting UTxO is available immediately at slot 0 so the
    /// first Byron transaction that spends a genesis output can resolve
    /// its inputs.
    ///
    /// Reference: `Cardano.Chain.Genesis.UTxO.genesisUtxo` and
    /// `Cardano.Chain.UTxO.UTxO.fromTxOut` in the upstream Byron ledger.
    pub fn seed_byron_genesis_utxo(&mut self, entries: impl IntoIterator<Item = (Vec<u8>, u64)>) {
        use crate::eras::shelley::{ShelleyTxIn, ShelleyTxOut};
        use crate::utxo::MultiEraTxOut;

        for (address, amount) in entries {
            if amount == 0 {
                continue;
            }
            // The base58-decoded address bytes are the canonical CBOR
            // encoding of the Byron `Address` (CBOR-in-CBOR with CRC32),
            // so `serializeCborHash txOutAddress` is this direct hash.
            let tx_id = yggdrasil_crypto::hash_bytes_256(&address).0;
            let txin = ShelleyTxIn {
                transaction_id: tx_id,
                index: 0,
            };
            let txout = ShelleyTxOut {
                address: address.clone(),
                amount,
            };
            self.multi_era_utxo
                .insert(txin, MultiEraTxOut::Shelley(txout));
        }
    }

    /// Configures Shelley genesis stake delegations that should become
    /// visible only when replay first reaches a Shelley-family block.
    pub fn configure_pending_shelley_genesis_stake(
        &mut self,
        entries: Vec<(StakeCredential, PoolKeyHash)>,
    ) {
        self.pending_shelley_genesis_stake = if entries.is_empty() {
            None
        } else {
            Some(entries)
        };
    }

    /// Configures genesis delegations (`genDelegs`) that should become
    /// active when replay first reaches a Shelley-family block.
    pub fn configure_pending_shelley_genesis_delegs(
        &mut self,
        entries: BTreeMap<GenesisHash, GenesisDelegationState>,
    ) {
        self.pending_shelley_genesis_delegs = if entries.is_empty() {
            None
        } else {
            Some(entries)
        };
    }

    /// Returns the active genesis delegation map.
    ///
    /// This is populated from the Shelley genesis file and updated by
    /// `GenesisDelegation` certificates during block application.
    pub fn gen_delegs(&self) -> &BTreeMap<GenesisHash, GenesisDelegationState> {
        &self.gen_delegs
    }

    /// Returns a mutable reference to the active genesis delegation map.
    pub fn gen_delegs_mut(&mut self) -> &mut BTreeMap<GenesisHash, GenesisDelegationState> {
        &mut self.gen_delegs
    }

    /// Returns the genesis delegation map effective for header validation,
    /// including any Shelley-genesis-derived entries that have not yet been
    /// activated by the first Shelley-family block.
    ///
    /// Upstream `Cardano.Ledger.Shelley.Genesis.initialState` populates
    /// `_dsGenDelegs` directly from `sgGenDelegs`, so the genesis delegate
    /// map is available from chain birth.  Yggdrasil keeps the entries in
    /// `pending_shelley_genesis_delegs` until the first Shelley-family
    /// block triggers `maybe_activate_pending_shelley_genesis`, but the
    /// TPraos overlay schedule and VRF checks must observe them
    /// immediately — otherwise the very first preview/preprod block from
    /// `Origin` is rejected as `TpraosOverlaySlotNotActive`.
    ///
    /// Reference: `Cardano.Protocol.TPraos.Rules.Overlay.overlaySchedule`
    /// (`genDelegs` parameter sourced from `nesEs.esLState.lsDPState`).
    pub fn effective_gen_delegs(&self) -> &BTreeMap<GenesisHash, GenesisDelegationState> {
        if !self.gen_delegs.is_empty() {
            &self.gen_delegs
        } else if let Some(pending) = self.pending_shelley_genesis_delegs.as_ref() {
            pending
        } else {
            &self.gen_delegs
        }
    }

    /// Returns a reference to pending Shelley-era protocol parameter update
    /// proposals, keyed by target epoch.
    pub fn pending_pparam_updates(
        &self,
    ) -> &BTreeMap<EpochNo, BTreeMap<GenesisHash, crate::protocol_params::ProtocolParameterUpdate>>
    {
        &self.pending_pparam_updates
    }

    /// Validates a Shelley-era protocol parameter update proposal against the
    /// upstream PPUP rule.
    ///
    /// Checks enforced:
    ///
    /// 1. **NonGenesisUpdatePPUP**: every proposer key hash in the update
    ///    must be a recognized genesis delegate in `gen_delegs`.
    /// 2. **PPUpdateWrongEpoch**: the target epoch must be valid. When an
    ///    optional `PpupSlotContext` is provided the check uses the upstream
    ///    slot-of-no-return boundary (`tooLate = first_slot(epoch+1) -
    ///    stability_window`) to enforce either `VoteForThisEpoch` or
    ///    `VoteForNextEpoch` semantics. Without slot context the relaxed
    ///    rule `target ∈ {current_epoch, current_epoch + 1}` applies.
    /// 3. **PVCannotFollowPPUP**: if a proposal includes a protocol version
    ///    update, it must follow `pvCanFollow` — either increment major by 1
    ///    (setting minor to 0) or keep major and increment minor by 1.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Ppup` — `ppupTransitionNonEmpty`.
    pub fn validate_ppup_proposal(
        &self,
        update: &crate::eras::shelley::ShelleyUpdate,
        slot_context: Option<&PpupSlotContext>,
    ) -> Result<(), LedgerError> {
        // 1. NonGenesisUpdatePPUP — every proposer must be a genesis delegate.
        for proposer in update.proposed_protocol_parameter_updates.keys() {
            if !self.gen_delegs.contains_key(proposer) {
                return Err(LedgerError::NonGenesisUpdatePPUP {
                    proposer: *proposer,
                });
            }
        }

        let target_epoch = update.epoch;
        let current = self.current_epoch.0;

        // 2. PPUpdateWrongEpoch
        if let Some(ctx) = slot_context {
            // Full upstream check using slot-of-no-return.
            // tooLate = first_slot_of_next_epoch - stability_window
            // R264: first_slot_next_epoch is now pre-computed era-aware
            // by the caller (`ppup_slot_context`) so any chain with a
            // Byron prefix gets the correct boundary.
            let too_late = ctx
                .first_slot_next_epoch
                .saturating_sub(ctx.stability_window);
            if ctx.slot < too_late {
                // Before the slot of no return: must vote for this epoch.
                if target_epoch != current {
                    return Err(LedgerError::PPUpdateWrongEpoch {
                        current_epoch: current,
                        target_epoch,
                        expected_epoch: current,
                        voting_period: "VoteForThisEpoch",
                    });
                }
            } else {
                // At or past the slot of no return: must vote for next epoch.
                if target_epoch != current + 1 {
                    return Err(LedgerError::PPUpdateWrongEpoch {
                        current_epoch: current,
                        target_epoch,
                        expected_epoch: current + 1,
                        voting_period: "VoteForNextEpoch",
                    });
                }
            }
        } else {
            // Relaxed check: target must be current or current + 1.
            if target_epoch != current && target_epoch != current + 1 {
                return Err(LedgerError::PPUpdateWrongEpoch {
                    current_epoch: current,
                    target_epoch,
                    expected_epoch: current,
                    voting_period: "VoteForThisEpoch or VoteForNextEpoch",
                });
            }
        }

        // 3. PVCannotFollowPPUP — each proposal with a protocol version
        //    update must have a legal successor version.
        if let Some((cur_major, cur_minor)) = self
            .protocol_params
            .as_ref()
            .and_then(|pp| pp.protocol_version)
        {
            for ppu in update.proposed_protocol_parameter_updates.values() {
                if let Some((new_major, new_minor)) = ppu.protocol_version {
                    if !pv_can_follow(cur_major, cur_minor, new_major, new_minor) {
                        return Err(LedgerError::PVCannotFollowPPUP {
                            current_major: cur_major,
                            current_minor: cur_minor,
                            proposed_major: new_major,
                            proposed_minor: new_minor,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Collects protocol parameter update proposals from a `ShelleyUpdate`.
    ///
    /// Each proposal is stored under its target epoch and genesis key hash.
    /// Duplicate proposals from the same genesis key for the same epoch
    /// overwrite the earlier entry (last-writer-wins per block ordering).
    ///
    /// **Pre-condition**: the caller should call [`Self::validate_ppup_proposal`]
    /// first to enforce the upstream PPUP rule.
    pub fn collect_pparam_proposals(&mut self, update: &crate::eras::shelley::ShelleyUpdate) {
        let epoch = EpochNo(update.epoch);
        let epoch_proposals = self.pending_pparam_updates.entry(epoch).or_default();
        for (genesis_hash, param_update) in &update.proposed_protocol_parameter_updates {
            epoch_proposals.insert(*genesis_hash, param_update.clone());
        }
    }

    /// Applies any pending protocol parameter proposals whose target epoch
    /// matches `epoch`.
    ///
    /// The upstream Shelley PPUP rule requires a quorum: more than 50% of
    /// the genesis delegates (`gen_delegs`) must propose identical updates
    /// for the same epoch.  When multiple distinct updates are proposed, the
    /// update with the most votes wins if it exceeds quorum; otherwise no
    /// change is applied.
    ///
    /// After processing, all proposals for epochs ≤ `epoch` are removed so
    /// stale proposals do not accumulate.
    ///
    /// Returns the number of parameter fields updated (0 if no quorum).
    pub fn apply_pending_pparam_updates(&mut self, epoch: EpochNo) -> usize {
        let proposals = self.pending_pparam_updates.remove(&epoch);
        // Remove stale proposals for earlier epochs.
        self.pending_pparam_updates.retain(|e, _| *e > epoch);

        let proposals = match proposals {
            Some(p) if !p.is_empty() => p,
            _ => return 0,
        };

        let gen_delegs_count = self.gen_delegs.len();
        if gen_delegs_count == 0 {
            // No genesis delegates — cannot reach quorum.
            return 0;
        }

        // Only consider proposals from recognized genesis delegates.
        let valid_proposals: Vec<&crate::protocol_params::ProtocolParameterUpdate> = proposals
            .iter()
            .filter(|(hash, _)| self.gen_delegs.contains_key(*hash))
            .map(|(_, update)| update)
            .collect();

        if valid_proposals.is_empty() {
            return 0;
        }

        let quorum = gen_delegs_count / 2 + 1;

        // Group identical proposals and find the one with the most votes.
        // We compare proposals by their Debug representation as a simple
        // equality check (ProtocolParameterUpdate derives Eq).
        let mut vote_counts: Vec<(&crate::protocol_params::ProtocolParameterUpdate, usize)> =
            Vec::new();
        for proposal in &valid_proposals {
            if let Some(entry) = vote_counts.iter_mut().find(|(p, _)| *p == *proposal) {
                entry.1 += 1;
            } else {
                vote_counts.push((proposal, 1));
            }
        }

        // Find the proposal with the most votes.
        let best = vote_counts.iter().max_by_key(|(_, count)| *count);
        match best {
            Some((winning_update, count)) if *count >= quorum => {
                let params = self.protocol_params.get_or_insert_with(Default::default);
                params.apply_update(winning_update);
                // Count non-None fields as the number of updates applied.
                winning_update.field_count()
            }
            _ => 0,
        }
    }

    /// Applies all pending protocol-parameter updates whose target epoch is
    /// less than or equal to `epoch`, in epoch order.
    ///
    /// This is equivalent to repeatedly running the upstream epoch update
    /// step for every epoch boundary that may have been skipped by a sparse
    /// replay or recovered from an older checkpoint.  The exact-epoch helper
    /// intentionally prunes stale proposals; callers crossing an epoch
    /// boundary should use this catch-up variant so a due update is applied
    /// before later-epoch validation consumes the active protocol parameters.
    pub fn apply_due_pending_pparam_updates(&mut self, epoch: EpochNo) -> usize {
        let due_epochs: Vec<EpochNo> = self
            .pending_pparam_updates
            .keys()
            .copied()
            .filter(|pending_epoch| *pending_epoch <= epoch)
            .collect();

        due_epochs
            .into_iter()
            .map(|due_epoch| self.apply_pending_pparam_updates(due_epoch))
            .sum()
    }

    /// Returns a reference to registered stake-pool state.
    pub fn pool_state(&self) -> &PoolState {
        &self.pool_state
    }

    /// Returns a mutable reference to registered stake-pool state.
    pub fn pool_state_mut(&mut self) -> &mut PoolState {
        &mut self.pool_state
    }

    /// Returns a reference to registered stake-credential state.
    pub fn stake_credentials(&self) -> &StakeCredentials {
        &self.stake_credentials
    }

    /// Returns a mutable reference to registered stake-credential state.
    pub fn stake_credentials_mut(&mut self) -> &mut StakeCredentials {
        &mut self.stake_credentials
    }

    /// Returns a reference to known committee-member state.
    pub fn committee_state(&self) -> &CommitteeState {
        &self.committee_state
    }

    /// Returns a mutable reference to known committee-member state.
    pub fn committee_state_mut(&mut self) -> &mut CommitteeState {
        &mut self.committee_state
    }

    /// Returns a reference to registered DRep state.
    pub fn drep_state(&self) -> &DrepState {
        &self.drep_state
    }

    /// Removes DRep delegations from accounts that point to
    /// non-existent DReps.
    ///
    /// Upstream: `updateDRepDelegations` in
    /// `Cardano.Ledger.Conway.Rules.HardFork` — called at the PV 9→10
    /// transition.  Returns the number of cleaned delegations.
    pub fn cleanup_dangling_drep_delegations(&mut self) -> usize {
        self.stake_credentials
            .cleanup_dangling_drep_delegations(&self.drep_state)
    }

    /// Returns a mutable reference to registered DRep state.
    pub fn drep_state_mut(&mut self) -> &mut DrepState {
        &mut self.drep_state
    }

    /// Returns a reference to reward-account state.
    pub fn reward_accounts(&self) -> &RewardAccounts {
        &self.reward_accounts
    }

    /// Returns a mutable reference to reward-account state.
    pub fn reward_accounts_mut(&mut self) -> &mut RewardAccounts {
        &mut self.reward_accounts
    }

    /// Returns the registered state for `operator`, if present.
    pub fn registered_pool(&self, operator: &PoolKeyHash) -> Option<&RegisteredPool> {
        self.pool_state.get(operator)
    }

    /// Returns the stake-credential state for `credential`, if present.
    pub fn stake_credential_state(
        &self,
        credential: &StakeCredential,
    ) -> Option<&StakeCredentialState> {
        self.stake_credentials.get(credential)
    }

    /// Returns the committee-member state for `credential`, if present.
    pub fn committee_member_state(
        &self,
        credential: &StakeCredential,
    ) -> Option<&CommitteeMemberState> {
        self.committee_state.get(credential)
    }

    /// Returns the registered DRep state for `drep`, if present.
    pub fn registered_drep(&self, drep: &DRep) -> Option<&RegisteredDrep> {
        self.drep_state.get(drep)
    }

    /// Returns the reward-account state for `account`, if present.
    pub fn reward_account_state(&self, account: &RewardAccount) -> Option<&RewardAccountState> {
        self.reward_accounts.get(account)
    }

    /// Returns the visible reward balance for `account`.
    pub fn query_reward_balance(&self, account: &RewardAccount) -> u64 {
        self.reward_accounts.balance(account)
    }

    /// Returns a reference to the legacy Shelley UTxO set.
    ///
    /// This provides backward compatibility for existing tests that
    /// inspect Shelley-era outputs via `ShelleyUtxo`.
    pub fn utxo(&self) -> &ShelleyUtxo {
        &self.shelley_utxo
    }

    /// Returns a mutable reference to the legacy Shelley UTxO set.
    ///
    /// Insertions via this accessor are mirrored into the multi-era UTxO
    /// so that block application works correctly.
    pub fn utxo_mut(&mut self) -> &mut ShelleyUtxo {
        &mut self.shelley_utxo
    }

    /// Returns a reference to the multi-era UTxO set.
    pub fn multi_era_utxo(&self) -> &MultiEraUtxo {
        &self.multi_era_utxo
    }

    /// Returns a mutable reference to the multi-era UTxO set.
    pub fn multi_era_utxo_mut(&mut self) -> &mut MultiEraUtxo {
        &mut self.multi_era_utxo
    }

    /// Returns the current protocol parameters, if set.
    pub fn protocol_params(&self) -> Option<&crate::protocol_params::ProtocolParameters> {
        self.protocol_params.as_ref()
    }

    /// Returns the **previous** protocol parameters (upstream
    /// `esPrevPParams`), if set — i.e. the value of `protocol_params`
    /// just before the most recent UPEC fired at an epoch boundary.
    /// Falls back to current `protocol_params` for early boundaries
    /// before any UPEC has run.
    ///
    /// Used by `apply_epoch_boundary` for the `eta` factor in the
    /// monetary expansion calc, matching upstream's
    /// `pr ^. ppDL` lookup in `startStep`.
    pub fn previous_protocol_params(&self) -> Option<&crate::protocol_params::ProtocolParameters> {
        self.previous_protocol_params
            .as_ref()
            .or(self.protocol_params.as_ref())
    }

    /// Captures the current `protocol_params` into `previous_protocol_params`.
    /// Called by `apply_epoch_boundary` immediately before UPEC applies any
    /// pending parameter update so that subsequent boundaries can see the
    /// pre-update value via `previous_protocol_params()`.
    pub fn snapshot_previous_protocol_params(&mut self) {
        self.previous_protocol_params = self.protocol_params.clone();
    }

    /// Returns a mutable reference to the protocol parameters slot.
    pub fn protocol_params_mut(
        &mut self,
    ) -> &mut Option<crate::protocol_params::ProtocolParameters> {
        &mut self.protocol_params
    }

    /// Returns the expected reward-account network id, if set.
    pub fn expected_network_id(&self) -> Option<u8> {
        self.expected_network_id
    }

    /// Returns the current epoch carried by the ledger state.
    pub fn current_epoch(&self) -> EpochNo {
        self.current_epoch
    }

    /// Returns stored governance action state for `id`, if present.
    pub fn governance_action(
        &self,
        id: &crate::eras::conway::GovActionId,
    ) -> Option<&GovernanceActionState> {
        self.governance_actions.get(id)
    }

    /// Returns all stored governance actions keyed by `GovActionId`.
    pub fn governance_actions(
        &self,
    ) -> &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState> {
        &self.governance_actions
    }

    /// Returns a mutable reference to stored governance actions.
    pub fn governance_actions_mut(
        &mut self,
    ) -> &mut BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState> {
        &mut self.governance_actions
    }

    /// Sets the expected reward-account network id used by environment-based validation.
    pub fn set_expected_network_id(&mut self, network_id: u8) {
        self.expected_network_id = Some(network_id);
    }

    /// Sets the genesis update quorum (number of genesis delegate signatures
    /// required to authorize a MIR certificate or protocol parameter update).
    ///
    /// Reference: `Cardano.Ledger.Shelley.Genesis` — `sgUpdateQuorum`.
    pub fn set_genesis_update_quorum(&mut self, quorum: u64) {
        self.genesis_update_quorum = quorum;
    }

    /// Returns the genesis update quorum threshold.
    pub fn genesis_update_quorum(&self) -> u64 {
        self.genesis_update_quorum
    }

    /// Returns the number of consecutive dormant epochs (no active governance proposals).
    pub fn num_dormant_epochs(&self) -> u64 {
        self.num_dormant_epochs
    }

    /// Returns a reference to the per-pool block production counts for the
    /// current epoch.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerState` — `nesBcur`.
    pub fn blocks_made(&self) -> &BTreeMap<PoolKeyHash, u64> {
        &self.blocks_made
    }

    /// Returns a reference to the delayed per-pool block production counts
    /// used by reward calculation.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerState` — `nesBprev`.
    pub fn previous_blocks_made(&self) -> &BTreeMap<PoolKeyHash, u64> {
        &self.blocks_made_prev
    }

    /// Records that the pool identified by `pool_hash` produced a
    /// counted block in the current epoch.
    ///
    /// This is the raw counter update.  Real block application should use
    /// [`Self::record_block_producer_for_block`] so TPraos overlay slots are
    /// skipped before incrementing `nesBcur`.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerState` — `nesBcur`.
    pub fn record_block_producer(&mut self, pool_hash: PoolKeyHash) {
        *self.blocks_made.entry(pool_hash).or_insert(0) += 1;
    }

    fn should_count_block_producer(
        &self,
        slot: u64,
        params: Option<&crate::protocol_params::ProtocolParameters>,
    ) -> bool {
        let Some(d) = params.and_then(|pp| pp.d) else {
            return true;
        };
        if self.slots_per_epoch == 0 {
            return true;
        }

        // R264: era-aware first_slot. Pre-fix this used fixed-length
        // `current_epoch * slots_per_epoch` which gives the wrong slot
        // for any chain with a Byron prefix — preprod's Shelley
        // epoch 4 has first_slot=86400, NOT 4*432000=1728000. Without
        // the era-aware lookup, every Shelley-overlay block on
        // preprod/mainnet was incorrectly counted in `nesBcur` and
        // distorted reward-cycle pool performance math.
        let first_slot = self.epoch_first_slot(self.current_epoch);
        !is_overlay_slot_for_blocks_made(first_slot, d, slot)
    }

    /// Records the block producer for a Shelley-family block when the block
    /// is not an overlay slot.
    ///
    /// This mirrors upstream `incrBlocks`: Byron blocks are excluded by the
    /// caller, and TPraos overlay slots are not inserted into `nesBcur`.
    /// The issuer may be a genesis delegate cold key in early eras; upstream
    /// still coerces it into the stake-pool key role for this accounting map.
    fn record_block_producer_for_block(
        &mut self,
        block: &crate::tx::Block,
        params: Option<&crate::protocol_params::ProtocolParameters>,
    ) {
        if !self.should_count_block_producer(block.header.slot_no.0, params) {
            return;
        }

        let pool_hash = yggdrasil_crypto::blake2b::hash_bytes_224(&block.header.issuer_vkey).0;
        self.record_block_producer(pool_hash);
    }

    /// Takes the current-epoch block production counts and replaces them
    /// with an empty map.
    ///
    /// Prefer [`Self::rotate_blocks_made_for_epoch_boundary`] for real
    /// epoch-boundary handling so `nesBprev` parity is preserved.
    pub fn take_blocks_made(&mut self) -> BTreeMap<PoolKeyHash, u64> {
        std::mem::take(&mut self.blocks_made)
    }

    /// Rotates current-epoch block counts into the delayed previous-epoch
    /// slot used by reward calculation, then clears current counts.
    ///
    /// This mirrors upstream `NewEpochState` rotation from `nesBcur` to
    /// `nesBprev`. Callers should run this after applying any reward update
    /// that used the old `nesBprev` value.
    pub fn rotate_blocks_made_for_epoch_boundary(&mut self) {
        self.blocks_made_prev = std::mem::take(&mut self.blocks_made);
    }

    /// Returns the maximum lovelace supply (genesis constant).
    pub fn max_lovelace_supply(&self) -> u64 {
        self.max_lovelace_supply
    }

    /// Sets the maximum lovelace supply from genesis configuration.
    pub fn set_max_lovelace_supply(&mut self, supply: u64) {
        self.max_lovelace_supply = supply;
    }

    /// Returns the slots-per-epoch genesis constant.
    pub fn slots_per_epoch(&self) -> u64 {
        self.slots_per_epoch
    }

    /// Sets the slots-per-epoch from genesis configuration.
    pub fn set_slots_per_epoch(&mut self, spe: u64) {
        self.slots_per_epoch = spe;
    }

    /// Sets the Byron→Shelley transition `(boundary_slot, first_shelley_epoch)`
    /// from the runtime config.  `None` keeps Shelley-only fixed-length
    /// math; `Some` enables era-aware first-slot computation across all
    /// PPUP / MIR / blocks_made boundary checks.
    ///
    /// R264: this MUST be set for any chain with a Byron prefix
    /// (mainnet, preprod) to avoid the same bug class as R263.
    pub fn set_byron_shelley_transition(&mut self, transition: Option<(u64, u64)>) {
        self.byron_shelley_transition = transition;
    }

    /// Era-aware first-slot of `epoch`.
    ///
    /// Mirrors `EpochSchedule::epoch_first_slot` (in `yggdrasil-consensus`)
    /// — returns the absolute slot number where epoch `epoch` begins.
    /// For chains with a Byron prefix this respects the boundary; for
    /// Shelley-only chains it falls back to fixed-length math anchored
    /// at slot 0.
    pub fn epoch_first_slot(&self, epoch: EpochNo) -> u64 {
        match self.byron_shelley_transition {
            Some((boundary_slot, first_shelley_epoch)) if epoch.0 >= first_shelley_epoch => {
                let post_epoch = epoch.0 - first_shelley_epoch;
                boundary_slot + post_epoch.saturating_mul(self.slots_per_epoch)
            }
            // Shelley-only path or pre-boundary epoch — fall back to
            // fixed-length math.  Pre-boundary path is exercised only
            // by Byron-internal callers that we do not currently have
            // (Byron blocks don't drive PPUP/MIR/blocks_made overlay
            // accounting).
            _ => epoch.0.saturating_mul(self.slots_per_epoch),
        }
    }

    /// Returns the active slot coefficient genesis constant.
    pub fn active_slot_coeff(&self) -> UnitInterval {
        self.active_slot_coeff
    }

    /// Sets the active slot coefficient from genesis configuration.
    pub fn set_active_slot_coeff(&mut self, asc: UnitInterval) {
        self.active_slot_coeff = asc;
    }

    /// Sets the stability window (`3k/f`) from genesis configuration.
    ///
    /// When set, PPUP validation uses the exact upstream slot-of-no-return
    /// rule instead of the relaxed epoch-boundary fallback.
    pub fn set_stability_window(&mut self, sw: u64) {
        self.stability_window = Some(sw);
    }

    /// Returns the configured stability window, if any.
    pub fn stability_window(&self) -> Option<u64> {
        self.stability_window
    }

    /// Rehydrates genesis-derived runtime fields after restoring a CBOR
    /// checkpoint.
    ///
    /// Checkpoint CBOR intentionally omits values that come from node
    /// configuration or genesis files rather than on-chain state. Storage and
    /// node recovery must call this with the genesis-seeded base ledger state
    /// before replaying blocks, otherwise epoch reward math loses the
    /// `maxLovelaceSupply` circulation denominator and falls back to active
    /// stake.
    pub fn rehydrate_runtime_genesis_from(&mut self, base_state: &Self) {
        self.max_lovelace_supply = base_state.max_lovelace_supply;
        self.slots_per_epoch = base_state.slots_per_epoch;
        self.active_slot_coeff = base_state.active_slot_coeff;
        self.stability_window = base_state.stability_window;

        // Pending Shelley genesis bootstrap bundles are also omitted from
        // checkpoint CBOR. They are only still relevant before the first
        // Shelley-family block has activated them.
        if self.current_era == Era::Byron {
            self.pending_shelley_genesis_utxo = base_state.pending_shelley_genesis_utxo.clone();
            self.pending_shelley_genesis_stake = base_state.pending_shelley_genesis_stake.clone();
            self.pending_shelley_genesis_delegs = base_state.pending_shelley_genesis_delegs.clone();
        }
    }

    /// Builds a [`PpupSlotContext`] for the given slot when the stability
    /// window is configured and `slots_per_epoch > 0`.
    ///
    /// Returns `None` when either value is unavailable, making the PPUP
    /// validator fall through to the relaxed epoch-boundary check.
    fn ppup_slot_context(&self, slot: u64) -> Option<PpupSlotContext> {
        let sw = self.stability_window?;
        if self.slots_per_epoch == 0 {
            return None;
        }
        // R264: era-aware first-slot-of-next-epoch (not
        // `(current_epoch + 1) * slots_per_epoch` which is wrong for
        // any chain with a Byron prefix).
        let first_slot_next_epoch = self.epoch_first_slot(EpochNo(self.current_epoch.0 + 1));
        Some(PpupSlotContext {
            slot,
            first_slot_next_epoch,
            stability_window: sw,
        })
    }

    /// Builds a [`MirValidationContext`] for MIR certificate validation.
    ///
    /// Returns `None` when the protocol parameters are unavailable (no
    /// validation will occur), which keeps mainnet-sync backward-compatible
    /// for the rare edges where genesis has not been loaded yet.
    fn mir_validation_context(
        &self,
        slot: u64,
        alonzo_mir_transfers: bool,
    ) -> Option<MirValidationContext<'_>> {
        let mir_deadline_slot = {
            let sw = self.stability_window?;
            if self.slots_per_epoch == 0 {
                None
            } else {
                // R264: era-aware first_slot. See `should_count_block_producer`.
                let first_slot_next_epoch =
                    self.epoch_first_slot(EpochNo(self.current_epoch.0 + 1));
                Some(first_slot_next_epoch.saturating_sub(sw))
            }
        };
        Some(MirValidationContext {
            current_slot: slot,
            mir_deadline_slot,
            alonzo_mir_transfers,
            reserves: self.accounting.reserves,
            treasury: self.accounting.treasury,
            instantaneous_rewards: &self.instantaneous_rewards,
        })
    }

    /// Sets the current epoch carried by the ledger state.
    pub fn set_current_epoch(&mut self, current_epoch: EpochNo) {
        self.current_epoch = current_epoch;
    }

    /// Sets the protocol parameters governing validation.
    pub fn set_protocol_params(&mut self, params: crate::protocol_params::ProtocolParameters) {
        self.protocol_params = Some(params);
    }

    /// Returns a reference to the deposit pot tracking key/pool/drep deposits.
    pub fn deposit_pot(&self) -> &DepositPot {
        &self.deposit_pot
    }

    /// Returns a mutable reference to the deposit pot.
    pub fn deposit_pot_mut(&mut self) -> &mut DepositPot {
        &mut self.deposit_pot
    }

    /// Returns a reference to the treasury/reserves accounting state.
    pub fn accounting(&self) -> &AccountingState {
        &self.accounting
    }

    /// Returns a mutable reference to the treasury/reserves accounting state.
    pub fn accounting_mut(&mut self) -> &mut AccountingState {
        &mut self.accounting
    }

    /// Returns the accumulated treasury donation total (Conway `utxosDonation`).
    ///
    /// This value accumulates per-transaction `treasury_donation` amounts
    /// during block application and is transferred to the treasury at
    /// each epoch boundary.
    pub fn utxos_donation(&self) -> u64 {
        self.utxos_donation
    }

    /// Adds `amount` to the accumulated treasury donation total.
    ///
    /// Called once per valid Conway transaction that carries a non-zero
    /// `treasury_donation` field.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Utxos` — UTXOS valid-tx
    /// branch: `utxos & utxosDonationL <>~ txBody ^. treasuryDonationTxBodyL`.
    pub fn accumulate_donation(&mut self, amount: u64) {
        self.utxos_donation = self.utxos_donation.saturating_add(amount);
    }

    /// Transfers accumulated donations to the treasury and resets the
    /// donation accumulator to zero.
    ///
    /// Returns the total transferred.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Epoch` — epoch boundary:
    /// `casTreasuryL <>~ utxosDonationL`, then `utxosDonationL .~ zero`.
    pub fn flush_donations_to_treasury(&mut self) -> u64 {
        let donated = self.utxos_donation;
        if donated > 0 {
            self.accounting.treasury = self.accounting.treasury.saturating_add(donated);
            self.utxos_donation = 0;
        }
        donated
    }

    /// Returns a reference to the accumulated instantaneous rewards state.
    pub fn instantaneous_rewards(&self) -> &InstantaneousRewards {
        &self.instantaneous_rewards
    }

    /// Returns a mutable reference to the accumulated instantaneous rewards state.
    pub fn instantaneous_rewards_mut(&mut self) -> &mut InstantaneousRewards {
        &mut self.instantaneous_rewards
    }

    /// Returns a reference to the Conway enactment state.
    pub fn enact_state(&self) -> &EnactState {
        &self.enact_state
    }

    /// Returns a mutable reference to the Conway enactment state.
    pub fn enact_state_mut(&mut self) -> &mut EnactState {
        &mut self.enact_state
    }

    /// Enacts a single ratified governance action against this ledger state.
    ///
    /// This avoids split-borrow issues by calling [`enact_gov_action`]
    /// with internal field references. The action is applied directly to
    /// the enact state, committee state, protocol parameters, reward
    /// accounts, and accounting.
    pub fn enact_action(
        &mut self,
        action_id: crate::eras::conway::GovActionId,
        action: &crate::eras::conway::GovAction,
    ) -> EnactOutcome {
        enact_gov_action_at_epoch(
            &mut self.enact_state,
            self.current_epoch,
            action_id,
            action,
            &mut self.committee_state,
            &mut self.protocol_params,
            &mut self.reward_accounts,
            &mut self.accounting,
        )
    }

    /// Captures a read-only snapshot of the current ledger state.
    pub fn snapshot(&self) -> LedgerStateSnapshot {
        LedgerStateSnapshot {
            current_era: self.current_era,
            tip: self.tip,
            latest_block_protocol_version: self.latest_block_protocol_version,
            tip_block_no: self.tip_block_no,
            current_epoch: self.current_epoch,
            expected_network_id: self.expected_network_id,
            governance_actions: self.governance_actions.clone(),
            pool_state: self.pool_state.clone(),
            stake_credentials: self.stake_credentials.clone(),
            committee_state: self.committee_state.clone(),
            drep_state: self.drep_state.clone(),
            reward_accounts: self.reward_accounts.clone(),
            multi_era_utxo: self.multi_era_utxo.clone(),
            shelley_utxo: self.shelley_utxo.clone(),
            protocol_params: self.protocol_params.clone(),
            deposit_pot: self.deposit_pot.clone(),
            accounting: self.accounting.clone(),
            enact_state: self.enact_state.clone(),
            gen_delegs: self.gen_delegs.clone(),
            stability_window: self.stability_window,
            num_dormant_epochs: self.num_dormant_epochs,
            // Round 192 — runtime attaches consensus-side ChainDepState
            // via `with_chain_dep_state(...)` after construction.
            chain_dep_state: None,
            // Round 202 — runtime attaches active stake snapshots via
            // `with_stake_snapshots(...)` after construction.
            stake_snapshots: None,
        }
    }

    /// Captures a restorable checkpoint of the current ledger state.
    ///
    /// This is a full-state clone intended for rollback-safe higher-layer
    /// coordination until more granular undo or replay machinery exists.
    pub fn checkpoint(&self) -> LedgerStateCheckpoint {
        LedgerStateCheckpoint {
            state: self.clone(),
        }
    }

    /// Restores the ledger state from a previously captured checkpoint.
    pub fn rollback_to_checkpoint(&mut self, checkpoint: &LedgerStateCheckpoint) {
        *self = checkpoint.restore();
    }

    /// Returns all UTxO entries paying to `address`.
    pub fn query_utxos_by_address(
        &self,
        address: &Address,
    ) -> Vec<(crate::eras::shelley::ShelleyTxIn, MultiEraTxOut)> {
        self.snapshot().query_utxos_by_address(address)
    }

    /// Returns the aggregate balance for `address` across visible UTxO entries.
    pub fn query_balance(&self, address: &Address) -> Value {
        self.snapshot().query_balance(address)
    }

    /// Applies a block to the current state.
    ///
    /// Each transaction body is decoded from CBOR according to the block's
    /// era and applied to the UTxO set. On any validation failure the state
    /// is unchanged (atomic per block).
    ///
    /// On success the tip advances to the applied block's slot and hash.
    pub fn apply_block(&mut self, block: &crate::tx::Block) -> Result<(), LedgerError> {
        self.apply_block_validated(block, None)
    }

    /// Applies a block with optional Plutus Phase-2 script evaluation.
    ///
    /// When `evaluator` is `Some`, Alonzo+ transactions with Plutus
    /// scripts have their scripts evaluated via the provided
    /// [`crate::plutus_validation::PlutusEvaluator`]. When `None`, Plutus scripts are silently
    /// skipped (soft-skip for sync without a CEK machine configured).
    pub fn apply_block_validated(
        &mut self,
        block: &crate::tx::Block,
        evaluator: Option<&dyn crate::plutus_validation::PlutusEvaluator>,
    ) -> Result<(), LedgerError> {
        let slot = block.header.slot_no.0;

        // Slot monotonicity: the block slot must strictly exceed the tip slot.
        // Byron-era blocks are exempt because Byron EBBs (Epoch Boundary
        // Blocks) share slot 0 with regular blocks — chain selection in
        // that era is driven by the block difficulty number instead.
        if block.era != Era::Byron {
            if let Some(tip_slot) = self.tip.slot() {
                if slot <= tip_slot.0 {
                    return Err(LedgerError::SlotNotIncreasing {
                        tip_slot: tip_slot.0,
                        block_slot: slot,
                    });
                }
            }
        }

        // Hard-fork combinator era-regression guard: once the ledger has
        // advanced to era N, it must never receive a block from era < N.
        // Era advances (N → N+1) and same-era blocks (N → N) are both valid.
        //
        // Genesis/origin state: when `current_era == Byron` and no blocks
        // have been applied yet, all eras are allowed (enables syncing from
        // a node configured to start at the latest era without having
        // replayed the full Byron chain).
        if self.tip != Point::Origin && self.current_era.is_era_regression(block.era) {
            return Err(LedgerError::BlockEraRegression {
                ledger_era: self.current_era,
                ledger_ordinal: self.current_era.era_ordinal(),
                block_era: block.era,
                block_ordinal: block.era.era_ordinal(),
            });
        }

        self.maybe_activate_pending_shelley_genesis(block.era);
        self.adopt_scheduled_genesis_delegations(slot);

        // Block-level size validation when protocol parameters are available.
        //
        // BBODY uses the full serialized transaction payload that appears in
        // the block body (body + witnesses + is_valid + aux/null), not just
        // transaction body bytes.
        //
        // Reference: `Cardano.Ledger.Shelley.Rules.Bbody` —
        // `validateMaxBlockBodySize`.
        if let Some(params) = &self.protocol_params {
            let body_size: usize = block
                .transactions
                .iter()
                .map(|tx| tx.serialized_size())
                .sum();
            if body_size > params.max_block_body_size as usize {
                return Err(LedgerError::BlockTooLarge {
                    actual: body_size,
                    max: params.max_block_body_size as usize,
                });
            }

            // BBODY header-size check: the serialized block header must
            // not exceed `max_block_header_size`.
            //
            // Reference: `Cardano.Ledger.Shelley.Rules.Bbody` —
            // `bHeaderSize bh ≤ maxBHSize pp`.
            if let Some(header_size) = block.header_cbor_size {
                if header_size > params.max_block_header_size as usize {
                    return Err(LedgerError::HeaderTooLarge {
                        actual: header_size,
                        max: params.max_block_header_size as usize,
                    });
                }
            }
        }

        let protocol_params_before_block = self.protocol_params.clone();
        self.adopt_block_protocol_version_for_validation(block.era, block.header.protocol_version);

        let apply_result = match block.era {
            Era::Byron => self.apply_byron_block(block, slot),
            Era::Shelley => self.apply_shelley_block(block, slot),
            Era::Allegra => self.apply_allegra_block(block, slot),
            Era::Mary => self.apply_mary_block(block, slot),
            Era::Alonzo => self.apply_alonzo_block(block, slot, evaluator),
            Era::Babbage => self.apply_babbage_block(block, slot, evaluator),
            Era::Conway => self.apply_conway_block(block, slot, evaluator),
        };
        if let Err(err) = apply_result {
            self.protocol_params = protocol_params_before_block;
            return Err(err);
        }

        // Track block producer for per-pool performance accounting.
        // Byron blocks are excluded because they predate the Shelley
        // reward system and have no meaningful issuer-pool identity.
        // TPraos overlay slots are skipped to mirror upstream `incrBlocks`.
        //
        // Reference: `Cardano.Ledger.Shelley.BlockBody.Internal.incrBlocks`.
        if block.era != Era::Byron {
            self.record_block_producer_for_block(block, protocol_params_before_block.as_ref());
        }

        self.current_era = block.era;
        self.tip = Point::BlockPoint(block.header.slot_no, block.header.hash);
        self.tip_block_no = Some(block.header.block_no);
        if let Some(pv) = block.header.protocol_version {
            self.latest_block_protocol_version = Some(pv);
        }
        Ok(())
    }

    /// Mirrors the HFC ledger-state translation step for protocol-version state.
    ///
    /// Upstream Babbage `validateScriptsWellFormed` checks Plutus availability
    /// against `ppProtocolVersionL`. In the full node, translating the ledger
    /// state at an era boundary sets that field to the new era's lower bound,
    /// not to every block-header minor version. This workspace keeps a single
    /// cross-era `ProtocolParameters` struct, so block application stages only
    /// a major-version era translation before era-specific validation and
    /// restores the old value if the block is rejected.
    fn adopt_block_protocol_version_for_validation(
        &mut self,
        era: Era,
        protocol_version: Option<(u64, u64)>,
    ) {
        let Some(protocol_version) = protocol_version else {
            return;
        };
        let Some(params) = self.protocol_params.as_mut() else {
            return;
        };
        let Some(min_major) = era_min_protocol_major(era) else {
            return;
        };
        if protocol_version.0 < min_major {
            return;
        }
        if params
            .protocol_version
            .is_none_or(|current| current.0 < min_major)
        {
            params.protocol_version = Some((min_major, 0));
        }
    }

    /// Applies a single submitted transaction to the current ledger state.
    ///
    /// This uses the same era-specific UTxO transition rules as block
    /// application while preserving atomicity: on validation failure, the
    /// ledger state is unchanged.
    pub fn apply_submitted_tx(
        &mut self,
        tx: &crate::tx::MultiEraSubmittedTx,
        current_slot: crate::types::SlotNo,
        evaluator: Option<&dyn crate::plutus_validation::PlutusEvaluator>,
    ) -> Result<(), LedgerError> {
        self.adopt_scheduled_genesis_delegations(current_slot.0);
        let gen_delg_set = crate::witnesses::gen_delg_hash_set(&self.gen_delegs);
        match tx {
            crate::tx::MultiEraSubmittedTx::Shelley(tx) => {
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Shelley(o.clone()))
                        .collect();
                    // Use the on-wire submitted bytes (`tx.raw_cbor`) rather
                    // than `to_cbor_bytes()` — the latter re-encodes from the
                    // typed parts and produces a byte-canonical envelope that
                    // does not always match what the wallet/cardano-cli sent.
                    // The linear fee formula is sensitive to that drift.
                    // Matches the Allegra/Mary/Alonzo+ submitted-tx paths.
                    validate_pre_alonzo_tx(params, tx.raw_cbor.len(), tx.body.fee, &outputs)?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Shelley(o.clone()))
                        .collect();
                    validate_output_network_ids(expected_net, &outputs)?;
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        validate_withdrawal_network_ids(expected_net, withdrawals)?;
                    }
                }
                // VKey witness validation (Shelley submitted).
                {
                    let mut required = HashSet::new();
                    crate::witnesses::required_vkey_hashes_from_inputs_shelley(
                        &tx.body.inputs,
                        &self.shelley_utxo,
                        &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(
                            withdrawals,
                            &mut required,
                        );
                    }
                    // Upstream propWits: proposer genesis key hashes.
                    if let Some(update) = &tx.body.update {
                        crate::witnesses::required_vkey_hashes_from_ppup(
                            update,
                            &self.gen_delegs,
                            &mut required,
                        );
                    }
                    // Hash the on-wire body bytes (`raw_body`), not a
                    // re-encoding — see `MultiEraSubmittedTx::tx_id` for
                    // the rationale.  A wallet that uses a non-canonical
                    // CBOR encoding (e.g. indefinite-length collections)
                    // must still get the same body hash that every other
                    // Cardano implementation computes for it.
                    let tx_body_hash = crate::tx::compute_tx_id(&tx.raw_body).0;
                    validate_witnesses_typed(&tx.witness_set, &required, &tx_body_hash)?;
                    crate::witnesses::validate_mir_genesis_quorum_typed(
                        tx.body.certificates.as_deref(),
                        &gen_delg_set,
                        self.genesis_update_quorum,
                        &tx.witness_set,
                    )?;
                }
                // Native (MultiSig) script witness validation (Shelley submitted).
                {
                    let mut required_scripts = HashSet::new();
                    crate::witnesses::required_script_hashes_from_inputs_shelley(
                        &tx.body.inputs,
                        &self.shelley_utxo,
                        &mut required_scripts,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_script_hashes_from_cert(
                                cert,
                                &mut required_scripts,
                            );
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_script_hashes_from_withdrawals(
                            withdrawals,
                            &mut required_scripts,
                        );
                    }
                    if !required_scripts.is_empty() {
                        let ws_bytes = tx.witness_set.to_cbor_bytes();
                        let native_satisfied = validate_native_scripts_if_present(
                            Some(&ws_bytes),
                            &required_scripts,
                            current_slot.0,
                        )?;
                        // Shelley has no Plutus and no reference inputs; an
                        // empty MultiEraUtxo is sufficient.
                        let empty_utxo = MultiEraUtxo::new();
                        validate_required_script_witnesses(
                            Some(&ws_bytes),
                            &required_scripts,
                            &native_satisfied,
                            &empty_utxo,
                            None,
                            None,
                        )?;
                    }
                    validate_no_extraneous_script_witnesses_typed(
                        &tx.witness_set,
                        &required_scripts,
                        None, // Shelley: no reference inputs
                    )?;
                }
                let mut staged = self.shelley_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let cert_adj = apply_certificates_and_withdrawals_with_future(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &mut staged_future_gen_delegs,
                    &self.governance_actions,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                    current_slot.0,
                    self.stability_window,
                    self.mir_validation_context(current_slot.0, false).as_ref(),
                )?;
                staged.apply_tx_with_withdrawals(
                    crate::tx::compute_tx_id(&tx.raw_body).0,
                    &tx.body,
                    current_slot.0,
                    cert_adj.withdrawal_total,
                    cert_adj.total_deposits,
                    cert_adj.total_refunds,
                )?;
                self.shelley_utxo = staged;
                self.multi_era_utxo = MultiEraUtxo::from_shelley_utxo(&self.shelley_utxo);
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
                self.future_gen_delegs = staged_future_gen_delegs;
                accumulate_mir_from_certs(
                    &mut self.instantaneous_rewards,
                    tx.body.certificates.as_deref(),
                );
                // PPUP validation + collection (Shelley submitted).
                if let Some(ref update) = tx.body.update {
                    self.validate_ppup_proposal(
                        update,
                        self.ppup_slot_context(current_slot.0).as_ref(),
                    )?;
                    self.collect_pparam_proposals(update);
                }
            }
            crate::tx::MultiEraSubmittedTx::Allegra(tx) => {
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Shelley(o.clone()))
                        .collect();
                    validate_pre_alonzo_tx(params, tx.raw_cbor.len(), tx.body.fee, &outputs)?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Shelley(o.clone()))
                        .collect();
                    validate_output_network_ids(expected_net, &outputs)?;
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        validate_withdrawal_network_ids(expected_net, withdrawals)?;
                    }
                }
                // VKey witness validation (Allegra submitted).
                {
                    let mut required = HashSet::new();
                    crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                        &tx.body.inputs,
                        &self.multi_era_utxo,
                        &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(
                            withdrawals,
                            &mut required,
                        );
                    }
                    // Upstream propWits: proposer genesis key hashes.
                    if let Some(update) = &tx.body.update {
                        crate::witnesses::required_vkey_hashes_from_ppup(
                            update,
                            &self.gen_delegs,
                            &mut required,
                        );
                    }
                    validate_witnesses_typed(&tx.witness_set, &required, &tx.tx_id().0)?;
                    crate::witnesses::validate_mir_genesis_quorum_typed(
                        tx.body.certificates.as_deref(),
                        &gen_delg_set,
                        self.genesis_update_quorum,
                        &tx.witness_set,
                    )?;
                }
                let mut staged = self.multi_era_utxo.clone();
                // Native script validation (Allegra submitted path)
                let witness_bytes = tx.witness_set.to_cbor_bytes();
                let mut required_scripts = HashSet::new();
                crate::witnesses::required_script_hashes_from_inputs_multi_era(
                    &tx.body.inputs,
                    &staged,
                    &mut required_scripts,
                );
                if let Some(certs) = &tx.body.certificates {
                    for cert in certs {
                        crate::witnesses::required_script_hashes_from_cert(
                            cert,
                            &mut required_scripts,
                        );
                    }
                }
                if let Some(withdrawals) = &tx.body.withdrawals {
                    crate::witnesses::required_script_hashes_from_withdrawals(
                        withdrawals,
                        &mut required_scripts,
                    );
                }
                let native_satisfied = validate_native_scripts_if_present(
                    Some(&witness_bytes),
                    &required_scripts,
                    current_slot.0,
                )?;
                validate_required_script_witnesses(
                    Some(&witness_bytes),
                    &required_scripts,
                    &native_satisfied,
                    &staged,
                    None,
                    None,
                )?;
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                    None, // Shelley: no reference inputs
                )?;
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let cert_adj = apply_certificates_and_withdrawals_with_future(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &mut staged_future_gen_delegs,
                    &self.governance_actions,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                    current_slot.0,
                    self.stability_window,
                    self.mir_validation_context(current_slot.0, false).as_ref(),
                )?;
                staged.apply_allegra_tx_withdrawals(
                    tx.tx_id().0,
                    &tx.body,
                    current_slot.0,
                    cert_adj.withdrawal_total,
                    cert_adj.total_deposits,
                    cert_adj.total_refunds,
                )?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
                self.future_gen_delegs = staged_future_gen_delegs;
                accumulate_mir_from_certs(
                    &mut self.instantaneous_rewards,
                    tx.body.certificates.as_deref(),
                );
                // PPUP validation + collection (Allegra submitted).
                if let Some(ref update) = tx.body.update {
                    self.validate_ppup_proposal(
                        update,
                        self.ppup_slot_context(current_slot.0).as_ref(),
                    )?;
                    self.collect_pparam_proposals(update);
                }
            }
            crate::tx::MultiEraSubmittedTx::Mary(tx) => {
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Mary(o.clone()))
                        .collect();
                    validate_pre_alonzo_tx(params, tx.raw_cbor.len(), tx.body.fee, &outputs)?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Mary(o.clone()))
                        .collect();
                    validate_output_network_ids(expected_net, &outputs)?;
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        validate_withdrawal_network_ids(expected_net, withdrawals)?;
                    }
                }
                // VKey witness validation (Mary submitted).
                {
                    let mut required = HashSet::new();
                    crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                        &tx.body.inputs,
                        &self.multi_era_utxo,
                        &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(
                            withdrawals,
                            &mut required,
                        );
                    }
                    // Upstream propWits: proposer genesis key hashes.
                    if let Some(update) = &tx.body.update {
                        crate::witnesses::required_vkey_hashes_from_ppup(
                            update,
                            &self.gen_delegs,
                            &mut required,
                        );
                    }
                    validate_witnesses_typed(&tx.witness_set, &required, &tx.tx_id().0)?;
                    crate::witnesses::validate_mir_genesis_quorum_typed(
                        tx.body.certificates.as_deref(),
                        &gen_delg_set,
                        self.genesis_update_quorum,
                        &tx.witness_set,
                    )?;
                }
                let mut staged = self.multi_era_utxo.clone();
                // Native script validation (Mary submitted path)
                let witness_bytes = tx.witness_set.to_cbor_bytes();
                let mut required_scripts = HashSet::new();
                crate::witnesses::required_script_hashes_from_inputs_multi_era(
                    &tx.body.inputs,
                    &staged,
                    &mut required_scripts,
                );
                if let Some(certs) = &tx.body.certificates {
                    for cert in certs {
                        crate::witnesses::required_script_hashes_from_cert(
                            cert,
                            &mut required_scripts,
                        );
                    }
                }
                if let Some(withdrawals) = &tx.body.withdrawals {
                    crate::witnesses::required_script_hashes_from_withdrawals(
                        withdrawals,
                        &mut required_scripts,
                    );
                }
                if let Some(mint) = &tx.body.mint {
                    crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
                }
                let native_satisfied = validate_native_scripts_if_present(
                    Some(&witness_bytes),
                    &required_scripts,
                    current_slot.0,
                )?;
                validate_required_script_witnesses(
                    Some(&witness_bytes),
                    &required_scripts,
                    &native_satisfied,
                    &staged,
                    None,
                    None,
                )?;
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                    None, // Allegra: no reference inputs
                )?;
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let cert_adj = apply_certificates_and_withdrawals_with_future(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &mut staged_future_gen_delegs,
                    &self.governance_actions,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                    current_slot.0,
                    self.stability_window,
                    self.mir_validation_context(current_slot.0, false).as_ref(),
                )?;
                staged.apply_mary_tx_withdrawals(
                    tx.tx_id().0,
                    &tx.body,
                    current_slot.0,
                    cert_adj.withdrawal_total,
                    cert_adj.total_deposits,
                    cert_adj.total_refunds,
                )?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
                self.future_gen_delegs = staged_future_gen_delegs;
                accumulate_mir_from_certs(
                    &mut self.instantaneous_rewards,
                    tx.body.certificates.as_deref(),
                );
                // PPUP validation + collection (Mary submitted).
                if let Some(ref update) = tx.body.update {
                    self.validate_ppup_proposal(
                        update,
                        self.ppup_slot_context(current_slot.0).as_ref(),
                    )?;
                    self.collect_pparam_proposals(update);
                }
            }
            crate::tx::MultiEraSubmittedTx::Alonzo(tx) => {
                // Reject submitted transactions with is_valid = false.
                // Only block producers may include Phase-2-failed transactions.
                if !tx.is_valid {
                    return Err(LedgerError::SubmittedTxIsInvalid);
                }
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                )?;
                let witness_bytes = tx.witness_set.to_cbor_bytes();
                let mut required_scripts = HashSet::new();
                crate::witnesses::required_script_hashes_from_inputs_multi_era(
                    &tx.body.inputs,
                    &self.multi_era_utxo,
                    &mut required_scripts,
                );
                if let Some(certs) = &tx.body.certificates {
                    for cert in certs {
                        crate::witnesses::required_script_hashes_from_cert(
                            cert,
                            &mut required_scripts,
                        );
                    }
                }
                if let Some(withdrawals) = &tx.body.withdrawals {
                    crate::witnesses::required_script_hashes_from_withdrawals(
                        withdrawals,
                        &mut required_scripts,
                    );
                }
                if let Some(mint) = &tx.body.mint {
                    crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
                }
                crate::plutus_validation::validate_script_data_hash(
                    tx.body.script_data_hash,
                    Some(&witness_bytes),
                    self.protocol_params.as_ref(),
                    false,
                    None,
                    None,
                    None,
                    Some(&required_scripts),
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                        .collect();
                    let total_eu = sum_redeemer_ex_units(&tx.witness_set);
                    let has_redeemers = !tx.witness_set.redeemers.is_empty();
                    validate_alonzo_plus_tx(
                        params,
                        &self.multi_era_utxo,
                        tx.size_for_fee_and_max(),
                        tx.body.fee,
                        &outputs,
                        None,
                        tx.body.collateral.as_deref(),
                        total_eu.as_ref(),
                        None,
                        None,
                        None,
                        has_redeemers,
                        0,
                        false,
                    )?;
                    // Per-redeemer ExUnits check (upstream validateExUnitsTooBigUTxO).
                    validate_per_redeemer_ex_units_from_witness_set(&tx.witness_set, params)?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal / WrongNetworkInTxBody)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                        .collect();
                    validate_output_network_ids(expected_net, &outputs)?;
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        validate_withdrawal_network_ids(expected_net, withdrawals)?;
                    }
                    validate_tx_body_network_id(expected_net, tx.body.network_id)?;
                }
                // VKey witness validation (Alonzo submitted).
                {
                    let mut required = HashSet::new();
                    crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                        &tx.body.inputs,
                        &self.multi_era_utxo,
                        &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(
                            withdrawals,
                            &mut required,
                        );
                    }
                    if let Some(signers) = &tx.body.required_signers {
                        for signer in signers {
                            required.insert(*signer);
                        }
                    }
                    // Upstream propWits: proposer genesis key hashes.
                    if let Some(update) = &tx.body.update {
                        crate::witnesses::required_vkey_hashes_from_ppup(
                            update,
                            &self.gen_delegs,
                            &mut required,
                        );
                    }
                    validate_witnesses_typed(&tx.witness_set, &required, &tx.tx_id().0)?;
                    crate::witnesses::validate_mir_genesis_quorum_typed(
                        tx.body.certificates.as_deref(),
                        &gen_delg_set,
                        self.genesis_update_quorum,
                        &tx.witness_set,
                    )?;
                }
                let mut staged = self.multi_era_utxo.clone();
                let mut required_scripts = HashSet::new();
                crate::witnesses::required_script_hashes_from_inputs_multi_era(
                    &tx.body.inputs,
                    &staged,
                    &mut required_scripts,
                );
                if let Some(certs) = &tx.body.certificates {
                    for cert in certs {
                        crate::witnesses::required_script_hashes_from_cert(
                            cert,
                            &mut required_scripts,
                        );
                    }
                }
                if let Some(withdrawals) = &tx.body.withdrawals {
                    crate::witnesses::required_script_hashes_from_withdrawals(
                        withdrawals,
                        &mut required_scripts,
                    );
                }
                if let Some(mint) = &tx.body.mint {
                    crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
                }
                let native_satisfied = validate_native_scripts_if_present(
                    Some(&witness_bytes),
                    &required_scripts,
                    current_slot.0,
                )?;
                validate_required_script_witnesses(
                    Some(&witness_bytes),
                    &required_scripts,
                    &native_satisfied,
                    &staged,
                    None,
                    None,
                )?;
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                    None, // Alonzo: no reference inputs
                )?;
                // Unspendable UTxO check (Alonzo — no datum on Plutus-locked input).
                crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                    &staged,
                    &tx.body.inputs,
                    &native_satisfied,
                    None, // Alonzo: no PlutusV3
                )?;
                // Output-side datum hash check: Alonzo outputs to script
                // addresses must carry datum_hash.
                // Reference: Cardano.Ledger.Alonzo.Rules.Utxo —
                //   validateOutputMissingDatumHashForScriptOutputs.
                crate::plutus_validation::validate_outputs_missing_datum_hash_alonzo(
                    &tx.body.outputs,
                )?;
                // Supplemental datum check (Alonzo submitted — no reference inputs).
                {
                    let tx_outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                        .collect();
                    crate::plutus_validation::validate_supplemental_datums(
                        Some(&witness_bytes),
                        &staged,
                        &tx.body.inputs,
                        &tx_outputs,
                        &[],
                    )?;
                }
                // ExtraRedeemer check (Alonzo submitted — Phase-1 UTXOW).
                {
                    let mut sorted_inputs = tx.body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = tx
                        .body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx
                        .body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    crate::plutus_validation::validate_no_extra_redeemers(
                        Some(&witness_bytes),
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &[],
                        &[],
                        None,
                    )?;
                    crate::plutus_validation::validate_no_missing_redeemers(
                        Some(&witness_bytes),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &[],
                        &[],
                        None,
                    )?;
                }
                // Phase-2 Plutus script validation (Alonzo submitted).
                // Submitted transactions always have is_valid = true (checked above),
                // so any Phase-2 failure is a hard reject.
                {
                    let mut sorted_inputs = tx.body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = tx
                        .body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx
                        .body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx.tx_id().0,
                        fee: tx.body.fee,
                        outputs: tx
                            .body
                            .outputs
                            .iter()
                            .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                            .collect(),
                        validity_start: tx.body.validity_interval_start,
                        ttl: tx.body.ttl,
                        required_signers: tx.body.required_signers.clone().unwrap_or_default(),
                        mint: tx.body.mint.clone().unwrap_or_default(),
                        withdrawals: tx.body.withdrawals.clone().unwrap_or_default(),
                        protocol_version: self
                            .protocol_params
                            .as_ref()
                            .and_then(|p| p.protocol_version),
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator,
                        Some(&witness_bytes),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &[],
                        &[],
                        &tx_ctx,
                        self.protocol_params
                            .as_ref()
                            .and_then(|p| p.cost_models.as_ref()),
                    )?;
                }
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let cert_adj = apply_certificates_and_withdrawals_with_future(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &mut staged_future_gen_delegs,
                    &self.governance_actions,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                    current_slot.0,
                    self.stability_window,
                    self.mir_validation_context(current_slot.0, true).as_ref(),
                )?;
                staged.apply_alonzo_tx_withdrawals(
                    tx.tx_id().0,
                    &tx.body,
                    current_slot.0,
                    cert_adj.withdrawal_total,
                    cert_adj.total_deposits,
                    cert_adj.total_refunds,
                )?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
                self.future_gen_delegs = staged_future_gen_delegs;
                accumulate_mir_from_certs(
                    &mut self.instantaneous_rewards,
                    tx.body.certificates.as_deref(),
                );
                // PPUP validation + collection (Alonzo submitted).
                if let Some(ref update) = tx.body.update {
                    self.validate_ppup_proposal(
                        update,
                        self.ppup_slot_context(current_slot.0).as_ref(),
                    )?;
                    self.collect_pparam_proposals(update);
                }
            }
            crate::tx::MultiEraSubmittedTx::Babbage(tx) => {
                // Reject submitted transactions with is_valid = false.
                if !tx.is_valid {
                    return Err(LedgerError::SubmittedTxIsInvalid);
                }
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                )?;
                // Babbage UTXOW: validateScriptsWellFormed.
                if let Some(eval) = evaluator {
                    let protocol_version = self
                        .protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version);
                    crate::witnesses::validate_script_witnesses_well_formed(
                        &tx.witness_set,
                        eval,
                        protocol_version,
                    )?;
                    crate::witnesses::validate_reference_scripts_well_formed(
                        &tx.body.outputs,
                        tx.body.collateral_return.as_ref(),
                        eval,
                        protocol_version,
                    )?;
                }
                let witness_bytes = tx.witness_set.to_cbor_bytes();
                if let Some(ref_inputs) = &tx.body.reference_inputs {
                    self.multi_era_utxo.validate_reference_inputs(ref_inputs)?;
                    // Babbage allows overlapping spending and reference inputs;
                    // disjointness is enforced only in Conway.
                }
                let mut required_scripts = HashSet::new();
                crate::witnesses::required_script_hashes_from_inputs_multi_era(
                    &tx.body.inputs,
                    &self.multi_era_utxo,
                    &mut required_scripts,
                );
                if let Some(certs) = &tx.body.certificates {
                    for cert in certs {
                        crate::witnesses::required_script_hashes_from_cert(
                            cert,
                            &mut required_scripts,
                        );
                    }
                }
                if let Some(withdrawals) = &tx.body.withdrawals {
                    crate::witnesses::required_script_hashes_from_withdrawals(
                        withdrawals,
                        &mut required_scripts,
                    );
                }
                if let Some(mint) = &tx.body.mint {
                    crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
                }
                crate::plutus_validation::validate_script_data_hash(
                    tx.body.script_data_hash,
                    Some(&witness_bytes),
                    self.protocol_params.as_ref(),
                    false,
                    Some(&self.multi_era_utxo),
                    tx.body.reference_inputs.as_deref(),
                    Some(&tx.body.inputs),
                    Some(&required_scripts),
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    let total_eu = sum_redeemer_ex_units(&tx.witness_set);
                    let has_redeemers = !tx.witness_set.redeemers.is_empty();
                    let coll_ret = tx
                        .body
                        .collateral_return
                        .as_ref()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()));
                    let output_sizes =
                        crate::eras::babbage::extract_babbage_tx_output_raw_sizes(tx.raw_body())?;
                    validate_alonzo_plus_tx(
                        params,
                        &self.multi_era_utxo,
                        tx.size_for_fee_and_max(),
                        tx.body.fee,
                        &outputs,
                        Some(&output_sizes.outputs),
                        tx.body.collateral.as_deref(),
                        total_eu.as_ref(),
                        coll_ret.as_ref(),
                        output_sizes.collateral_return,
                        tx.body.total_collateral,
                        has_redeemers,
                        0,
                        true,
                    )?;
                    // Per-redeemer ExUnits check (upstream validateExUnitsTooBigUTxO).
                    validate_per_redeemer_ex_units_from_witness_set(&tx.witness_set, params)?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal / WrongNetworkInTxBody)
                if let Some(expected_net) = self.expected_network_id {
                    let mut outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    // Upstream allSizedOutputsTxBodyF includes collateral_return.
                    if let Some(cr) = &tx.body.collateral_return {
                        outputs.push(MultiEraTxOut::Babbage(cr.clone()));
                    }
                    validate_output_network_ids(expected_net, &outputs)?;
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        validate_withdrawal_network_ids(expected_net, withdrawals)?;
                    }
                    validate_tx_body_network_id(expected_net, tx.body.network_id)?;
                }
                // VKey witness validation (Babbage submitted).
                {
                    let mut required = HashSet::new();
                    crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                        &tx.body.inputs,
                        &self.multi_era_utxo,
                        &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(
                            withdrawals,
                            &mut required,
                        );
                    }
                    if let Some(signers) = &tx.body.required_signers {
                        for signer in signers {
                            required.insert(*signer);
                        }
                    }
                    // Upstream propWits: proposer genesis key hashes.
                    if let Some(update) = &tx.body.update {
                        crate::witnesses::required_vkey_hashes_from_ppup(
                            update,
                            &self.gen_delegs,
                            &mut required,
                        );
                    }
                    validate_witnesses_typed(&tx.witness_set, &required, &tx.tx_id().0)?;
                    crate::witnesses::validate_mir_genesis_quorum_typed(
                        tx.body.certificates.as_deref(),
                        &gen_delg_set,
                        self.genesis_update_quorum,
                        &tx.witness_set,
                    )?;
                }
                let mut staged = self.multi_era_utxo.clone();
                let mut required_scripts = HashSet::new();
                crate::witnesses::required_script_hashes_from_inputs_multi_era(
                    &tx.body.inputs,
                    &staged,
                    &mut required_scripts,
                );
                if let Some(certs) = &tx.body.certificates {
                    for cert in certs {
                        crate::witnesses::required_script_hashes_from_cert(
                            cert,
                            &mut required_scripts,
                        );
                    }
                }
                if let Some(withdrawals) = &tx.body.withdrawals {
                    crate::witnesses::required_script_hashes_from_withdrawals(
                        withdrawals,
                        &mut required_scripts,
                    );
                }
                if let Some(mint) = &tx.body.mint {
                    crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
                }
                let native_satisfied = validate_native_scripts_if_present(
                    Some(&witness_bytes),
                    &required_scripts,
                    current_slot.0,
                )?;
                validate_required_script_witnesses(
                    Some(&witness_bytes),
                    &required_scripts,
                    &native_satisfied,
                    &staged,
                    tx.body.reference_inputs.as_deref(),
                    Some(&tx.body.inputs),
                )?;
                let babbage_ref_scripts =
                    collect_reference_script_hashes(&staged, tx.body.reference_inputs.as_deref());
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                    if babbage_ref_scripts.is_empty() {
                        None
                    } else {
                        Some(&babbage_ref_scripts)
                    },
                )?;
                // Unspendable UTxO check (Babbage — no datum on Plutus-locked input).
                crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                    &staged,
                    &tx.body.inputs,
                    &native_satisfied,
                    None, // Babbage: no PlutusV3
                )?;
                // Supplemental datum check (Babbage submitted — includes reference inputs).
                {
                    let mut tx_outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    if let Some(collateral_return) = &tx.body.collateral_return {
                        tx_outputs.push(MultiEraTxOut::Babbage(collateral_return.clone()));
                    }
                    let ref_utxos: Vec<(ShelleyTxIn, MultiEraTxOut)> = tx
                        .body
                        .reference_inputs
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .filter_map(|txin| {
                            staged.get(txin).map(|txout| (txin.clone(), txout.clone()))
                        })
                        .collect();
                    crate::plutus_validation::validate_supplemental_datums(
                        Some(&witness_bytes),
                        &staged,
                        &tx.body.inputs,
                        &tx_outputs,
                        &ref_utxos,
                    )?;
                }
                // ExtraRedeemer check (Babbage submitted — Phase-1 UTXOW).
                {
                    let mut sorted_inputs = tx.body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = tx
                        .body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx
                        .body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    crate::plutus_validation::validate_no_extra_redeemers(
                        Some(&witness_bytes),
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &[],
                        &[],
                        tx.body.reference_inputs.as_deref(),
                    )?;
                    crate::plutus_validation::validate_no_missing_redeemers(
                        Some(&witness_bytes),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &[],
                        &[],
                        tx.body.reference_inputs.as_deref(),
                    )?;
                }
                // Phase-2 Plutus script validation (Babbage submitted).
                {
                    let mut sorted_inputs = tx.body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = tx
                        .body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx
                        .body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx.tx_id().0,
                        fee: tx.body.fee,
                        outputs: tx
                            .body
                            .outputs
                            .iter()
                            .map(|o| MultiEraTxOut::Babbage(o.clone()))
                            .collect(),
                        validity_start: tx.body.validity_interval_start,
                        ttl: tx.body.ttl,
                        required_signers: tx.body.required_signers.clone().unwrap_or_default(),
                        mint: tx.body.mint.clone().unwrap_or_default(),
                        withdrawals: tx.body.withdrawals.clone().unwrap_or_default(),
                        reference_inputs: tx.body.reference_inputs.clone().unwrap_or_default(),
                        protocol_version: self
                            .protocol_params
                            .as_ref()
                            .and_then(|p| p.protocol_version),
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator,
                        Some(&witness_bytes),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &[],
                        &[],
                        &tx_ctx,
                        self.protocol_params
                            .as_ref()
                            .and_then(|p| p.cost_models.as_ref()),
                    )?;
                }
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let cert_adj = apply_certificates_and_withdrawals_with_future(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &mut staged_future_gen_delegs,
                    &self.governance_actions,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                    current_slot.0,
                    self.stability_window,
                    self.mir_validation_context(current_slot.0, true).as_ref(),
                )?;
                staged.apply_babbage_tx_withdrawals(
                    tx.tx_id().0,
                    &tx.body,
                    current_slot.0,
                    cert_adj.withdrawal_total,
                    cert_adj.total_deposits,
                    cert_adj.total_refunds,
                )?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
                self.future_gen_delegs = staged_future_gen_delegs;
                accumulate_mir_from_certs(
                    &mut self.instantaneous_rewards,
                    tx.body.certificates.as_deref(),
                );
                // PPUP validation + collection (Babbage submitted).
                if let Some(ref update) = tx.body.update {
                    self.validate_ppup_proposal(
                        update,
                        self.ppup_slot_context(current_slot.0).as_ref(),
                    )?;
                    self.collect_pparam_proposals(update);
                }
            }
            crate::tx::MultiEraSubmittedTx::Conway(tx) => {
                // Reject submitted transactions with is_valid = false.
                if !tx.is_valid {
                    return Err(LedgerError::SubmittedTxIsInvalid);
                }
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                )?;
                // Conway UTXOW: validateScriptsWellFormed.
                if let Some(eval) = evaluator {
                    let protocol_version = self
                        .protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version);
                    crate::witnesses::validate_script_witnesses_well_formed(
                        &tx.witness_set,
                        eval,
                        protocol_version,
                    )?;
                    crate::witnesses::validate_reference_scripts_well_formed(
                        &tx.body.outputs,
                        tx.body.collateral_return.as_ref(),
                        eval,
                        protocol_version,
                    )?;
                }
                let witness_bytes = tx.witness_set.to_cbor_bytes();
                if let Some(ref_inputs) = &tx.body.reference_inputs {
                    self.multi_era_utxo.validate_reference_inputs(ref_inputs)?;
                    if disjoint_ref_inputs_enforced(
                        self.protocol_params
                            .as_ref()
                            .and_then(|p| p.protocol_version),
                    ) {
                        MultiEraUtxo::validate_reference_input_disjointness(
                            &tx.body.inputs,
                            ref_inputs,
                        )?;
                    }
                }
                let mut required_scripts = HashSet::new();
                crate::witnesses::required_script_hashes_from_inputs_multi_era(
                    &tx.body.inputs,
                    &self.multi_era_utxo,
                    &mut required_scripts,
                );
                if let Some(certs) = &tx.body.certificates {
                    for cert in certs {
                        crate::witnesses::required_script_hashes_from_cert(
                            cert,
                            &mut required_scripts,
                        );
                    }
                }
                if let Some(withdrawals) = &tx.body.withdrawals {
                    crate::witnesses::required_script_hashes_from_withdrawals(
                        withdrawals,
                        &mut required_scripts,
                    );
                }
                if let Some(mint) = &tx.body.mint {
                    crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
                }
                if let Some(voting_procedures) = &tx.body.voting_procedures {
                    crate::witnesses::required_script_hashes_from_voting_procedures(
                        voting_procedures,
                        &mut required_scripts,
                    );
                }
                if let Some(proposal_procedures) = &tx.body.proposal_procedures {
                    crate::witnesses::required_script_hashes_from_proposal_procedures(
                        proposal_procedures,
                        &mut required_scripts,
                    );
                }
                crate::plutus_validation::validate_script_data_hash(
                    tx.body.script_data_hash,
                    Some(&witness_bytes),
                    self.protocol_params.as_ref(),
                    true,
                    Some(&self.multi_era_utxo),
                    tx.body.reference_inputs.as_deref(),
                    Some(&tx.body.inputs),
                    Some(&required_scripts),
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    let total_eu = sum_redeemer_ex_units(&tx.witness_set);
                    let has_redeemers = !tx.witness_set.redeemers.is_empty();
                    let coll_ret = tx
                        .body
                        .collateral_return
                        .as_ref()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()));
                    let ref_scripts_size = self.multi_era_utxo.total_ref_scripts_size(
                        &tx.body.inputs,
                        tx.body.reference_inputs.as_deref(),
                    );
                    let output_sizes =
                        crate::eras::babbage::extract_babbage_tx_output_raw_sizes(tx.raw_body())?;
                    validate_alonzo_plus_tx(
                        params,
                        &self.multi_era_utxo,
                        tx.size_for_fee_and_max(),
                        tx.body.fee,
                        &outputs,
                        Some(&output_sizes.outputs),
                        tx.body.collateral.as_deref(),
                        total_eu.as_ref(),
                        coll_ret.as_ref(),
                        output_sizes.collateral_return,
                        tx.body.total_collateral,
                        has_redeemers,
                        ref_scripts_size,
                        true,
                    )?;
                    // Per-redeemer ExUnits check (upstream validateExUnitsTooBigUTxO).
                    validate_per_redeemer_ex_units_from_witness_set(&tx.witness_set, params)?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal / WrongNetworkInTxBody)
                if let Some(expected_net) = self.expected_network_id {
                    let mut outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    // Upstream allSizedOutputsTxBodyF includes collateral_return.
                    if let Some(cr) = &tx.body.collateral_return {
                        outputs.push(MultiEraTxOut::Babbage(cr.clone()));
                    }
                    validate_output_network_ids(expected_net, &outputs)?;
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        validate_withdrawal_network_ids(expected_net, withdrawals)?;
                    }
                    validate_tx_body_network_id(expected_net, tx.body.network_id)?;
                }
                // VKey witness validation (Conway submitted).
                {
                    let mut required = HashSet::new();
                    crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                        &tx.body.inputs,
                        &self.multi_era_utxo,
                        &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(
                            withdrawals,
                            &mut required,
                        );
                    }
                    if let Some(signers) = &tx.body.required_signers {
                        for signer in signers {
                            required.insert(*signer);
                        }
                    }
                    if let Some(voting_procedures) = &tx.body.voting_procedures {
                        crate::witnesses::required_vkey_hashes_from_voting_procedures(
                            voting_procedures,
                            &mut required,
                        );
                    }
                    validate_witnesses_typed(&tx.witness_set, &required, &tx.tx_id().0)?;
                }
                let mut staged = self.multi_era_utxo.clone();
                // Conway LEDGER rule: total reference script size limit
                staged.validate_tx_ref_scripts_size(
                    &tx.body.inputs,
                    tx.body.reference_inputs.as_deref(),
                )?;
                let mut required_scripts = HashSet::new();
                crate::witnesses::required_script_hashes_from_inputs_multi_era(
                    &tx.body.inputs,
                    &staged,
                    &mut required_scripts,
                );
                if let Some(certs) = &tx.body.certificates {
                    for cert in certs {
                        crate::witnesses::required_script_hashes_from_cert(
                            cert,
                            &mut required_scripts,
                        );
                    }
                }
                if let Some(withdrawals) = &tx.body.withdrawals {
                    crate::witnesses::required_script_hashes_from_withdrawals(
                        withdrawals,
                        &mut required_scripts,
                    );
                }
                if let Some(mint) = &tx.body.mint {
                    crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
                }
                if let Some(voting_procedures) = &tx.body.voting_procedures {
                    crate::witnesses::required_script_hashes_from_voting_procedures(
                        voting_procedures,
                        &mut required_scripts,
                    );
                }
                if let Some(proposal_procedures) = &tx.body.proposal_procedures {
                    crate::witnesses::required_script_hashes_from_proposal_procedures(
                        proposal_procedures,
                        &mut required_scripts,
                    );
                }
                let native_satisfied = validate_native_scripts_if_present(
                    Some(&witness_bytes),
                    &required_scripts,
                    current_slot.0,
                )?;
                validate_required_script_witnesses(
                    Some(&witness_bytes),
                    &required_scripts,
                    &native_satisfied,
                    &staged,
                    tx.body.reference_inputs.as_deref(),
                    Some(&tx.body.inputs),
                )?;
                let conway_ref_scripts =
                    collect_reference_script_hashes(&staged, tx.body.reference_inputs.as_deref());
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                    if conway_ref_scripts.is_empty() {
                        None
                    } else {
                        Some(&conway_ref_scripts)
                    },
                )?;
                // Unspendable UTxO check (Conway — no datum on Plutus-locked input).
                // CIP-0069: collect PlutusV3 script hashes so V3-locked inputs
                // are exempt from the datum requirement.
                let v3_hashes = crate::plutus_validation::collect_v3_script_hashes(
                    Some(&tx.witness_set),
                    Some(&staged),
                    tx.body.reference_inputs.as_deref(),
                );
                crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                    &staged,
                    &tx.body.inputs,
                    &native_satisfied,
                    if v3_hashes.is_empty() {
                        None
                    } else {
                        Some(&v3_hashes)
                    },
                )?;
                // Supplemental datum check (Conway submitted — includes reference inputs).
                {
                    let mut tx_outputs: Vec<MultiEraTxOut> = tx
                        .body
                        .outputs
                        .iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    if let Some(collateral_return) = &tx.body.collateral_return {
                        tx_outputs.push(MultiEraTxOut::Babbage(collateral_return.clone()));
                    }
                    let ref_utxos: Vec<(ShelleyTxIn, MultiEraTxOut)> = tx
                        .body
                        .reference_inputs
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .filter_map(|txin| {
                            staged.get(txin).map(|txout| (txin.clone(), txout.clone()))
                        })
                        .collect();
                    crate::plutus_validation::validate_supplemental_datums(
                        Some(&witness_bytes),
                        &staged,
                        &tx.body.inputs,
                        &tx_outputs,
                        &ref_utxos,
                    )?;
                }
                // ExtraRedeemer check (Conway submitted — Phase-1 UTXOW).
                {
                    let mut sorted_inputs = tx.body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = tx
                        .body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx
                        .body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let sorted_voters: Vec<crate::eras::conway::Voter> = tx
                        .body
                        .voting_procedures
                        .as_ref()
                        .map(|vp| {
                            let mut vs: Vec<_> = vp.procedures.keys().cloned().collect();
                            vs.sort();
                            vs
                        })
                        .unwrap_or_default();
                    let proposal_slice: Vec<crate::eras::conway::ProposalProcedure> = tx
                        .body
                        .proposal_procedures
                        .as_deref()
                        .unwrap_or(&[])
                        .to_vec();
                    crate::plutus_validation::validate_no_extra_redeemers(
                        Some(&witness_bytes),
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &sorted_voters,
                        &proposal_slice,
                        tx.body.reference_inputs.as_deref(),
                    )?;
                    crate::plutus_validation::validate_no_missing_redeemers(
                        Some(&witness_bytes),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &sorted_voters,
                        &proposal_slice,
                        tx.body.reference_inputs.as_deref(),
                    )?;
                }
                // Conway UTXO rule: validate current_treasury_value declaration.
                // Phase-1 check — runs BEFORE Plutus evaluation, matching upstream UTXO rule ordering
                // and block-apply path placement (reference: Cardano.Ledger.Conway.Rules.Utxo).
                let current_treasury = self.accounting.treasury;
                validate_conway_current_treasury_value(
                    tx.body.current_treasury_value,
                    current_treasury,
                )?;

                // Phase-2 Plutus script validation (Conway submitted).
                {
                    let mut sorted_inputs = tx.body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = tx
                        .body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx
                        .body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let sorted_voters: Vec<crate::eras::conway::Voter> = tx
                        .body
                        .voting_procedures
                        .as_ref()
                        .map(|v| v.procedures.keys().cloned().collect())
                        .unwrap_or_default();
                    let proposal_slice = tx.body.proposal_procedures.as_deref().unwrap_or(&[]);
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx.tx_id().0,
                        fee: tx.body.fee,
                        outputs: tx
                            .body
                            .outputs
                            .iter()
                            .map(|o| MultiEraTxOut::Babbage(o.clone()))
                            .collect(),
                        validity_start: tx.body.validity_interval_start,
                        ttl: tx.body.ttl,
                        required_signers: tx.body.required_signers.clone().unwrap_or_default(),
                        mint: tx.body.mint.clone().unwrap_or_default(),
                        withdrawals: tx.body.withdrawals.clone().unwrap_or_default(),
                        reference_inputs: tx.body.reference_inputs.clone().unwrap_or_default(),
                        current_treasury_value: tx.body.current_treasury_value,
                        treasury_donation: tx.body.treasury_donation,
                        voting_procedures: tx.body.voting_procedures.clone(),
                        proposal_procedures: proposal_slice.to_vec(),
                        protocol_version: self
                            .protocol_params
                            .as_ref()
                            .and_then(|p| p.protocol_version),
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator,
                        Some(&witness_bytes),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &sorted_voters,
                        proposal_slice,
                        &tx_ctx,
                        self.protocol_params
                            .as_ref()
                            .and_then(|p| p.cost_models.as_ref()),
                    )?;
                }

                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
                let mut staged_governance_actions = self.governance_actions.clone();
                let mut staged_num_dormant = self.num_dormant_epochs;
                let cert_ctx = self.certificate_validation_context();

                // Upstream `updateDormantDRepExpiries` — bump all DRep
                // expiries and reset dormant counter when tx has proposals.
                let drep_activity = self
                    .protocol_params
                    .as_ref()
                    .and_then(|pp| pp.drep_activity)
                    .unwrap_or(0);
                update_dormant_drep_expiries(
                    tx.body
                        .proposal_procedures
                        .as_ref()
                        .is_some_and(|p| !p.is_empty()),
                    &mut staged_drep_state,
                    &mut staged_num_dormant,
                    self.current_epoch,
                    drep_activity,
                );

                // Conway LEDGER rule: withdrawal credentials must be delegated
                // to a DRep (post-bootstrap only, uses pre-CERTS state).
                validate_withdrawals_delegated(
                    tx.body.withdrawals.as_ref(),
                    &staged_stake_credentials,
                    cert_ctx.bootstrap_phase,
                )?;

                // Conway governance validation (voters, proposals, votes).
                let unregistered_drep_voters =
                    collect_conway_unregistered_drep_voters(tx.body.certificates.as_deref());

                if tx.body.voting_procedures.is_some()
                    || tx.body.proposal_procedures.is_some()
                    || !unregistered_drep_voters.is_empty()
                {
                    let (
                        governance_pool_state,
                        governance_stake_credentials,
                        governance_committee_state,
                        governance_drep_state,
                    ) = conway_governance_state_after_certificates(
                        &staged_pool_state,
                        &staged_stake_credentials,
                        &staged_committee_state,
                        &staged_drep_state,
                        &staged_reward_accounts,
                        &staged_deposit_pot,
                        &staged_gen_delegs,
                        &staged_governance_actions,
                        &cert_ctx,
                        tx.body.certificates.as_deref(),
                    )?;

                    let mut governance_actions_for_tx = staged_governance_actions.clone();

                    if let Some(voting_procedures) = &tx.body.voting_procedures {
                        // Upstream: UnelectedCommitteeVoters check runs first
                        // (hardforkConwayDisallowUnelectedCommitteeFromVoting).
                        validate_unelected_committee_voters(
                            voting_procedures,
                            &governance_committee_state,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.protocol_version),
                        )?;
                        validate_conway_voters(
                            voting_procedures,
                            &governance_pool_state,
                            &governance_committee_state,
                            &governance_drep_state,
                        )?;
                    }

                    if let Some(proposal_procedures) = &tx.body.proposal_procedures {
                        validate_conway_proposals(
                            tx.tx_id(),
                            proposal_procedures,
                            self.current_epoch,
                            &mut governance_actions_for_tx,
                            &governance_stake_credentials,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.protocol_version),
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.gov_action_deposit),
                            self.expected_network_id,
                            self.protocol_params.as_ref(),
                            &self.enact_state,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.gov_action_lifetime),
                        )?;
                    }

                    if let Some(voting_procedures) = &tx.body.voting_procedures {
                        validate_conway_vote_targets(
                            voting_procedures,
                            &governance_actions_for_tx,
                        )?;
                        validate_conway_voter_permissions(
                            self.current_epoch,
                            voting_procedures,
                            &governance_actions_for_tx,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.protocol_version),
                        )?;
                    }

                    staged_governance_actions = governance_actions_for_tx;
                    if let Some(voting_procedures) = &tx.body.voting_procedures {
                        apply_conway_votes(
                            voting_procedures,
                            &mut staged_governance_actions,
                            &mut staged_drep_state,
                            self.current_epoch,
                            staged_num_dormant,
                            cert_ctx.bootstrap_phase,
                        );
                    }
                    remove_conway_drep_votes(
                        &unregistered_drep_voters,
                        &mut staged_governance_actions,
                    );
                }

                let cert_adj = apply_certificates_and_withdrawals_with_future(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &mut staged_future_gen_delegs,
                    &self.governance_actions,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                    current_slot.0,
                    self.stability_window,
                    None, // Conway: MIR certs rejected as UnsupportedCertificate
                )?;
                // Track DRep activity for registration and update certificates.
                touch_drep_activity_for_certs(
                    tx.body.certificates.as_deref(),
                    &mut staged_drep_state,
                    self.current_epoch,
                    staged_num_dormant,
                    cert_ctx.bootstrap_phase,
                );
                // Conway UTXO rule: totalTxDeposits includes both certificate
                // deposits and proposal procedure deposits.
                // Reference: Cardano.Ledger.Conway.TxInfo — totalTxDeposits.
                let proposal_deposits: u64 = tx
                    .body
                    .proposal_procedures
                    .as_ref()
                    .map(|ps| ps.iter().map(|p| p.deposit).fold(0u64, u64::saturating_add))
                    .unwrap_or(0);
                // Track proposal deposits in the deposit pot (upstream oblProposal).
                staged_deposit_pot.add_proposal_deposit(proposal_deposits);
                let total_deposits = cert_adj.total_deposits.saturating_add(proposal_deposits);
                staged.apply_conway_tx_withdrawals(
                    tx.tx_id().0,
                    &tx.body,
                    current_slot.0,
                    cert_adj.withdrawal_total,
                    total_deposits,
                    cert_adj.total_refunds,
                )?;
                // Accumulate treasury donation (Conway UTXOS rule).
                // Reference: Cardano.Ledger.Conway.Rules.Utxos — utxosDonationL.
                // Reference: Cardano.Ledger.Conway.Rules.Utxo — validateZeroDonation.
                if let Some(donation) = tx.body.treasury_donation {
                    if donation == 0 {
                        return Err(LedgerError::ZeroDonation);
                    }
                    self.utxos_donation = self.utxos_donation.saturating_add(donation);
                }
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
                self.future_gen_delegs = staged_future_gen_delegs;
                self.governance_actions = staged_governance_actions;
                self.num_dormant_epochs = staged_num_dormant;
            }
        }

        Ok(())
    }

    // -- Private helpers ------------------------------------------------------

    /// Builds the context needed for certificate validation from the
    /// current protocol parameters and ledger state.
    fn certificate_validation_context(&self) -> CertificateValidationContext {
        let is_conway = matches!(self.current_era, Era::Conway);
        let pv = self
            .protocol_params
            .as_ref()
            .and_then(|p| p.protocol_version);
        let bootstrap_phase = is_conway && conway_bootstrap_phase(pv);
        let post_pv10 = is_conway && conway_post_pv10(pv);
        match &self.protocol_params {
            Some(p) => CertificateValidationContext {
                key_deposit: p.key_deposit,
                pool_deposit: p.pool_deposit,
                min_pool_cost: p.min_pool_cost,
                e_max: p.e_max,
                current_epoch: self.current_epoch,
                expected_network_id: self.expected_network_id,
                drep_deposit: p.drep_deposit,
                is_conway,
                bootstrap_phase,
                post_pv10,
            },
            None => CertificateValidationContext {
                key_deposit: 0,
                pool_deposit: 0,
                min_pool_cost: 0,
                e_max: u64::MAX,
                current_epoch: self.current_epoch,
                expected_network_id: self.expected_network_id,
                drep_deposit: None,
                is_conway,
                bootstrap_phase,
                post_pv10,
            },
        }
    }

    fn maybe_activate_pending_shelley_genesis(&mut self, next_era: Era) {
        if self.current_era != Era::Byron || next_era == Era::Byron {
            return;
        }

        // Byron→Shelley UTxO translation.
        //
        // Upstream `Cardano.Ledger.Shelley.Translation.translateUtxo`
        // converts every Byron `TxOut(addr, val)` into a Shelley
        // `TxOut(addr, Coin val)`, preserving `TxIn` keys bit-for-bit
        // (Byron txids are the same hash space as Shelley txids; the
        // 32-bit Byron output index always fits in the 16-bit Shelley
        // index in practice).  Without this step the first Shelley
        // block that spends a Byron-era output (e.g. preprod's seed
        // distribution) would fail with `InputNotFound`, since
        // `apply_shelley_block` reads exclusively from `shelley_utxo`.
        //
        // The Byron entries already live in `multi_era_utxo` as
        // `MultiEraTxOut::Shelley` (see `apply_byron_tx_with_id`), so
        // the translation reduces to draining those into `shelley_utxo`.
        let translated: Vec<_> = self
            .multi_era_utxo
            .iter()
            .filter_map(|(txin, txout)| match txout {
                crate::utxo::MultiEraTxOut::Shelley(out) => Some((txin.clone(), out.clone())),
                _ => None,
            })
            .collect();
        for (txin, txout) in translated {
            self.shelley_utxo.insert(txin, txout);
        }

        let utxo_entries = self.pending_shelley_genesis_utxo.take();
        let stake_entries = self.pending_shelley_genesis_stake.take();
        let deleg_entries = self.pending_shelley_genesis_delegs.take();
        if utxo_entries.is_none() && stake_entries.is_none() && deleg_entries.is_none() {
            return;
        }

        if let Some(entries) = utxo_entries {
            for (txin, txout) in entries {
                self.shelley_utxo.insert(txin.clone(), txout.clone());
                self.multi_era_utxo.insert_shelley(txin, txout);
            }
        }

        if let Some(entries) = stake_entries {
            for (credential, pool) in entries {
                match self.stake_credentials.get_mut(&credential) {
                    Some(state) => state.set_delegated_pool(Some(pool)),
                    None => {
                        self.stake_credentials.entries.insert(
                            credential,
                            StakeCredentialState::new_with_deposit(Some(pool), None, 0),
                        );
                    }
                }
            }
        }

        if let Some(entries) = deleg_entries {
            self.gen_delegs = entries;
        }
    }

    fn adopt_scheduled_genesis_delegations(&mut self, current_slot: u64) {
        apply_scheduled_genesis_delegations(
            &mut self.gen_delegs,
            &mut self.future_gen_delegs,
            current_slot,
        );
    }

    // -- Private per-era apply helpers --------------------------------------

    fn apply_byron_block(
        &mut self,
        block: &crate::tx::Block,
        _slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        // Decode each Tx.body (which is CBOR-encoded ByronTx) back into typed form.
        let decoded: Vec<ByronTx> = block
            .transactions
            .iter()
            .map(|tx| ByronTx::from_cbor_bytes(&tx.body))
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // Atomic: clone the multi-era UTxO, apply all txs, then commit.
        //
        // Use the pre-computed `Tx.id` (derived from the on-wire CBOR
        // bytes by `multi_era_block_to_block`) rather than re-deriving
        // from the decoded structure: Byron tx_ids are over the
        // annotated wire bytes, and re-encoding can produce a different
        // byte sequence (e.g. definite vs indefinite arrays) which
        // would yield a wrong tx_id and cause every spend of that
        // output to fail with `InputNotFound`.
        let mut staged = self.multi_era_utxo.clone();
        for (tx, byron_tx) in block.transactions.iter().zip(decoded.iter()) {
            staged.apply_byron_tx_with_id(tx.id.0, byron_tx)?;
        }
        self.multi_era_utxo = staged;
        Ok(())
    }

    fn apply_shelley_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(
            crate::types::TxId,
            usize,
            ShelleyTxBody,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ShelleyTxBody::from_cbor_bytes(&tx.body)?;
                Ok((
                    tx.id,
                    tx.serialized_size(),
                    body,
                    tx.witnesses.clone(),
                    tx.auxiliary_data.clone(),
                ))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // Atomic: clone the Shelley UTxO, apply all txs, then commit.
        // The legacy shelley_utxo is the authoritative source for Shelley
        // blocks (preserves backward compatibility with tests that seed
        // via utxo_mut()).
        let mut staged = self.shelley_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let mut staged_gen_delegs = self.gen_delegs.clone();
        let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
        let cert_ctx = self.certificate_validation_context();
        // Pre-compute genesis delegate key hash set for MIR quorum validation
        // (uses pre-block gen_delegs per upstream UTXOW rule).
        let gen_delg_set = crate::witnesses::gen_delg_hash_set(&self.gen_delegs);
        for (tx_id, tx_size, body, witness_bytes, aux_data) in &decoded {
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
            }
            // Network validation (Shelley UTXO rule: WrongNetwork / WrongNetworkWithdrawal)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
            }
            // Witness validation: compute required VKey hashes and check
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_shelley(
                &body.inputs,
                &staged,
                &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            // Upstream propWits: proposer genesis key hashes.
            if let Some(update) = &body.update {
                crate::witnesses::required_vkey_hashes_from_ppup(
                    update,
                    &self.gen_delegs,
                    &mut required,
                );
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // MIR genesis quorum check (validateMIRInsufficientGenesisSigs).
            crate::witnesses::validate_mir_genesis_quorum_if_present(
                body.certificates.as_deref(),
                &gen_delg_set,
                self.genesis_update_quorum,
                witness_bytes.as_deref(),
            )?;
            // Native (MultiSig) script validation (Shelley UTXOW —
            // validateFailedNativeScripts / validateMissingScripts /
            // extraneousScriptWitnessesUTXOW).
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_shelley(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            let native_satisfied = validate_native_scripts_if_present(
                witness_bytes.as_deref(),
                &required_scripts,
                slot,
            )?;
            // Shelley has no reference inputs and no Plutus; an empty
            // MultiEraUtxo is sufficient for required-witness collection.
            let empty_utxo = MultiEraUtxo::new();
            validate_required_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                &native_satisfied,
                &empty_utxo,
                None,
                None,
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                None, // Shelley: no reference inputs
            )?;
            let cert_adj = apply_certificates_and_withdrawals_with_future(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &mut staged_future_gen_delegs,
                &self.governance_actions,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
                slot,
                self.stability_window,
                self.mir_validation_context(slot, false).as_ref(),
            )?;
            staged.apply_tx_with_withdrawals(
                tx_id.0,
                body,
                slot,
                cert_adj.withdrawal_total,
                cert_adj.total_deposits,
                cert_adj.total_refunds,
            )?;
        }
        self.shelley_utxo = staged;
        self.multi_era_utxo = MultiEraUtxo::from_shelley_utxo(&self.shelley_utxo);
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        self.gen_delegs = staged_gen_delegs;
        self.future_gen_delegs = staged_future_gen_delegs;
        // Collect protocol parameter update proposals (PPUP rule) and
        // accumulate MIR certificates (Shelley through Babbage only).
        let ppup_ctx = self.ppup_slot_context(slot);
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data) in &decoded {
            if let Some(ref update) = body.update {
                self.validate_ppup_proposal(update, ppup_ctx.as_ref())?;
                self.collect_pparam_proposals(update);
            }
            accumulate_mir_from_certs(
                &mut self.instantaneous_rewards,
                body.certificates.as_deref(),
            );
        }
        Ok(())
    }

    fn apply_allegra_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(
            crate::types::TxId,
            usize,
            AllegraTxBody,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AllegraTxBody::from_cbor_bytes(&tx.body)?;
                Ok((
                    tx.id,
                    tx.serialized_size(),
                    body,
                    tx.witnesses.clone(),
                    tx.auxiliary_data.clone(),
                ))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let mut staged_gen_delegs = self.gen_delegs.clone();
        let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
        let cert_ctx = self.certificate_validation_context();
        // Pre-compute genesis delegate key hash set for MIR quorum validation.
        let gen_delg_set = crate::witnesses::gen_delg_hash_set(&self.gen_delegs);
        for (tx_id, tx_size, body, witness_bytes, aux_data) in &decoded {
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
            }
            // Network validation (Allegra UTXO rule)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
            }
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            // Upstream propWits: proposer genesis key hashes.
            if let Some(update) = &body.update {
                crate::witnesses::required_vkey_hashes_from_ppup(
                    update,
                    &self.gen_delegs,
                    &mut required,
                );
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // MIR genesis quorum check (validateMIRInsufficientGenesisSigs).
            crate::witnesses::validate_mir_genesis_quorum_if_present(
                body.certificates.as_deref(),
                &gen_delg_set,
                self.genesis_update_quorum,
                witness_bytes.as_deref(),
            )?;
            // Native script validation (Allegra+)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            let native_satisfied = validate_native_scripts_if_present(
                witness_bytes.as_deref(),
                &required_scripts,
                slot,
            )?;
            validate_required_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                &native_satisfied,
                &staged,
                None,
                None,
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                None, // Allegra: no reference inputs
            )?;
            let cert_adj = apply_certificates_and_withdrawals_with_future(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &mut staged_future_gen_delegs,
                &self.governance_actions,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
                slot,
                self.stability_window,
                self.mir_validation_context(slot, false).as_ref(),
            )?;
            staged.apply_allegra_tx_withdrawals(
                tx_id.0,
                body,
                slot,
                cert_adj.withdrawal_total,
                cert_adj.total_deposits,
                cert_adj.total_refunds,
            )?;
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        self.gen_delegs = staged_gen_delegs;
        self.future_gen_delegs = staged_future_gen_delegs;
        // Collect protocol parameter update proposals (PPUP rule) and
        // accumulate MIR certificates (Shelley through Babbage only).
        let ppup_ctx = self.ppup_slot_context(slot);
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data) in &decoded {
            if let Some(ref update) = body.update {
                self.validate_ppup_proposal(update, ppup_ctx.as_ref())?;
                self.collect_pparam_proposals(update);
            }
            accumulate_mir_from_certs(
                &mut self.instantaneous_rewards,
                body.certificates.as_deref(),
            );
        }
        Ok(())
    }

    fn apply_mary_block(&mut self, block: &crate::tx::Block, slot: u64) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(
            crate::types::TxId,
            usize,
            crate::eras::mary::MaryTxBody,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = crate::eras::mary::MaryTxBody::from_cbor_bytes(&tx.body)?;
                Ok((
                    tx.id,
                    tx.serialized_size(),
                    body,
                    tx.witnesses.clone(),
                    tx.auxiliary_data.clone(),
                ))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let mut staged_gen_delegs = self.gen_delegs.clone();
        let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
        let cert_ctx = self.certificate_validation_context();
        let gen_delg_set = crate::witnesses::gen_delg_hash_set(&self.gen_delegs);
        for (tx_id, tx_size, body, witness_bytes, aux_data) in &decoded {
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Mary(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
            }
            // Network validation (Mary UTXO rule)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Mary(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
            }
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            // Upstream propWits: proposer genesis key hashes.
            if let Some(update) = &body.update {
                crate::witnesses::required_vkey_hashes_from_ppup(
                    update,
                    &self.gen_delegs,
                    &mut required,
                );
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // MIR genesis quorum check (validateMIRInsufficientGenesisSigs).
            crate::witnesses::validate_mir_genesis_quorum_if_present(
                body.certificates.as_deref(),
                &gen_delg_set,
                self.genesis_update_quorum,
                witness_bytes.as_deref(),
            )?;
            // Native script validation (Mary)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            let native_satisfied = validate_native_scripts_if_present(
                witness_bytes.as_deref(),
                &required_scripts,
                slot,
            )?;
            validate_required_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                &native_satisfied,
                &staged,
                None,
                None,
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                None, // Mary: no reference inputs
            )?;
            let cert_adj = apply_certificates_and_withdrawals_with_future(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &mut staged_future_gen_delegs,
                &self.governance_actions,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
                slot,
                self.stability_window,
                self.mir_validation_context(slot, false).as_ref(),
            )?;
            staged.apply_mary_tx_withdrawals(
                tx_id.0,
                body,
                slot,
                cert_adj.withdrawal_total,
                cert_adj.total_deposits,
                cert_adj.total_refunds,
            )?;
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        self.gen_delegs = staged_gen_delegs;
        self.future_gen_delegs = staged_future_gen_delegs;
        // Collect protocol parameter update proposals (PPUP rule) and
        // accumulate MIR certificates (Shelley through Babbage only).
        let ppup_ctx = self.ppup_slot_context(slot);
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data) in &decoded {
            if let Some(ref update) = body.update {
                self.validate_ppup_proposal(update, ppup_ctx.as_ref())?;
                self.collect_pparam_proposals(update);
            }
            accumulate_mir_from_certs(
                &mut self.instantaneous_rewards,
                body.certificates.as_deref(),
            );
        }
        Ok(())
    }

    fn apply_alonzo_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
        evaluator: Option<&dyn crate::plutus_validation::PlutusEvaluator>,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(
            crate::types::TxId,
            usize,
            AlonzoTxBody,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<bool>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AlonzoTxBody::from_cbor_bytes(&tx.body)?;
                Ok((
                    tx.id,
                    tx.serialized_size(),
                    body,
                    tx.witnesses.clone(),
                    tx.auxiliary_data.clone(),
                    tx.is_valid,
                ))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // BBODY rule: block-level ExUnits limit.
        {
            let wb_refs: Vec<Option<&[u8]>> = decoded
                .iter()
                .map(|(_, _, _, wb, _, _)| wb.as_deref())
                .collect();
            validate_block_ex_units(self.protocol_params.as_ref(), &wb_refs)?;
        }

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let mut staged_gen_delegs = self.gen_delegs.clone();
        let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
        let cert_ctx = self.certificate_validation_context();
        let gen_delg_set = crate::witnesses::gen_delg_hash_set(&self.gen_delegs);
        for (tx_id, tx_size, body, witness_bytes, aux_data, is_valid) in &decoded {
            let tx_is_valid = is_valid.unwrap_or(true);
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            if let Some(mint) = &body.mint {
                crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
            }
            crate::plutus_validation::validate_script_data_hash(
                body.script_data_hash,
                witness_bytes.as_deref(),
                self.protocol_params.as_ref(),
                false,
                None,
                None,
                None,
                Some(&required_scripts),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                    .collect();
                validate_alonzo_plus_tx(
                    params,
                    &staged,
                    *tx_size,
                    body.fee,
                    &outputs,
                    None,
                    body.collateral.as_deref(),
                    total_eu.as_ref(),
                    None,
                    None,
                    None,
                    total_eu.is_some(),
                    0,
                    false,
                )?;
                // Per-redeemer ExUnits check (upstream validateExUnitsTooBigUTxO).
                validate_per_redeemer_ex_units_from_bytes(witness_bytes.as_deref(), params)?;
            }
            // Network validation (Alonzo UTXO rule: WrongNetwork + WrongNetworkInTxBody)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
                validate_tx_body_network_id(expected_net, body.network_id)?;
            }
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            if let Some(signers) = &body.required_signers {
                for signer in signers {
                    required.insert(*signer);
                }
            }
            // Upstream propWits: proposer genesis key hashes.
            if let Some(update) = &body.update {
                crate::witnesses::required_vkey_hashes_from_ppup(
                    update,
                    &self.gen_delegs,
                    &mut required,
                );
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // MIR genesis quorum check (validateMIRInsufficientGenesisSigs).
            crate::witnesses::validate_mir_genesis_quorum_if_present(
                body.certificates.as_deref(),
                &gen_delg_set,
                self.genesis_update_quorum,
                witness_bytes.as_deref(),
            )?;
            // Native script validation (Alonzo)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            if let Some(mint) = &body.mint {
                crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
            }
            let native_satisfied = validate_native_scripts_if_present(
                witness_bytes.as_deref(),
                &required_scripts,
                slot,
            )?;
            validate_required_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                &native_satisfied,
                &staged,
                None,
                None,
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                None, // Alonzo block: no reference inputs
            )?;
            // Unspendable UTxO check (Alonzo block — no datum on Plutus-locked input).
            crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                &staged,
                &body.inputs,
                &native_satisfied,
                None, // Alonzo: no PlutusV3
            )?;
            // Output-side datum hash check: Alonzo outputs to script
            // addresses must carry datum_hash.
            // Reference: Cardano.Ledger.Alonzo.Rules.Utxo —
            //   validateOutputMissingDatumHashForScriptOutputs.
            crate::plutus_validation::validate_outputs_missing_datum_hash_alonzo(&body.outputs)?;
            // Supplemental datum check (Alonzo — no reference inputs).
            {
                let tx_outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                    .collect();
                crate::plutus_validation::validate_supplemental_datums(
                    witness_bytes.as_deref(),
                    &staged,
                    &body.inputs,
                    &tx_outputs,
                    &[], // no reference inputs in Alonzo
                )?;
            }
            // ExtraRedeemer check (Alonzo block — Phase-1 UTXOW).
            // Upstream: hasExactSetOfRedeemers in alonzoUtxowTransition runs
            // unconditionally before UTXOS is_valid dispatching.
            {
                let mut sorted_inputs = body.inputs.clone();
                sorted_inputs.sort();
                let sorted_policies: Vec<[u8; 28]> = body
                    .mint
                    .as_ref()
                    .map(|m| m.keys().copied().collect())
                    .unwrap_or_default();
                let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                let sorted_rewards: Vec<Vec<u8>> = body
                    .withdrawals
                    .as_ref()
                    .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                    .unwrap_or_default();
                crate::plutus_validation::validate_no_extra_redeemers(
                    witness_bytes.as_deref(),
                    &staged,
                    &sorted_inputs,
                    &sorted_policies,
                    certs_slice,
                    &sorted_rewards,
                    &[],
                    &[],
                    None,
                )?;
                crate::plutus_validation::validate_no_missing_redeemers(
                    witness_bytes.as_deref(),
                    &required_scripts,
                    &staged,
                    &sorted_inputs,
                    &sorted_policies,
                    certs_slice,
                    &sorted_rewards,
                    &[],
                    &[],
                    None,
                )?;
            }
            // ── is_valid bifurcation (Phase-2 / collateral-only) ──
            let run_phase2 = || -> Result<(), LedgerError> {
                // Plutus script validation (Alonzo)
                {
                    let mut sorted_inputs = body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx_id.0,
                        fee: body.fee,
                        outputs: body
                            .outputs
                            .iter()
                            .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                            .collect(),
                        validity_start: body.validity_interval_start,
                        ttl: body.ttl,
                        required_signers: body.required_signers.clone().unwrap_or_default(),
                        mint: body.mint.clone().unwrap_or_default(),
                        withdrawals: body.withdrawals.clone().unwrap_or_default(),
                        protocol_version: self
                            .protocol_params
                            .as_ref()
                            .and_then(|p| p.protocol_version),
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator,
                        witness_bytes.as_deref(),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &[],
                        &[],
                        &tx_ctx,
                        self.protocol_params
                            .as_ref()
                            .and_then(|p| p.cost_models.as_ref()),
                    )
                }
            };
            if tx_is_valid {
                match run_phase2() {
                    Ok(()) => {}
                    Err(LedgerError::PlutusScriptFailed { hash, reason })
                        if evaluator.is_some() =>
                    {
                        return Err(LedgerError::ValidationTagMismatch {
                            claimed: true,
                            actual: false,
                            reason: phase2_failure_reason(&hash, &reason),
                        });
                    }
                    Err(e) => return Err(e),
                }
                let cert_adj = apply_certificates_and_withdrawals_with_future(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &mut staged_future_gen_delegs,
                    &self.governance_actions,
                    &cert_ctx,
                    body.certificates.as_deref(),
                    body.withdrawals.as_ref(),
                    slot,
                    self.stability_window,
                    self.mir_validation_context(slot, true).as_ref(),
                )?;
                staged.apply_alonzo_tx_withdrawals(
                    tx_id.0,
                    body,
                    slot,
                    cert_adj.withdrawal_total,
                    cert_adj.total_deposits,
                    cert_adj.total_refunds,
                )?;
            } else {
                if evaluator.is_some() {
                    match run_phase2() {
                        Ok(()) => {
                            return Err(LedgerError::ValidationTagMismatch {
                                claimed: false,
                                actual: true,
                                reason: "phase-2 unexpectedly succeeded".to_string(),
                            });
                        }
                        Err(LedgerError::PlutusScriptFailed { .. }) => {}
                        Err(e) => return Err(e),
                    }
                }
                // is_valid = false: collateral-only transition.
                // Alonzo has no collateral_return, so only consume collateral inputs.
                crate::utxo::apply_collateral_only(
                    &mut staged,
                    tx_id.0,
                    body.collateral.as_deref(),
                    None,
                    body.outputs.len(),
                );
            }
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        self.gen_delegs = staged_gen_delegs;
        self.future_gen_delegs = staged_future_gen_delegs;
        // Collect protocol parameter update proposals (PPUP rule) and
        // accumulate MIR certificates (Shelley through Babbage only).
        // Skip is_valid=false transactions — upstream alonzoEvalScriptsTxInvalid
        // returns `pure pup` (no PPUP) and does not run DELEGS (no MIR).
        let ppup_ctx = self.ppup_slot_context(slot);
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data, is_valid) in &decoded {
            if is_valid.unwrap_or(true) {
                if let Some(ref update) = body.update {
                    self.validate_ppup_proposal(update, ppup_ctx.as_ref())?;
                    self.collect_pparam_proposals(update);
                }
                accumulate_mir_from_certs(
                    &mut self.instantaneous_rewards,
                    body.certificates.as_deref(),
                );
            }
        }
        Ok(())
    }

    fn apply_babbage_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
        evaluator: Option<&dyn crate::plutus_validation::PlutusEvaluator>,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(
            crate::types::TxId,
            usize,
            BabbageTxBody,
            crate::eras::babbage::BabbageTxOutputRawSizes,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<bool>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = BabbageTxBody::from_cbor_bytes(&tx.body)?;
                let output_sizes =
                    crate::eras::babbage::extract_babbage_tx_output_raw_sizes(&tx.body)?;
                Ok((
                    tx.id,
                    tx.serialized_size(),
                    body,
                    output_sizes,
                    tx.witnesses.clone(),
                    tx.auxiliary_data.clone(),
                    tx.is_valid,
                ))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // BBODY rule: block-level ExUnits limit.
        {
            let wb_refs: Vec<Option<&[u8]>> = decoded
                .iter()
                .map(|(_, _, _, _, wb, _, _)| wb.as_deref())
                .collect();
            validate_block_ex_units(self.protocol_params.as_ref(), &wb_refs)?;
        }

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let mut staged_gen_delegs = self.gen_delegs.clone();
        let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
        let cert_ctx = self.certificate_validation_context();
        let gen_delg_set = crate::witnesses::gen_delg_hash_set(&self.gen_delegs);
        for (tx_id, tx_size, body, output_sizes, witness_bytes, aux_data, is_valid) in &decoded {
            let tx_is_valid = is_valid.unwrap_or(true);
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            // Babbage UTXOW: validateScriptsWellFormed.
            if let Some(eval) = evaluator {
                let protocol_version = self
                    .protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version);
                if let Some(wb) = witness_bytes.as_deref() {
                    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?;
                    crate::witnesses::validate_script_witnesses_well_formed(
                        &ws,
                        eval,
                        protocol_version,
                    )?;
                }
                let produced_outputs = if tx_is_valid {
                    body.outputs.as_slice()
                } else {
                    &[]
                };
                crate::witnesses::validate_reference_scripts_well_formed(
                    produced_outputs,
                    body.collateral_return.as_ref(),
                    eval,
                    protocol_version,
                )?;
            }
            if let Some(ref_inputs) = &body.reference_inputs {
                staged.validate_reference_inputs(ref_inputs)?;
                // Babbage allows overlapping spending and reference inputs;
                // disjointness is enforced only in Conway.
            }
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            if let Some(mint) = &body.mint {
                crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
            }
            crate::plutus_validation::validate_script_data_hash(
                body.script_data_hash,
                witness_bytes.as_deref(),
                self.protocol_params.as_ref(),
                false,
                Some(&staged),
                body.reference_inputs.as_deref(),
                Some(&body.inputs),
                Some(&required_scripts),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                let coll_ret = body
                    .collateral_return
                    .as_ref()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()));
                validate_alonzo_plus_tx(
                    params,
                    &staged,
                    *tx_size,
                    body.fee,
                    &outputs,
                    Some(&output_sizes.outputs),
                    body.collateral.as_deref(),
                    total_eu.as_ref(),
                    coll_ret.as_ref(),
                    output_sizes.collateral_return,
                    body.total_collateral,
                    total_eu.is_some(),
                    0,
                    true,
                )?;
                // Per-redeemer ExUnits check (upstream validateExUnitsTooBigUTxO).
                validate_per_redeemer_ex_units_from_bytes(witness_bytes.as_deref(), params)?;
            }
            // Network validation (Babbage UTXO rule: WrongNetwork + WrongNetworkInTxBody)
            if let Some(expected_net) = self.expected_network_id {
                let mut outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                // Upstream allSizedOutputsTxBodyF includes collateral_return.
                if let Some(cr) = &body.collateral_return {
                    outputs.push(MultiEraTxOut::Babbage(cr.clone()));
                }
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
                validate_tx_body_network_id(expected_net, body.network_id)?;
            }
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            if let Some(signers) = &body.required_signers {
                for signer in signers {
                    required.insert(*signer);
                }
            }
            // Upstream propWits: proposer genesis key hashes.
            if let Some(update) = &body.update {
                crate::witnesses::required_vkey_hashes_from_ppup(
                    update,
                    &self.gen_delegs,
                    &mut required,
                );
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // MIR genesis quorum check (validateMIRInsufficientGenesisSigs).
            crate::witnesses::validate_mir_genesis_quorum_if_present(
                body.certificates.as_deref(),
                &gen_delg_set,
                self.genesis_update_quorum,
                witness_bytes.as_deref(),
            )?;
            // Native script validation (Babbage)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            if let Some(mint) = &body.mint {
                crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
            }
            let native_satisfied = validate_native_scripts_if_present(
                witness_bytes.as_deref(),
                &required_scripts,
                slot,
            )?;
            validate_required_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                &native_satisfied,
                &staged,
                body.reference_inputs.as_deref(),
                Some(&body.inputs),
            )?;
            let babbage_blk_ref_scripts =
                collect_reference_script_hashes(&staged, body.reference_inputs.as_deref());
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                if babbage_blk_ref_scripts.is_empty() {
                    None
                } else {
                    Some(&babbage_blk_ref_scripts)
                },
            )?;
            // Unspendable UTxO check (Babbage block — no datum on Plutus-locked input).
            crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                &staged,
                &body.inputs,
                &native_satisfied,
                None, // Babbage: no PlutusV3
            )?;
            // Supplemental datum check (Babbage — includes reference inputs).
            {
                let mut tx_outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                if let Some(collateral_return) = &body.collateral_return {
                    tx_outputs.push(MultiEraTxOut::Babbage(collateral_return.clone()));
                }
                let ref_utxos: Vec<(ShelleyTxIn, MultiEraTxOut)> = body
                    .reference_inputs
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|txin| staged.get(txin).map(|txout| (txin.clone(), txout.clone())))
                    .collect();
                crate::plutus_validation::validate_supplemental_datums(
                    witness_bytes.as_deref(),
                    &staged,
                    &body.inputs,
                    &tx_outputs,
                    &ref_utxos,
                )?;
            }
            // ExtraRedeemer check (Babbage block — Phase-1 UTXOW).
            // Upstream: hasExactSetOfRedeemers in alonzoUtxowTransition runs
            // unconditionally before UTXOS is_valid dispatching.
            {
                let mut sorted_inputs = body.inputs.clone();
                sorted_inputs.sort();
                let sorted_policies: Vec<[u8; 28]> = body
                    .mint
                    .as_ref()
                    .map(|m| m.keys().copied().collect())
                    .unwrap_or_default();
                let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                let sorted_rewards: Vec<Vec<u8>> = body
                    .withdrawals
                    .as_ref()
                    .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                    .unwrap_or_default();
                crate::plutus_validation::validate_no_extra_redeemers(
                    witness_bytes.as_deref(),
                    &staged,
                    &sorted_inputs,
                    &sorted_policies,
                    certs_slice,
                    &sorted_rewards,
                    &[],
                    &[],
                    body.reference_inputs.as_deref(),
                )?;
                crate::plutus_validation::validate_no_missing_redeemers(
                    witness_bytes.as_deref(),
                    &required_scripts,
                    &staged,
                    &sorted_inputs,
                    &sorted_policies,
                    certs_slice,
                    &sorted_rewards,
                    &[],
                    &[],
                    body.reference_inputs.as_deref(),
                )?;
            }
            let run_phase2 = || -> Result<(), LedgerError> {
                // Plutus script validation (Babbage)
                {
                    let mut sorted_inputs = body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx_id.0,
                        fee: body.fee,
                        outputs: body
                            .outputs
                            .iter()
                            .map(|o| MultiEraTxOut::Babbage(o.clone()))
                            .collect(),
                        validity_start: body.validity_interval_start,
                        ttl: body.ttl,
                        required_signers: body.required_signers.clone().unwrap_or_default(),
                        mint: body.mint.clone().unwrap_or_default(),
                        withdrawals: body.withdrawals.clone().unwrap_or_default(),
                        reference_inputs: body.reference_inputs.clone().unwrap_or_default(),
                        protocol_version: self
                            .protocol_params
                            .as_ref()
                            .and_then(|p| p.protocol_version),
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator,
                        witness_bytes.as_deref(),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &[],
                        &[],
                        &tx_ctx,
                        self.protocol_params
                            .as_ref()
                            .and_then(|p| p.cost_models.as_ref()),
                    )
                }
            };
            if tx_is_valid {
                match run_phase2() {
                    Ok(()) => {}
                    Err(LedgerError::PlutusScriptFailed { hash, reason })
                        if evaluator.is_some() =>
                    {
                        return Err(LedgerError::ValidationTagMismatch {
                            claimed: true,
                            actual: false,
                            reason: phase2_failure_reason(&hash, &reason),
                        });
                    }
                    Err(e) => return Err(e),
                }
                let cert_adj = apply_certificates_and_withdrawals_with_future(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &mut staged_future_gen_delegs,
                    &self.governance_actions,
                    &cert_ctx,
                    body.certificates.as_deref(),
                    body.withdrawals.as_ref(),
                    slot,
                    self.stability_window,
                    self.mir_validation_context(slot, true).as_ref(),
                )?;
                staged.apply_babbage_tx_withdrawals(
                    tx_id.0,
                    body,
                    slot,
                    cert_adj.withdrawal_total,
                    cert_adj.total_deposits,
                    cert_adj.total_refunds,
                )?;
            } else {
                if evaluator.is_some() {
                    match run_phase2() {
                        Ok(()) => {
                            return Err(LedgerError::ValidationTagMismatch {
                                claimed: false,
                                actual: true,
                                reason: "phase-2 unexpectedly succeeded".to_string(),
                            });
                        }
                        Err(LedgerError::PlutusScriptFailed { .. }) => {}
                        Err(e) => return Err(e),
                    }
                }
                // is_valid = false: collateral-only transition.
                crate::utxo::apply_collateral_only(
                    &mut staged,
                    tx_id.0,
                    body.collateral.as_deref(),
                    body.collateral_return.as_ref(),
                    body.outputs.len(),
                );
            }
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        self.gen_delegs = staged_gen_delegs;
        self.future_gen_delegs = staged_future_gen_delegs;
        // Collect protocol parameter update proposals (PPUP rule) and
        // accumulate MIR certificates (Shelley through Babbage only).
        // Skip is_valid=false transactions — upstream alonzoEvalScriptsTxInvalid
        // returns `pure pup` (no PPUP) and does not run DELEGS (no MIR).
        let ppup_ctx = self.ppup_slot_context(slot);
        for (_tx_id, _tx_size, body, _output_sizes, _witness_bytes, _aux_data, is_valid) in &decoded
        {
            if is_valid.unwrap_or(true) {
                if let Some(ref update) = body.update {
                    self.validate_ppup_proposal(update, ppup_ctx.as_ref())?;
                    self.collect_pparam_proposals(update);
                }
                accumulate_mir_from_certs(
                    &mut self.instantaneous_rewards,
                    body.certificates.as_deref(),
                );
            }
        }
        Ok(())
    }

    fn apply_conway_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
        evaluator: Option<&dyn crate::plutus_validation::PlutusEvaluator>,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(
            crate::types::TxId,
            usize,
            ConwayTxBody,
            crate::eras::babbage::BabbageTxOutputRawSizes,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<bool>,
        )> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ConwayTxBody::from_cbor_bytes(&tx.body)?;
                let output_sizes =
                    crate::eras::babbage::extract_babbage_tx_output_raw_sizes(&tx.body)?;
                Ok((
                    tx.id,
                    tx.serialized_size(),
                    body,
                    output_sizes,
                    tx.witnesses.clone(),
                    tx.auxiliary_data.clone(),
                    tx.is_valid,
                ))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // BBODY rule: block-level ExUnits limit.
        {
            let wb_refs: Vec<Option<&[u8]>> = decoded
                .iter()
                .map(|(_, _, _, _, wb, _, _)| wb.as_deref())
                .collect();
            validate_block_ex_units(self.protocol_params.as_ref(), &wb_refs)?;
        }

        // Conway BBODY rule: block-level reference-script size limit.
        // Reference: `Cardano.Ledger.Conway.Rules.Bbody` — `BodyRefScriptsSizeTooBig`.
        //
        // At PV <= 10: sum of `txNonDistinctRefScriptsSize` per tx over the
        // pre-block UTxO (static).
        // At PV > 10 (`hardforkConwayRefactorTotalRefScriptsSize`): fold
        // through txs with a running UTxO that accumulates each tx's outputs
        // (valid tx → regular outputs, invalid tx → collateral return) before
        // measuring the next tx's ref-script size.  Spent inputs are NOT
        // removed.  The current tx is measured against the running UTxO
        // BEFORE its own outputs are added.
        // Reference: `Cardano.Ledger.Conway.Rules.Bbody` — `totalRefScriptSizeInBlock`.
        {
            let pv = self
                .protocol_params
                .as_ref()
                .and_then(|p| p.protocol_version);
            let use_running_utxo = conway_post_pv10(pv);
            let mut block_ref_total: usize = 0;
            if use_running_utxo {
                // PV > 10: fold with a running UTxO overlay that accumulates
                // newly produced outputs from preceding txs.
                let mut overlay: std::collections::HashMap<ShelleyTxIn, MultiEraTxOut> =
                    std::collections::HashMap::new();
                for (tx_id, _, body, _, _, _, is_valid) in &decoded {
                    // Measure ref-script size from ORIGINAL utxo + overlay
                    // (overlay entries take precedence conceptually but won't
                    // collide with existing entries since they use fresh TxIds).
                    let mut tx_ref_size: usize = 0;
                    for input in body
                        .inputs
                        .iter()
                        .chain(body.reference_inputs.as_deref().unwrap_or(&[]).iter())
                    {
                        // Check overlay first, then original UTxO.
                        let txout = overlay
                            .get(input)
                            .or_else(|| self.multi_era_utxo.get(input));
                        if let Some(out) = txout {
                            if let Some(sr) = out.script_ref() {
                                tx_ref_size = tx_ref_size.saturating_add(sr.0.binary_size());
                            }
                        }
                    }
                    block_ref_total = block_ref_total.saturating_add(tx_ref_size);
                    // Add this tx's outputs to overlay for the NEXT tx.
                    let tx_is_valid = is_valid.unwrap_or(true);
                    if tx_is_valid {
                        for (idx, out) in body.outputs.iter().enumerate() {
                            let txin = ShelleyTxIn {
                                transaction_id: tx_id.0,
                                index: idx as u16,
                            };
                            overlay.insert(txin, MultiEraTxOut::Babbage(out.clone()));
                        }
                    } else if let Some(collateral_return) = &body.collateral_return {
                        // Invalid tx: add collateral return output (upstream `collOuts`).
                        // Upstream `mkCollateralTxIn`: index = length(outputs).
                        let txin = ShelleyTxIn {
                            transaction_id: tx_id.0,
                            index: body.outputs.len() as u16,
                        };
                        overlay.insert(txin, MultiEraTxOut::Babbage(collateral_return.clone()));
                    }
                }
            } else {
                // PV <= 10: use pre-block UTxO (static) for all txs.
                for (_, _, body, _, _, _, _) in &decoded {
                    block_ref_total = block_ref_total.saturating_add(
                        self.multi_era_utxo
                            .total_ref_scripts_size(&body.inputs, body.reference_inputs.as_deref()),
                    );
                }
            }
            if block_ref_total > crate::utxo::MAX_REF_SCRIPT_SIZE_PER_BLOCK {
                return Err(LedgerError::BodyRefScriptsSizeTooBig {
                    actual: block_ref_total,
                    max_allowed: crate::utxo::MAX_REF_SCRIPT_SIZE_PER_BLOCK,
                });
            }
        }

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let mut staged_gen_delegs = self.gen_delegs.clone();
        let mut staged_future_gen_delegs = self.future_gen_delegs.clone();
        let mut staged_governance_actions = self.governance_actions.clone();
        let mut staged_utxos_donation: u64 = 0;
        let mut staged_num_dormant = self.num_dormant_epochs;
        let drep_activity = self
            .protocol_params
            .as_ref()
            .and_then(|pp| pp.drep_activity)
            .unwrap_or(0);
        let current_treasury = self.accounting.treasury;
        let cert_ctx = self.certificate_validation_context();
        for (tx_id, tx_size, body, output_sizes, witness_bytes, aux_data, is_valid) in &decoded {
            let tx_is_valid = is_valid.unwrap_or(true);
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            // Conway UTXOW: validateScriptsWellFormed.
            if let Some(eval) = evaluator {
                let protocol_version = self
                    .protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version);
                if let Some(wb) = witness_bytes.as_deref() {
                    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb)?;
                    crate::witnesses::validate_script_witnesses_well_formed(
                        &ws,
                        eval,
                        protocol_version,
                    )?;
                }
                let produced_outputs = if tx_is_valid {
                    body.outputs.as_slice()
                } else {
                    &[]
                };
                crate::witnesses::validate_reference_scripts_well_formed(
                    produced_outputs,
                    body.collateral_return.as_ref(),
                    eval,
                    protocol_version,
                )?;
            }
            if let Some(ref_inputs) = &body.reference_inputs {
                staged.validate_reference_inputs(ref_inputs)?;
                if disjoint_ref_inputs_enforced(
                    self.protocol_params
                        .as_ref()
                        .and_then(|p| p.protocol_version),
                ) {
                    MultiEraUtxo::validate_reference_input_disjointness(&body.inputs, ref_inputs)?;
                }
            }
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            if let Some(mint) = &body.mint {
                crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
            }
            if let Some(voting_procedures) = &body.voting_procedures {
                crate::witnesses::required_script_hashes_from_voting_procedures(
                    voting_procedures,
                    &mut required_scripts,
                );
            }
            if let Some(proposal_procedures) = &body.proposal_procedures {
                crate::witnesses::required_script_hashes_from_proposal_procedures(
                    proposal_procedures,
                    &mut required_scripts,
                );
            }
            crate::plutus_validation::validate_script_data_hash(
                body.script_data_hash,
                witness_bytes.as_deref(),
                self.protocol_params.as_ref(),
                true,
                Some(&staged),
                body.reference_inputs.as_deref(),
                Some(&body.inputs),
                Some(&required_scripts),
                self.protocol_params
                    .as_ref()
                    .and_then(|p| p.protocol_version),
            )?;
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                let coll_ret = body
                    .collateral_return
                    .as_ref()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()));
                let ref_scripts_size =
                    staged.total_ref_scripts_size(&body.inputs, body.reference_inputs.as_deref());
                validate_alonzo_plus_tx(
                    params,
                    &staged,
                    *tx_size,
                    body.fee,
                    &outputs,
                    Some(&output_sizes.outputs),
                    body.collateral.as_deref(),
                    total_eu.as_ref(),
                    coll_ret.as_ref(),
                    output_sizes.collateral_return,
                    body.total_collateral,
                    total_eu.is_some(),
                    ref_scripts_size,
                    true,
                )?;
                // Per-redeemer ExUnits check (upstream validateExUnitsTooBigUTxO).
                validate_per_redeemer_ex_units_from_bytes(witness_bytes.as_deref(), params)?;
            }
            // Network validation (Conway UTXO rule: WrongNetwork + WrongNetworkInTxBody)
            if let Some(expected_net) = self.expected_network_id {
                let mut outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                // Upstream allSizedOutputsTxBodyF includes collateral_return.
                if let Some(cr) = &body.collateral_return {
                    outputs.push(MultiEraTxOut::Babbage(cr.clone()));
                }
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
                validate_tx_body_network_id(expected_net, body.network_id)?;
            }
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            if let Some(signers) = &body.required_signers {
                for signer in signers {
                    required.insert(*signer);
                }
            }
            if let Some(voting_procedures) = &body.voting_procedures {
                crate::witnesses::required_vkey_hashes_from_voting_procedures(
                    voting_procedures,
                    &mut required,
                );
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // Native script validation (Conway)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs,
                &staged,
                &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(
                    withdrawals,
                    &mut required_scripts,
                );
            }
            if let Some(mint) = &body.mint {
                crate::witnesses::required_script_hashes_from_mint(mint, &mut required_scripts);
            }
            if let Some(voting_procedures) = &body.voting_procedures {
                crate::witnesses::required_script_hashes_from_voting_procedures(
                    voting_procedures,
                    &mut required_scripts,
                );
            }
            if let Some(proposal_procedures) = &body.proposal_procedures {
                crate::witnesses::required_script_hashes_from_proposal_procedures(
                    proposal_procedures,
                    &mut required_scripts,
                );
            }
            let native_satisfied = validate_native_scripts_if_present(
                witness_bytes.as_deref(),
                &required_scripts,
                slot,
            )?;
            validate_required_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                &native_satisfied,
                &staged,
                body.reference_inputs.as_deref(),
                Some(&body.inputs),
            )?;
            let conway_blk_ref_scripts =
                collect_reference_script_hashes(&staged, body.reference_inputs.as_deref());
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
                if conway_blk_ref_scripts.is_empty() {
                    None
                } else {
                    Some(&conway_blk_ref_scripts)
                },
            )?;
            // Unspendable UTxO check (Conway block — no datum on Plutus-locked input).
            // CIP-0069: collect PlutusV3 script hashes for V3 datum exemption.
            let conway_blk_v3_hashes = {
                let ws_bytes = witness_bytes.as_deref();
                let ws_decoded =
                    ws_bytes.map(crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes);
                let ws_ref = match &ws_decoded {
                    Some(Ok(w)) => Some(w),
                    _ => None,
                };
                crate::plutus_validation::collect_v3_script_hashes(
                    ws_ref,
                    Some(&staged),
                    body.reference_inputs.as_deref(),
                )
            };
            crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                &staged,
                &body.inputs,
                &native_satisfied,
                if conway_blk_v3_hashes.is_empty() {
                    None
                } else {
                    Some(&conway_blk_v3_hashes)
                },
            )?;
            // Supplemental datum check (Conway — includes reference inputs).
            {
                let mut tx_outputs: Vec<MultiEraTxOut> = body
                    .outputs
                    .iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                if let Some(collateral_return) = &body.collateral_return {
                    tx_outputs.push(MultiEraTxOut::Babbage(collateral_return.clone()));
                }
                let ref_utxos: Vec<(ShelleyTxIn, MultiEraTxOut)> = body
                    .reference_inputs
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|txin| staged.get(txin).map(|txout| (txin.clone(), txout.clone())))
                    .collect();
                crate::plutus_validation::validate_supplemental_datums(
                    witness_bytes.as_deref(),
                    &staged,
                    &body.inputs,
                    &tx_outputs,
                    &ref_utxos,
                )?;
            }
            // ExtraRedeemer check (Conway block — Phase-1 UTXOW).
            // Upstream: hasExactSetOfRedeemers in alonzoUtxowTransition runs
            // unconditionally before UTXOS is_valid dispatching.
            {
                let mut sorted_inputs = body.inputs.clone();
                sorted_inputs.sort();
                let sorted_policies: Vec<[u8; 28]> = body
                    .mint
                    .as_ref()
                    .map(|m| m.keys().copied().collect())
                    .unwrap_or_default();
                let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                let sorted_rewards: Vec<Vec<u8>> = body
                    .withdrawals
                    .as_ref()
                    .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                    .unwrap_or_default();
                let sorted_voters: Vec<crate::eras::conway::Voter> = body
                    .voting_procedures
                    .as_ref()
                    .map(|vp| {
                        let mut vs: Vec<_> = vp.procedures.keys().cloned().collect();
                        vs.sort();
                        vs
                    })
                    .unwrap_or_default();
                let proposal_slice: Vec<crate::eras::conway::ProposalProcedure> =
                    body.proposal_procedures.as_deref().unwrap_or(&[]).to_vec();
                crate::plutus_validation::validate_no_extra_redeemers(
                    witness_bytes.as_deref(),
                    &staged,
                    &sorted_inputs,
                    &sorted_policies,
                    certs_slice,
                    &sorted_rewards,
                    &sorted_voters,
                    &proposal_slice,
                    body.reference_inputs.as_deref(),
                )?;
                crate::plutus_validation::validate_no_missing_redeemers(
                    witness_bytes.as_deref(),
                    &required_scripts,
                    &staged,
                    &sorted_inputs,
                    &sorted_policies,
                    certs_slice,
                    &sorted_rewards,
                    &sorted_voters,
                    &proposal_slice,
                    body.reference_inputs.as_deref(),
                )?;
            }
            let run_phase2 = || -> Result<(), LedgerError> {
                // Plutus script validation (Conway)
                {
                    let mut sorted_inputs = body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = body
                        .mint
                        .as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = body
                        .withdrawals
                        .as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let sorted_voters: Vec<crate::eras::conway::Voter> = body
                        .voting_procedures
                        .as_ref()
                        .map(|v| v.procedures.keys().cloned().collect())
                        .unwrap_or_default();
                    let proposal_slice = body.proposal_procedures.as_deref().unwrap_or(&[]);
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx_id.0,
                        fee: body.fee,
                        outputs: body
                            .outputs
                            .iter()
                            .map(|o| MultiEraTxOut::Babbage(o.clone()))
                            .collect(),
                        validity_start: body.validity_interval_start,
                        ttl: body.ttl,
                        required_signers: body.required_signers.clone().unwrap_or_default(),
                        mint: body.mint.clone().unwrap_or_default(),
                        withdrawals: body.withdrawals.clone().unwrap_or_default(),
                        reference_inputs: body.reference_inputs.clone().unwrap_or_default(),
                        current_treasury_value: body.current_treasury_value,
                        treasury_donation: body.treasury_donation,
                        voting_procedures: body.voting_procedures.clone(),
                        proposal_procedures: proposal_slice.to_vec(),
                        protocol_version: self
                            .protocol_params
                            .as_ref()
                            .and_then(|p| p.protocol_version),
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator,
                        witness_bytes.as_deref(),
                        &required_scripts,
                        &staged,
                        &sorted_inputs,
                        &sorted_policies,
                        certs_slice,
                        &sorted_rewards,
                        &sorted_voters,
                        proposal_slice,
                        &tx_ctx,
                        self.protocol_params
                            .as_ref()
                            .and_then(|p| p.cost_models.as_ref()),
                    )
                }
            };
            if tx_is_valid {
                match run_phase2() {
                    Ok(()) => {}
                    Err(LedgerError::PlutusScriptFailed { hash, reason })
                        if evaluator.is_some() =>
                    {
                        return Err(LedgerError::ValidationTagMismatch {
                            claimed: true,
                            actual: false,
                            reason: phase2_failure_reason(&hash, &reason),
                        });
                    }
                    Err(e) => return Err(e),
                }
                // Conway LEDGER rule: total reference script size limit
                // (upstream runs inside IsValid True branch).
                staged
                    .validate_tx_ref_scripts_size(&body.inputs, body.reference_inputs.as_deref())?;
                // Conway LEDGER rule: treasury value consistency
                // (upstream `validateTreasuryValue`, inside IsValid True branch).
                validate_conway_current_treasury_value(
                    body.current_treasury_value,
                    current_treasury,
                )?;
                // Conway LEDGER rule: withdrawal credentials must be delegated
                // to a DRep (post-bootstrap only, uses pre-CERTS state).
                validate_withdrawals_delegated(
                    body.withdrawals.as_ref(),
                    &staged_stake_credentials,
                    cert_ctx.bootstrap_phase,
                )?;
                let unregistered_drep_voters =
                    collect_conway_unregistered_drep_voters(body.certificates.as_deref());

                // Upstream `updateDormantDRepExpiries` — bump all DRep
                // expiries and reset dormant counter when tx has proposals.
                update_dormant_drep_expiries(
                    body.proposal_procedures
                        .as_ref()
                        .is_some_and(|p| !p.is_empty()),
                    &mut staged_drep_state,
                    &mut staged_num_dormant,
                    self.current_epoch,
                    drep_activity,
                );

                if body.voting_procedures.is_some()
                    || body.proposal_procedures.is_some()
                    || !unregistered_drep_voters.is_empty()
                {
                    let (
                        governance_pool_state,
                        governance_stake_credentials,
                        governance_committee_state,
                        governance_drep_state,
                    ) = conway_governance_state_after_certificates(
                        &staged_pool_state,
                        &staged_stake_credentials,
                        &staged_committee_state,
                        &staged_drep_state,
                        &staged_reward_accounts,
                        &staged_deposit_pot,
                        &staged_gen_delegs,
                        &staged_governance_actions,
                        &cert_ctx,
                        body.certificates.as_deref(),
                    )?;

                    let mut governance_actions_for_tx = staged_governance_actions.clone();

                    if let Some(voting_procedures) = &body.voting_procedures {
                        // Upstream: UnelectedCommitteeVoters check runs first
                        // (hardforkConwayDisallowUnelectedCommitteeFromVoting).
                        validate_unelected_committee_voters(
                            voting_procedures,
                            &governance_committee_state,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.protocol_version),
                        )?;
                        validate_conway_voters(
                            voting_procedures,
                            &governance_pool_state,
                            &governance_committee_state,
                            &governance_drep_state,
                        )?;
                    }

                    if let Some(proposal_procedures) = &body.proposal_procedures {
                        validate_conway_proposals(
                            *tx_id,
                            proposal_procedures,
                            self.current_epoch,
                            &mut governance_actions_for_tx,
                            &governance_stake_credentials,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.protocol_version),
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.gov_action_deposit),
                            self.expected_network_id,
                            self.protocol_params.as_ref(),
                            &self.enact_state,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.gov_action_lifetime),
                        )?;
                    }

                    if let Some(voting_procedures) = &body.voting_procedures {
                        validate_conway_vote_targets(
                            voting_procedures,
                            &governance_actions_for_tx,
                        )?;
                        validate_conway_voter_permissions(
                            self.current_epoch,
                            voting_procedures,
                            &governance_actions_for_tx,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.protocol_version),
                        )?;
                    }

                    staged_governance_actions = governance_actions_for_tx;
                    if let Some(voting_procedures) = &body.voting_procedures {
                        apply_conway_votes(
                            voting_procedures,
                            &mut staged_governance_actions,
                            &mut staged_drep_state,
                            self.current_epoch,
                            staged_num_dormant,
                            cert_ctx.bootstrap_phase,
                        );
                    }
                    remove_conway_drep_votes(
                        &unregistered_drep_voters,
                        &mut staged_governance_actions,
                    );
                }
                let cert_adj = apply_certificates_and_withdrawals_with_future(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &mut staged_future_gen_delegs,
                    &self.governance_actions,
                    &cert_ctx,
                    body.certificates.as_deref(),
                    body.withdrawals.as_ref(),
                    slot,
                    self.stability_window,
                    None, // Conway: MIR certs rejected as UnsupportedCertificate
                )?;
                // Track DRep activity for registration and update certificates.
                touch_drep_activity_for_certs(
                    body.certificates.as_deref(),
                    &mut staged_drep_state,
                    self.current_epoch,
                    staged_num_dormant,
                    cert_ctx.bootstrap_phase,
                );
                // Conway UTXO rule: totalTxDeposits includes both certificate
                // deposits and proposal procedure deposits.
                // Reference: Cardano.Ledger.Conway.TxInfo — totalTxDeposits.
                let proposal_deposits: u64 = body
                    .proposal_procedures
                    .as_ref()
                    .map(|ps| ps.iter().map(|p| p.deposit).fold(0u64, u64::saturating_add))
                    .unwrap_or(0);
                // Track proposal deposits in the deposit pot (upstream oblProposal).
                staged_deposit_pot.add_proposal_deposit(proposal_deposits);
                let total_deposits = cert_adj.total_deposits.saturating_add(proposal_deposits);
                staged.apply_conway_tx_withdrawals(
                    tx_id.0,
                    body,
                    slot,
                    cert_adj.withdrawal_total,
                    total_deposits,
                    cert_adj.total_refunds,
                )?;
                // Accumulate treasury donation (Conway UTXOS rule).
                // Reference: Cardano.Ledger.Conway.Rules.Utxo — validateZeroDonation.
                if let Some(donation) = body.treasury_donation {
                    if donation == 0 {
                        return Err(LedgerError::ZeroDonation);
                    }
                    staged_utxos_donation = staged_utxos_donation.saturating_add(donation);
                }
            } else {
                if evaluator.is_some() {
                    match run_phase2() {
                        Ok(()) => {
                            return Err(LedgerError::ValidationTagMismatch {
                                claimed: false,
                                actual: true,
                                reason: "phase-2 unexpectedly succeeded".to_string(),
                            });
                        }
                        Err(LedgerError::PlutusScriptFailed { .. }) => {}
                        Err(e) => return Err(e),
                    }
                }
                // is_valid = false: collateral-only transition.
                crate::utxo::apply_collateral_only(
                    &mut staged,
                    tx_id.0,
                    body.collateral.as_deref(),
                    body.collateral_return.as_ref(),
                    body.outputs.len(),
                );
            }
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        self.gen_delegs = staged_gen_delegs;
        self.future_gen_delegs = staged_future_gen_delegs;
        self.governance_actions = staged_governance_actions;
        self.utxos_donation = self.utxos_donation.saturating_add(staged_utxos_donation);
        self.num_dormant_epochs = staged_num_dormant;
        Ok(())
    }
}

fn conway_governance_state_after_certificates(
    pool_state: &PoolState,
    stake_credentials: &StakeCredentials,
    committee_state: &CommitteeState,
    drep_state: &DrepState,
    reward_accounts: &RewardAccounts,
    deposit_pot: &DepositPot,
    gen_delegs: &BTreeMap<GenesisHash, GenesisDelegationState>,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    ctx: &CertificateValidationContext,
    certificates: Option<&[DCert]>,
) -> Result<(PoolState, StakeCredentials, CommitteeState, DrepState), LedgerError> {
    let mut simulated_pool_state = pool_state.clone();
    let mut simulated_stake_credentials = stake_credentials.clone();
    let mut simulated_committee_state = committee_state.clone();
    let mut simulated_drep_state = drep_state.clone();
    let mut simulated_reward_accounts = reward_accounts.clone();
    let mut simulated_deposit_pot = deposit_pot.clone();
    let mut simulated_gen_delegs = gen_delegs.clone();

    let _cert_adj = apply_certificates_and_withdrawals(
        &mut simulated_pool_state,
        &mut simulated_stake_credentials,
        &mut simulated_committee_state,
        &mut simulated_drep_state,
        &mut simulated_reward_accounts,
        &mut simulated_deposit_pot,
        &mut simulated_gen_delegs,
        governance_actions,
        ctx,
        certificates,
        None,
    )?;

    Ok((
        simulated_pool_state,
        simulated_stake_credentials,
        simulated_committee_state,
        simulated_drep_state,
    ))
}

fn validate_conway_voters(
    voting_procedures: &crate::eras::conway::VotingProcedures,
    pool_state: &PoolState,
    committee_state: &CommitteeState,
    drep_state: &DrepState,
) -> Result<(), LedgerError> {
    let unknown_voters: Vec<crate::eras::conway::Voter> = voting_procedures
        .procedures
        .keys()
        .filter(|voter| !conway_voter_exists(voter, pool_state, committee_state, drep_state))
        .cloned()
        .collect();

    if unknown_voters.is_empty() {
        Ok(())
    } else {
        Err(LedgerError::VotersDoNotExist(unknown_voters))
    }
}

fn validate_conway_vote_targets(
    voting_procedures: &crate::eras::conway::VotingProcedures,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
) -> Result<(), LedgerError> {
    let mut unknown_action_ids = Vec::new();

    for votes in voting_procedures.procedures.values() {
        for gov_action_id in votes.keys() {
            if !governance_actions.contains_key(gov_action_id)
                && !unknown_action_ids.contains(gov_action_id)
            {
                unknown_action_ids.push(gov_action_id.clone());
            }
        }
    }

    if unknown_action_ids.is_empty() {
        Ok(())
    } else {
        Err(LedgerError::GovActionsDoNotExist(unknown_action_ids))
    }
}

fn validate_conway_voter_permissions(
    current_epoch: EpochNo,
    voting_procedures: &crate::eras::conway::VotingProcedures,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    protocol_version: Option<(u64, u64)>,
) -> Result<(), LedgerError> {
    let mut bootstrap_disallowed_votes = Vec::new();
    let mut disallowed_votes = Vec::new();
    let mut expired_votes = Vec::new();

    for (voter, votes) in &voting_procedures.procedures {
        for gov_action_id in votes.keys() {
            let Some(governance_action) = governance_actions.get(gov_action_id) else {
                continue;
            };

            if conway_bootstrap_phase(protocol_version)
                && !conway_bootstrap_vote_is_allowed(voter, &governance_action.proposal.gov_action)
            {
                bootstrap_disallowed_votes.push((voter.clone(), gov_action_id.clone()));
                continue;
            }

            if let Some(expires_after) = governance_action.expires_after() {
                if current_epoch > expires_after {
                    expired_votes.push((voter.clone(), gov_action_id.clone()));
                    continue;
                }
            }

            if !conway_voter_is_allowed_for_action(voter, &governance_action.proposal.gov_action) {
                disallowed_votes.push((voter.clone(), gov_action_id.clone()));
            }
        }
    }

    if !bootstrap_disallowed_votes.is_empty() {
        return Err(LedgerError::DisallowedVotesDuringBootstrap(
            bootstrap_disallowed_votes,
        ));
    }

    if !expired_votes.is_empty() {
        return Err(LedgerError::VotingOnExpiredGovAction(expired_votes));
    }

    if disallowed_votes.is_empty() {
        Ok(())
    } else {
        Err(LedgerError::DisallowedVoters(disallowed_votes))
    }
}

fn conway_voter_is_allowed_for_action(
    voter: &crate::eras::conway::Voter,
    gov_action: &crate::eras::conway::GovAction,
) -> bool {
    match voter {
        crate::eras::conway::Voter::CommitteeKeyHash(_)
        | crate::eras::conway::Voter::CommitteeScript(_) => !matches!(
            gov_action,
            crate::eras::conway::GovAction::NoConfidence { .. }
                | crate::eras::conway::GovAction::UpdateCommittee { .. }
        ),
        crate::eras::conway::Voter::DRepKeyHash(_) | crate::eras::conway::Voter::DRepScript(_) => {
            true
        }
        crate::eras::conway::Voter::StakePool(_) => match gov_action {
            crate::eras::conway::GovAction::NoConfidence { .. }
            | crate::eras::conway::GovAction::UpdateCommittee { .. }
            | crate::eras::conway::GovAction::HardForkInitiation { .. }
            | crate::eras::conway::GovAction::InfoAction => true,
            crate::eras::conway::GovAction::TreasuryWithdrawals { .. }
            | crate::eras::conway::GovAction::NewConstitution { .. } => false,
            crate::eras::conway::GovAction::ParameterChange {
                protocol_param_update,
                ..
            } => conway_parameter_change_has_spo_security_vote_group(protocol_param_update),
        },
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ConwayModifiedPParamGroups {
    network: bool,
    economic: bool,
    technical: bool,
    gov: bool,
    security: bool,
}

impl ConwayModifiedPParamGroups {
    fn has_drep_group(self) -> bool {
        self.network || self.economic || self.technical || self.gov
    }
}

fn conway_modified_pparam_groups(
    update: &crate::protocol_params::ProtocolParameterUpdate,
) -> ConwayModifiedPParamGroups {
    let mut groups = ConwayModifiedPParamGroups::default();

    // Economic + Security (upstream: EconomicGroup + SecurityGroup)
    if update.min_fee_a.is_some()
        || update.min_fee_b.is_some()
        || update.coins_per_utxo_byte.is_some()
        || update.min_fee_ref_script_cost_per_byte.is_some()
    {
        groups.economic = true;
        groups.security = true;
    }

    // Network + Security (upstream: NetworkGroup + SecurityGroup)
    if update.max_block_body_size.is_some()
        || update.max_tx_size.is_some()
        || update.max_block_header_size.is_some()
        || update.max_block_ex_units.is_some()
        || update.max_val_size.is_some()
    {
        groups.network = true;
        groups.security = true;
    }

    // Network (no SPO) (upstream: NetworkGroup + NoStakePoolGroup)
    if update.max_tx_ex_units.is_some() || update.max_collateral_inputs.is_some() {
        groups.network = true;
    }

    // Economic (no SPO) (upstream: EconomicGroup + NoStakePoolGroup)
    if update.key_deposit.is_some()
        || update.pool_deposit.is_some()
        || update.rho.is_some()
        || update.tau.is_some()
        || update.min_pool_cost.is_some()
        || update.price_mem.is_some()
        || update.price_step.is_some()
        || update.min_utxo_value.is_some()
    {
        groups.economic = true;
    }

    // Technical (no SPO) (upstream: TechnicalGroup + NoStakePoolGroup)
    if update.e_max.is_some()
        || update.n_opt.is_some()
        || update.a0.is_some()
        || update.collateral_percentage.is_some()
        || update.cost_models.is_some()
    {
        groups.technical = true;
    }

    // Gov (no SPO unless explicitly marked otherwise)
    if update.pool_voting_thresholds.is_some()
        || update.drep_voting_thresholds.is_some()
        || update.min_committee_size.is_some()
        || update.committee_term_limit.is_some()
        || update.gov_action_lifetime.is_some()
        || update.drep_deposit.is_some()
        || update.drep_activity.is_some()
    {
        groups.gov = true;
    }

    // Gov + Security
    if update.gov_action_deposit.is_some() {
        groups.gov = true;
        groups.security = true;
    }

    // In upstream Conway this update path is disabled for parameter updates,
    // but if present in this bounded slice treat it as security-relevant.
    if update.protocol_version.is_some() {
        groups.security = true;
    }

    groups
}

fn conway_parameter_change_has_spo_security_vote_group(
    update: &crate::protocol_params::ProtocolParameterUpdate,
) -> bool {
    conway_modified_pparam_groups(update).security
}

fn conway_drep_parameter_change_threshold(
    update: &crate::protocol_params::ProtocolParameterUpdate,
    thresholds: &DRepVotingThresholds,
) -> Option<UnitInterval> {
    let groups = conway_modified_pparam_groups(update);
    if !groups.has_drep_group() {
        return None;
    }

    let mut selected: Option<UnitInterval> = None;
    let mut include = |candidate: UnitInterval| {
        selected = Some(match selected {
            Some(current)
                if (current.numerator as u128) * (candidate.denominator as u128)
                    >= (candidate.numerator as u128) * (current.denominator as u128) =>
            {
                current
            }
            _ => candidate,
        });
    };

    if groups.network {
        include(thresholds.pp_network_group);
    }
    if groups.economic {
        include(thresholds.pp_economic_group);
    }
    if groups.technical {
        include(thresholds.pp_technical_group);
    }
    if groups.gov {
        include(thresholds.pp_gov_group);
    }

    selected
}

fn era_min_protocol_major(era: Era) -> Option<u64> {
    match era {
        Era::Byron => None,
        Era::Shelley => Some(2),
        Era::Allegra => Some(3),
        Era::Mary => Some(4),
        Era::Alonzo => Some(5),
        Era::Babbage => Some(7),
        Era::Conway => Some(9),
    }
}

fn phase2_failure_reason(hash: &[u8; 28], reason: &str) -> String {
    use std::fmt::Write as _;

    let mut hash_hex = String::with_capacity(hash.len() * 2);
    for byte in hash {
        let _ = write!(&mut hash_hex, "{byte:02x}");
    }
    format!("script {hash_hex} failed: {reason}")
}

fn conway_bootstrap_phase(protocol_version: Option<(u64, u64)>) -> bool {
    matches!(protocol_version, Some((9, _)))
}

/// `true` when protocol version major > 10 (i.e., PV 11+).
///
/// Upstream:
/// - `hardforkConwayDELEGIncorrectDepositsAndRefunds`
/// - `hardforkConwayDisallowUnelectedCommitteeFromVoting`
/// - `hardforkConwayMoveWithdrawalsAndDRepChecksToLedgerRule`
///
/// All three are gated on `pvMajor pv > natVersion @10`.
fn conway_post_pv10(protocol_version: Option<(u64, u64)>) -> bool {
    matches!(protocol_version, Some((major, _)) if major > 10)
}

/// `true` when the `disjointRefInputs` check should be enforced.
///
/// Upstream: `Cardano.Ledger.Babbage.Rules.Utxo` — `disjointRefInputs` is
/// gated on `pvMajor > eraProtVerHigh @BabbageEra && pvMajor < natVersion @11`.
/// Since `eraProtVerHigh @BabbageEra = 8`, this enforces disjointness only
/// for PV 9–10 (early Conway).  At PV 11+ it is relaxed.
fn disjoint_ref_inputs_enforced(protocol_version: Option<(u64, u64)>) -> bool {
    matches!(protocol_version, Some((major, _)) if major > 8 && major < 11)
}

fn conway_bootstrap_action(gov_action: &crate::eras::conway::GovAction) -> bool {
    matches!(
        gov_action,
        crate::eras::conway::GovAction::ParameterChange { .. }
            | crate::eras::conway::GovAction::HardForkInitiation { .. }
            | crate::eras::conway::GovAction::InfoAction
    )
}

fn conway_bootstrap_vote_is_allowed(
    voter: &crate::eras::conway::Voter,
    gov_action: &crate::eras::conway::GovAction,
) -> bool {
    match voter {
        crate::eras::conway::Voter::DRepKeyHash(_) | crate::eras::conway::Voter::DRepScript(_) => {
            matches!(gov_action, crate::eras::conway::GovAction::InfoAction)
        }
        crate::eras::conway::Voter::CommitteeKeyHash(_)
        | crate::eras::conway::Voter::CommitteeScript(_)
        | crate::eras::conway::Voter::StakePool(_) => conway_bootstrap_action(gov_action),
    }
}

fn conway_pv_can_follow(previous: (u64, u64), new: (u64, u64)) -> bool {
    // Upstream `pvCanFollow`: new protocol version is valid iff it is
    // exactly one step above `previous` — either `(major, minor+1)` (same
    // major, next minor) or `(major+1, 0)` (next major, reset minor).
    //
    // `checked_add` on both branches rejects the `u64::MAX` saturating
    // edge case that would otherwise let `(M, u64::MAX) → (M, u64::MAX)`
    // be accepted as an identity increment. A previous `saturating_add(1)`
    // form collapsed to identity at MAX, which would silently let
    // same-version proposals slip past the first branch at that boundary.
    previous
        .1
        .checked_add(1)
        .is_some_and(|next_minor| (previous.0, next_minor) == new)
        || previous
            .0
            .checked_add(1)
            .is_some_and(|next_major| (next_major, 0) == new)
}

fn conway_expected_previous_hard_fork_version(
    proposal: &crate::eras::conway::ProposalProcedure,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    current_protocol_version: Option<(u64, u64)>,
) -> Option<(
    Option<crate::eras::conway::GovActionId>,
    (u64, u64),
    (u64, u64),
)> {
    use crate::eras::conway::GovAction;

    match &proposal.gov_action {
        GovAction::HardForkInitiation {
            prev_action_id,
            protocol_version,
        } => {
            // Upstream safety guard from `preceedingHardFork`: when the
            // proposed major version exceeds `succVersion(pvMajor current)`,
            // always compare against the current protocol version instead of
            // following the proposal chain.  This prevents chaining
            // HardFork proposals that would result in jumping more than
            // one major version ahead of the live protocol.
            //
            // Reference: `Cardano.Ledger.Conway.Rules.Gov` —
            // `preceedingHardFork`:
            //   | Just (pvMajor newProtVer) > succVersion (pvMajor (pp ^. ppProtocolVersionL))
            //   -> Just (mPrev, newProtVer, pp ^. ppProtocolVersionL)
            let cur = current_protocol_version?;
            if protocol_version.0 > cur.0.saturating_add(1) {
                return Some((prev_action_id.clone(), *protocol_version, cur));
            }

            let expected = match prev_action_id {
                Some(action_id) => governance_actions.get(action_id).and_then(|action_state| {
                    match &action_state.proposal().gov_action {
                        GovAction::HardForkInitiation {
                            protocol_version, ..
                        } => Some(*protocol_version),
                        _ => None,
                    }
                }),
                None => current_protocol_version,
            }?;
            Some((prev_action_id.clone(), *protocol_version, expected))
        }
        _ => None,
    }
}

fn conway_proposal_prev_action_id(
    gov_action: &crate::eras::conway::GovAction,
) -> Option<&crate::eras::conway::GovActionId> {
    use crate::eras::conway::GovAction;

    match gov_action {
        GovAction::ParameterChange { prev_action_id, .. }
        | GovAction::HardForkInitiation { prev_action_id, .. }
        | GovAction::NoConfidence { prev_action_id }
        | GovAction::UpdateCommittee { prev_action_id, .. }
        | GovAction::NewConstitution { prev_action_id, .. } => prev_action_id.as_ref(),
        GovAction::TreasuryWithdrawals { .. } | GovAction::InfoAction => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum ConwayGovActionPurpose {
    ParameterChange,
    HardFork,
    Committee,
    Constitution,
    TreasuryWithdrawals,
    Info,
}

pub(crate) fn conway_gov_action_purpose(
    gov_action: &crate::eras::conway::GovAction,
) -> ConwayGovActionPurpose {
    use crate::eras::conway::GovAction;

    match gov_action {
        GovAction::ParameterChange { .. } => ConwayGovActionPurpose::ParameterChange,
        GovAction::HardForkInitiation { .. } => ConwayGovActionPurpose::HardFork,
        GovAction::NoConfidence { .. } | GovAction::UpdateCommittee { .. } => {
            ConwayGovActionPurpose::Committee
        }
        GovAction::NewConstitution { .. } => ConwayGovActionPurpose::Constitution,
        GovAction::TreasuryWithdrawals { .. } => ConwayGovActionPurpose::TreasuryWithdrawals,
        GovAction::InfoAction => ConwayGovActionPurpose::Info,
    }
}

/// Applies the upstream `updateDormantDRepExpiries` rule.
///
/// If the transaction contains governance proposals and the dormant epoch
/// counter is non-zero, every registered DRep's `last_active_epoch` is
/// bumped forward by the dormant count (extending their effective expiry),
/// and the dormant counter is reset to zero.  DReps whose bumped expiry
/// would still be before `current_epoch` are left unchanged (they have
/// already lapsed beyond recovery by dormancy alone).
///
/// Reference: `Cardano.Ledger.Conway.Rules.Certs` —
/// `updateDormantDRepExpiries`, `updateDormantDRepExpiry`.
fn update_dormant_drep_expiries(
    has_proposals: bool,
    drep_state: &mut DrepState,
    num_dormant: &mut u64,
    current_epoch: EpochNo,
    drep_activity: u64,
) {
    if !has_proposals || *num_dormant == 0 {
        return;
    }
    let dormant = *num_dormant;
    for entry in drep_state.values_mut() {
        if let Some(last_active) = entry.last_active_epoch() {
            // new_expiry = (last_active + drep_activity) + dormant
            // Guard: new_expiry >= current_epoch
            let old_expiry = last_active.0.saturating_add(drep_activity);
            let new_expiry = old_expiry.saturating_add(dormant);
            if new_expiry >= current_epoch.0 {
                // Equivalent: last_active_new + drep_activity = new_expiry
                //           → last_active_new = last_active + dormant
                entry.touch_activity(EpochNo(last_active.0.saturating_add(dormant)));
            }
        }
    }
    *num_dormant = 0;
}

fn apply_conway_votes(
    voting_procedures: &crate::eras::conway::VotingProcedures,
    governance_actions: &mut BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    drep_state: &mut DrepState,
    current_epoch: EpochNo,
    num_dormant_epochs: u64,
    bootstrap_phase: bool,
) {
    for (voter, votes) in &voting_procedures.procedures {
        for (gov_action_id, voting_procedure) in votes {
            if let Some(action_state) = governance_actions.get_mut(gov_action_id) {
                action_state
                    .votes
                    .insert(voter.clone(), voting_procedure.vote);
            }
        }
        // Mark DRep as active in the current epoch when it casts any vote.
        // Upstream `updateVotingDRepExpiries` / `computeDRepExpiry`:
        //   expiry = currentEpoch + drepActivity - numDormantEpochs
        // In our model: last_active_epoch = currentEpoch - numDormantEpochs
        //
        // During bootstrap: last_active_epoch = currentEpoch (no dormant).
        if let Some(drep) = voter_to_drep(voter) {
            if let Some(entry) = drep_state.get_mut(&drep) {
                let dormant = if bootstrap_phase {
                    0
                } else {
                    num_dormant_epochs
                };
                entry.touch_activity(EpochNo(current_epoch.0.saturating_sub(dormant)));
            }
        }
    }
}

/// Extracts the DRep identity from a Voter, if applicable.
fn voter_to_drep(voter: &crate::eras::conway::Voter) -> Option<DRep> {
    match voter {
        crate::eras::conway::Voter::DRepKeyHash(hash) => Some(DRep::KeyHash(*hash)),
        crate::eras::conway::Voter::DRepScript(hash) => Some(DRep::ScriptHash(*hash)),
        _ => None,
    }
}

fn collect_conway_unregistered_drep_voters(
    certificates: Option<&[DCert]>,
) -> Vec<crate::eras::conway::Voter> {
    let Some(certificates) = certificates else {
        return Vec::new();
    };

    let mut unregistered = Vec::new();
    for certificate in certificates {
        if let DCert::DrepUnregistration(credential, _) = certificate {
            let voter = match credential {
                StakeCredential::AddrKeyHash(hash) => {
                    crate::eras::conway::Voter::DRepKeyHash(*hash)
                }
                StakeCredential::ScriptHash(hash) => crate::eras::conway::Voter::DRepScript(*hash),
            };
            if !unregistered.contains(&voter) {
                unregistered.push(voter);
            }
        }
    }

    unregistered
}

fn remove_conway_drep_votes(
    unregistered_drep_voters: &[crate::eras::conway::Voter],
    governance_actions: &mut BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
) {
    if unregistered_drep_voters.is_empty() {
        return;
    }

    for governance_action in governance_actions.values_mut() {
        governance_action
            .votes
            .retain(|voter, _| !unregistered_drep_voters.contains(voter));
    }
}

fn conway_voter_exists(
    voter: &crate::eras::conway::Voter,
    pool_state: &PoolState,
    committee_state: &CommitteeState,
    drep_state: &DrepState,
) -> bool {
    use crate::eras::conway::Voter;

    match voter {
        Voter::CommitteeKeyHash(hash) => {
            committee_hot_credential_exists(committee_state, StakeCredential::AddrKeyHash(*hash))
        }
        Voter::CommitteeScript(hash) => {
            committee_hot_credential_exists(committee_state, StakeCredential::ScriptHash(*hash))
        }
        Voter::DRepKeyHash(hash) => drep_state.is_registered(&DRep::KeyHash(*hash)),
        Voter::DRepScript(hash) => drep_state.is_registered(&DRep::ScriptHash(*hash)),
        Voter::StakePool(hash) => pool_state.is_registered(hash),
    }
}

fn committee_hot_credential_exists(
    committee_state: &CommitteeState,
    credential: StakeCredential,
) -> bool {
    committee_state
        .iter()
        .any(|(_, member_state)| member_state.hot_credential() == Some(credential))
}

/// Returns the set of hot committee credentials that are authorized by
/// currently-elected, non-resigned committee members.
///
/// Upstream: `authorizedElectedHotCommitteeCredentials` from
/// `Cardano.Ledger.Conway.Governance`.
///
/// In our architecture `CommitteeState` IS the elected committee (entries are
/// added/removed during `UpdateCommittee` enactment), so this returns hot
/// credentials from all non-resigned entries.  Resigned entries already yield
/// `None` from `hot_credential()` and are therefore excluded.
fn authorized_elected_hot_committee_credentials(
    committee_state: &CommitteeState,
) -> Vec<StakeCredential> {
    committee_state
        .iter()
        .filter_map(|(_, member_state)| member_state.hot_credential())
        .collect()
}

/// Upstream: `unelectedCommitteeVoters` from `Cardano.Ledger.Conway.Rules.Gov`.
///
/// Collects committee voters whose hot credentials are NOT in the set of
/// authorized-elected hot committee credentials.  Only applies after the
/// `hardforkConwayDisallowUnelectedCommitteeFromVoting` gate (protocol
/// version > 10, i.e., PV 11+).
fn validate_unelected_committee_voters(
    voting_procedures: &crate::eras::conway::VotingProcedures,
    committee_state: &CommitteeState,
    protocol_version: Option<(u64, u64)>,
) -> Result<(), LedgerError> {
    // Gate: only enforce after protocol version 10
    // (upstream `harforkConwayDisallowUnelectedCommitteeFromVoting pv = pvMajor pv > natVersion @10`)
    if !conway_post_pv10(protocol_version) {
        return Ok(());
    }

    let authorized = authorized_elected_hot_committee_credentials(committee_state);

    let mut unelected: Vec<StakeCredential> = Vec::new();
    for voter in voting_procedures.procedures.keys() {
        let hot_cred = match voter {
            crate::eras::conway::Voter::CommitteeKeyHash(hash) => {
                Some(StakeCredential::AddrKeyHash(*hash))
            }
            crate::eras::conway::Voter::CommitteeScript(hash) => {
                Some(StakeCredential::ScriptHash(*hash))
            }
            _ => None,
        };
        if let Some(cred) = hot_cred {
            if !authorized.contains(&cred) && !unelected.contains(&cred) {
                unelected.push(cred);
            }
        }
    }

    if unelected.is_empty() {
        Ok(())
    } else {
        Err(LedgerError::UnelectedCommitteeVoters(unelected))
    }
}

fn conway_unit_interval_well_formed(value: &UnitInterval) -> bool {
    value.denominator != 0 && value.numerator <= value.denominator
}

fn conway_protocol_param_update_well_formed(
    update: &crate::protocol_params::ProtocolParameterUpdate,
    protocol_version: Option<(u64, u64)>,
) -> bool {
    let unit_interval_fields = [
        update.a0.as_ref(),
        update.rho.as_ref(),
        update.tau.as_ref(),
        update.price_mem.as_ref(),
        update.price_step.as_ref(),
    ];
    if unit_interval_fields
        .iter()
        .flatten()
        .any(|value| !conway_unit_interval_well_formed(value))
    {
        return false;
    }

    if let Some(thresholds) = &update.pool_voting_thresholds {
        let values = [
            &thresholds.motion_no_confidence,
            &thresholds.committee_normal,
            &thresholds.committee_no_confidence,
            &thresholds.hard_fork_initiation,
            &thresholds.pp_security_group,
        ];
        if values
            .iter()
            .any(|value| !conway_unit_interval_well_formed(value))
        {
            return false;
        }
    }

    if let Some(thresholds) = &update.drep_voting_thresholds {
        let values = [
            &thresholds.motion_no_confidence,
            &thresholds.committee_normal,
            &thresholds.committee_no_confidence,
            &thresholds.update_to_constitution,
            &thresholds.hard_fork_initiation,
            &thresholds.pp_network_group,
            &thresholds.pp_economic_group,
            &thresholds.pp_technical_group,
            &thresholds.pp_gov_group,
            &thresholds.treasury_withdrawal,
        ];
        if values
            .iter()
            .any(|value| !conway_unit_interval_well_formed(value))
        {
            return false;
        }
    }

    // In Conway, protocol version is advanced via HardForkInitiation,
    // not via protocol-parameter updates.
    if update.protocol_version.is_some() {
        return false;
    }

    // Upstream `ppuWellFormed` — exact set of zero-reject fields.
    // Reference: `Cardano.Ledger.Conway.PParams` — `ppuWellFormed`.
    if update.max_block_body_size == Some(0)
        || update.max_tx_size == Some(0)
        || update.max_block_header_size == Some(0)
        || update.max_val_size == Some(0)
        || update.collateral_percentage == Some(0)
        || update.committee_term_limit == Some(0)
        || update.gov_action_lifetime == Some(0)
        || update.pool_deposit == Some(0)
        || update.gov_action_deposit == Some(0)
        || update.drep_deposit == Some(0)
    {
        return false;
    }

    // Upstream: `coinsPerUTxOByte /= 0` only enforced outside bootstrap
    // (hardforkConwayBootstrapPhase pv == False).
    if !conway_bootstrap_phase(protocol_version) && update.coins_per_utxo_byte == Some(0) {
        return false;
    }

    // Upstream: `nOpt /= 0` only enforced at PV >= 11.
    // (pvMajor pv < natVersion @11 || isValid (/= 0) ppuNOptL)
    if conway_post_pv10(protocol_version) && update.n_opt == Some(0) {
        return false;
    }

    true
}

/// Validates and stages Conway governance proposal procedures in sequential
/// order, matching upstream `conwayGovTransition`'s `foldlM'` +
/// `processProposal` semantics.  Each proposal is validated first; only
/// valid proposals are staged into `governance_actions` before the next
/// proposal is validated.  This ensures proposal N+1 can reference
/// proposal N via `prev_action_id`, but a bad-lineage proposal N is never
/// visible to subsequent proposals.
fn validate_conway_proposals(
    tx_id: crate::types::TxId,
    proposal_procedures: &[crate::eras::conway::ProposalProcedure],
    current_epoch: EpochNo,
    governance_actions: &mut BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    stake_credentials: &StakeCredentials,
    protocol_version: Option<(u64, u64)>,
    gov_action_deposit: Option<u64>,
    expected_network_id: Option<u8>,
    _protocol_params: Option<&crate::protocol_params::ProtocolParameters>,
    enact_state: &EnactState,
    gov_action_lifetime: Option<u64>,
) -> Result<(), LedgerError> {
    use crate::eras::conway::GovAction;

    for (proposal_index, proposal) in proposal_procedures.iter().enumerate() {
        if conway_bootstrap_phase(protocol_version)
            && !conway_bootstrap_action(&proposal.gov_action)
        {
            return Err(LedgerError::DisallowedProposalDuringBootstrap(
                proposal.clone(),
            ));
        }

        if let GovAction::ParameterChange {
            protocol_param_update,
            ..
        } = &proposal.gov_action
        {
            if protocol_param_update.is_empty() {
                return Err(LedgerError::MalformedProposal(proposal.gov_action.clone()));
            }

            if !conway_protocol_param_update_well_formed(protocol_param_update, protocol_version) {
                return Err(LedgerError::MalformedProposal(proposal.gov_action.clone()));
            }
        }

        if let Some(prev_action_id) = conway_proposal_prev_action_id(&proposal.gov_action) {
            if prev_action_id.transaction_id == tx_id.0
                && usize::from(prev_action_id.gov_action_index) >= proposal_index
            {
                return Err(LedgerError::InvalidPrevGovActionId(proposal.clone()));
            }

            // Accept if prev_action_id matches the enacted root for this
            // purpose group (upstream GovRelation lineage check).
            let purpose = conway_gov_action_purpose(&proposal.gov_action);
            let matches_enacted_root = enact_state.enacted_root(purpose) == Some(prev_action_id);

            if !matches_enacted_root {
                // Otherwise must reference a stored pending proposal with
                // matching purpose.
                let Some(prev_action) = governance_actions.get(prev_action_id) else {
                    return Err(LedgerError::InvalidPrevGovActionId(proposal.clone()));
                };

                if conway_gov_action_purpose(&prev_action.proposal().gov_action) != purpose {
                    return Err(LedgerError::InvalidPrevGovActionId(proposal.clone()));
                }
            }
        } else {
            // Actions with lineage and prev_action_id = None — valid only
            // when the enacted root for this purpose is also None.
            // TreasuryWithdrawals and InfoAction have no lineage concept
            // and are always accepted here.
            let purpose = conway_gov_action_purpose(&proposal.gov_action);
            match purpose {
                ConwayGovActionPurpose::ParameterChange
                | ConwayGovActionPurpose::HardFork
                | ConwayGovActionPurpose::Committee
                | ConwayGovActionPurpose::Constitution => {
                    if enact_state.enacted_root(purpose).is_some() {
                        return Err(LedgerError::InvalidPrevGovActionId(proposal.clone()));
                    }
                }
                ConwayGovActionPurpose::TreasuryWithdrawals | ConwayGovActionPurpose::Info => { /* no lineage */
                }
            }
        }

        if let Some((prev_action_id, supplied, expected)) =
            conway_expected_previous_hard_fork_version(
                proposal,
                governance_actions,
                protocol_version,
            )
        {
            if !conway_pv_can_follow(expected, supplied) {
                return Err(LedgerError::ProposalCantFollow {
                    prev_action_id,
                    supplied,
                    expected,
                });
            }
        } else if let GovAction::HardForkInitiation {
            prev_action_id: Some(prev_action_id),
            protocol_version: supplied,
        } = &proposal.gov_action
        {
            if enact_state.prev_hard_fork() == Some(prev_action_id) {
                let Some(expected) = protocol_version else {
                    return Err(LedgerError::MissingProtocolVersionForHardFork(
                        proposal.clone(),
                    ));
                };
                if !conway_pv_can_follow(expected, *supplied) {
                    return Err(LedgerError::ProposalCantFollow {
                        prev_action_id: Some(prev_action_id.clone()),
                        supplied: *supplied,
                        expected,
                    });
                }
            }
        } else if matches!(
            proposal.gov_action,
            GovAction::HardForkInitiation {
                prev_action_id: None,
                ..
            }
        ) {
            return Err(LedgerError::MissingProtocolVersionForHardFork(
                proposal.clone(),
            ));
        }

        if let Some(expected_deposit) = gov_action_deposit {
            if proposal.deposit != expected_deposit {
                return Err(LedgerError::ProposalDepositIncorrect {
                    supplied: proposal.deposit,
                    expected: expected_deposit,
                });
            }
        }

        let reward_account =
            RewardAccount::from_bytes(&proposal.reward_account).ok_or_else(|| {
                LedgerError::InvalidRewardAccountBytes(proposal.reward_account.clone())
            })?;
        if let Some(expected_network) = expected_network_id {
            if reward_account.network != expected_network {
                return Err(LedgerError::ProposalProcedureNetworkIdMismatch {
                    account: reward_account,
                    expected_network,
                });
            }
        }
        // Upstream: ProposalReturnAccountDoesNotExist is only enforced
        // post-bootstrap (PV major ≥ 10).  During Conway bootstrap phase (PV 9),
        // proposals for ParameterChange / HardForkInitiation / InfoAction are
        // allowed even when the return account is unregistered.
        // Reference: Cardano.Ledger.Conway.Rules.Gov — conwayGovTransition
        //   `unless (hardforkConwayBootstrapPhase ...) $ do ...`
        let past_bootstrap = !conway_bootstrap_phase(protocol_version);
        if past_bootstrap && !stake_credentials.is_registered(&reward_account.credential) {
            return Err(LedgerError::ProposalReturnAccountDoesNotExist(
                reward_account,
            ));
        }

        if let GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash,
        } = &proposal.gov_action
        {
            for wdrl_account in withdrawals.keys() {
                if let Some(expected_network) = expected_network_id {
                    if wdrl_account.network != expected_network {
                        return Err(LedgerError::TreasuryWithdrawalsNetworkIdMismatch {
                            account: *wdrl_account,
                            expected_network,
                        });
                    }
                }
            }

            // Upstream: TreasuryWithdrawalReturnAccountsDoNotExist — only
            // enforced post-bootstrap (PV major ≥ 10), same gate as
            // ProposalReturnAccountDoesNotExist.
            // Reference: Cardano.Ledger.Conway.Rules.Gov — conwayGovTransition
            if past_bootstrap {
                let non_registered: Vec<RewardAccount> = withdrawals
                    .keys()
                    .filter(|ra| !stake_credentials.is_registered(&ra.credential))
                    .copied()
                    .collect();
                if !non_registered.is_empty() {
                    return Err(LedgerError::TreasuryWithdrawalReturnAccountsDoNotExist(
                        non_registered,
                    ));
                }
            }

            // Upstream: `ZeroTreasuryWithdrawals` is only enforced after
            // the Conway bootstrap phase (PV major ≥ 10).
            // `hardforkConwayBootstrapPhase` returns true for PV < 10.
            if past_bootstrap && withdrawals.values().all(|amount| *amount == 0) {
                return Err(LedgerError::ZeroTreasuryWithdrawals(
                    proposal.gov_action.clone(),
                ));
            }

            // Upstream: checkGuardrailsScriptHash — the proposal's policy
            // hash must match the constitution's guardrails script hash.
            let constitution_hash = enact_state.constitution.guardrails_script_hash;
            if *guardrails_script_hash != constitution_hash {
                return Err(LedgerError::InvalidGuardrailsScriptHash {
                    proposal_hash: *guardrails_script_hash,
                    constitution_hash,
                });
            }
        }

        if let GovAction::ParameterChange {
            guardrails_script_hash,
            ..
        } = &proposal.gov_action
        {
            // Upstream: checkGuardrailsScriptHash — the proposal's policy
            // hash must match the constitution's guardrails script hash.
            let constitution_hash = enact_state.constitution.guardrails_script_hash;
            if *guardrails_script_hash != constitution_hash {
                return Err(LedgerError::InvalidGuardrailsScriptHash {
                    proposal_hash: *guardrails_script_hash,
                    constitution_hash,
                });
            }
        }

        if let GovAction::UpdateCommittee {
            members_to_remove,
            members_to_add,
            quorum,
            ..
        } = &proposal.gov_action
        {
            // Upstream: `WellFormedUnitIntervalRatification` — quorum must be
            // a valid unit interval (denominator > 0, numerator <= denominator).
            // Reference: `Cardano.Ledger.Conway.Rules.Gov` —
            // `checkWellFormedUnitIntervalRatification`.
            if quorum.denominator == 0 || quorum.numerator > quorum.denominator {
                return Err(LedgerError::WellFormedUnitIntervalRatification {
                    numerator: quorum.numerator,
                    denominator: quorum.denominator,
                });
            }

            let conflicting_members: Vec<_> = members_to_add
                .keys()
                .copied()
                .filter(|member| members_to_remove.contains(member))
                .collect();
            if !conflicting_members.is_empty() {
                return Err(LedgerError::ConflictingCommitteeUpdate(conflicting_members));
            }

            let invalid_members: Vec<_> = members_to_add
                .iter()
                .filter(|(_, epoch)| **epoch <= current_epoch.0)
                .map(|(member, epoch)| (*member, EpochNo(*epoch)))
                .collect();
            if !invalid_members.is_empty() {
                return Err(LedgerError::ExpirationEpochTooSmall(invalid_members));
            }
        }

        // Stage validated proposal (upstream foldlM' + processProposal:
        // each proposal is validated then staged, so subsequent proposals
        // in the same tx can reference it via prev_action_id lineage).
        governance_actions.insert(
            crate::eras::conway::GovActionId {
                transaction_id: tx_id.0,
                gov_action_index: proposal_index as u16,
            },
            GovernanceActionState::new_with_lifetime(
                proposal.clone(),
                current_epoch,
                gov_action_lifetime,
            ),
        );
    }

    Ok(())
}

fn validate_conway_current_treasury_value(
    submitted_treasury_value: Option<u64>,
    actual_treasury_value: u64,
) -> Result<(), LedgerError> {
    if let Some(submitted) = submitted_treasury_value {
        if submitted != actual_treasury_value {
            return Err(LedgerError::CurrentTreasuryValueIncorrect {
                supplied: submitted,
                actual: actual_treasury_value,
            });
        }
    }

    Ok(())
}

/// Validates that every key-hash withdrawal credential has a DRep delegation
/// in the pre-CERTS stake credential state (Conway post-bootstrap rule).
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Ledger` —
/// `validateWithdrawalsDelegated`.
///
/// This is gated by `!bootstrap_phase`; during bootstrap phase (PV 9) the
/// check is skipped.
fn validate_withdrawals_delegated(
    withdrawals: Option<&BTreeMap<RewardAccount, u64>>,
    stake_credentials: &StakeCredentials,
    bootstrap_phase: bool,
) -> Result<(), LedgerError> {
    // Upstream: unless (hardforkConwayBootstrapPhase ...) $ runTest $
    //   validateWithdrawalsDelegated accounts tx
    if bootstrap_phase {
        return Ok(());
    }
    let wdrls = match withdrawals {
        Some(w) if !w.is_empty() => w,
        _ => return Ok(()),
    };
    for ra in wdrls.keys() {
        if let StakeCredential::AddrKeyHash(kh) = &ra.credential {
            // Upstream: lookupAccountState (KeyHashObj keyHash) accounts >>= dRepDelegationAccountStateL
            let has_drep = stake_credentials
                .get(&StakeCredential::AddrKeyHash(*kh))
                .and_then(|state| state.delegated_drep())
                .is_some();
            if !has_drep {
                return Err(LedgerError::WithdrawalNotDelegatedToDRep { credential: *kh });
            }
        }
        // Script-hash credentials are not checked (upstream filters with `credKeyHash`).
    }
    Ok(())
}

/// Context for MIR certificate validation at admission time.
///
/// Upstream DELEG rule enforces seven checks on `MoveInstantaneousReward`
/// certificates before recording the MIR data.  All fields are optional so
/// callers that lack the full context can perform a best-effort subset.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` (MIR handling).
struct MirValidationContext<'a> {
    /// Current slot number for the timing check.
    current_slot: u64,
    /// `firstSlot(current_epoch + 1) - stability_window`: deadline after
    /// which MIR certs are too late.  Pre-computed by the caller.
    mir_deadline_slot: Option<u64>,
    /// Whether the Alonzo MIR-transfer hardfork is active (PV >= 6).
    alonzo_mir_transfers: bool,
    /// Current reserves balance.
    reserves: u64,
    /// Current treasury balance.
    treasury: u64,
    /// Snapshot of accumulated `InstantaneousRewards` so far this block.
    instantaneous_rewards: &'a InstantaneousRewards,
}

/// Context for certificate validation, bundling protocol parameters and
/// ledger state needed during `apply_certificates_and_withdrawals`.
struct CertificateValidationContext {
    key_deposit: u64,
    pool_deposit: u64,
    min_pool_cost: u64,
    e_max: u64,
    current_epoch: EpochNo,
    expected_network_id: Option<u8>,
    /// Conway governance DRep deposit (`ppDRepDeposit`).
    drep_deposit: Option<u64>,
    /// `true` when the current era is Conway or later (tag ≥ 7).
    is_conway: bool,
    /// `true` during Conway bootstrap phase (PV major == 9).
    ///
    /// Upstream: `hardforkConwayBootstrapPhase` gates DRep registration
    /// checks in `Cardano.Ledger.Conway.Rules.Deleg`.
    bootstrap_phase: bool,
    /// `true` when PV major > 10 (PV 11+).
    ///
    /// Upstream: `harforkConwayDELEGIncorrectDepositsAndRefunds` gates
    /// `DepositIncorrectDELEG` / `RefundIncorrectDELEG` error variants.
    post_pv10: bool,
}

/// Results of certificate and withdrawal processing for the value preservation
/// equation.
///
/// Upstream reference: `Cardano.Ledger.Shelley.Rules.Utxo`
/// ```text
/// consumed = balance(txins ◁ utxo) + refunds + withdrawals
/// produced = balance(outs) + fee + deposits [+ donation]
/// ```
#[derive(Debug)]
struct CertBalanceAdjustment {
    /// Sum of all withdrawal amounts from the transaction.
    withdrawal_total: u64,
    /// Total new deposits from registration certificates (key, pool, DRep).
    total_deposits: u64,
    /// Total deposit refunds from deregistration certificates.
    total_refunds: u64,
}

fn apply_scheduled_genesis_delegations(
    gen_delegs: &mut BTreeMap<GenesisHash, GenesisDelegationState>,
    future_gen_delegs: &mut BTreeMap<FutureGenesisDelegKey, GenesisDelegationState>,
    current_slot: u64,
) {
    let mut ready: Vec<(FutureGenesisDelegKey, GenesisDelegationState)> = Vec::new();
    for (key, value) in future_gen_delegs.iter() {
        if key.0 > current_slot {
            break;
        }
        ready.push((*key, value.clone()));
    }

    for (key, value) in ready {
        future_gen_delegs.remove(&key);
        gen_delegs.insert(key.1, value);
    }
}

fn schedule_future_genesis_delegation(
    future_gen_delegs: &mut BTreeMap<FutureGenesisDelegKey, GenesisDelegationState>,
    activation_slot: u64,
    genesis_hash: GenesisHash,
    delegation: GenesisDelegationState,
) {
    future_gen_delegs
        .retain(|(_, existing_genesis_hash), _| *existing_genesis_hash != genesis_hash);
    future_gen_delegs.insert((activation_slot, genesis_hash), delegation);
}

/// Scans a certificate list for `MoveInstantaneousReward` entries and
/// accumulates their effects into the given `InstantaneousRewards` state.
///
/// For `StakeCredentials` targets the per-credential deltas are merged
/// (Alonzo+ `unionWith (<>)` semantics) into the per-pot map.
///
/// For `SendToOppositePot` targets the signed pot-to-pot deltas are
/// adjusted.  The invariant `delta_reserves + delta_treasury == 0` is
/// maintained.
///
/// This function is called after each successful transaction commit
/// during block application (Shelley through Babbage).  MIR certificates
/// are absent in Conway.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — DELEG MIR handling.
pub fn accumulate_mir_from_certs(ir: &mut InstantaneousRewards, certs: Option<&[DCert]>) {
    let Some(certs) = certs else { return };
    for cert in certs {
        if let DCert::MoveInstantaneousReward(pot, target) = cert {
            match target {
                MirTarget::StakeCredentials(map) => {
                    let ir_map = match pot {
                        MirPot::Reserves => &mut ir.ir_reserves,
                        MirPot::Treasury => &mut ir.ir_treasury,
                    };
                    for (cred, &delta) in map {
                        *ir_map.entry(*cred).or_insert(0) += delta;
                    }
                }
                MirTarget::SendToOppositePot(coin) => {
                    let signed_coin = *coin as i64;
                    match pot {
                        MirPot::Reserves => {
                            ir.delta_reserves = ir.delta_reserves.saturating_sub(signed_coin);
                            ir.delta_treasury = ir.delta_treasury.saturating_add(signed_coin);
                        }
                        MirPot::Treasury => {
                            ir.delta_reserves = ir.delta_reserves.saturating_add(signed_coin);
                            ir.delta_treasury = ir.delta_treasury.saturating_sub(signed_coin);
                        }
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_certificates_and_withdrawals(
    pool_state: &mut PoolState,
    stake_credentials: &mut StakeCredentials,
    committee_state: &mut CommitteeState,
    drep_state: &mut DrepState,
    reward_accounts: &mut RewardAccounts,
    deposit_pot: &mut DepositPot,
    gen_delegs: &mut BTreeMap<GenesisHash, GenesisDelegationState>,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    ctx: &CertificateValidationContext,
    certificates: Option<&[DCert]>,
    withdrawals: Option<&BTreeMap<RewardAccount, u64>>,
) -> Result<CertBalanceAdjustment, LedgerError> {
    let mut future_gen_delegs = BTreeMap::new();
    apply_certificates_and_withdrawals_with_future(
        pool_state,
        stake_credentials,
        committee_state,
        drep_state,
        reward_accounts,
        deposit_pot,
        gen_delegs,
        &mut future_gen_delegs,
        governance_actions,
        ctx,
        certificates,
        withdrawals,
        0,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_certificates_and_withdrawals_with_future(
    pool_state: &mut PoolState,
    stake_credentials: &mut StakeCredentials,
    committee_state: &mut CommitteeState,
    drep_state: &mut DrepState,
    reward_accounts: &mut RewardAccounts,
    deposit_pot: &mut DepositPot,
    gen_delegs: &mut BTreeMap<GenesisHash, GenesisDelegationState>,
    future_gen_delegs: &mut BTreeMap<FutureGenesisDelegKey, GenesisDelegationState>,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    ctx: &CertificateValidationContext,
    certificates: Option<&[DCert]>,
    withdrawals: Option<&BTreeMap<RewardAccount, u64>>,
    current_slot: u64,
    stability_window: Option<u64>,
    mir_ctx: Option<&MirValidationContext<'_>>,
) -> Result<CertBalanceAdjustment, LedgerError> {
    let key_deposit = ctx.key_deposit;
    let pool_deposit = ctx.pool_deposit;

    // ── Withdrawal validation + account draining ──────────────────────
    // Upstream Conway CERTS rule (STS recursive base case) and Shelley
    // DELEGS both validate and drain reward-account withdrawals BEFORE
    // processing any certificates.  At PV >= 11
    // (`hardforkConwayMoveWithdrawalsAndDRepChecksToLedgerRule`) the
    // withdrawal draining is lifted from the CERTS base case into LEDGER
    // but still executes before CERTS, keeping the same relative
    // ordering.
    //
    // This ordering is semantically relevant: a transaction that
    // unregisters a stake credential AND withdraws from its reward
    // account succeeds because draining sets the balance to zero before
    // the unregistration check (`StakeCredentialHasRewards`).
    //
    // Reference: `Cardano.Ledger.Conway.Rules.Certs` —
    // `conwayCertsTransition` base case `Empty`, and
    // `Cardano.Ledger.Conway.Rules.Ledger` —
    // `hardforkConwayMoveWithdrawalsAndDRepChecksToLedgerRule`.
    let mut withdrawal_total = 0u64;
    if let Some(entries) = withdrawals {
        for (account, requested) in entries {
            // `withdrawalsThatDoNotDrainAccounts` checks the submitted
            // account address network, then upstream `drainAccounts`
            // adjusts the registered account by staking credential.
            let Some(reward_key) = reward_accounts
                .find_account_by_credential(&account.credential)
                .copied()
            else {
                return Err(LedgerError::RewardAccountNotRegistered(*account));
            };
            let state = reward_accounts
                .get_mut(&reward_key)
                .expect("reward account key resolved from RewardAccounts must exist");

            let available = state.balance();
            if *requested > available {
                return Err(LedgerError::WithdrawalExceedsBalance {
                    account: *account,
                    requested: *requested,
                    available,
                });
            }

            // Formal spec: wdrls ⊆ rewards — withdrawal amount must
            // exactly match the reward account balance for all Shelley+
            // eras. Upstream: `validateWithdrawals` enforces equal-value
            // map subset in Shelley through Conway.
            // Reference: `Cardano.Ledger.Shelley.Rules.Utxo`,
            // `Cardano.Ledger.Conway.Rules.Certs`.
            if *requested != available {
                return Err(LedgerError::WithdrawalNotFullDrain {
                    account: *account,
                    requested: *requested,
                    balance: available,
                });
            }

            state.set_balance(available - *requested);
            withdrawal_total = withdrawal_total.saturating_add(*requested);
        }
    }

    // ── Certificate processing ────────────────────────────────────────
    let mut total_deposits: u64 = 0;
    let mut total_refunds: u64 = 0;
    if let Some(certs) = certificates {
        for cert in certs {
            // -- Era-gate: Conway-only certs (CDDL tags 7–18) must be
            // rejected in Shelley–Babbage, and Shelley-only certs (tags 5–6:
            // GenesisDelegation, MoveInstantaneousReward) must be rejected
            // in Conway.
            // Reference: Conway CDDL `certificate` removes tags 5–6 and
            // adds tags 7–18; Shelley–Babbage CDDL only includes tags 0–6.
            match cert {
                DCert::AccountRegistrationDeposit(..)
                | DCert::AccountUnregistrationDeposit(..)
                | DCert::DelegationToDrep(..)
                | DCert::DelegationToStakePoolAndDrep(..)
                | DCert::AccountRegistrationDelegationToStakePool(..)
                | DCert::AccountRegistrationDelegationToDrep(..)
                | DCert::AccountRegistrationDelegationToStakePoolAndDrep(..)
                | DCert::CommitteeAuthorization(..)
                | DCert::CommitteeResignation(..)
                | DCert::DrepRegistration(..)
                | DCert::DrepUnregistration(..)
                | DCert::DrepUpdate(..)
                    if !ctx.is_conway =>
                {
                    return Err(LedgerError::UnsupportedCertificate(
                        "Conway certificate in pre-Conway era",
                    ));
                }
                DCert::GenesisDelegation(..) | DCert::MoveInstantaneousReward(..)
                    if ctx.is_conway =>
                {
                    return Err(LedgerError::UnsupportedCertificate(
                        "pre-Conway certificate in Conway era",
                    ));
                }
                _ => {}
            }
            match cert {
                DCert::AccountRegistration(credential) => {
                    register_stake_credential(stake_credentials, *credential, key_deposit)?;
                    register_reward_account_for_credential(
                        reward_accounts,
                        *credential,
                        ctx.expected_network_id,
                    );
                    deposit_pot.add_key_deposit(key_deposit);
                    total_deposits = total_deposits.saturating_add(key_deposit);
                }
                DCert::AccountRegistrationDeposit(credential, deposit) => {
                    // Conway DELEG rule: deposit must match ppKeyDeposit.
                    // Upstream `hardforkConwayDELEGIncorrectDepositsAndRefunds`:
                    // PV > 10 uses `DepositIncorrectDELEG`, PV <= 10 keeps
                    // legacy `IncorrectDepositDELEG`.
                    if ctx.is_conway && *deposit != key_deposit {
                        return Err(if ctx.post_pv10 {
                            LedgerError::DepositIncorrectDELEG {
                                supplied: *deposit,
                                expected: key_deposit,
                            }
                        } else {
                            LedgerError::IncorrectDepositDELEG {
                                supplied: *deposit,
                                expected: key_deposit,
                            }
                        });
                    }
                    // Conway DELEG rule: `checkStakeKeyNotRegistered` —
                    // already-registered credentials are rejected.
                    // Reference: `Cardano.Ledger.Conway.Rules.Deleg` —
                    // `StakeKeyRegisteredDELEG`.
                    if ctx.is_conway && stake_credentials.is_registered(credential) {
                        return Err(LedgerError::StakeCredentialAlreadyRegistered(*credential));
                    }
                    if !stake_credentials.is_registered(credential) {
                        register_stake_credential(stake_credentials, *credential, *deposit)?;
                        register_reward_account_for_credential(
                            reward_accounts,
                            *credential,
                            ctx.expected_network_id,
                        );
                        deposit_pot.add_key_deposit(*deposit);
                        total_deposits = total_deposits.saturating_add(*deposit);
                    }
                }
                DCert::AccountUnregistration(credential) => {
                    unregister_stake_credential(stake_credentials, reward_accounts, *credential)?;
                    deposit_pot.return_key_deposit(key_deposit);
                    total_refunds = total_refunds.saturating_add(key_deposit);
                }
                DCert::AccountUnregistrationDeposit(credential, refund) => {
                    // Conway DELEG rule: refund must match the stored per-credential
                    // deposit (upstream `lookupDeposit umap cred` / `checkInvalidRefund`).
                    // When stored deposit is 0 (legacy state from before deposit
                    // tracking was introduced), fall back to current `key_deposit`
                    // which matches upstream Shelley-era `shelleyKeyDepositsRefunds`
                    // behavior.
                    //
                    // Upstream `hardforkConwayDELEGIncorrectDepositsAndRefunds`:
                    // PV > 10 uses `RefundIncorrectDELEG Mismatch`,
                    // PV <= 10 uses the legacy `IncorrectDepositDELEG`.
                    if ctx.is_conway {
                        let raw_stored = stake_credentials
                            .get(credential)
                            .map(|s| s.deposit())
                            .unwrap_or(0);
                        let expected_deposit = if raw_stored > 0 {
                            raw_stored
                        } else {
                            key_deposit
                        };
                        if *refund != expected_deposit {
                            return Err(if ctx.post_pv10 {
                                // PV > 10: new error variant
                                LedgerError::RefundIncorrectDELEG {
                                    supplied: *refund,
                                    expected: expected_deposit,
                                }
                            } else {
                                // PV <= 10 (bootstrap or initial Conway): legacy error variant
                                LedgerError::IncorrectKeyDepositRefund {
                                    supplied: *refund,
                                    expected: expected_deposit,
                                }
                            });
                        }
                    }
                    // Upstream `ConwayUnRegCert` also enforces
                    // `StakeKeyHasNonZeroAccountBalanceDELEG` — reward balance
                    // must be zero before unregistering.
                    unregister_stake_credential(stake_credentials, reward_accounts, *credential)?;
                    deposit_pot.return_key_deposit(*refund);
                    total_refunds = total_refunds.saturating_add(*refund);
                }
                DCert::DelegationToStakePool(credential, pool) => {
                    // Upstream Shelley DELEG enforces
                    // DelegateeNotRegisteredDELEG for ALL eras (Shelley
                    // through Babbage).  Conway uses
                    // DelegateeStakePoolNotRegisteredDELEG in ConwayDELEG.
                    delegate_stake_credential(
                        pool_state,
                        stake_credentials,
                        reward_accounts,
                        *credential,
                        *pool,
                        true,
                    )?;
                }
                DCert::AccountRegistrationDelegationToStakePool(credential, pool, deposit) => {
                    // Conway DELEG rule: deposit must match ppKeyDeposit.
                    // PV split follows `hardforkConwayDELEGIncorrectDepositsAndRefunds`.
                    if ctx.is_conway && *deposit != key_deposit {
                        return Err(if ctx.post_pv10 {
                            LedgerError::DepositIncorrectDELEG {
                                supplied: *deposit,
                                expected: key_deposit,
                            }
                        } else {
                            LedgerError::IncorrectDepositDELEG {
                                supplied: *deposit,
                                expected: key_deposit,
                            }
                        });
                    }
                    // Conway DELEG rule: `checkStakeKeyNotRegistered` —
                    // already-registered credentials are rejected.
                    if ctx.is_conway && stake_credentials.is_registered(credential) {
                        return Err(LedgerError::StakeCredentialAlreadyRegistered(*credential));
                    }
                    if !stake_credentials.is_registered(credential) {
                        register_stake_credential(stake_credentials, *credential, *deposit)?;
                        register_reward_account_for_credential(
                            reward_accounts,
                            *credential,
                            ctx.expected_network_id,
                        );
                        deposit_pot.add_key_deposit(*deposit);
                        total_deposits = total_deposits.saturating_add(*deposit);
                    }
                    delegate_stake_credential(
                        pool_state,
                        stake_credentials,
                        reward_accounts,
                        *credential,
                        *pool,
                        true, // Conway-only cert — always check
                    )?;
                }
                DCert::DelegationToDrep(credential, drep) => {
                    delegate_drep(
                        stake_credentials,
                        drep_state,
                        *credential,
                        *drep,
                        ctx.bootstrap_phase,
                    )?;
                }
                DCert::DelegationToStakePoolAndDrep(credential, pool, drep) => {
                    delegate_stake_credential(
                        pool_state,
                        stake_credentials,
                        reward_accounts,
                        *credential,
                        *pool,
                        true, // Conway-only cert — always check
                    )?;
                    delegate_drep(
                        stake_credentials,
                        drep_state,
                        *credential,
                        *drep,
                        ctx.bootstrap_phase,
                    )?;
                }
                DCert::AccountRegistrationDelegationToDrep(credential, drep, deposit) => {
                    // Conway DELEG rule: deposit must match ppKeyDeposit.
                    // PV split follows `hardforkConwayDELEGIncorrectDepositsAndRefunds`.
                    if ctx.is_conway && *deposit != key_deposit {
                        return Err(if ctx.post_pv10 {
                            LedgerError::DepositIncorrectDELEG {
                                supplied: *deposit,
                                expected: key_deposit,
                            }
                        } else {
                            LedgerError::IncorrectDepositDELEG {
                                supplied: *deposit,
                                expected: key_deposit,
                            }
                        });
                    }
                    // Conway DELEG rule: `checkStakeKeyNotRegistered` —
                    // already-registered credentials are rejected.
                    if ctx.is_conway && stake_credentials.is_registered(credential) {
                        return Err(LedgerError::StakeCredentialAlreadyRegistered(*credential));
                    }
                    if !stake_credentials.is_registered(credential) {
                        register_stake_credential(stake_credentials, *credential, *deposit)?;
                        register_reward_account_for_credential(
                            reward_accounts,
                            *credential,
                            ctx.expected_network_id,
                        );
                        deposit_pot.add_key_deposit(*deposit);
                        total_deposits = total_deposits.saturating_add(*deposit);
                    }
                    delegate_drep(
                        stake_credentials,
                        drep_state,
                        *credential,
                        *drep,
                        ctx.bootstrap_phase,
                    )?;
                }
                DCert::AccountRegistrationDelegationToStakePoolAndDrep(
                    credential,
                    pool,
                    drep,
                    deposit,
                ) => {
                    // Conway DELEG rule: deposit must match ppKeyDeposit.
                    // PV split follows `hardforkConwayDELEGIncorrectDepositsAndRefunds`.
                    if ctx.is_conway && *deposit != key_deposit {
                        return Err(if ctx.post_pv10 {
                            LedgerError::DepositIncorrectDELEG {
                                supplied: *deposit,
                                expected: key_deposit,
                            }
                        } else {
                            LedgerError::IncorrectDepositDELEG {
                                supplied: *deposit,
                                expected: key_deposit,
                            }
                        });
                    }
                    // Conway DELEG rule: `checkStakeKeyNotRegistered` —
                    // already-registered credentials are rejected.
                    if ctx.is_conway && stake_credentials.is_registered(credential) {
                        return Err(LedgerError::StakeCredentialAlreadyRegistered(*credential));
                    }
                    if !stake_credentials.is_registered(credential) {
                        register_stake_credential(stake_credentials, *credential, *deposit)?;
                        register_reward_account_for_credential(
                            reward_accounts,
                            *credential,
                            ctx.expected_network_id,
                        );
                        deposit_pot.add_key_deposit(*deposit);
                        total_deposits = total_deposits.saturating_add(*deposit);
                    }
                    delegate_stake_credential(
                        pool_state,
                        stake_credentials,
                        reward_accounts,
                        *credential,
                        *pool,
                        true, // Conway-only cert — always check
                    )?;
                    delegate_drep(
                        stake_credentials,
                        drep_state,
                        *credential,
                        *drep,
                        ctx.bootstrap_phase,
                    )?;
                }
                DCert::CommitteeAuthorization(cold_credential, hot_credential) => {
                    authorize_committee_hot_credential(
                        committee_state,
                        governance_actions,
                        *cold_credential,
                        *hot_credential,
                    )?;
                }
                DCert::CommitteeResignation(cold_credential, anchor) => {
                    resign_committee_cold_credential(
                        committee_state,
                        governance_actions,
                        *cold_credential,
                        anchor.clone(),
                    )?;
                }
                DCert::PoolRegistration(params) => {
                    // POOL rule: cost must meet minPoolCost.
                    if params.cost < ctx.min_pool_cost {
                        return Err(LedgerError::PoolCostTooLow {
                            cost: params.cost,
                            min_pool_cost: ctx.min_pool_cost,
                        });
                    }
                    // POOL rule: margin must be a valid unit interval.
                    if params.margin.denominator == 0
                        || params.margin.numerator > params.margin.denominator
                    {
                        return Err(LedgerError::PoolMarginInvalid {
                            numerator: params.margin.numerator,
                            denominator: params.margin.denominator,
                        });
                    }
                    // POOL rule: reward account network must match.
                    if let Some(expected) = ctx.expected_network_id {
                        if params.reward_account.network != expected {
                            return Err(LedgerError::PoolRewardAccountNetworkMismatch {
                                actual: params.reward_account.network,
                                expected,
                            });
                        }
                    }
                    // POOL rule: metadata URL ≤ 64 bytes.
                    if let Some(ref metadata) = params.pool_metadata {
                        if metadata.url.len() > 64 {
                            return Err(LedgerError::PoolMetadataUrlTooLong {
                                length: metadata.url.len(),
                            });
                        }
                    }
                    // CDDL: pool_owners = set<addr_keyhash> — no duplicates.
                    {
                        let mut seen = std::collections::HashSet::new();
                        for owner in &params.pool_owners {
                            if !seen.insert(*owner) {
                                return Err(LedgerError::DuplicatePoolOwner { owner: *owner });
                            }
                        }
                    }
                    // NOTE: The upstream POOL rule (`Cardano.Ledger.Shelley.Rules.Pool`)
                    // intentionally does NOT check that pool owners are registered
                    // stake credentials. The formal Shelley spec included such a check,
                    // but the Haskell implementation omits it. Delegating with an
                    // unregistered owner is harmless — the owner simply cannot claim
                    // rewards until registered.
                    // POOL rule: VRF key must not already be registered
                    // by another pool (PV > 10 only).
                    // Reference: `Cardano.Ledger.Shelley.Rules.Pool` —
                    // `hardforkConwayDisallowDuplicatedVRFKeys pv = pvMajor pv > natVersion @10`.
                    if ctx.post_pv10 {
                        let is_new = !pool_state.is_registered(&params.operator);
                        if let Some(existing) = pool_state.find_pool_by_vrf_key(&params.vrf_keyhash)
                        {
                            // For new registration: VRF must not be used at all.
                            // For re-registration: VRF may be the same pool's own key.
                            if is_new || existing != params.operator {
                                return Err(LedgerError::VrfKeyAlreadyRegistered {
                                    pool: params.operator,
                                    vrf_key: params.vrf_keyhash,
                                    existing_pool: existing,
                                });
                            }
                        }
                    }
                    let is_new = !pool_state.is_registered(&params.operator);
                    pool_state.register_with_deposit(params.clone(), pool_deposit);
                    if is_new {
                        deposit_pot.add_pool_deposit(pool_deposit);
                        total_deposits = total_deposits.saturating_add(pool_deposit);
                    }
                }
                DCert::PoolRetirement(pool, epoch) => {
                    // POOL rule: retirement epoch must satisfy cEpoch < e <= cEpoch + eMax.
                    // Reference: `StakePoolRetirementWrongEpochPOOL`.
                    // Validate BEFORE mutating pool state to avoid corrupting
                    // `retiring_epoch` on validation failure.
                    if epoch.0 <= ctx.current_epoch.0 {
                        return Err(LedgerError::PoolRetirementTooEarly {
                            retirement_epoch: epoch.0,
                            current_epoch: ctx.current_epoch.0,
                        });
                    }
                    let max_epoch = ctx.current_epoch.0.saturating_add(ctx.e_max);
                    if epoch.0 > max_epoch {
                        return Err(LedgerError::PoolRetirementTooFar {
                            retirement_epoch: epoch.0,
                            current_epoch: ctx.current_epoch.0,
                            e_max: ctx.e_max,
                            max_epoch,
                        });
                    }
                    if !pool_state.retire(*pool, *epoch) {
                        return Err(LedgerError::PoolNotRegistered(*pool));
                    }
                }
                DCert::DrepRegistration(credential, deposit, anchor) => {
                    // Conway GOVCERT rule: deposit must match ppDRepDeposit.
                    if let Some(expected_drep_deposit) = ctx.drep_deposit {
                        if *deposit != expected_drep_deposit {
                            return Err(LedgerError::DrepIncorrectDeposit {
                                supplied: *deposit,
                                expected: expected_drep_deposit,
                            });
                        }
                    }
                    register_drep(drep_state, *credential, *deposit, anchor.clone())?;
                    deposit_pot.add_drep_deposit(*deposit);
                    total_deposits = total_deposits.saturating_add(*deposit);
                }
                DCert::DrepUnregistration(credential, refund) => {
                    unregister_drep(drep_state, stake_credentials, *credential, Some(*refund))?;
                    deposit_pot.return_drep_deposit(*refund);
                    total_refunds = total_refunds.saturating_add(*refund);
                }
                DCert::DrepUpdate(credential, anchor) => {
                    update_drep(drep_state, *credential, anchor.clone())?;
                }
                DCert::GenesisDelegation(genesis_hash, delegate_hash, vrf_hash) => {
                    // DELEG rule: genesis key must be in current mapping.
                    // Upstream: `GenesisKeyNotInMappingDELEG`.
                    if !gen_delegs.contains_key(genesis_hash) {
                        return Err(LedgerError::GenesisKeyNotInMapping {
                            genesis_hash: *genesis_hash,
                        });
                    }
                    // DELEG rule: delegate key must not be used by another
                    // genesis key in either current (`gen_delegs`) or
                    // future (`future_gen_delegs`) mappings.
                    // Upstream: `DuplicateGenesisDelegateDELEG` checks both
                    // current and future maps.
                    for (other_gk, other_ds) in gen_delegs.iter() {
                        if other_gk != genesis_hash && other_ds.delegate == *delegate_hash {
                            return Err(LedgerError::DuplicateGenesisDelegate {
                                delegate_hash: *delegate_hash,
                            });
                        }
                    }
                    for ((_, other_gk), other_ds) in future_gen_delegs.iter() {
                        if other_gk != genesis_hash && other_ds.delegate == *delegate_hash {
                            return Err(LedgerError::DuplicateGenesisDelegate {
                                delegate_hash: *delegate_hash,
                            });
                        }
                    }
                    // DELEG rule: VRF key must not be used by another genesis
                    // key in either current or future mappings.
                    // Upstream: `DuplicateGenesisVRFDELEG` checks both current
                    // and future maps.
                    for (other_gk, other_ds) in gen_delegs.iter() {
                        if other_gk != genesis_hash && other_ds.vrf == *vrf_hash {
                            return Err(LedgerError::DuplicateGenesisVrf {
                                vrf_hash: *vrf_hash,
                            });
                        }
                    }
                    for ((_, other_gk), other_ds) in future_gen_delegs.iter() {
                        if other_gk != genesis_hash && other_ds.vrf == *vrf_hash {
                            return Err(LedgerError::DuplicateGenesisVrf {
                                vrf_hash: *vrf_hash,
                            });
                        }
                    }

                    let deleg = GenesisDelegationState {
                        delegate: *delegate_hash,
                        vrf: *vrf_hash,
                    };

                    if let Some(sw) = stability_window {
                        let activation_slot = current_slot.saturating_add(sw);
                        schedule_future_genesis_delegation(
                            future_gen_delegs,
                            activation_slot,
                            *genesis_hash,
                            deleg,
                        );
                    } else {
                        gen_delegs.insert(*genesis_hash, deleg);
                    }
                }
                DCert::MoveInstantaneousReward(pot, target) => {
                    // ── Upstream DELEG MIR validation ──────────────────
                    // Reference: `Cardano.Ledger.Shelley.Rules.Deleg`
                    if let Some(mir_ctx) = mir_ctx {
                        // 1. Timing check: MIR must arrive before the
                        //    epoch deadline.
                        //    Upstream: `MIRCertificateTooLateinEpochDELEG`.
                        if let Some(deadline) = mir_ctx.mir_deadline_slot {
                            if mir_ctx.current_slot >= deadline {
                                return Err(LedgerError::MIRCertificateTooLateInEpoch {
                                    slot: mir_ctx.current_slot,
                                    deadline,
                                });
                            }
                        }

                        match target {
                            MirTarget::StakeCredentials(map) => {
                                if !mir_ctx.alonzo_mir_transfers {
                                    // 2. Pre-Alonzo: negative deltas
                                    //    not allowed.
                                    //    Upstream: `MIRNegativesNotCurrentlyAllowed`.
                                    for (_, &delta) in map.iter() {
                                        if delta < 0 {
                                            return Err(
                                                LedgerError::MIRNegativesNotCurrentlyAllowed,
                                            );
                                        }
                                    }
                                } else {
                                    // 3. Alonzo+: combined map must
                                    //    not produce negatives.
                                    //    Upstream: `MIRProducesNegativeUpdate`.
                                    let ir_map = match pot {
                                        MirPot::Reserves => {
                                            &mir_ctx.instantaneous_rewards.ir_reserves
                                        }
                                        MirPot::Treasury => {
                                            &mir_ctx.instantaneous_rewards.ir_treasury
                                        }
                                    };
                                    for (cred, &delta) in map.iter() {
                                        let existing = ir_map.get(cred).copied().unwrap_or(0);
                                        if existing.saturating_add(delta) < 0 {
                                            return Err(LedgerError::MIRProducesNegativeUpdate);
                                        }
                                    }
                                }

                                // 4. Pot sufficiency: total combined rewards
                                //    must not exceed pot balance.
                                //    Upstream: `InsufficientForInstantaneousRewardsDELEG`.
                                let ir_map = match pot {
                                    MirPot::Reserves => &mir_ctx.instantaneous_rewards.ir_reserves,
                                    MirPot::Treasury => &mir_ctx.instantaneous_rewards.ir_treasury,
                                };
                                // Merge new deltas with existing for total.
                                let mut combined = ir_map.clone();
                                for (cred, &delta) in map.iter() {
                                    *combined.entry(*cred).or_insert(0) += delta;
                                }
                                let total_required: u64 = combined
                                    .values()
                                    .filter(|&&v| v > 0)
                                    .map(|&v| v as u64)
                                    .sum();

                                let pot_balance = match pot {
                                    MirPot::Reserves => mir_ctx.reserves,
                                    MirPot::Treasury => mir_ctx.treasury,
                                };
                                let available = if mir_ctx.alonzo_mir_transfers {
                                    // Alonzo+: add delta for this pot.
                                    let delta = match pot {
                                        MirPot::Reserves => {
                                            mir_ctx.instantaneous_rewards.delta_reserves
                                        }
                                        MirPot::Treasury => {
                                            mir_ctx.instantaneous_rewards.delta_treasury
                                        }
                                    };
                                    if delta >= 0 {
                                        pot_balance.saturating_add(delta as u64)
                                    } else {
                                        pot_balance.saturating_sub((-delta) as u64)
                                    }
                                } else {
                                    pot_balance
                                };
                                if total_required > available {
                                    return Err(LedgerError::MIRInsufficientPotBalance {
                                        pot: *pot,
                                        available,
                                        required: total_required,
                                    });
                                }
                            }
                            MirTarget::SendToOppositePot(coin) => {
                                if !mir_ctx.alonzo_mir_transfers {
                                    // 5. Pre-Alonzo: transfers not
                                    //    allowed.
                                    //    Upstream: `MIRTransferNotCurrentlyAllowed`.
                                    return Err(LedgerError::MIRTransferNotCurrentlyAllowed);
                                }

                                // 6. Non-negative transfer.
                                //    Upstream: `MIRNegativeTransfer`.
                                // NOTE: Our `SendToOppositePot(u64)` is
                                // unsigned, so this is inherently satisfied.
                                // Keep the check as documentation.
                                let _ = coin;

                                // 7. Transfer <= available after MIR.
                                //    Upstream: `InsufficientForTransferDELEG`.
                                //    `availableAfterMIR pot acnt iRewards`:
                                //    pot_balance + delta - sum(positive combined ir entries)
                                let ir_map = match pot {
                                    MirPot::Reserves => &mir_ctx.instantaneous_rewards.ir_reserves,
                                    MirPot::Treasury => &mir_ctx.instantaneous_rewards.ir_treasury,
                                };
                                let pot_balance = match pot {
                                    MirPot::Reserves => mir_ctx.reserves,
                                    MirPot::Treasury => mir_ctx.treasury,
                                };
                                let delta = match pot {
                                    MirPot::Reserves => {
                                        mir_ctx.instantaneous_rewards.delta_reserves
                                    }
                                    MirPot::Treasury => {
                                        mir_ctx.instantaneous_rewards.delta_treasury
                                    }
                                };
                                let with_delta = if delta >= 0 {
                                    pot_balance.saturating_add(delta as u64)
                                } else {
                                    pot_balance.saturating_sub((-delta) as u64)
                                };
                                let ir_committed: u64 =
                                    ir_map.values().filter(|&&v| v > 0).map(|&v| v as u64).sum();
                                let available_after = with_delta.saturating_sub(ir_committed);
                                if *coin > available_after {
                                    return Err(LedgerError::MIRInsufficientForTransfer {
                                        pot: *pot,
                                        available: available_after,
                                        required: *coin,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(CertBalanceAdjustment {
        withdrawal_total,
        total_deposits,
        total_refunds,
    })
}

fn register_stake_credential(
    stake_credentials: &mut StakeCredentials,
    credential: StakeCredential,
    deposit: u64,
) -> Result<(), LedgerError> {
    if !stake_credentials.register_with_deposit(credential, deposit) {
        return Err(LedgerError::StakeCredentialAlreadyRegistered(credential));
    }

    Ok(())
}

fn register_reward_account_for_credential(
    reward_accounts: &mut RewardAccounts,
    credential: StakeCredential,
    expected_network_id: Option<u8>,
) {
    let Some(network) = expected_network_id else {
        return;
    };
    if reward_accounts
        .find_account_by_credential(&credential)
        .is_some()
    {
        return;
    }
    reward_accounts.insert(
        RewardAccount {
            network,
            credential,
        },
        RewardAccountState::new(0, None),
    );
}

fn unregister_stake_credential(
    stake_credentials: &mut StakeCredentials,
    reward_accounts: &mut RewardAccounts,
    credential: StakeCredential,
) -> Result<(), LedgerError> {
    if !stake_credentials.is_registered(&credential) {
        return Err(LedgerError::StakeCredentialNotRegistered(credential));
    }

    let reward_balance: u64 = reward_accounts
        .entries
        .iter()
        .filter(|(account, _)| account.credential == credential)
        .map(|(_, state)| state.balance())
        .sum();
    if reward_balance != 0 {
        return Err(LedgerError::StakeCredentialHasRewards {
            credential,
            balance: reward_balance,
        });
    }

    stake_credentials.unregister(&credential);
    reward_accounts
        .entries
        .retain(|account, _| account.credential != credential);
    Ok(())
}

fn delegate_stake_credential(
    pool_state: &PoolState,
    stake_credentials: &mut StakeCredentials,
    reward_accounts: &mut RewardAccounts,
    credential: StakeCredential,
    pool: PoolKeyHash,
    check_pool_registered: bool,
) -> Result<(), LedgerError> {
    // Upstream: both Shelley DELEG (`DelegateeNotRegisteredDELEG`) and
    // Conway DELEG (`DelegateeStakePoolNotRegisteredDELEG`) enforce that
    // the target pool must be registered.  The `check_pool_registered`
    // flag controls whether this crate enforces the check (always true
    // in practice).
    //
    // Reference: `Cardano.Ledger.Shelley.Rules.Deleg` —
    //   `DelegStakeTxCert cred stakePool -> Map.member stakePool ...`;
    // `Cardano.Ledger.Conway.Rules.Deleg` — `checkStakeDelegateeRegistered`.
    if check_pool_registered && !pool_state.is_registered(&pool) {
        return Err(LedgerError::PoolNotRegistered(pool));
    }

    let Some(state) = stake_credentials.get_mut(&credential) else {
        return Err(LedgerError::StakeCredentialNotRegistered(credential));
    };
    state.set_delegated_pool(Some(pool));

    for (account, account_state) in &mut reward_accounts.entries {
        if account.credential == credential {
            account_state.set_delegated_pool(Some(pool));
        }
    }

    Ok(())
}

fn authorize_committee_hot_credential(
    committee_state: &mut CommitteeState,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    cold_credential: StakeCredential,
    hot_credential: StakeCredential,
) -> Result<(), LedgerError> {
    // Upstream `checkAndOverwriteCommitteeMemberState` in
    // `Cardano.Ledger.Conway.Rules.GovCert`:
    //
    // 1. Check csCommitteeCreds for resignation — BEFORE membership check.
    // 2. Check committeeMembers (enacted) or pending UpdateCommittee proposals.
    // 3. Insert new authorization state.
    //
    // This ordering matters: a resigned member re-added via UpdateCommittee
    // still gets `ConwayCommitteeHasPreviouslyResigned` because resignation
    // lives in csCommitteeCreds which is separate from committeeMembers.

    // Step 1: resignation check (upstream checks csCommitteeCreds first).
    if committee_state
        .get(&cold_credential)
        .is_some_and(|m| m.is_resigned())
    {
        return Err(LedgerError::CommitteeHasPreviouslyResigned(cold_credential));
    }

    // Step 2: membership check (enacted member OR potential future member).
    let is_current_member = committee_state
        .get(&cold_credential)
        .is_some_and(|m| m.expires_at().is_some());
    if !is_current_member && !is_potential_future_member(&cold_credential, governance_actions) {
        return Err(LedgerError::CommitteeIsUnknown(cold_credential));
    }

    // Auto-register if not yet in the map (potential future member only).
    if committee_state.get(&cold_credential).is_none() {
        committee_state.register(cold_credential);
    }

    // Step 3: insert new hot-key authorization.
    let Some(member_state) = committee_state.get_mut(&cold_credential) else {
        return Err(LedgerError::CommitteeIsUnknown(cold_credential));
    };
    member_state.set_authorization(Some(CommitteeAuthorization::CommitteeHotCredential(
        hot_credential,
    )));
    Ok(())
}

fn resign_committee_cold_credential(
    committee_state: &mut CommitteeState,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    cold_credential: StakeCredential,
    anchor: Option<Anchor>,
) -> Result<(), LedgerError> {
    // Same upstream `checkAndOverwriteCommitteeMemberState` flow as
    // authorization: resignation checked BEFORE membership.

    // Step 1: resignation check.
    if committee_state
        .get(&cold_credential)
        .is_some_and(|m| m.is_resigned())
    {
        return Err(LedgerError::CommitteeHasPreviouslyResigned(cold_credential));
    }

    // Step 2: membership check.
    let is_current_member = committee_state
        .get(&cold_credential)
        .is_some_and(|m| m.expires_at().is_some());
    if !is_current_member && !is_potential_future_member(&cold_credential, governance_actions) {
        return Err(LedgerError::CommitteeIsUnknown(cold_credential));
    }

    // Auto-register if not yet in the map.
    if committee_state.get(&cold_credential).is_none() {
        committee_state.register(cold_credential);
    }

    // Step 3: insert resignation.
    let Some(member_state) = committee_state.get_mut(&cold_credential) else {
        return Err(LedgerError::CommitteeIsUnknown(cold_credential));
    };
    member_state.set_authorization(Some(CommitteeAuthorization::CommitteeMemberResigned(
        anchor,
    )));
    Ok(())
}

/// Upstream `isPotentialFutureMember`: returns true when `cold_credential`
/// appears in any pending `UpdateCommittee` proposal's `members_to_add` map.
fn is_potential_future_member(
    cold_credential: &StakeCredential,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
) -> bool {
    for action_state in governance_actions.values() {
        if let crate::eras::conway::GovAction::UpdateCommittee { members_to_add, .. } =
            &action_state.proposal.gov_action
        {
            // members_to_add keys are StakeCredential
            if members_to_add.contains_key(cold_credential) {
                return true;
            }
        }
    }
    false
}

fn register_drep(
    drep_state: &mut DrepState,
    credential: StakeCredential,
    deposit: u64,
    anchor: Option<Anchor>,
) -> Result<(), LedgerError> {
    let drep = drep_from_credential(credential);
    if !drep_state.register(drep, RegisteredDrep::new(deposit, anchor)) {
        return Err(LedgerError::DrepAlreadyRegistered(drep));
    }

    Ok(())
}

fn unregister_drep(
    drep_state: &mut DrepState,
    stake_credentials: &mut StakeCredentials,
    credential: StakeCredential,
    refund: Option<u64>,
) -> Result<(), LedgerError> {
    let drep = drep_from_credential(credential);
    // Conway GOVCERT rule: refund must match stored deposit.
    if let Some(supplied_refund) = refund {
        if let Some(entry) = drep_state.get(&drep) {
            let expected = entry.deposit();
            if supplied_refund != expected {
                return Err(LedgerError::DrepIncorrectRefund {
                    supplied: supplied_refund,
                    expected,
                });
            }
        }
    }
    if drep_state.unregister(&drep).is_none() {
        return Err(LedgerError::DrepNotRegistered(drep));
    }

    // Upstream `clearDRepDelegations` in `Cardano.Ledger.Conway.Rules.GovCert`:
    // When a DRep unregisters, clear the DRep delegation from all staker
    // accounts that were delegated to it.
    stake_credentials.clear_drep_delegation(&drep);

    Ok(())
}

fn update_drep(
    drep_state: &mut DrepState,
    credential: StakeCredential,
    anchor: Option<Anchor>,
) -> Result<(), LedgerError> {
    let drep = drep_from_credential(credential);
    let Some(state) = drep_state.get_mut(&drep) else {
        return Err(LedgerError::DrepNotRegistered(drep));
    };

    state.set_anchor(anchor);
    Ok(())
}

fn delegate_drep(
    stake_credentials: &mut StakeCredentials,
    drep_state: &DrepState,
    credential: StakeCredential,
    drep: DRep,
    bootstrap_phase: bool,
) -> Result<(), LedgerError> {
    let Some(state) = stake_credentials.get_mut(&credential) else {
        return Err(LedgerError::StakeCredentialNotRegistered(credential));
    };

    // Upstream `checkDRepRegistered` in `Cardano.Ledger.Conway.Rules.Deleg`:
    //   unless (hardforkConwayBootstrapPhase pv) $
    //     targetDRep `Map.member` dReps ?! DelegateeDRepNotRegisteredDELEG
    //
    // During bootstrap phase (PV == 9), delegating to an unregistered DRep
    // is allowed.
    if !bootstrap_phase && !is_builtin_drep(drep) && !drep_state.is_registered(&drep) {
        return Err(LedgerError::DelegateeDRepNotRegistered(drep));
    }

    state.set_delegated_drep(Some(drep));
    Ok(())
}

fn drep_from_credential(credential: StakeCredential) -> DRep {
    match credential {
        StakeCredential::AddrKeyHash(hash) => DRep::KeyHash(hash),
        StakeCredential::ScriptHash(hash) => DRep::ScriptHash(hash),
    }
}

/// Updates `last_active_epoch` for DReps that were registered or updated
/// in the current batch of certificates.
///
/// Upstream `computeDRepExpiryVersioned`:
///   - Bootstrap phase (PV == 9): `addEpochInterval currentEpoch drepActivity`
///     (no dormant subtraction).
///   - Post-bootstrap (PV >= 10): `computeDRepExpiry` subtracts dormant epochs.
fn touch_drep_activity_for_certs(
    certificates: Option<&[DCert]>,
    drep_state: &mut DrepState,
    current_epoch: EpochNo,
    num_dormant_epochs: u64,
    bootstrap_phase: bool,
) {
    let Some(certs) = certificates else {
        return;
    };
    for cert in certs {
        let credential = match cert {
            DCert::DrepRegistration(c, _, _) | DCert::DrepUpdate(c, _) => *c,
            _ => continue,
        };
        let drep = drep_from_credential(credential);
        if let Some(entry) = drep_state.get_mut(&drep) {
            // Upstream `computeDRepExpiryVersioned` (post-bootstrap) /
            // `updateDRepExpiry`:
            //   expiry = currentEpoch + drepActivity - dormant
            // In our model: last_active_epoch = currentEpoch - dormant
            //
            // During bootstrap: last_active_epoch = currentEpoch (no dormant).
            let dormant = if bootstrap_phase {
                0
            } else {
                num_dormant_epochs
            };
            entry.touch_activity(EpochNo(current_epoch.0.saturating_sub(dormant)));
        }
    }
}

fn is_builtin_drep(drep: DRep) -> bool {
    matches!(drep, DRep::AlwaysAbstain | DRep::AlwaysNoConfidence)
}

// ---------------------------------------------------------------------------
// Phase-1 transaction validation helpers
// ---------------------------------------------------------------------------

/// Validates a pre-Alonzo transaction against protocol parameters.
///
/// Checks: transaction size limit, linear fee minimum, and min-UTxO per output.
fn validate_pre_alonzo_tx(
    params: &crate::protocol_params::ProtocolParameters,
    tx_body_size: usize,
    declared_fee: u64,
    outputs: &[MultiEraTxOut],
) -> Result<(), LedgerError> {
    crate::fees::validate_tx_size(params, tx_body_size)?;
    crate::fees::validate_fee(params, tx_body_size, None, declared_fee)?;
    crate::min_utxo::validate_all_outputs_min_utxo(params, outputs)?;
    // Mary+ can carry multi-asset output values; enforce max_val_size
    // when the protocol parameter is set (no-op for Shelley/Allegra
    // where max_val_size is None).
    // Reference: `Cardano.Ledger.Mary.Rules.Utxo` — `validateOutputTooBigUTxO`.
    crate::min_utxo::validate_output_not_too_big(params, outputs)?;
    // Mary+ values are expected to be normalized before validation. Pre-Conway
    // raw decoders prune zero quantities via upstream `decodeWithPrunning`;
    // keep this as a defensive invariant for constructed values.
    crate::min_utxo::validate_no_zero_valued_multi_asset(outputs)?;
    crate::min_utxo::validate_output_boot_addr_attrs(outputs)?;
    Ok(())
}

/// Validates an Alonzo+ transaction against protocol parameters.
///
/// Checks: transaction size limit, fee minimum (including script costs
/// when `total_ex_units` is provided), min-UTxO per output, per-tx
/// execution-unit limits, mandatory collateral when redeemers are present,
/// and collateral sufficiency when collateral inputs are declared.
///
/// `has_redeemers` indicates whether the transaction's witness set
/// contains at least one redeemer (phase-2 scripts).  When `true`,
/// collateral inputs are mandatory per the upstream `feesOK` rule.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `feesOK`.
fn validate_alonzo_plus_tx(
    params: &crate::protocol_params::ProtocolParameters,
    utxo: &MultiEraUtxo,
    tx_body_size: usize,
    declared_fee: u64,
    outputs: &[MultiEraTxOut],
    output_raw_sizes: Option<&[usize]>,
    collateral_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
    total_ex_units: Option<&crate::eras::alonzo::ExUnits>,
    collateral_return: Option<&MultiEraTxOut>,
    collateral_return_raw_size: Option<usize>,
    total_collateral: Option<u64>,
    has_redeemers: bool,
    ref_scripts_size: usize,
    enforce_collateral_input_limit: bool,
) -> Result<(), LedgerError> {
    crate::fees::validate_tx_size(params, tx_body_size)?;
    // Conway adds the tiered reference-script fee to the minimum.
    // For pre-Conway eras, ref_scripts_size is 0 so this is equivalent
    // to the standard `validate_fee`.
    crate::fees::validate_conway_fee(
        params,
        tx_body_size,
        total_ex_units,
        ref_scripts_size,
        declared_fee,
    )?;
    if let Some(eu) = total_ex_units {
        crate::fees::validate_tx_ex_units(params, eu)?;
    }
    // Upstream uses `allSizedOutputsTxBodyF` which includes collateral_return.
    // Reference: Cardano.Ledger.Babbage.TxBody — allSizedOutputsTxBodyF.
    let mut all_outputs_buf: Vec<MultiEraTxOut>;
    let all_outputs: &[MultiEraTxOut] = if let Some(cr) = collateral_return {
        all_outputs_buf = Vec::with_capacity(outputs.len() + 1);
        all_outputs_buf.extend_from_slice(outputs);
        all_outputs_buf.push(cr.clone());
        &all_outputs_buf
    } else {
        outputs
    };
    let mut all_output_sizes_buf: Vec<usize>;
    let all_output_raw_sizes = match (output_raw_sizes, collateral_return) {
        (Some(sizes), Some(_)) => {
            all_output_sizes_buf = Vec::with_capacity(sizes.len() + 1);
            all_output_sizes_buf.extend_from_slice(sizes);
            if let Some(size) = collateral_return_raw_size {
                all_output_sizes_buf.push(size);
                Some(all_output_sizes_buf.as_slice())
            } else {
                None
            }
        }
        (Some(sizes), None) => Some(sizes),
        _ => None,
    };
    if let Some(sizes) = all_output_raw_sizes {
        crate::min_utxo::validate_all_outputs_min_utxo_with_sizes(params, all_outputs, sizes)?;
    } else {
        crate::min_utxo::validate_all_outputs_min_utxo(params, all_outputs)?;
    }
    crate::min_utxo::validate_output_not_too_big(params, all_outputs)?;
    // Pre-Conway raw decoders prune zero quantities before this point; this is
    // a defensive invariant until Conway/Dijkstra strict decode is era-gated.
    crate::min_utxo::validate_no_zero_valued_multi_asset(all_outputs)?;
    crate::min_utxo::validate_output_boot_addr_attrs(all_outputs)?;

    // Babbage/Conway apply this as a standalone UTXO check, independent of
    // redeemer presence.
    // Reference: Cardano.Ledger.Babbage.Rules.Utxo — validateTooManyCollateralInputs.
    if enforce_collateral_input_limit {
        if let Some(collateral) = collateral_inputs {
            if let Some(max) = params.max_collateral_inputs {
                let count = collateral.len();
                if count > max as usize {
                    return Err(LedgerError::TooManyCollateralInputs { count, max });
                }
            }
        }
    }

    // When the transaction carries phase-2 scripts (redeemers ≠ ∅),
    // collateral is mandatory.
    // Reference: Cardano.Ledger.Alonzo.Rules.Utxo — feesOK Part 2.
    if has_redeemers {
        let has_collateral = collateral_inputs.is_some_and(|c| !c.is_empty());
        if !has_collateral {
            return Err(LedgerError::MissingCollateralForScripts);
        }
    }

    // Upstream `feesOK` only validates collateral when redeemers are present.
    // Reference: Cardano.Ledger.Alonzo.Rules.Utxo `feesOK` part 2.
    if has_redeemers {
        if let Some(collateral) = collateral_inputs {
            if !collateral.is_empty() {
                crate::collateral::validate_collateral(
                    params,
                    utxo,
                    collateral,
                    declared_fee,
                    collateral_return,
                    total_collateral,
                )?;
            }
        }
    }
    Ok(())
}

/// Validates that the total execution units across all transactions in a block
/// do not exceed `max_block_ex_units` from protocol parameters.
///
/// Implements the upstream Alonzo BBODY rule:
/// `totalExUnits(txs) <= maxBlockExUnits(pp)`.
///
/// Each transaction's redeemer ExUnits are summed from their witness sets.
/// When protocol parameters or `max_block_ex_units` are absent the check is
/// skipped (soft-skip semantics for pre-Alonzo eras or missing params).
fn validate_block_ex_units(
    params: Option<&crate::protocol_params::ProtocolParameters>,
    witness_sets: &[Option<&[u8]>],
) -> Result<(), LedgerError> {
    let params = match params {
        Some(p) => p,
        None => return Ok(()),
    };
    let max = match &params.max_block_ex_units {
        Some(m) => m,
        None => return Ok(()),
    };
    let mut block_mem: u64 = 0;
    let mut block_steps: u64 = 0;
    for wb in witness_sets {
        if let Some(eu) = sum_redeemer_ex_units_from_bytes(*wb) {
            block_mem = block_mem.saturating_add(eu.mem);
            block_steps = block_steps.saturating_add(eu.steps);
        }
    }
    if block_mem > max.mem || block_steps > max.steps {
        return Err(LedgerError::BlockExUnitsExceeded {
            block_mem,
            block_steps,
            max_mem: max.mem,
            max_steps: max.steps,
        });
    }
    Ok(())
}

/// Sums execution units across all redeemers in a witness set.
fn sum_redeemer_ex_units(
    witness_set: &crate::eras::shelley::ShelleyWitnessSet,
) -> Option<crate::eras::alonzo::ExUnits> {
    if witness_set.redeemers.is_empty() {
        return None;
    }
    let mut total = crate::eras::alonzo::ExUnits { mem: 0, steps: 0 };
    for redeemer in &witness_set.redeemers {
        total.mem = total.mem.saturating_add(redeemer.ex_units.mem);
        total.steps = total.steps.saturating_add(redeemer.ex_units.steps);
    }
    Some(total)
}

/// Validates each individual redeemer's ExUnits against `maxTxExUnits`.
///
/// Upstream: `validateExUnitsTooBigUTxO` checks `all pointWiseExUnits (<=)`.
fn validate_per_redeemer_ex_units_from_witness_set(
    witness_set: &crate::eras::shelley::ShelleyWitnessSet,
    params: &crate::protocol_params::ProtocolParameters,
) -> Result<(), LedgerError> {
    if witness_set.redeemers.is_empty() {
        return Ok(());
    }
    crate::fees::validate_per_redeemer_ex_units(params, &witness_set.redeemers)
}

/// Validates each individual redeemer's ExUnits from raw witness bytes.
fn validate_per_redeemer_ex_units_from_bytes(
    witness_bytes: Option<&[u8]>,
    params: &crate::protocol_params::ProtocolParameters,
) -> Result<(), LedgerError> {
    let wb = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };
    let ws = match crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb) {
        Ok(ws) => ws,
        Err(_) => return Ok(()), // malformed witness handled elsewhere
    };
    validate_per_redeemer_ex_units_from_witness_set(&ws, params)
}

/// Extracts total redeemer execution units from raw witness bytes.
///
/// Returns `None` when witness bytes are absent, malformed, or carry no
/// redeemers — matching the soft-skip semantics used elsewhere.
fn sum_redeemer_ex_units_from_bytes(
    witness_bytes: Option<&[u8]>,
) -> Option<crate::eras::alonzo::ExUnits> {
    let wb = witness_bytes?;
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(wb).ok()?;
    sum_redeemer_ex_units(&ws)
}

/// Decodes a witness set from raw bytes and validates that all required
/// VKey hashes are covered.
///
/// `required` is the set of 28-byte Blake2b-224 hashes that must be
/// witnessed (spending inputs, certificates, withdrawals, required_signers).
///
/// `tx_body_hash` is the 32-byte Blake2b-256 hash of the serialized
/// transaction body — the message that each VKey witness must sign.
fn validate_witnesses_if_present(
    witness_bytes: Option<&[u8]>,
    required: &HashSet<[u8; 28]>,
    tx_body_hash: &[u8; 32],
) -> Result<(), LedgerError> {
    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    // Merge VKey hashes + bootstrap witness address-root hashes into the
    // provided set.  Reference: `keyHashWitnessesTxWits` in
    // `Cardano.Ledger.Core` combines `witVKeyHash` and `bootstrapWitKeyHash`.
    let mut provided = crate::witnesses::witness_vkey_hash_set(&ws.vkey_witnesses);
    for bw_hash in crate::witnesses::bootstrap_witness_key_hash_set(&ws.bootstrap_witnesses) {
        provided.insert(bw_hash);
    }
    crate::witnesses::validate_vkey_witnesses(required, &provided)?;
    crate::witnesses::verify_vkey_signatures(tx_body_hash, &ws.vkey_witnesses)?;
    crate::witnesses::verify_bootstrap_witnesses(tx_body_hash, &ws.bootstrap_witnesses)
}

/// Validates VKey witnesses given a typed witness set (no re-parse).
///
/// Used by submitted-tx paths where the witness set is already decoded.
fn validate_witnesses_typed(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
    required: &HashSet<[u8; 28]>,
    tx_body_hash: &[u8; 32],
) -> Result<(), LedgerError> {
    // Merge VKey hashes + bootstrap witness address-root hashes.
    let mut provided = crate::witnesses::witness_vkey_hash_set(&ws.vkey_witnesses);
    for bw_hash in crate::witnesses::bootstrap_witness_key_hash_set(&ws.bootstrap_witnesses) {
        provided.insert(bw_hash);
    }
    crate::witnesses::validate_vkey_witnesses(required, &provided)?;
    crate::witnesses::verify_vkey_signatures(tx_body_hash, &ws.vkey_witnesses)?;
    crate::witnesses::verify_bootstrap_witnesses(tx_body_hash, &ws.bootstrap_witnesses)
}

/// Validates native scripts referenced by script-hash credentials.
///
/// For each required script hash, looks up the native script in the
/// witness set, computes its hash, and evaluates it. Skips validation
/// when witness bytes are absent (backward compatibility).
fn validate_native_scripts_if_present(
    witness_bytes: Option<&[u8]>,
    required_script_hashes: &HashSet<[u8; 28]>,
    current_slot: u64,
) -> Result<HashSet<[u8; 28]>, LedgerError> {
    if required_script_hashes.is_empty() {
        return Ok(HashSet::new());
    }
    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(HashSet::new()),
    };
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    let vkey_hashes = crate::witnesses::witness_vkey_hash_set(&ws.vkey_witnesses);
    let mut native_satisfied = HashSet::new();

    // Build a lookup from script hash → native script
    let mut script_map: std::collections::HashMap<[u8; 28], &crate::eras::allegra::NativeScript> =
        std::collections::HashMap::new();
    for ns in &ws.native_scripts {
        let h = crate::native_script::native_script_hash(ns);
        script_map.insert(h, ns);
    }

    let ctx = crate::native_script::NativeScriptContext {
        vkey_hashes: &vkey_hashes,
        current_slot,
    };

    for required_hash in required_script_hashes {
        if let Some(script) = script_map.get(required_hash) {
            if !crate::native_script::evaluate_native_script(script, &ctx) {
                return Err(LedgerError::NativeScriptFailed {
                    hash: *required_hash,
                });
            }
            native_satisfied.insert(*required_hash);
        }
        // When a required script is not in the native_scripts witness
        // list, it may be a Plutus script and is checked separately.
    }

    Ok(native_satisfied)
}

/// Ensures every required script hash is present in either native or Plutus
/// script witnesses (including reference scripts).
fn validate_required_script_witnesses(
    witness_bytes: Option<&[u8]>,
    required_script_hashes: &HashSet<[u8; 28]>,
    native_satisfied: &HashSet<[u8; 28]>,
    spending_utxo: &MultiEraUtxo,
    reference_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
    spending_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
) -> Result<(), LedgerError> {
    if required_script_hashes.is_empty() {
        return Ok(());
    }

    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => {
            let missing = required_script_hashes
                .iter()
                .find(|hash| !native_satisfied.contains(*hash))
                .copied();
            return match missing {
                Some(hash) => Err(LedgerError::MissingScriptWitness { hash }),
                None => Ok(()),
            };
        }
    };

    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    let plutus_scripts = crate::plutus_validation::collect_all_plutus_scripts(
        &ws,
        spending_utxo,
        reference_inputs,
        spending_inputs,
    );

    for required_hash in required_script_hashes {
        if native_satisfied.contains(required_hash) {
            continue;
        }
        if !plutus_scripts.contains_key(required_hash) {
            return Err(LedgerError::MissingScriptWitness {
                hash: *required_hash,
            });
        }
    }

    Ok(())
}

/// Collect the set of script hashes provided in the witness set (native
/// scripts + Plutus V1/V2/V3 scripts).
fn provided_script_hashes_from_witnesses(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
) -> HashSet<[u8; 28]> {
    let mut provided = HashSet::new();
    for ns in &ws.native_scripts {
        provided.insert(crate::native_script::native_script_hash(ns));
    }
    for s in &ws.plutus_v1_scripts {
        provided.insert(crate::plutus_validation::plutus_script_hash(
            crate::plutus_validation::PlutusVersion::V1,
            s,
        ));
    }
    for s in &ws.plutus_v2_scripts {
        provided.insert(crate::plutus_validation::plutus_script_hash(
            crate::plutus_validation::PlutusVersion::V2,
            s,
        ));
    }
    for s in &ws.plutus_v3_scripts {
        provided.insert(crate::plutus_validation::plutus_script_hash(
            crate::plutus_validation::PlutusVersion::V3,
            s,
        ));
    }
    provided
}

/// Collects script hashes from reference input UTxOs (Babbage+).
///
/// For each reference input that resolves to a Babbage `BabbageTxOut` with a
/// `script_ref`, computes the script hash. Returns the set of script hashes
/// available via references.
///
/// Reference: upstream `getReferenceScripts` — `referenceScriptHashes`.
fn collect_reference_script_hashes(
    utxo: &crate::utxo::MultiEraUtxo,
    reference_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
) -> HashSet<[u8; 28]> {
    let mut hashes = HashSet::new();
    if let Some(ref_inputs) = reference_inputs {
        for txin in ref_inputs {
            if let Some(txout) = utxo.get(txin) {
                if let Some(sr) = txout.script_ref() {
                    hashes.insert(crate::witnesses::script_hash(&sr.0));
                }
            }
        }
    }
    hashes
}

/// Validates that no scripts in the witness set are extraneous — every
/// provided script must be required by an input, certificate, withdrawal,
/// mint, vote, or proposal in the transaction.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.extraneousScriptWitnessesUTXOW`.
fn validate_no_extraneous_script_witnesses(
    witness_bytes: Option<&[u8]>,
    required_script_hashes: &HashSet<[u8; 28]>,
    reference_script_hashes: Option<&HashSet<[u8; 28]>>,
) -> Result<(), LedgerError> {
    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    let provided = provided_script_hashes_from_witnesses(&ws);
    // Upstream Babbage: `neededNonRefs = sNeeded \ sRefs`; extraneous = `sReceived \ neededNonRefs`.
    // For Shelley/Alonzo (no reference inputs), reference_script_hashes is None which
    // degenerates to `sReceived \ sNeeded`.
    let needed_non_refs: HashSet<[u8; 28]> = match reference_script_hashes {
        Some(refs) => required_script_hashes.difference(refs).copied().collect(),
        None => required_script_hashes.clone(),
    };
    for hash in &provided {
        if !needed_non_refs.contains(hash) {
            return Err(LedgerError::ExtraneousScriptWitness { hash: *hash });
        }
    }
    Ok(())
}

/// Typed variant for submitted-path where we already have a decoded
/// `ShelleyWitnessSet`.
fn validate_no_extraneous_script_witnesses_typed(
    ws: &crate::eras::shelley::ShelleyWitnessSet,
    required_script_hashes: &HashSet<[u8; 28]>,
    reference_script_hashes: Option<&HashSet<[u8; 28]>>,
) -> Result<(), LedgerError> {
    let provided = provided_script_hashes_from_witnesses(ws);
    let needed_non_refs: HashSet<[u8; 28]> = match reference_script_hashes {
        Some(refs) => required_script_hashes.difference(refs).copied().collect(),
        None => required_script_hashes.clone(),
    };
    for hash in &provided {
        if !needed_non_refs.contains(hash) {
            return Err(LedgerError::ExtraneousScriptWitness { hash: *hash });
        }
    }
    Ok(())
}

/// Validates that a transaction's auxiliary data hash matches its auxiliary
/// data content.
///
/// If the transaction body declares an `auxiliary_data_hash`, the
/// corresponding raw CBOR auxiliary data must be present and its
/// Blake2b-256 hash must match the declared value. If no hash is declared
/// the data must be absent.
///
/// When `protocol_version` is `Some((major, minor))` and the version is
/// greater than (2, 0), the metadata content is additionally validated:
/// all byte strings and text strings within transaction metadatum entries
/// must be ≤ 64 bytes (upstream `validMetadatum`).
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxow` — `validateMetadata`.
fn validate_auxiliary_data(
    declared_hash: Option<&[u8; 32]>,
    auxiliary_data: Option<&[u8]>,
    protocol_version: Option<(u64, u64)>,
) -> Result<(), LedgerError> {
    match (declared_hash, auxiliary_data) {
        (Some(declared), Some(data)) => {
            let computed = yggdrasil_crypto::hash_bytes_256(data).0;
            if *declared != computed {
                return Err(LedgerError::AuxiliaryDataHashMismatch {
                    declared: *declared,
                    computed,
                });
            }
            // Upstream `SoftForks.validMetadata`: active when pv > ProtVer 2 0.
            if let Some((major, minor)) = protocol_version {
                if major > 2 || (major == 2 && minor > 0) {
                    validate_auxiliary_data_metadata_sizes(data)?;
                }
            }
            Ok(())
        }
        (Some(_), None) => Err(LedgerError::AuxiliaryDataMissing),
        // Upstream `validateMissingTxBodyMetadataHash`: if auxiliary data is
        // present in the transaction, the body MUST declare its hash.
        (None, Some(_)) => Err(LedgerError::MissingTxBodyMetadataHash),
        // Neither hash nor data — nothing to validate.
        (None, None) => Ok(()),
    }
}

/// Validates that all transaction metadatum values within auxiliary data
/// conform to CDDL size constraints: byte strings ≤ 64 and text strings
/// ≤ 64 bytes.
///
/// Auxiliary data CBOR layouts:
/// - Shelley: `metadata` (a map of uint → transaction_metadatum)
/// - Allegra/Mary: `[metadata, [scripts]]`
/// - Alonzo+: `#6.259({? 0 => metadata, ? 1 => [native_scripts], ...})`
///
/// Reference: `Cardano.Ledger.Metadata` — `validMetadatum`.
fn validate_auxiliary_data_metadata_sizes(raw: &[u8]) -> Result<(), LedgerError> {
    use crate::cbor::Decoder;
    let mut dec = Decoder::new(raw);
    if dec.is_empty() {
        return Ok(());
    }
    let major = dec.peek_major().unwrap_or(0xff);
    match major {
        // Major type 5 (map): Shelley-style metadata — the whole thing is
        // `{ * uint => transaction_metadatum }`.
        5 => validate_metadata_map(&mut dec),
        // Major type 4 (array): Allegra/Mary `[metadata, [scripts]]`.
        4 => {
            let len = dec.array().map_err(|_| LedgerError::InvalidMetadata)?;
            if len == 0 {
                return Ok(());
            }
            // First element is the metadata map.
            validate_metadata_map(&mut dec)
            // Remaining elements (scripts) are not checked for metadata sizes.
        }
        // Major type 6 (tag): Alonzo+ `#6.259({...})`.
        6 => {
            let _tag = dec.tag().map_err(|_| LedgerError::InvalidMetadata)?;
            // Expect a map inside the tag.
            let count = dec.map().map_err(|_| LedgerError::InvalidMetadata)?;
            for _ in 0..count {
                let key = dec.unsigned().map_err(|_| LedgerError::InvalidMetadata)?;
                if key == 0 {
                    // Key 0 is the metadata map.
                    return validate_metadata_map(&mut dec);
                }
                // Skip non-metadata entries (scripts, etc.).
                dec.skip().map_err(|_| LedgerError::InvalidMetadata)?;
            }
            Ok(())
        }
        _ => {
            // Unknown auxiliary data format — skip validation rather than
            // reject valid blocks with future CBOR layouts.
            Ok(())
        }
    }
}

/// Validates entries in a `metadata = { * uint => transaction_metadatum }` map.
fn validate_metadata_map(dec: &mut crate::cbor::Decoder<'_>) -> Result<(), LedgerError> {
    let count = dec.map().map_err(|_| LedgerError::InvalidMetadata)?;
    for _ in 0..count {
        // Key is uint — skip it.
        dec.skip().map_err(|_| LedgerError::InvalidMetadata)?;
        // Value is a transaction_metadatum — recursively validate.
        if !validate_metadatum(dec)? {
            return Err(LedgerError::InvalidMetadata);
        }
    }
    Ok(())
}

/// Recursively validates a single `transaction_metadatum` CBOR item.
///
/// Returns `Ok(true)` when the metadatum and all sub-items are valid,
/// `Ok(false)` when a bytes/text item exceeds 64 bytes.
///
/// ```text
/// transaction_metadatum =
///     int
///   / bytes .size (0..64)
///   / text .size (0..64)
///   / [ * transaction_metadatum ]
///   / { * transaction_metadatum => transaction_metadatum }
/// ```
fn validate_metadatum(dec: &mut crate::cbor::Decoder<'_>) -> Result<bool, LedgerError> {
    let major = dec.peek_major().map_err(|_| LedgerError::InvalidMetadata)?;
    match major {
        // Major type 0 (unsigned) or 1 (negative): integer — always valid.
        0 | 1 => {
            dec.skip().map_err(|_| LedgerError::InvalidMetadata)?;
            Ok(true)
        }
        // Major type 2 (bytes): must be ≤ 64 bytes.
        2 => {
            let bs = dec.bytes().map_err(|_| LedgerError::InvalidMetadata)?;
            Ok(bs.len() <= 64)
        }
        // Major type 3 (text): UTF-8 bytes must be ≤ 64.
        3 => {
            let s = dec.text().map_err(|_| LedgerError::InvalidMetadata)?;
            Ok(s.len() <= 64)
        }
        // Major type 4 (array): recurse into elements.
        4 => {
            let count = dec.array().map_err(|_| LedgerError::InvalidMetadata)?;
            for _ in 0..count {
                if !validate_metadatum(dec)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        // Major type 5 (map): recurse into keys and values.
        5 => {
            let count = dec.map().map_err(|_| LedgerError::InvalidMetadata)?;
            for _ in 0..count {
                if !validate_metadatum(dec)? {
                    return Ok(false);
                }
                if !validate_metadatum(dec)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        // Tags or other types — skip (not standard metadatum but tolerate).
        _ => {
            dec.skip().map_err(|_| LedgerError::InvalidMetadata)?;
            Ok(true)
        }
    }
}

/// Extracts the network ID from raw Shelley-family address bytes.
///
/// Returns `None` for Byron addresses (header type 8) and reserved types
/// (9–13), and `Some(net)` for Shelley types 0–7 (base/pointer/enterprise)
/// and 14–15 (reward key/script) where `net = header & 0x0f`.
fn shelley_address_network_id(addr_bytes: &[u8]) -> Option<u8> {
    let header = *addr_bytes.first()?;
    let addr_type = (header >> 4) & 0x0f;
    // Shelley address types: 0–7 (base/pointer/enterprise), 14–15 (reward).
    // Byron type 8 and reserved 9–13 do not carry a Shelley network ID.
    match addr_type {
        0..=7 | 14 | 15 => Some(header & 0x0f),
        _ => None,
    }
}

/// Validates that all transaction output addresses have the expected network
/// ID.
///
/// Byron addresses are exempt since they do not carry a network ID in the
/// Shelley sense.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `WrongNetwork`.
fn validate_output_network_ids(expected: u8, outputs: &[MultiEraTxOut]) -> Result<(), LedgerError> {
    for output in outputs {
        let addr_bytes = output.address();
        if let Some(net) = shelley_address_network_id(addr_bytes) {
            if net != expected {
                return Err(LedgerError::WrongNetwork {
                    expected,
                    found: net,
                });
            }
        }
    }
    Ok(())
}

/// Validates that all withdrawal reward accounts have the expected network
/// ID.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `WrongNetworkWithdrawal`.
fn validate_withdrawal_network_ids<'a, I>(expected: u8, withdrawals: I) -> Result<(), LedgerError>
where
    I: IntoIterator<Item = (&'a RewardAccount, &'a u64)>,
{
    for (acct, _) in withdrawals {
        if acct.network != expected {
            return Err(LedgerError::WrongNetworkWithdrawal {
                expected,
                found: acct.network,
            });
        }
    }
    Ok(())
}

/// Validates that the `network_id` field declared in the transaction body
/// (Alonzo+) matches the expected network.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `WrongNetworkInTxBody`.
fn validate_tx_body_network_id(expected: u8, declared: Option<u8>) -> Result<(), LedgerError> {
    if let Some(net) = declared {
        if net != expected {
            return Err(LedgerError::WrongNetworkInTxBody {
                expected,
                found: net,
            });
        }
    }
    Ok(())
}

fn accumulate_multi_asset(total: &mut MultiAsset, assets: &MultiAsset) {
    for (policy, entries) in assets {
        let policy_total = total.entry(*policy).or_default();
        for (asset_name, quantity) in entries {
            let entry = policy_total.entry(asset_name.clone()).or_default();
            *entry = entry.saturating_add(*quantity);
        }
    }
}

fn relay_access_points_from_relays(relays: &[Relay]) -> Vec<PoolRelayAccessPoint> {
    let mut access_points = Vec::new();

    for relay in relays {
        match relay {
            Relay::SingleHostAddr(Some(port), ipv4, ipv6) => {
                if let Some(ipv4) = ipv4 {
                    access_points.push(PoolRelayAccessPoint {
                        address: Ipv4Addr::from(*ipv4).to_string(),
                        port: *port,
                    });
                }
                if let Some(ipv6) = ipv6 {
                    access_points.push(PoolRelayAccessPoint {
                        address: Ipv6Addr::from(*ipv6).to_string(),
                        port: *port,
                    });
                }
            }
            Relay::SingleHostName(Some(port), domain) => {
                access_points.push(PoolRelayAccessPoint {
                    address: domain.clone(),
                    port: *port,
                });
            }
            Relay::SingleHostAddr(None, _, _)
            | Relay::SingleHostName(None, _)
            | Relay::MultiHostName(_) => {}
        }
    }

    access_points
}

// ---------------------------------------------------------------------------
// Ratification tally engine (Conway RATIFY rule)
// ---------------------------------------------------------------------------
//
// Reference: `Cardano.Ledger.Conway.Rules.Ratify` and
// `Cardano.Ledger.Conway.Governance.DRepPulser`.
//
// The ratification functions below tally stored votes for each voter role
// (constitutional committee, DReps, stake-pool operators) against the
// per-action-type thresholds in `PoolVotingThresholds` /
// `DRepVotingThresholds`. The combined predicate `ratify_action()`
// determines whether a governance action has been accepted.

use crate::protocol_params::{DRepVotingThresholds, PoolVotingThresholds};
use crate::stake::PoolStakeDistribution;

/// Tally result for one voter role.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoteTally {
    /// Weighted "yes" votes.
    pub yes: u64,
    /// Weighted "no" votes (explicit only — abstentions excluded).
    pub no: u64,
    /// Weighted "abstain" votes.
    pub abstain: u64,
    /// Total eligible voting weight (yes + no + abstain + non-voting).
    pub total: u64,
}

impl VoteTally {
    /// Whether the "yes" fraction of **non-abstaining** weight meets `threshold`.
    ///
    /// Upstream semantics: `yes / (total - abstain) >= threshold`.
    /// Avoids float arithmetic by cross-multiplying.
    pub fn meets_threshold(&self, threshold: &UnitInterval) -> bool {
        let active = self.total.saturating_sub(self.abstain);
        if active == 0 {
            // Upstream: `a %? b = if b == 0 then 0 else a % b`
            // (Cardano.Ledger.BaseTypes).  A zero ratio only meets a zero
            // threshold (`r == minBound` short-circuit in committeeAccepted,
            // dRepAccepted, spoAccepted).
            return threshold.numerator == 0;
        }
        // yes * denominator >= threshold_numerator * active
        (self.yes as u128) * (threshold.denominator as u128)
            >= (threshold.numerator as u128) * (active as u128)
    }
}

/// Counts the number of active (non-resigned, non-expired) committee members.
///
/// A member is active when:
/// - They have a registered hot credential (not resigned), **and**
/// - Their term has not expired (`current_epoch <= expiry`).
///
/// This matches the upstream `activeCommitteeSize` calculation inside
/// `votingCommitteeThresholdInternal`.
fn count_active_committee_members(committee_state: &CommitteeState, current_epoch: EpochNo) -> u64 {
    committee_state
        .iter()
        .filter(|(_, member)| {
            member.is_enacted_member() && !member.is_resigned() && !member.is_expired(current_epoch)
        })
        .count() as u64
}

/// Tally constitutional-committee votes for a governance action.
///
/// Each non-resigned, non-expired committee member has equal weight (1).
/// Resigned members and members whose term has expired are excluded from
/// the total.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify` —
/// `ccVotesSatisfied` filters `committeeMembers` by
/// `currentEpoch <= expirationEpoch` before tallying.
pub(crate) fn tally_committee_votes(
    action: &GovernanceActionState,
    committee_state: &CommitteeState,
    current_epoch: EpochNo,
) -> VoteTally {
    use crate::eras::conway::{Vote, Voter};

    let mut yes: u64 = 0;
    let mut no: u64 = 0;
    let mut abstain: u64 = 0;
    let mut eligible: u64 = 0;

    for (_cold_cred, member_state) in committee_state.iter() {
        // Non-enacted members (e.g., auto-registered via isPotentialFutureMember
        // or membership-cleared via NoConfidence) do not count.
        if !member_state.is_enacted_member() {
            continue;
        }
        // Resigned members do not count.
        if member_state.is_resigned() {
            continue;
        }
        // Expired members do not count (upstream: currentEpoch <= expirationEpoch).
        if member_state.is_expired(current_epoch) {
            continue;
        }
        eligible += 1;

        // Find whether this committee member voted.
        // Votes are keyed by Voter which carries HOT credential hashes
        // (Conway CDDL tags 0/1 = `committee_hot_credential`).  We must
        // look up the member's authorized hot credential and build the Voter
        // from that, not from the cold credential.
        //
        // Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `ccVotesSatisfied`
        // iterates `committeeMembers`, resolves each cold credential to its
        // hot credential via `votingCommitteeCredentials`, and then looks up
        // the vote keyed by the hot credential.
        let hot_voter = member_state
            .hot_credential()
            .map(|hot_cred| match hot_cred {
                StakeCredential::AddrKeyHash(h) => Voter::CommitteeKeyHash(h),
                StakeCredential::ScriptHash(h) => Voter::CommitteeScript(h),
            });

        match hot_voter.and_then(|v| action.votes.get(&v)) {
            Some(Vote::Yes) => yes += 1,
            Some(Vote::No) => no += 1,
            Some(Vote::Abstain) => abstain += 1,
            None => {} // no hot credential or did not vote — counted in eligible but not tallied
        }
    }

    VoteTally {
        yes,
        no,
        abstain,
        total: eligible,
    }
}

/// Tally DRep votes for a governance action, weighted by delegated stake.
///
/// Only active DReps (not exceeding the `drep_activity` window) are
/// counted. Inactive DReps are excluded from both the vote tally and the
/// total eligible weight.
///
/// **`AlwaysAbstain`** delegated stake is excluded from the total,
/// effectively reducing the quorum denominator.
///
/// **`AlwaysNoConfidence`** delegated stake is always included in the
/// total.  When `count_no_confidence_as_yes` is true (i.e. for
/// `NoConfidence` and `UpdateCommittee`-in-state-of-no-confidence
/// actions), that stake is additionally counted as automatic "Yes"
/// votes.
///
/// `drep_delegated_stake` maps each `DRep` to the total lovelace
/// delegated to it. The caller is responsible for computing this from
/// the stake distribution (see `compute_drep_stake_distribution`).
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `dRepVotesSatisfied`.
pub(crate) fn tally_drep_votes(
    action: &GovernanceActionState,
    drep_state: &DrepState,
    drep_delegated_stake: &BTreeMap<DRep, u64>,
    current_epoch: EpochNo,
    drep_activity: u64,
    count_no_confidence_as_yes: bool,
) -> VoteTally {
    use crate::eras::conway::{Vote, Voter};

    let mut yes: u64 = 0;
    let mut no: u64 = 0;
    let mut abstain: u64 = 0;
    let mut total: u64 = 0;

    for (drep, stake) in drep_delegated_stake {
        match drep {
            DRep::AlwaysAbstain => {
                // Excluded from total — reduces quorum denominator.
                continue;
            }
            DRep::AlwaysNoConfidence => {
                // Always included in total.  Counted as automatic "Yes"
                // for NoConfidence/UpdateCommittee(no-confidence) actions.
                total = total.saturating_add(*stake);
                if count_no_confidence_as_yes {
                    yes = yes.saturating_add(*stake);
                }
                continue;
            }
            _ => {}
        }

        // Only active registered DReps count.
        let Some(reg) = drep_state.get(drep) else {
            continue;
        };
        // Check activity window.
        if reg
            .last_active_epoch
            .is_some_and(|e| e.0.saturating_add(drep_activity) < current_epoch.0)
        {
            continue; // inactive — excluded from quorum
        }

        total = total.saturating_add(*stake);

        // Find vote keyed by DRep voter tag. `AlwaysAbstain` /
        // `AlwaysNoConfidence` are already handled via `continue` in the
        // early match at the top of this loop so they cannot reach here
        // under current control-flow. `continue` (rather than
        // `unreachable!()`) keeps us defensive: if a future refactor
        // removes the early filter, we silently skip the variant instead
        // of panicking in production.
        let voter = match drep {
            DRep::KeyHash(h) => Voter::DRepKeyHash(*h),
            DRep::ScriptHash(h) => Voter::DRepScript(*h),
            DRep::AlwaysAbstain | DRep::AlwaysNoConfidence => continue,
        };

        match action.votes.get(&voter) {
            Some(Vote::Yes) => yes = yes.saturating_add(*stake),
            Some(Vote::No) => no = no.saturating_add(*stake),
            Some(Vote::Abstain) => abstain = abstain.saturating_add(*stake),
            None => {} // non-voting weight already in total
        }
    }

    VoteTally {
        yes,
        no,
        abstain,
        total,
    }
}

/// Default vote for a stake pool that did not vote explicitly.
///
/// Reference: `Cardano.Ledger.Conway.Governance.DefaultVote`,
/// `Cardano.Ledger.Conway.Governance.defaultStakePoolVote`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefaultVote {
    /// Pool reward account delegates to a DRep key/script or is undelegated.
    No,
    /// Pool reward account delegates to `DRepAlwaysAbstain`.
    Abstain,
    /// Pool reward account delegates to `DRepAlwaysNoConfidence`.
    NoConfidence,
}

/// Determine the default SPO vote from the pool's reward-account DRep delegation.
///
/// Upstream: `defaultStakePoolVote poolId poolParams accounts`
/// 1. Look up the pool's `PoolParams` → `reward_account` → extract credential.
/// 2. Look up that credential in stake credentials → `delegated_drep`.
/// 3. Map `AlwaysAbstain → DefaultAbstain`, `AlwaysNoConfidence → DefaultNoConfidence`,
///    everything else (including undelegated) → `DefaultNo`.
pub(crate) fn default_stake_pool_vote(
    pool_hash: &PoolKeyHash,
    pool_state: &PoolState,
    stake_credentials: &StakeCredentials,
) -> DefaultVote {
    let pool = match pool_state.get(pool_hash) {
        Some(p) => p,
        None => return DefaultVote::No,
    };
    let cred = &pool.params().reward_account.credential;
    let drep = match stake_credentials.get(cred) {
        Some(state) => state.delegated_drep(),
        None => return DefaultVote::No,
    };
    match drep {
        Some(crate::types::DRep::AlwaysAbstain) => DefaultVote::Abstain,
        Some(crate::types::DRep::AlwaysNoConfidence) => DefaultVote::NoConfidence,
        _ => DefaultVote::No,
    }
}

/// Tally stake-pool operator (SPO) votes for a governance action, weighted
/// by delegated pool stake.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `spoVotesSatisfied`.
pub(crate) fn tally_spo_votes(
    action: &GovernanceActionState,
    pool_stake_dist: &PoolStakeDistribution,
    is_bootstrap_phase: bool,
    pool_state: &PoolState,
    stake_credentials: &StakeCredentials,
) -> VoteTally {
    use crate::eras::conway::{Vote, Voter};

    let is_hard_fork = matches!(
        &action.proposal.gov_action,
        crate::eras::conway::GovAction::HardForkInitiation { .. }
    );
    let is_no_confidence = matches!(
        &action.proposal.gov_action,
        crate::eras::conway::GovAction::NoConfidence { .. }
    );

    let mut yes: u64 = 0;
    let mut no: u64 = 0;
    let mut abstain: u64 = 0;

    for (pool_hash, &pool_stake) in pool_stake_dist.iter() {
        let voter = Voter::StakePool(*pool_hash);
        match action.votes.get(&voter) {
            Some(Vote::Yes) => yes = yes.saturating_add(pool_stake),
            Some(Vote::No) => no = no.saturating_add(pool_stake),
            Some(Vote::Abstain) => abstain = abstain.saturating_add(pool_stake),
            None => {
                // Upstream spoAcceptedRatio:
                // - HardForkInitiation: non-voting → implicit No (always)
                // - Bootstrap phase: non-voting → implicit Abstain
                // - Post-bootstrap: uses defaultStakePoolVote
                //
                // Reference: Cardano.Ledger.Conway.Governance.defaultStakePoolVote
                if is_hard_fork {
                    // Non-voting on HardFork is always implicit No (not counted
                    // as yes or abstain, falls through to total denominator).
                } else if is_bootstrap_phase {
                    abstain = abstain.saturating_add(pool_stake);
                } else {
                    // Post-bootstrap: derive default vote from pool's reward
                    // account DRep delegation.
                    match default_stake_pool_vote(pool_hash, pool_state, stake_credentials) {
                        DefaultVote::Abstain => {
                            abstain = abstain.saturating_add(pool_stake);
                        }
                        DefaultVote::NoConfidence => {
                            if is_no_confidence {
                                yes = yes.saturating_add(pool_stake);
                            }
                            // else: implicit No (only counted in total)
                        }
                        DefaultVote::No => {
                            // implicit No (only counted in total)
                        }
                    }
                }
            }
        }
    }

    VoteTally {
        yes,
        no,
        abstain,
        total: pool_stake_dist.total_active_stake(),
    }
}

/// Look up the required DRep voting threshold for a governance action type.
///
/// Returns `None` for action types where DRep votes are not required
/// (InfoAction — always accepted, never enacted).
pub(crate) fn drep_threshold_for_action(
    action: &crate::eras::conway::GovAction,
    has_committee: bool,
    thresholds: &DRepVotingThresholds,
) -> Option<UnitInterval> {
    match action {
        crate::eras::conway::GovAction::ParameterChange {
            protocol_param_update,
            ..
        } => conway_drep_parameter_change_threshold(protocol_param_update, thresholds),
        crate::eras::conway::GovAction::HardForkInitiation { .. } => {
            Some(thresholds.hard_fork_initiation)
        }
        crate::eras::conway::GovAction::NoConfidence { .. } => {
            Some(thresholds.motion_no_confidence)
        }
        crate::eras::conway::GovAction::UpdateCommittee { .. } => {
            // Upstream: `isElectedCommittee = isSJust (ensCommitteeL)`.
            // When no committee exists (post-NoConfidence), use no-confidence
            // threshold.
            Some(if has_committee {
                thresholds.committee_normal
            } else {
                thresholds.committee_no_confidence
            })
        }
        crate::eras::conway::GovAction::NewConstitution { .. } => {
            Some(thresholds.update_to_constitution)
        }
        crate::eras::conway::GovAction::TreasuryWithdrawals { .. } => {
            Some(thresholds.treasury_withdrawal)
        }
        crate::eras::conway::GovAction::InfoAction => None,
    }
}

/// Look up the required SPO voting threshold for a governance action.
///
/// Returns `None` for actions where SPO votes are not required.
pub(crate) fn spo_threshold_for_action(
    action: &crate::eras::conway::GovAction,
    has_committee: bool,
    thresholds: &PoolVotingThresholds,
) -> Option<UnitInterval> {
    match action {
        crate::eras::conway::GovAction::ParameterChange {
            protocol_param_update,
            ..
        } => conway_parameter_change_has_spo_security_vote_group(protocol_param_update)
            .then_some(thresholds.pp_security_group),
        crate::eras::conway::GovAction::HardForkInitiation { .. } => {
            Some(thresholds.hard_fork_initiation)
        }
        crate::eras::conway::GovAction::NoConfidence { .. } => {
            Some(thresholds.motion_no_confidence)
        }
        crate::eras::conway::GovAction::UpdateCommittee { .. } => {
            // Upstream: `isElectedCommittee = isSJust (ensCommitteeL)`.
            Some(if has_committee {
                thresholds.committee_normal
            } else {
                thresholds.committee_no_confidence
            })
        }
        crate::eras::conway::GovAction::NewConstitution { .. }
        | crate::eras::conway::GovAction::TreasuryWithdrawals { .. }
        | crate::eras::conway::GovAction::InfoAction => None,
    }
}

/// Determines whether a governance action is accepted by the
/// constitutional committee.
///
/// The committee must meet a quorum (`committee_quorum` threshold)
/// with equal-weight per-member votes.
///
/// Upstream `votingCommitteeThresholdInternal` logic determines per-action
/// voting semantics:
/// - `NoConfidence` and `UpdateCommittee`: committee vote is not required
///   (`NoVotingAllowed` → always passes, threshold 0).
/// - `InfoAction`: no voting threshold available (`NoVotingThreshold` →
///   committee never accepts, matching upstream behavior where InfoAction
///   proposals are never ratified via committee vote).
/// - For all other actions (NewConstitution, HardForkInitiation,
///   ParameterChange, TreasuryWithdrawals): if the number of active
///   (non-resigned, non-expired) committee members is below
///   `min_committee_size` and we are **not** in bootstrap phase, the
///   committee never accepts (upstream: too-small committee treated as
///   absent).
///
/// Reference: `Cardano.Ledger.Conway.Governance.Internal` —
/// `votingCommitteeThresholdInternal`, `committeeAccepted`.
pub(crate) fn accepted_by_committee(
    action: &GovernanceActionState,
    committee_state: &CommitteeState,
    committee_quorum: &UnitInterval,
    current_epoch: EpochNo,
    min_committee_size: u64,
    is_bootstrap_phase: bool,
    has_committee: bool,
) -> bool {
    use crate::eras::conway::GovAction;

    match &action.proposal.gov_action {
        // NoVotingAllowed → threshold 0 → always passes.
        GovAction::NoConfidence { .. } | GovAction::UpdateCommittee { .. } => true,

        // NoVotingThreshold → SNothing → always fails.
        GovAction::InfoAction => false,

        // All other actions use the committee quorum threshold,
        // but only if a committee currently exists and is large enough.
        _ => {
            if !has_committee {
                // Upstream: ensCommitteeL == SNothing → NoVotingThreshold
                // → committeeAccepted returns False.
                return false;
            }
            if !is_bootstrap_phase {
                let active = count_active_committee_members(committee_state, current_epoch);
                if active < min_committee_size {
                    return false;
                }
            }
            let tally = tally_committee_votes(action, committee_state, current_epoch);
            tally.meets_threshold(committee_quorum)
        }
    }
}

/// Determines whether a governance action is accepted by DReps.
///
/// Returns `true` when:
/// - The action type does not require DRep approval, or
/// - The stake-weighted DRep tally meets the per-type threshold.
///
/// For `NoConfidence` and `UpdateCommittee`-in-state-of-no-confidence
/// actions, stake delegated to `AlwaysNoConfidence` is counted as
/// automatic "Yes" votes.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `dRepVotesSatisfied`.
pub(crate) fn accepted_by_dreps(
    action: &GovernanceActionState,
    has_committee: bool,
    drep_state: &DrepState,
    drep_delegated_stake: &BTreeMap<DRep, u64>,
    current_epoch: EpochNo,
    drep_activity: u64,
    thresholds: &DRepVotingThresholds,
) -> bool {
    let Some(threshold) =
        drep_threshold_for_action(&action.proposal.gov_action, has_committee, thresholds)
    else {
        return true; // no DRep vote required for this action type
    };

    // AlwaysNoConfidence stake counts as "Yes" only for NoConfidence actions.
    //
    // Upstream reference: `dRepAcceptedRatio` in
    // `Cardano.Ledger.Conway.Rules.Ratify`:
    //   DRepAlwaysNoConfidence ->
    //     case govAction of
    //       NoConfidence _ -> (yes + stake, tot + stake)
    //       _              -> (yes, tot + stake)
    let count_no_confidence_as_yes = matches!(
        &action.proposal.gov_action,
        crate::eras::conway::GovAction::NoConfidence { .. }
    );

    let tally = tally_drep_votes(
        action,
        drep_state,
        drep_delegated_stake,
        current_epoch,
        drep_activity,
        count_no_confidence_as_yes,
    );
    tally.meets_threshold(&threshold)
}

/// Determines whether a governance action is accepted by stake-pool
/// operators.
///
/// Returns `true` when:
/// - The action type does not require SPO approval, or
/// - The stake-weighted SPO tally meets the per-type threshold.
pub(crate) fn accepted_by_spo(
    action: &GovernanceActionState,
    has_committee: bool,
    pool_stake_dist: &PoolStakeDistribution,
    thresholds: &PoolVotingThresholds,
    is_bootstrap_phase: bool,
    pool_state: &PoolState,
    stake_credentials: &StakeCredentials,
) -> bool {
    let Some(threshold) =
        spo_threshold_for_action(&action.proposal.gov_action, has_committee, thresholds)
    else {
        return true; // no SPO vote required for this action type
    };
    let tally = tally_spo_votes(
        action,
        pool_stake_dist,
        is_bootstrap_phase,
        pool_state,
        stake_credentials,
    );
    tally.meets_threshold(&threshold)
}

/// Combined ratification predicate: checks whether a governance action is
/// accepted by **all** required voter roles (CC + DRep + SPO).
///
/// This implements the core of the Conway RATIFY rule acceptance test.
/// InfoAction proposals are always accepted (they have no side effects).
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `ratifyTransition`.
pub(crate) fn ratify_action(
    action: &GovernanceActionState,
    committee_state: &CommitteeState,
    committee_quorum: &UnitInterval,
    drep_state: &DrepState,
    drep_delegated_stake: &BTreeMap<DRep, u64>,
    current_epoch: EpochNo,
    drep_activity: u64,
    drep_thresholds: &DRepVotingThresholds,
    pool_stake_dist: &PoolStakeDistribution,
    pool_thresholds: &PoolVotingThresholds,
    min_committee_size: u64,
    is_bootstrap_phase: bool,
    has_committee: bool,
    pool_state: &PoolState,
    stake_credentials: &StakeCredentials,
) -> bool {
    // Upstream: during Conway bootstrap phase (PV 9), all DRep thresholds are
    // zeroed (`def` = minBound for every field).  With zero thresholds the
    // `r == minBound` short-circuit in `dRepAccepted` makes every non-Info
    // action pass the DRep gate automatically.
    //
    // Reference: `votingDRepThresholdInternal` in
    // `Cardano.Ledger.Conway.Governance.Internal`:
    //   | hardforkConwayBootstrapPhase (pp ^. ppProtocolVersionL) = def
    let zero_drep = DRepVotingThresholds {
        motion_no_confidence: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        committee_normal: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        committee_no_confidence: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        update_to_constitution: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        hard_fork_initiation: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        pp_network_group: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        pp_economic_group: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        pp_technical_group: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        pp_gov_group: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        treasury_withdrawal: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
    };
    let effective_drep_thresholds = if is_bootstrap_phase {
        &zero_drep
    } else {
        drep_thresholds
    };

    accepted_by_committee(
        action,
        committee_state,
        committee_quorum,
        current_epoch,
        min_committee_size,
        is_bootstrap_phase,
        has_committee,
    ) && accepted_by_dreps(
        action,
        has_committee,
        drep_state,
        drep_delegated_stake,
        current_epoch,
        drep_activity,
        effective_drep_thresholds,
    ) && accepted_by_spo(
        action,
        has_committee,
        pool_stake_dist,
        pool_thresholds,
        is_bootstrap_phase,
        pool_state,
        stake_credentials,
    )
}

#[cfg(test)]
mod tests;
