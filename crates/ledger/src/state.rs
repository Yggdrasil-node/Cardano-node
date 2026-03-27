use crate::eras::allegra::AllegraTxBody;
use crate::eras::alonzo::AlonzoTxBody;
use crate::eras::babbage::BabbageTxBody;
use crate::eras::byron::ByronTx;
use crate::eras::conway::ConwayTxBody;
use crate::eras::mary::{MultiAsset, Value};
use crate::eras::shelley::{ShelleyTxBody, ShelleyTxIn, ShelleyUtxo};
use crate::types::{
    Address, Anchor, DCert, DRep, EpochNo, GenesisDelegateHash, GenesisHash, Point,
    PoolKeyHash, PoolParams, RewardAccount, Relay, StakeCredential, UnitInterval,
    VrfKeyHash,
};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use crate::{CborDecode, CborEncode, Decoder, Encoder, Era, LedgerError};
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};

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
    let hash: [u8; 28] = raw
        .try_into()
        .map_err(|_| LedgerError::CborInvalidLength {
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
/// This is a narrow container for pool registration data plus an optional
/// retirement epoch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredPool {
    params: PoolParams,
    retiring_epoch: Option<EpochNo>,
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
        enc.array(2);
        self.params.encode_cbor(enc);
        encode_optional_epoch_no(self.retiring_epoch, enc);
    }
}

impl CborDecode for RegisteredPool {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }

        Ok(Self {
            params: PoolParams::decode_cbor(dec)?,
            retiring_epoch: decode_optional_epoch_no(dec)?,
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
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PoolState {
    entries: BTreeMap<PoolKeyHash, RegisteredPool>,
}

impl CborEncode for PoolState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(self.entries.len() as u64);
        for pool in self.entries.values() {
            pool.encode_cbor(enc);
        }
    }
}

impl CborDecode for PoolState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        let mut entries = BTreeMap::new();
        for _ in 0..len {
            let pool = RegisteredPool::decode_cbor(dec)?;
            entries.insert(pool.params.operator, pool);
        }
        Ok(Self { entries })
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

    /// Inserts or replaces the registration for a pool operator.
    pub fn register(&mut self, params: PoolParams) {
        let operator = params.operator;
        self.entries.insert(
            operator,
            RegisteredPool {
                params,
                retiring_epoch: None,
            },
        );
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
    /// Returns the operator keys of the pools that were retired.
    pub fn process_retirements(&mut self, current_epoch: EpochNo) -> Vec<PoolKeyHash> {
        let retiring: Vec<PoolKeyHash> = self
            .entries
            .iter()
            .filter(|(_, pool)| {
                pool.retiring_epoch.is_some_and(|e| e <= current_epoch)
            })
            .map(|(k, _)| *k)
            .collect();
        for key in &retiring {
            self.entries.remove(key);
        }
        retiring
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
}

impl CborEncode for StakeCredentialState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        encode_optional_pool_key_hash(self.delegated_pool, enc);
        encode_optional_drep(self.delegated_drep.as_ref(), enc);
    }
}

impl CborDecode for StakeCredentialState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }

        Ok(Self {
            delegated_pool: decode_optional_pool_key_hash(dec)?,
            delegated_drep: decode_optional_drep(dec)?,
        })
    }
}

impl StakeCredentialState {
    /// Creates stake-credential state with the given delegation targets.
    pub fn new(delegated_pool: Option<PoolKeyHash>, delegated_drep: Option<DRep>) -> Self {
        Self {
            delegated_pool,
            delegated_drep,
        }
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

    /// Returns true when `credential` is registered.
    pub fn is_registered(&self, credential: &StakeCredential) -> bool {
        self.entries.contains_key(credential)
    }

    /// Registers a stake credential with no delegation target.
    pub fn register(&mut self, credential: StakeCredential) -> bool {
        self.entries
            .insert(credential, StakeCredentialState::new(None, None))
            .is_none()
    }

    /// Removes a registered stake credential.
    pub fn unregister(&mut self, credential: &StakeCredential) -> Option<StakeCredentialState> {
        self.entries.remove(credential)
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
        Self { anchor, deposit, last_active_epoch: None }
    }

    /// Creates registered DRep state with an initial activity epoch.
    pub fn new_active(deposit: u64, anchor: Option<Anchor>, epoch: EpochNo) -> Self {
        Self { anchor, deposit, last_active_epoch: Some(epoch) }
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

    /// Registers a DRep.
    pub fn register(&mut self, drep: DRep, state: RegisteredDrep) -> bool {
        self.entries.insert(drep, state).is_none()
    }

    /// Unregisters a DRep.
    pub fn unregister(&mut self, drep: &DRep) -> Option<RegisteredDrep> {
        self.entries.remove(drep)
    }

    /// Returns the number of registered DReps.
    pub fn len(&self) -> usize {
        self.entries.len()
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
    pub fn inactive_dreps(
        &self,
        epoch: EpochNo,
        drep_activity: u64,
    ) -> Vec<DRep> {
        self.entries
            .iter()
            .filter(|(_, state)| {
                state
                    .last_active_epoch
                    .is_some_and(|e| e.0.saturating_add(drep_activity) < epoch.0)
            })
            .map(|(drep, _)| drep.clone())
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
            0 => Ok(Self::CommitteeHotCredential(StakeCredential::decode_cbor(dec)?)),
            1 => Ok(Self::CommitteeMemberResigned(decode_optional_anchor(dec)?)),
            tag => Err(LedgerError::CborInvalidAdditionalInfo(tag as u8)),
        }
    }
}

/// State for a known constitutional-committee cold credential.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitteeMemberState {
    authorization: Option<CommitteeAuthorization>,
}

impl CborEncode for CommitteeMemberState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self.authorization.as_ref() {
            Some(authorization) => authorization.encode_cbor(enc),
            None => {
                enc.null();
            }
        }
    }
}

impl CborDecode for CommitteeMemberState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let authorization = if dec.peek_major()? == 7 {
            dec.null()?;
            None
        } else {
            Some(CommitteeAuthorization::decode_cbor(dec)?)
        };

        Ok(Self { authorization })
    }
}

impl CommitteeMemberState {
    /// Creates member state with no authorized hot credential.
    pub fn new() -> Self {
        Self::default()
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
    pub fn votes(
        &self,
    ) -> &BTreeMap<crate::eras::conway::Voter, crate::eras::conway::Vote> {
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

    /// Removes a known committee member.
    pub fn unregister(&mut self, credential: &StakeCredential) -> Option<CommitteeMemberState> {
        self.entries.remove(credential)
    }

    /// Returns the number of known committee members.
    pub fn len(&self) -> usize {
        self.entries.len()
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
            prev_pparams_update: None,
            prev_hard_fork: None,
            prev_committee: None,
            prev_constitution: None,
        }
    }
}

impl CborEncode for EnactState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(6);
        self.constitution.encode_cbor(enc);
        self.committee_quorum.encode_cbor(enc);
        encode_optional_gov_action_id(self.prev_pparams_update.as_ref(), enc);
        encode_optional_gov_action_id(self.prev_hard_fork.as_ref(), enc);
        encode_optional_gov_action_id(self.prev_committee.as_ref(), enc);
        encode_optional_gov_action_id(self.prev_constitution.as_ref(), enc);
    }
}

impl CborDecode for EnactState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 6 {
            return Err(LedgerError::CborInvalidLength {
                expected: 6,
                actual: len as usize,
            });
        }
        let constitution = crate::eras::conway::Constitution::decode_cbor(dec)?;
        let committee_quorum = UnitInterval::decode_cbor(dec)?;
        let prev_pparams_update = decode_optional_gov_action_id(dec)?;
        let prev_hard_fork = decode_optional_gov_action_id(dec)?;
        let prev_committee = decode_optional_gov_action_id(dec)?;
        let prev_constitution = decode_optional_gov_action_id(dec)?;
        Ok(Self {
            constitution,
            committee_quorum,
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
            ConwayGovActionPurpose::TreasuryWithdrawals
            | ConwayGovActionPurpose::Info => None,
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
    HardForkEnacted {
        new_version: (u64, u64),
    },
    /// Treasury withdrawals were enacted — lovelace credited to reward
    /// accounts from the treasury.
    TreasuryWithdrawn {
        total_withdrawn: u64,
    },
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
    current_epoch: EpochNo,
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

        GovAction::NewConstitution {
            constitution, ..
        } => {
            enact.constitution = constitution.clone();
            enact.prev_constitution = Some(action_id);
            EnactOutcome::ConstitutionUpdated
        }

        GovAction::NoConfidence { .. } => {
            // Remove all committee members — upstream ENACT removes
            // the entire committee on no-confidence.
            let count = committee.len();
            *committee = CommitteeState::new();
            enact.committee_quorum = UnitInterval {
                numerator: 0,
                denominator: 1,
            };
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
            let max_term_epoch = protocol_params
                .as_ref()
                .and_then(|pp| pp.committee_term_limit)
                .map(|limit| current_epoch.0.saturating_add(limit));

            let mut removed = 0usize;
            for cred in members_to_remove {
                if committee.unregister(cred).is_some() {
                    removed += 1;
                }
            }
            let mut added = 0usize;
            for (cred, term_epoch) in members_to_add {
                if *term_epoch <= current_epoch.0 {
                    continue;
                }
                if max_term_epoch.is_some_and(|max_epoch| *term_epoch > max_epoch) {
                    continue;
                }
                // Register the new member with no hot-key authorization.
                if committee.register(cred.clone()) {
                    added += 1;
                }
            }
            enact.committee_quorum = *quorum;
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

        GovAction::TreasuryWithdrawals {
            withdrawals, ..
        } => {
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
}

impl LedgerStateSnapshot {
    /// Returns the era active at the time this snapshot was captured.
    pub fn current_era(&self) -> Era {
        self.current_era
    }

    /// Returns the chain tip captured in this snapshot.
    pub fn tip(&self) -> &Point {
        &self.tip
    }

    /// Returns the current epoch captured in this snapshot.
    pub fn current_epoch(&self) -> EpochNo {
        self.current_epoch
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

    /// Returns all UTxO entries paying to `address`.
    ///
    /// Entries from the multi-era UTxO set take precedence when the same
    /// `ShelleyTxIn` is visible through both backing stores.
    pub fn query_utxos_by_address(&self, address: &Address) -> Vec<(crate::eras::shelley::ShelleyTxIn, MultiEraTxOut)> {
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
// DepositPot — aggregate deposit tracking
// ---------------------------------------------------------------------------

/// Aggregate deposit accounting tracked by the ledger.
///
/// Tracks the total lovelace locked in key deposits, pool deposits, and DRep
/// deposits.  At epoch boundaries deposit refunds (from unregistrations and
/// pool retirements) are paid out and deducted from this pot.
///
/// Reference: `Cardano.Ledger.Shelley.LedgerState` — `utxosDeposited`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DepositPot {
    /// Total lovelace deposited for key registrations.
    pub key_deposits: u64,
    /// Total lovelace deposited for pool registrations.
    pub pool_deposits: u64,
    /// Total lovelace deposited for DRep registrations (Conway+).
    pub drep_deposits: u64,
}

impl DepositPot {
    /// Returns the total deposits across all categories.
    pub fn total(&self) -> u64 {
        self.key_deposits
            .saturating_add(self.pool_deposits)
            .saturating_add(self.drep_deposits)
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
}

impl CborEncode for DepositPot {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(3);
        enc.unsigned(self.key_deposits);
        enc.unsigned(self.pool_deposits);
        enc.unsigned(self.drep_deposits);
    }
}

impl CborDecode for DepositPot {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            });
        }
        Ok(Self {
            key_deposits: dec.unsigned()?,
            pool_deposits: dec.unsigned()?,
            drep_deposits: dec.unsigned()?,
        })
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
    /// Aggregate deposit accounting.
    deposit_pot: DepositPot,
    /// Treasury and reserves accounting.
    accounting: AccountingState,
    /// Conway governance enactment state (constitution, quorum, lineage).
    enact_state: EnactState,
    /// Shelley genesis UTxO entries to activate when replay first reaches a
    /// Shelley-family block.
    pending_shelley_genesis_utxo: Option<Vec<(crate::eras::shelley::ShelleyTxIn, crate::eras::shelley::ShelleyTxOut)>>,
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
    /// Pending Shelley-era protocol parameter update proposals keyed by
    /// target epoch and genesis delegate key hash.
    ///
    /// Each transaction carrying a `ShelleyUpdate` (CDDL key 6) adds its
    /// per-genesis-hash proposals here.  At the epoch boundary when the
    /// target epoch arrives, proposals that reach a quorum (> 50% of
    /// `gen_delegs`) are merged and applied to `protocol_params`.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Ppup` — PPUP rule.
    pending_pparam_updates: BTreeMap<EpochNo, BTreeMap<GenesisHash, crate::protocol_params::ProtocolParameterUpdate>>,
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
        enc.array(18);
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
            None => { enc.null(); }
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
    }
}

impl CborDecode for LedgerState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        // Accept legacy 9/10-element arrays and current 12-18-element arrays.
        if len != 9 && len != 10 && !(12..=18).contains(&len) {
            return Err(LedgerError::CborInvalidLength {
                expected: 18,
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
                Some(crate::protocol_params::ProtocolParameters::decode_cbor(dec)?)
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
                    arr.copy_from_slice(&bytes);
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
                    arr.copy_from_slice(&bytes);
                    arr
                };
                let vrf: VrfKeyHash = {
                    let bytes = dec.bytes()?;
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
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
                        arr.copy_from_slice(&bytes);
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

        Ok(Self {
            current_era,
            tip,
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
            deposit_pot,
            accounting,
            enact_state,
            gen_delegs,
            pending_pparam_updates,
            pending_shelley_genesis_utxo: None,
            pending_shelley_genesis_stake: None,
            pending_shelley_genesis_delegs: None,
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
            deposit_pot: DepositPot::default(),
            accounting: AccountingState::default(),
            enact_state: EnactState::default(),
            pending_shelley_genesis_utxo: None,
            pending_shelley_genesis_stake: None,
            pending_shelley_genesis_delegs: None,
            gen_delegs: BTreeMap::new(),
            pending_pparam_updates: BTreeMap::new(),
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
        entries: Vec<(crate::eras::shelley::ShelleyTxIn, crate::eras::shelley::ShelleyTxOut)>,
    ) {
        self.pending_shelley_genesis_utxo = if entries.is_empty() {
            None
        } else {
            Some(entries)
        };
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

    /// Returns a reference to pending Shelley-era protocol parameter update
    /// proposals, keyed by target epoch.
    pub fn pending_pparam_updates(
        &self,
    ) -> &BTreeMap<EpochNo, BTreeMap<GenesisHash, crate::protocol_params::ProtocolParameterUpdate>>
    {
        &self.pending_pparam_updates
    }

    /// Collects protocol parameter update proposals from a `ShelleyUpdate`.
    ///
    /// Each proposal is stored under its target epoch and genesis key hash.
    /// Duplicate proposals from the same genesis key for the same epoch
    /// overwrite the earlier entry (last-writer-wins per block ordering).
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
        let mut vote_counts: Vec<(&crate::protocol_params::ProtocolParameterUpdate, usize)> = Vec::new();
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

    /// Returns a mutable reference to the protocol parameters slot.
    pub fn protocol_params_mut(&mut self) -> &mut Option<crate::protocol_params::ProtocolParameters> {
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
    pub fn query_utxos_by_address(&self, address: &Address) -> Vec<(crate::eras::shelley::ShelleyTxIn, MultiEraTxOut)> {
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
    /// [`PlutusEvaluator`]. When `None`, Plutus scripts are silently
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

        // Block-level size validation when protocol parameters are available.
        if let Some(params) = &self.protocol_params {
            let body_size: usize = block.transactions.iter().map(|tx| tx.body.len()).sum();
            if body_size > params.max_block_body_size as usize {
                return Err(LedgerError::BlockTooLarge {
                    actual: body_size,
                    max: params.max_block_body_size as usize,
                });
            }
        }

        match block.era {
            Era::Byron => self.apply_byron_block(block, slot)?,
            Era::Shelley => self.apply_shelley_block(block, slot)?,
            Era::Allegra => self.apply_allegra_block(block, slot)?,
            Era::Mary => self.apply_mary_block(block, slot)?,
            Era::Alonzo => self.apply_alonzo_block(block, slot, evaluator)?,
            Era::Babbage => self.apply_babbage_block(block, slot, evaluator)?,
            Era::Conway => self.apply_conway_block(block, slot, evaluator)?,
        }

        self.current_era = block.era;
        self.tip = Point::BlockPoint(block.header.slot_no, block.header.hash);
        Ok(())
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
        match tx {
            crate::tx::MultiEraSubmittedTx::Shelley(tx) => {
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Shelley(o.clone()))
                        .collect();
                    let tx_size = tx.to_cbor_bytes().len();
                    validate_pre_alonzo_tx(
                        params, tx_size, tx.body.fee, &outputs,
                    )?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
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
                        &tx.body.inputs, &self.shelley_utxo, &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
                    }
                    let tx_body_hash = crate::tx::compute_tx_id(&tx.body.to_cbor_bytes()).0;
                    validate_witnesses_typed(&tx.witness_set, &required, &tx_body_hash)?;
                }
                let mut staged = self.shelley_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                )?;
                staged.apply_tx_with_withdrawals(
                    crate::tx::compute_tx_id(&tx.body.to_cbor_bytes()).0,
                    &tx.body,
                    current_slot.0,
                    withdrawal_total,
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
            }
            crate::tx::MultiEraSubmittedTx::Allegra(tx) => {
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Shelley(o.clone()))
                        .collect();
                    validate_pre_alonzo_tx(
                        params, tx.raw_cbor.len(), tx.body.fee, &outputs,
                    )?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
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
                        &tx.body.inputs, &self.multi_era_utxo, &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
                    }
                    validate_witnesses_typed(&tx.witness_set, &required, &tx.tx_id().0)?;
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
                )?;
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                )?;
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                )?;
                staged.apply_allegra_tx_withdrawals(tx.tx_id().0, &tx.body, current_slot.0, withdrawal_total)?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
            }
            crate::tx::MultiEraSubmittedTx::Mary(tx) => {
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Mary(o.clone()))
                        .collect();
                    validate_pre_alonzo_tx(
                        params, tx.raw_cbor.len(), tx.body.fee, &outputs,
                    )?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
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
                        &tx.body.inputs, &self.multi_era_utxo, &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
                    }
                    validate_witnesses_typed(&tx.witness_set, &required, &tx.tx_id().0)?;
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
                )?;
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                )?;
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                )?;
                staged.apply_mary_tx_withdrawals(tx.tx_id().0, &tx.body, current_slot.0, withdrawal_total)?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
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
                )?;
                let witness_bytes = tx.witness_set.to_cbor_bytes();
                crate::plutus_validation::validate_script_data_hash(
                    tx.body.script_data_hash,
                    Some(&witness_bytes),
                    self.protocol_params.as_ref(),
                    false,
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                        .collect();
                    let total_eu = sum_redeemer_ex_units(&tx.witness_set);
                    let has_redeemers = !tx.witness_set.redeemers.is_empty();
                    validate_alonzo_plus_tx(
                        params, &self.multi_era_utxo,
                        tx.raw_cbor.len(), tx.body.fee, &outputs,
                        tx.body.collateral.as_deref(), total_eu.as_ref(),
                        None, None, has_redeemers,
                    )?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal / WrongNetworkInTxBody)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
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
                        &tx.body.inputs, &self.multi_era_utxo, &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
                    }
                    if let Some(signers) = &tx.body.required_signers {
                        for signer in signers {
                            required.insert(*signer);
                        }
                    }
                    validate_witnesses_typed(&tx.witness_set, &required, &tx.tx_id().0)?;
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
                )?;
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                )?;
                // Unspendable UTxO check (Alonzo — no datum on Plutus-locked input).
                crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                    &staged,
                    &tx.body.inputs,
                    &native_satisfied,
                )?;
                // Supplemental datum check (Alonzo submitted — no reference inputs).
                {
                    let tx_outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
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
                    let sorted_policies: Vec<[u8; 28]> = tx.body.mint.as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx.body.withdrawals.as_ref()
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
                }
                // Phase-2 Plutus script validation (Alonzo submitted).
                // Submitted transactions always have is_valid = true (checked above),
                // so any Phase-2 failure is a hard reject.
                {
                    let mut sorted_inputs = tx.body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = tx.body.mint.as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx.body.withdrawals.as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx.tx_id().0,
                        fee: tx.body.fee,
                        outputs: tx.body.outputs.iter()
                            .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                            .collect(),
                        validity_start: tx.body.validity_interval_start,
                        ttl: tx.body.ttl,
                        required_signers: tx.body.required_signers.clone().unwrap_or_default(),
                        mint: tx.body.mint.clone().unwrap_or_default(),
                        withdrawals: tx.body.withdrawals.clone().unwrap_or_default(),
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator, Some(&witness_bytes), &required_scripts,
                        &staged,
                        &sorted_inputs, &sorted_policies, certs_slice, &sorted_rewards, &[], &[],
                        &tx_ctx,
                    )?;
                }
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                )?;
                staged.apply_alonzo_tx_withdrawals(tx.tx_id().0, &tx.body, current_slot.0, withdrawal_total)?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
            }
            crate::tx::MultiEraSubmittedTx::Babbage(tx) => {
                // Reject submitted transactions with is_valid = false.
                if !tx.is_valid {
                    return Err(LedgerError::SubmittedTxIsInvalid);
                }
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                )?;
                let witness_bytes = tx.witness_set.to_cbor_bytes();
                crate::plutus_validation::validate_script_data_hash(
                    tx.body.script_data_hash,
                    Some(&witness_bytes),
                    self.protocol_params.as_ref(),
                    true,
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    let total_eu = sum_redeemer_ex_units(&tx.witness_set);
                    let has_redeemers = !tx.witness_set.redeemers.is_empty();
                    let coll_ret = tx.body.collateral_return.as_ref().map(|o| MultiEraTxOut::Babbage(o.clone()));
                    validate_alonzo_plus_tx(
                        params, &self.multi_era_utxo,
                        tx.raw_cbor.len(), tx.body.fee, &outputs,
                        tx.body.collateral.as_deref(), total_eu.as_ref(),
                        coll_ret.as_ref(), tx.body.total_collateral, has_redeemers,
                    )?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal / WrongNetworkInTxBody)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
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
                        &tx.body.inputs, &self.multi_era_utxo, &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
                    }
                    if let Some(signers) = &tx.body.required_signers {
                        for signer in signers {
                            required.insert(*signer);
                        }
                    }
                    validate_witnesses_typed(&tx.witness_set, &required, &tx.tx_id().0)?;
                }
                let mut staged = self.multi_era_utxo.clone();
                // Reference input validation (Babbage+ rule)
                if let Some(ref_inputs) = &tx.body.reference_inputs {
                    staged.validate_reference_inputs(ref_inputs)?;
                    MultiEraUtxo::validate_reference_input_disjointness(&tx.body.inputs, ref_inputs)?;
                }
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
                )?;
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                )?;
                // Unspendable UTxO check (Babbage — no datum on Plutus-locked input).
                crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                    &staged,
                    &tx.body.inputs,
                    &native_satisfied,
                )?;
                // Supplemental datum check (Babbage submitted — includes reference inputs).
                {
                    let tx_outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    let ref_utxos: Vec<(ShelleyTxIn, MultiEraTxOut)> = tx.body.reference_inputs
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .filter_map(|txin| staged.get(txin).map(|txout| (txin.clone(), txout.clone())))
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
                    let sorted_policies: Vec<[u8; 28]> = tx.body.mint.as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx.body.withdrawals.as_ref()
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
                }
                // Phase-2 Plutus script validation (Babbage submitted).
                {
                    let mut sorted_inputs = tx.body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = tx.body.mint.as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx.body.withdrawals.as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx.tx_id().0,
                        fee: tx.body.fee,
                        outputs: tx.body.outputs.iter()
                            .map(|o| MultiEraTxOut::Babbage(o.clone()))
                            .collect(),
                        validity_start: tx.body.validity_interval_start,
                        ttl: tx.body.ttl,
                        required_signers: tx.body.required_signers.clone().unwrap_or_default(),
                        mint: tx.body.mint.clone().unwrap_or_default(),
                        withdrawals: tx.body.withdrawals.clone().unwrap_or_default(),
                        reference_inputs: tx.body.reference_inputs.clone().unwrap_or_default(),
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator, Some(&witness_bytes), &required_scripts,
                        &staged,
                        &sorted_inputs, &sorted_policies, certs_slice, &sorted_rewards, &[], &[],
                        &tx_ctx,
                    )?;
                }
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let cert_ctx = self.certificate_validation_context();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                )?;
                staged.apply_babbage_tx_withdrawals(tx.tx_id().0, &tx.body, current_slot.0, withdrawal_total)?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
            }
            crate::tx::MultiEraSubmittedTx::Conway(tx) => {
                // Reject submitted transactions with is_valid = false.
                if !tx.is_valid {
                    return Err(LedgerError::SubmittedTxIsInvalid);
                }
                validate_auxiliary_data(
                    tx.body.auxiliary_data_hash.as_ref(),
                    tx.auxiliary_data.as_deref(),
                )?;
                let witness_bytes = tx.witness_set.to_cbor_bytes();
                crate::plutus_validation::validate_script_data_hash(
                    tx.body.script_data_hash,
                    Some(&witness_bytes),
                    self.protocol_params.as_ref(),
                    true,
                )?;
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    let total_eu = sum_redeemer_ex_units(&tx.witness_set);
                    let has_redeemers = !tx.witness_set.redeemers.is_empty();
                    let coll_ret = tx.body.collateral_return.as_ref().map(|o| MultiEraTxOut::Babbage(o.clone()));
                    validate_alonzo_plus_tx(
                        params, &self.multi_era_utxo,
                        tx.raw_cbor.len(), tx.body.fee, &outputs,
                        tx.body.collateral.as_deref(), total_eu.as_ref(),
                        coll_ret.as_ref(), tx.body.total_collateral, has_redeemers,
                    )?;
                }
                // Network validation (WrongNetwork / WrongNetworkWithdrawal / WrongNetworkInTxBody)
                if let Some(expected_net) = self.expected_network_id {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
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
                        &tx.body.inputs, &self.multi_era_utxo, &mut required,
                    );
                    if let Some(certs) = &tx.body.certificates {
                        for cert in certs {
                            crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                        }
                    }
                    if let Some(withdrawals) = &tx.body.withdrawals {
                        crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
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
                // Reference input validation (Babbage+ rule)
                if let Some(ref_inputs) = &tx.body.reference_inputs {
                    staged.validate_reference_inputs(ref_inputs)?;
                    MultiEraUtxo::validate_reference_input_disjointness(&tx.body.inputs, ref_inputs)?;
                }
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
                )?;
                validate_no_extraneous_script_witnesses_typed(
                    &tx.witness_set,
                    &required_scripts,
                )?;
                // Unspendable UTxO check (Conway — no datum on Plutus-locked input).
                crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                    &staged,
                    &tx.body.inputs,
                    &native_satisfied,
                )?;
                // Supplemental datum check (Conway submitted — includes reference inputs).
                {
                    let tx_outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    let ref_utxos: Vec<(ShelleyTxIn, MultiEraTxOut)> = tx.body.reference_inputs
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .filter_map(|txin| staged.get(txin).map(|txout| (txin.clone(), txout.clone())))
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
                    let sorted_policies: Vec<[u8; 28]> = tx.body.mint.as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx.body.withdrawals.as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let sorted_voters: Vec<crate::eras::conway::Voter> = tx.body.voting_procedures.as_ref()
                        .map(|vp| {
                            let mut vs: Vec<_> = vp.procedures.keys().cloned().collect();
                            vs.sort();
                            vs
                        })
                        .unwrap_or_default();
                    let proposal_slice: Vec<crate::eras::conway::ProposalProcedure> = tx.body.proposal_procedures
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
                }
                // Phase-2 Plutus script validation (Conway submitted).
                {
                    let mut sorted_inputs = tx.body.inputs.clone();
                    sorted_inputs.sort();
                    let sorted_policies: Vec<[u8; 28]> = tx.body.mint.as_ref()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    let certs_slice = tx.body.certificates.as_deref().unwrap_or(&[]);
                    let sorted_rewards: Vec<Vec<u8>> = tx.body.withdrawals.as_ref()
                        .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                        .unwrap_or_default();
                    let sorted_voters: Vec<crate::eras::conway::Voter> = tx.body.voting_procedures.as_ref()
                        .map(|v| v.procedures.keys().cloned().collect())
                        .unwrap_or_default();
                    let proposal_slice = tx.body.proposal_procedures.as_deref().unwrap_or(&[]);
                    let tx_ctx = crate::plutus_validation::TxContext {
                        tx_hash: tx.tx_id().0,
                        fee: tx.body.fee,
                        outputs: tx.body.outputs.iter()
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
                        ..Default::default()
                    };
                    crate::plutus_validation::validate_plutus_scripts(
                        evaluator, Some(&witness_bytes), &required_scripts,
                        &staged,
                        &sorted_inputs, &sorted_policies, certs_slice, &sorted_rewards,
                        &sorted_voters, proposal_slice,
                        &tx_ctx,
                    )?;
                }
                // Conway UTXO rule: validate current_treasury_value declaration.
                let current_treasury = self.accounting.treasury;
                validate_conway_current_treasury_value(tx.body.current_treasury_value, current_treasury)?;

                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let mut staged_gen_delegs = self.gen_delegs.clone();
                let mut staged_governance_actions = self.governance_actions.clone();
                let cert_ctx = self.certificate_validation_context();

                // Conway governance validation (voters, proposals, votes).
                let unregistered_drep_voters = collect_conway_unregistered_drep_voters(
                    tx.body.certificates.as_deref(),
                );

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
                        &cert_ctx,
                        tx.body.certificates.as_deref(),
                    )?;

                    let mut governance_actions_for_tx = staged_governance_actions.clone();

                    if let Some(voting_procedures) = &tx.body.voting_procedures {
                        validate_conway_voters(
                            voting_procedures,
                            &governance_pool_state,
                            &governance_committee_state,
                            &governance_drep_state,
                        )?;
                    }

                    if let Some(proposal_procedures) = &tx.body.proposal_procedures {
                        stage_conway_proposals(
                            tx.tx_id(),
                            self.current_epoch,
                            self.protocol_params
                                .as_ref()
                                .and_then(|params| params.gov_action_lifetime),
                            proposal_procedures,
                            &mut governance_actions_for_tx,
                        );
                        validate_conway_proposals(
                            tx.tx_id(),
                            proposal_procedures,
                            self.current_epoch,
                            &governance_actions_for_tx,
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
                        )?;
                    }

                    if let Some(voting_procedures) = &tx.body.voting_procedures {
                        validate_conway_vote_targets(voting_procedures, &governance_actions_for_tx)?;
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
                        apply_conway_votes(voting_procedures, &mut staged_governance_actions, &mut staged_drep_state, self.current_epoch);
                    }
                    remove_conway_drep_votes(
                        &unregistered_drep_voters,
                        &mut staged_governance_actions,
                    );
                }

                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    &mut staged_gen_delegs,
                    &cert_ctx,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                )?;
                // Track DRep activity for registration and update certificates.
                touch_drep_activity_for_certs(
                    tx.body.certificates.as_deref(),
                    &mut staged_drep_state,
                    self.current_epoch,
                );
                staged.apply_conway_tx_withdrawals(tx.tx_id().0, &tx.body, current_slot.0, withdrawal_total)?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
                self.gen_delegs = staged_gen_delegs;
                self.governance_actions = staged_governance_actions;
            }
        }

        Ok(())
    }

    // -- Private helpers ------------------------------------------------------

    /// Builds the context needed for certificate validation from the
    /// current protocol parameters and ledger state.
    fn certificate_validation_context(&self) -> CertificateValidationContext {
        match &self.protocol_params {
            Some(p) => CertificateValidationContext {
                key_deposit: p.key_deposit,
                pool_deposit: p.pool_deposit,
                min_pool_cost: p.min_pool_cost,
                e_max: p.e_max,
                current_epoch: self.current_epoch,
                expected_network_id: self.expected_network_id,
            },
            None => CertificateValidationContext {
                key_deposit: 0,
                pool_deposit: 0,
                min_pool_cost: 0,
                e_max: u64::MAX,
                current_epoch: self.current_epoch,
                expected_network_id: self.expected_network_id,
            },
        }
    }

    fn maybe_activate_pending_shelley_genesis(&mut self, next_era: Era) {
        if self.current_era != Era::Byron || next_era == Era::Byron {
            return;
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
                        self.stake_credentials
                            .entries
                            .insert(credential, StakeCredentialState::new(Some(pool), None));
                    }
                }
            }
        }

        if let Some(entries) = deleg_entries {
            self.gen_delegs = entries;
        }
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
        let mut staged = self.multi_era_utxo.clone();
        for byron_tx in &decoded {
            staged.apply_byron_tx(byron_tx)?;
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

        let decoded: Vec<(crate::types::TxId, usize, ShelleyTxBody, Option<Vec<u8>>, Option<Vec<u8>>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ShelleyTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone(), tx.auxiliary_data.clone()))
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
        let cert_ctx = self.certificate_validation_context();
        for (tx_id, tx_size, body, witness_bytes, aux_data) in &decoded {
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
            )?;
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
            }
            // Network validation (Shelley UTXO rule: WrongNetwork / WrongNetworkWithdrawal)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
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
                &body.inputs, &staged, &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            staged.apply_tx_with_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
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
        // Collect protocol parameter update proposals (PPUP rule).
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data) in &decoded {
            if let Some(ref update) = body.update {
                self.collect_pparam_proposals(update);
            }
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

        let decoded: Vec<(crate::types::TxId, usize, AllegraTxBody, Option<Vec<u8>>, Option<Vec<u8>>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AllegraTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone(), tx.auxiliary_data.clone()))
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
        let cert_ctx = self.certificate_validation_context();
        for (tx_id, tx_size, body, witness_bytes, aux_data) in &decoded {
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
            )?;
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
            }
            // Network validation (Allegra UTXO rule)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
            }
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                &body.inputs, &staged, &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // Native script validation (Allegra+)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs, &staged, &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(withdrawals, &mut required_scripts);
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
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
            )?;
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            staged.apply_allegra_tx_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        self.gen_delegs = staged_gen_delegs;
        // Collect protocol parameter update proposals (PPUP rule).
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data) in &decoded {
            if let Some(ref update) = body.update {
                self.collect_pparam_proposals(update);
            }
        }
        Ok(())
    }

    fn apply_mary_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, usize, crate::eras::mary::MaryTxBody, Option<Vec<u8>>, Option<Vec<u8>>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = crate::eras::mary::MaryTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone(), tx.auxiliary_data.clone()))
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
        let cert_ctx = self.certificate_validation_context();
        for (tx_id, tx_size, body, witness_bytes, aux_data) in &decoded {
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
            )?;
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Mary(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
            }
            // Network validation (Mary UTXO rule)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Mary(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
            }
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                &body.inputs, &staged, &mut required,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_vkey_hashes_from_cert(cert, &mut required);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_vkey_hashes_from_withdrawals(withdrawals, &mut required);
            }
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // Native script validation (Mary)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs, &staged, &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(withdrawals, &mut required_scripts);
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
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
            )?;
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            staged.apply_mary_tx_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        self.gen_delegs = staged_gen_delegs;
        // Collect protocol parameter update proposals (PPUP rule).
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data) in &decoded {
            if let Some(ref update) = body.update {
                self.collect_pparam_proposals(update);
            }
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

        let decoded: Vec<(crate::types::TxId, usize, AlonzoTxBody, Option<Vec<u8>>, Option<Vec<u8>>, Option<bool>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AlonzoTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone(), tx.auxiliary_data.clone(), tx.is_valid))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // BBODY rule: block-level ExUnits limit.
        {
            let wb_refs: Vec<Option<&[u8]>> = decoded.iter()
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
        let cert_ctx = self.certificate_validation_context();
        for (tx_id, tx_size, body, witness_bytes, aux_data, is_valid) in &decoded {
            let tx_is_valid = is_valid.unwrap_or(true);
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
            )?;
            crate::plutus_validation::validate_script_data_hash(
                body.script_data_hash,
                witness_bytes.as_deref(),
                self.protocol_params.as_ref(),
                false,
            )?;
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                    .collect();
                validate_alonzo_plus_tx(
                    params, &staged, *tx_size, body.fee, &outputs,
                    body.collateral.as_deref(), total_eu.as_ref(),
                    None, None, total_eu.is_some(),
                )?;
            }
            // Network validation (Alonzo UTXO rule: WrongNetwork + WrongNetworkInTxBody)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
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
                &body.inputs, &staged, &mut required,
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
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // Native script validation (Alonzo)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs, &staged, &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(withdrawals, &mut required_scripts);
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
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
            )?;
            // Unspendable UTxO check (Alonzo block — no datum on Plutus-locked input).
            crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                &staged,
                &body.inputs,
                &native_satisfied,
            )?;
            // Supplemental datum check (Alonzo — no reference inputs).
            {
                let tx_outputs: Vec<MultiEraTxOut> = body.outputs.iter()
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
            // ── is_valid bifurcation (Phase-2 / collateral-only) ──
            let run_phase2 = || -> Result<(), LedgerError> {
            // Plutus script validation (Alonzo)
            {
                let mut sorted_inputs = body.inputs.clone();
                sorted_inputs.sort();
                let sorted_policies: Vec<[u8; 28]> = body.mint.as_ref()
                    .map(|m| m.keys().copied().collect())
                    .unwrap_or_default();
                let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                let sorted_rewards: Vec<Vec<u8>> = body.withdrawals.as_ref()
                    .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                    .unwrap_or_default();
                let tx_ctx = crate::plutus_validation::TxContext {
                    tx_hash: tx_id.0,
                    fee: body.fee,
                    outputs: body.outputs.iter()
                        .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                        .collect(),
                    validity_start: body.validity_interval_start,
                    ttl: body.ttl,
                    required_signers: body.required_signers.clone().unwrap_or_default(),
                    mint: body.mint.clone().unwrap_or_default(),
                    withdrawals: body.withdrawals.clone().unwrap_or_default(),
                    ..Default::default()
                };
                crate::plutus_validation::validate_plutus_scripts(
                    evaluator, witness_bytes.as_deref(), &required_scripts,
                    &staged,
                    &sorted_inputs, &sorted_policies, certs_slice, &sorted_rewards, &[], &[],
                    &tx_ctx,
                )
            }
            };
            if tx_is_valid {
                match run_phase2() {
                    Ok(()) => {}
                    Err(LedgerError::PlutusScriptFailed { .. }) if evaluator.is_some() => {
                        return Err(LedgerError::ValidationTagMismatch {
                            claimed: true,
                            actual: false,
                        });
                    }
                    Err(e) => return Err(e),
                }
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            staged.apply_alonzo_tx_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
            } else {
                if evaluator.is_some() {
                    match run_phase2() {
                        Ok(()) => {
                            return Err(LedgerError::ValidationTagMismatch {
                                claimed: false,
                                actual: true,
                            });
                        }
                        Err(LedgerError::PlutusScriptFailed { .. }) => {}
                        Err(e) => return Err(e),
                    }
                }
                // is_valid = false: collateral-only transition.
                // Alonzo has no collateral_return, so only consume collateral inputs.
                crate::utxo::apply_collateral_only(
                    &mut staged, tx_id.0,
                    body.collateral.as_deref(), None,
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
        // Collect protocol parameter update proposals (PPUP rule).
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data, _is_valid) in &decoded {
            if let Some(ref update) = body.update {
                self.collect_pparam_proposals(update);
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

        let decoded: Vec<(crate::types::TxId, usize, BabbageTxBody, Option<Vec<u8>>, Option<Vec<u8>>, Option<bool>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = BabbageTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone(), tx.auxiliary_data.clone(), tx.is_valid))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // BBODY rule: block-level ExUnits limit.
        {
            let wb_refs: Vec<Option<&[u8]>> = decoded.iter()
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
        let cert_ctx = self.certificate_validation_context();
        for (tx_id, tx_size, body, witness_bytes, aux_data, is_valid) in &decoded {
            let tx_is_valid = is_valid.unwrap_or(true);
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
            )?;
            crate::plutus_validation::validate_script_data_hash(
                body.script_data_hash,
                witness_bytes.as_deref(),
                self.protocol_params.as_ref(),
                true,
            )?;
            if let Some(ref_inputs) = &body.reference_inputs {
                staged.validate_reference_inputs(ref_inputs)?;
                MultiEraUtxo::validate_reference_input_disjointness(&body.inputs, ref_inputs)?;
            }
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                let coll_ret = body.collateral_return.as_ref().map(|o| MultiEraTxOut::Babbage(o.clone()));
                validate_alonzo_plus_tx(
                    params, &staged, *tx_size, body.fee, &outputs,
                    body.collateral.as_deref(), total_eu.as_ref(),
                    coll_ret.as_ref(), body.total_collateral, total_eu.is_some(),
                )?;
            }
            // Network validation (Babbage UTXO rule: WrongNetwork + WrongNetworkInTxBody)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
                validate_tx_body_network_id(expected_net, body.network_id)?;
            }
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                &body.inputs, &staged, &mut required,
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
            validate_witnesses_if_present(witness_bytes.as_deref(), &required, &tx_id.0)?;
            // Native script validation (Babbage)
            let mut required_scripts = HashSet::new();
            crate::witnesses::required_script_hashes_from_inputs_multi_era(
                &body.inputs, &staged, &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(withdrawals, &mut required_scripts);
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
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
            )?;
            // Unspendable UTxO check (Babbage block — no datum on Plutus-locked input).
            crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                &staged,
                &body.inputs,
                &native_satisfied,
            )?;
            // Supplemental datum check (Babbage — includes reference inputs).
            {
                let tx_outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                let ref_utxos: Vec<(ShelleyTxIn, MultiEraTxOut)> = body.reference_inputs
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
            let run_phase2 = || -> Result<(), LedgerError> {
            // Plutus script validation (Babbage)
            {
                let mut sorted_inputs = body.inputs.clone();
                sorted_inputs.sort();
                let sorted_policies: Vec<[u8; 28]> = body.mint.as_ref()
                    .map(|m| m.keys().copied().collect())
                    .unwrap_or_default();
                let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                let sorted_rewards: Vec<Vec<u8>> = body.withdrawals.as_ref()
                    .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                    .unwrap_or_default();
                let tx_ctx = crate::plutus_validation::TxContext {
                    tx_hash: tx_id.0,
                    fee: body.fee,
                    outputs: body.outputs.iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect(),
                    validity_start: body.validity_interval_start,
                    ttl: body.ttl,
                    required_signers: body.required_signers.clone().unwrap_or_default(),
                    mint: body.mint.clone().unwrap_or_default(),
                    withdrawals: body.withdrawals.clone().unwrap_or_default(),
                    reference_inputs: body.reference_inputs.clone().unwrap_or_default(),
                    ..Default::default()
                };
                crate::plutus_validation::validate_plutus_scripts(
                    evaluator, witness_bytes.as_deref(), &required_scripts,
                    &staged,
                    &sorted_inputs, &sorted_policies, certs_slice, &sorted_rewards, &[], &[],
                    &tx_ctx,
                )
            }
            };
            if tx_is_valid {
                match run_phase2() {
                    Ok(()) => {}
                    Err(LedgerError::PlutusScriptFailed { .. }) if evaluator.is_some() => {
                        return Err(LedgerError::ValidationTagMismatch {
                            claimed: true,
                            actual: false,
                        });
                    }
                    Err(e) => return Err(e),
                }
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            staged.apply_babbage_tx_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
            } else {
                if evaluator.is_some() {
                    match run_phase2() {
                        Ok(()) => {
                            return Err(LedgerError::ValidationTagMismatch {
                                claimed: false,
                                actual: true,
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
        // Collect protocol parameter update proposals (PPUP rule).
        for (_tx_id, _tx_size, body, _witness_bytes, _aux_data, _is_valid) in &decoded {
            if let Some(ref update) = body.update {
                self.collect_pparam_proposals(update);
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

        let decoded: Vec<(crate::types::TxId, usize, ConwayTxBody, Option<Vec<u8>>, Option<Vec<u8>>, Option<bool>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ConwayTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone(), tx.auxiliary_data.clone(), tx.is_valid))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        // BBODY rule: block-level ExUnits limit.
        {
            let wb_refs: Vec<Option<&[u8]>> = decoded.iter()
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
        let mut staged_governance_actions = self.governance_actions.clone();
        let current_treasury = self.accounting.treasury;
        let cert_ctx = self.certificate_validation_context();
        for (tx_id, tx_size, body, witness_bytes, aux_data, is_valid) in &decoded {
            let tx_is_valid = is_valid.unwrap_or(true);
            validate_auxiliary_data(
                body.auxiliary_data_hash.as_ref(),
                aux_data.as_deref(),
            )?;
            crate::plutus_validation::validate_script_data_hash(
                body.script_data_hash,
                witness_bytes.as_deref(),
                self.protocol_params.as_ref(),
                true,
            )?;
            if let Some(ref_inputs) = &body.reference_inputs {
                staged.validate_reference_inputs(ref_inputs)?;
                MultiEraUtxo::validate_reference_input_disjointness(&body.inputs, ref_inputs)?;
            }
            // Conway LEDGER rule: total reference script size limit
            staged.validate_tx_ref_scripts_size(
                &body.inputs,
                body.reference_inputs.as_deref(),
            )?;
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                let coll_ret = body.collateral_return.as_ref().map(|o| MultiEraTxOut::Babbage(o.clone()));
                validate_alonzo_plus_tx(
                    params, &staged, *tx_size, body.fee, &outputs,
                    body.collateral.as_deref(), total_eu.as_ref(),
                    coll_ret.as_ref(), body.total_collateral, total_eu.is_some(),
                )?;
            }
            // Network validation (Conway UTXO rule: WrongNetwork + WrongNetworkInTxBody)
            if let Some(expected_net) = self.expected_network_id {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                validate_output_network_ids(expected_net, &outputs)?;
                if let Some(withdrawals) = &body.withdrawals {
                    validate_withdrawal_network_ids(expected_net, withdrawals)?;
                }
                validate_tx_body_network_id(expected_net, body.network_id)?;
            }
            validate_conway_current_treasury_value(body.current_treasury_value, current_treasury)?;
            let mut required = HashSet::new();
            crate::witnesses::required_vkey_hashes_from_inputs_multi_era(
                &body.inputs, &staged, &mut required,
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
                &body.inputs, &staged, &mut required_scripts,
            );
            if let Some(certs) = &body.certificates {
                for cert in certs {
                    crate::witnesses::required_script_hashes_from_cert(cert, &mut required_scripts);
                }
            }
            if let Some(withdrawals) = &body.withdrawals {
                crate::witnesses::required_script_hashes_from_withdrawals(withdrawals, &mut required_scripts);
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
            )?;
            validate_no_extraneous_script_witnesses(
                witness_bytes.as_deref(),
                &required_scripts,
            )?;
            // Unspendable UTxO check (Conway block — no datum on Plutus-locked input).
            crate::plutus_validation::validate_unspendable_utxo_no_datum_hash(
                &staged,
                &body.inputs,
                &native_satisfied,
            )?;
            // Supplemental datum check (Conway — includes reference inputs).
            {
                let tx_outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                let ref_utxos: Vec<(ShelleyTxIn, MultiEraTxOut)> = body.reference_inputs
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
            let run_phase2 = || -> Result<(), LedgerError> {
            // Plutus script validation (Conway)
            {
                let mut sorted_inputs = body.inputs.clone();
                sorted_inputs.sort();
                let sorted_policies: Vec<[u8; 28]> = body.mint.as_ref()
                    .map(|m| m.keys().copied().collect())
                    .unwrap_or_default();
                let certs_slice = body.certificates.as_deref().unwrap_or(&[]);
                let sorted_rewards: Vec<Vec<u8>> = body.withdrawals.as_ref()
                    .map(|w| w.keys().map(|ra| ra.to_bytes().to_vec()).collect())
                    .unwrap_or_default();
                let sorted_voters: Vec<crate::eras::conway::Voter> = body.voting_procedures.as_ref()
                    .map(|v| v.procedures.keys().cloned().collect())
                    .unwrap_or_default();
                let proposal_slice = body.proposal_procedures.as_deref().unwrap_or(&[]);
                let tx_ctx = crate::plutus_validation::TxContext {
                    tx_hash: tx_id.0,
                    fee: body.fee,
                    outputs: body.outputs.iter()
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
                    ..Default::default()
                };
                crate::plutus_validation::validate_plutus_scripts(
                    evaluator, witness_bytes.as_deref(), &required_scripts,
                    &staged,
                    &sorted_inputs, &sorted_policies, certs_slice, &sorted_rewards,
                    &sorted_voters, proposal_slice,
                    &tx_ctx,
                )
            }
            };
            if tx_is_valid {
                match run_phase2() {
                    Ok(()) => {}
                    Err(LedgerError::PlutusScriptFailed { .. }) if evaluator.is_some() => {
                        return Err(LedgerError::ValidationTagMismatch {
                            claimed: true,
                            actual: false,
                        });
                    }
                    Err(e) => return Err(e),
                }
            let unregistered_drep_voters = collect_conway_unregistered_drep_voters(
                body.certificates.as_deref(),
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
                    &cert_ctx,
                    body.certificates.as_deref(),
                )?;

                let mut governance_actions_for_tx = staged_governance_actions.clone();

                if let Some(voting_procedures) = &body.voting_procedures {
                    validate_conway_voters(
                        voting_procedures,
                        &governance_pool_state,
                        &governance_committee_state,
                        &governance_drep_state,
                    )?;
                }

                if let Some(proposal_procedures) = &body.proposal_procedures {
                    stage_conway_proposals(
                        *tx_id,
                        self.current_epoch,
                        self.protocol_params
                            .as_ref()
                            .and_then(|params| params.gov_action_lifetime),
                        proposal_procedures,
                        &mut governance_actions_for_tx,
                    );
                    validate_conway_proposals(
                        *tx_id,
                        proposal_procedures,
                        self.current_epoch,
                        &governance_actions_for_tx,
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
                    )?;
                }

                if let Some(voting_procedures) = &body.voting_procedures {
                    validate_conway_vote_targets(voting_procedures, &governance_actions_for_tx)?;
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
                    apply_conway_votes(voting_procedures, &mut staged_governance_actions, &mut staged_drep_state, self.current_epoch);
                }
                remove_conway_drep_votes(
                    &unregistered_drep_voters,
                    &mut staged_governance_actions,
                );
            }
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                &mut staged_gen_delegs,
                &cert_ctx,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            // Track DRep activity for registration and update certificates.
            touch_drep_activity_for_certs(
                body.certificates.as_deref(),
                &mut staged_drep_state,
                self.current_epoch,
            );
            staged.apply_conway_tx_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
            } else {
                if evaluator.is_some() {
                    match run_phase2() {
                        Ok(()) => {
                            return Err(LedgerError::ValidationTagMismatch {
                                claimed: false,
                                actual: true,
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
        self.governance_actions = staged_governance_actions;
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

    apply_certificates_and_withdrawals(
        &mut simulated_pool_state,
        &mut simulated_stake_credentials,
        &mut simulated_committee_state,
        &mut simulated_drep_state,
        &mut simulated_reward_accounts,
        &mut simulated_deposit_pot,
        &mut simulated_gen_delegs,
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
        | crate::eras::conway::Voter::CommitteeScript(_) => {
            !matches!(
                gov_action,
                crate::eras::conway::GovAction::NoConfidence { .. }
                    | crate::eras::conway::GovAction::UpdateCommittee { .. }
            )
        }
        crate::eras::conway::Voter::DRepKeyHash(_)
        | crate::eras::conway::Voter::DRepScript(_) => true,
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

    // Economic + Security
    if update.min_fee_a.is_some() || update.min_fee_b.is_some() {
        groups.economic = true;
        groups.security = true;
    }

    // Network + Security
    if update.max_block_body_size.is_some()
        || update.max_tx_size.is_some()
        || update.max_block_header_size.is_some()
        || update.max_block_ex_units.is_some()
        || update.max_val_size.is_some()
    {
        groups.network = true;
        groups.security = true;
    }

    // Economic (no SPO)
    if update.key_deposit.is_some()
        || update.pool_deposit.is_some()
        || update.rho.is_some()
        || update.tau.is_some()
        || update.min_pool_cost.is_some()
        || update.coins_per_utxo_byte.is_some()
        || update.price_mem.is_some()
        || update.price_step.is_some()
        || update.min_utxo_value.is_some()
    {
        groups.economic = true;
    }

    // Technical (no SPO)
    if update.e_max.is_some()
        || update.n_opt.is_some()
        || update.a0.is_some()
        || update.max_tx_ex_units.is_some()
        || update.collateral_percentage.is_some()
        || update.max_collateral_inputs.is_some()
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

fn conway_bootstrap_phase(protocol_version: Option<(u64, u64)>) -> bool {
    matches!(protocol_version, Some((9, _)))
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
        crate::eras::conway::Voter::DRepKeyHash(_)
        | crate::eras::conway::Voter::DRepScript(_) => {
            matches!(gov_action, crate::eras::conway::GovAction::InfoAction)
        }
        crate::eras::conway::Voter::CommitteeKeyHash(_)
        | crate::eras::conway::Voter::CommitteeScript(_)
        | crate::eras::conway::Voter::StakePool(_) => conway_bootstrap_action(gov_action),
    }
}

fn conway_pv_can_follow(previous: (u64, u64), new: (u64, u64)) -> bool {
    (previous.0, previous.1.saturating_add(1)) == new
        || previous
            .0
            .checked_add(1)
            .is_some_and(|next_major| (next_major, 0) == new)
}

fn conway_expected_previous_hard_fork_version(
    proposal: &crate::eras::conway::ProposalProcedure,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    current_protocol_version: Option<(u64, u64)>,
) -> Option<(Option<crate::eras::conway::GovActionId>, (u64, u64), (u64, u64))> {
    use crate::eras::conway::GovAction;

    match &proposal.gov_action {
        GovAction::HardForkInitiation {
            prev_action_id,
            protocol_version,
        } => {
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

fn stage_conway_proposals(
    tx_id: crate::types::TxId,
    current_epoch: EpochNo,
    gov_action_lifetime: Option<u64>,
    proposal_procedures: &[crate::eras::conway::ProposalProcedure],
    governance_actions: &mut BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
) {
    for (index, proposal) in proposal_procedures.iter().enumerate() {
        governance_actions.insert(
            crate::eras::conway::GovActionId {
                transaction_id: tx_id.0,
                gov_action_index: index as u16,
            },
            GovernanceActionState::new_with_lifetime(
                proposal.clone(),
                current_epoch,
                gov_action_lifetime,
            ),
        );
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

fn apply_conway_votes(
    voting_procedures: &crate::eras::conway::VotingProcedures,
    governance_actions: &mut BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    drep_state: &mut DrepState,
    current_epoch: EpochNo,
) {
    for (voter, votes) in &voting_procedures.procedures {
        for (gov_action_id, voting_procedure) in votes {
            if let Some(action_state) = governance_actions.get_mut(gov_action_id) {
                action_state.votes.insert(voter.clone(), voting_procedure.vote);
            }
        }
        // Mark DRep as active in the current epoch when it casts any vote.
        if let Some(drep) = voter_to_drep(voter) {
            if let Some(entry) = drep_state.get_mut(&drep) {
                entry.touch_activity(current_epoch);
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
                StakeCredential::AddrKeyHash(hash) => crate::eras::conway::Voter::DRepKeyHash(*hash),
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
        Voter::CommitteeKeyHash(hash) => committee_hot_credential_exists(
            committee_state,
            StakeCredential::AddrKeyHash(*hash),
        ),
        Voter::CommitteeScript(hash) => committee_hot_credential_exists(
            committee_state,
            StakeCredential::ScriptHash(*hash),
        ),
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

fn conway_unit_interval_well_formed(value: &UnitInterval) -> bool {
    value.denominator != 0 && value.numerator <= value.denominator
}

fn conway_protocol_param_update_well_formed(
    update: &crate::protocol_params::ProtocolParameterUpdate,
    protocol_params: Option<&crate::protocol_params::ProtocolParameters>,
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

    if update.max_block_body_size == Some(0)
        || update.max_tx_size == Some(0)
        || update.max_block_header_size == Some(0)
        || update.max_val_size == Some(0)
        || update.max_collateral_inputs == Some(0)
        || update.collateral_percentage == Some(0)
        || update.pool_deposit == Some(0)
        || update.gov_action_deposit == Some(0)
        || update.drep_deposit == Some(0)
        || update.min_committee_size == Some(0)
        || update.committee_term_limit == Some(0)
        || update.gov_action_lifetime == Some(0)
        || update.drep_activity == Some(0)
    {
        return false;
    }

    let effective_max_block_body_size = update
        .max_block_body_size
        .or_else(|| protocol_params.map(|params| params.max_block_body_size));
    let effective_max_tx_size = update
        .max_tx_size
        .or_else(|| protocol_params.map(|params| params.max_tx_size));

    if effective_max_block_body_size == Some(0) || effective_max_tx_size == Some(0) {
        return false;
    }

    if let (Some(max_tx_size), Some(max_block_body_size)) =
        (effective_max_tx_size, effective_max_block_body_size)
    {
        if max_tx_size > max_block_body_size {
            return false;
        }
    }

    true
}

fn validate_conway_proposals(
    tx_id: crate::types::TxId,
    proposal_procedures: &[crate::eras::conway::ProposalProcedure],
    current_epoch: EpochNo,
    governance_actions: &BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    stake_credentials: &StakeCredentials,
    protocol_version: Option<(u64, u64)>,
    gov_action_deposit: Option<u64>,
    expected_network_id: Option<u8>,
    protocol_params: Option<&crate::protocol_params::ProtocolParameters>,
    enact_state: &EnactState,
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

            if !conway_protocol_param_update_well_formed(protocol_param_update, protocol_params) {
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
                ConwayGovActionPurpose::TreasuryWithdrawals
                | ConwayGovActionPurpose::Info => { /* no lineage */ }
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

        let reward_account = RewardAccount::from_bytes(&proposal.reward_account)
            .ok_or_else(|| LedgerError::InvalidRewardAccountBytes(proposal.reward_account.clone()))?;
        if let Some(expected_network) = expected_network_id {
            if reward_account.network != expected_network {
                return Err(LedgerError::ProposalProcedureNetworkIdMismatch {
                    account: reward_account,
                    expected_network,
                });
            }
        }
        if !stake_credentials.is_registered(&reward_account.credential) {
            return Err(LedgerError::RewardAccountNotRegistered(reward_account));
        }

        if let GovAction::TreasuryWithdrawals { withdrawals, .. } = &proposal.gov_action {
            for reward_account in withdrawals.keys() {
                if let Some(expected_network) = expected_network_id {
                    if reward_account.network != expected_network {
                        return Err(LedgerError::TreasuryWithdrawalsNetworkIdMismatch {
                            account: *reward_account,
                            expected_network,
                        });
                    }
                }
                if !stake_credentials.is_registered(&reward_account.credential) {
                    return Err(LedgerError::RewardAccountNotRegistered(*reward_account));
                }
            }

            if withdrawals.values().all(|amount| *amount == 0) {
                return Err(LedgerError::ZeroTreasuryWithdrawals(
                    proposal.gov_action.clone(),
                ));
            }
        }

        if let GovAction::UpdateCommittee {
            members_to_remove,
            members_to_add,
            ..
        } = &proposal.gov_action
        {
            let conflicting_members: Vec<_> = members_to_add
                .keys()
                .copied()
                .filter(|member| members_to_remove.contains(member))
                .collect();
            if !conflicting_members.is_empty() {
                return Err(LedgerError::ConflictingCommitteeUpdate(
                    conflicting_members,
                ));
            }

            let invalid_members: Vec<_> = members_to_add
                .iter()
                .filter(|(_, epoch)| **epoch <= current_epoch.0)
                .map(|(member, epoch)| (*member, EpochNo(*epoch)))
                .collect();
            if !invalid_members.is_empty() {
                return Err(LedgerError::ExpirationEpochTooSmall(invalid_members));
            }

            if let Some(term_limit) = protocol_params.and_then(|pp| pp.committee_term_limit) {
                let max_epoch = EpochNo(current_epoch.0.saturating_add(term_limit));
                let invalid_members: Vec<_> = members_to_add
                    .iter()
                    .filter(|(_, epoch)| **epoch > max_epoch.0)
                    .map(|(member, epoch)| (*member, EpochNo(*epoch)))
                    .collect();
                if !invalid_members.is_empty() {
                    return Err(LedgerError::ExpirationEpochTooLarge {
                        members: invalid_members,
                        max_epoch,
                    });
                }
            }
        }
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

/// Context for certificate validation, bundling protocol parameters and
/// ledger state needed during `apply_certificates_and_withdrawals`.
struct CertificateValidationContext {
    key_deposit: u64,
    pool_deposit: u64,
    min_pool_cost: u64,
    e_max: u64,
    current_epoch: EpochNo,
    expected_network_id: Option<u8>,
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
    ctx: &CertificateValidationContext,
    certificates: Option<&[DCert]>,
    withdrawals: Option<&BTreeMap<RewardAccount, u64>>,
) -> Result<u64, LedgerError> {
    let key_deposit = ctx.key_deposit;
    let pool_deposit = ctx.pool_deposit;
    if let Some(certs) = certificates {
        for cert in certs {
            match cert {
                DCert::AccountRegistration(credential) => {
                    register_stake_credential(stake_credentials, *credential)?;
                    deposit_pot.add_key_deposit(key_deposit);
                }
                DCert::AccountRegistrationDeposit(credential, deposit) => {
                    register_stake_credential(stake_credentials, *credential)?;
                    deposit_pot.add_key_deposit(*deposit);
                }
                DCert::AccountUnregistration(credential) => {
                    unregister_stake_credential(stake_credentials, reward_accounts, *credential)?;
                    deposit_pot.return_key_deposit(key_deposit);
                }
                DCert::AccountUnregistrationDeposit(credential, refund) => {
                    unregister_stake_credential(stake_credentials, reward_accounts, *credential)?;
                    deposit_pot.return_key_deposit(*refund);
                }
                DCert::DelegationToStakePool(credential, pool) => {
                    delegate_stake_credential(
                        pool_state,
                        stake_credentials,
                        reward_accounts,
                        *credential,
                        *pool,
                    )?;
                }
                DCert::AccountRegistrationDelegationToStakePool(credential, pool, deposit) => {
                    register_stake_credential(stake_credentials, *credential)?;
                    deposit_pot.add_key_deposit(*deposit);
                    delegate_stake_credential(
                        pool_state,
                        stake_credentials,
                        reward_accounts,
                        *credential,
                        *pool,
                    )?;
                }
                DCert::DelegationToDrep(credential, drep) => {
                    delegate_drep(stake_credentials, drep_state, *credential, *drep)?;
                }
                DCert::DelegationToStakePoolAndDrep(credential, pool, drep) => {
                    delegate_stake_credential(
                        pool_state,
                        stake_credentials,
                        reward_accounts,
                        *credential,
                        *pool,
                    )?;
                    delegate_drep(stake_credentials, drep_state, *credential, *drep)?;
                }
                DCert::AccountRegistrationDelegationToDrep(credential, drep, deposit) => {
                    register_stake_credential(stake_credentials, *credential)?;
                    deposit_pot.add_key_deposit(*deposit);
                    delegate_drep(stake_credentials, drep_state, *credential, *drep)?;
                }
                DCert::AccountRegistrationDelegationToStakePoolAndDrep(credential, pool, drep, deposit) => {
                    register_stake_credential(stake_credentials, *credential)?;
                    deposit_pot.add_key_deposit(*deposit);
                    delegate_stake_credential(
                        pool_state,
                        stake_credentials,
                        reward_accounts,
                        *credential,
                        *pool,
                    )?;
                    delegate_drep(stake_credentials, drep_state, *credential, *drep)?;
                }
                DCert::CommitteeAuthorization(cold_credential, hot_credential) => {
                    authorize_committee_hot_credential(
                        committee_state,
                        *cold_credential,
                        *hot_credential,
                    )?;
                }
                DCert::CommitteeResignation(cold_credential, anchor) => {
                    resign_committee_cold_credential(
                        committee_state,
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
                    let is_new = !pool_state.is_registered(&params.operator);
                    pool_state.register(params.clone());
                    if is_new {
                        deposit_pot.add_pool_deposit(pool_deposit);
                    }
                }
                DCert::PoolRetirement(pool, epoch) => {
                    if !pool_state.retire(*pool, *epoch) {
                        return Err(LedgerError::PoolNotRegistered(*pool));
                    }
                    // POOL rule: retirement epoch must be within eMax of current epoch.
                    let max_epoch = ctx.current_epoch.0.saturating_add(ctx.e_max);
                    if epoch.0 > max_epoch {
                        return Err(LedgerError::PoolRetirementTooFar {
                            retirement_epoch: epoch.0,
                            current_epoch: ctx.current_epoch.0,
                            e_max: ctx.e_max,
                            max_epoch,
                        });
                    }
                }
                DCert::DrepRegistration(credential, deposit, anchor) => {
                    register_drep(drep_state, *credential, *deposit, anchor.clone())?;
                    deposit_pot.add_drep_deposit(*deposit);
                }
                DCert::DrepUnregistration(credential, refund) => {
                    unregister_drep(drep_state, *credential)?;
                    deposit_pot.return_drep_deposit(*refund);
                }
                DCert::DrepUpdate(credential, anchor) => {
                    update_drep(drep_state, *credential, anchor.clone())?;
                }
                DCert::GenesisDelegation(genesis_hash, delegate_hash, vrf_hash) => {
                    gen_delegs.insert(*genesis_hash, GenesisDelegationState {
                        delegate: *delegate_hash,
                        vrf: *vrf_hash,
                    });
                }
                DCert::MoveInstantaneousReward(_pot, _target) => {
                    // MIR certs are recorded but the actual reserves/treasury
                    // transfer is applied at the epoch boundary (TICK rule).
                    // Accepting the cert here allows mainnet blocks containing
                    // MIR to be decoded and applied without error.
                }
            }
        }
    }

    let mut withdrawal_total = 0u64;
    if let Some(entries) = withdrawals {
        for (account, requested) in entries {
            let Some(state) = reward_accounts.get_mut(account) else {
                return Err(LedgerError::RewardAccountNotRegistered(*account));
            };

            let available = state.balance();
            if *requested > available {
                return Err(LedgerError::WithdrawalExceedsBalance {
                    account: *account,
                    requested: *requested,
                    available,
                });
            }

            state.set_balance(available - *requested);
            withdrawal_total = withdrawal_total.saturating_add(*requested);
        }
    }

    Ok(withdrawal_total)
}

fn register_stake_credential(
    stake_credentials: &mut StakeCredentials,
    credential: StakeCredential,
) -> Result<(), LedgerError> {
    if !stake_credentials.register(credential) {
        return Err(LedgerError::StakeCredentialAlreadyRegistered(credential));
    }

    Ok(())
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
) -> Result<(), LedgerError> {
    if !pool_state.is_registered(&pool) {
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
    cold_credential: StakeCredential,
    hot_credential: StakeCredential,
) -> Result<(), LedgerError> {
    let Some(member_state) = committee_state.get_mut(&cold_credential) else {
        return Err(LedgerError::CommitteeIsUnknown(cold_credential));
    };

    if member_state.is_resigned() {
        return Err(LedgerError::CommitteeHasPreviouslyResigned(cold_credential));
    }

    member_state.set_authorization(Some(CommitteeAuthorization::CommitteeHotCredential(
        hot_credential,
    )));
    Ok(())
}

fn resign_committee_cold_credential(
    committee_state: &mut CommitteeState,
    cold_credential: StakeCredential,
    anchor: Option<Anchor>,
) -> Result<(), LedgerError> {
    let Some(member_state) = committee_state.get_mut(&cold_credential) else {
        return Err(LedgerError::CommitteeIsUnknown(cold_credential));
    };

    if member_state.is_resigned() {
        return Err(LedgerError::CommitteeHasPreviouslyResigned(cold_credential));
    }

    member_state.set_authorization(Some(CommitteeAuthorization::CommitteeMemberResigned(
        anchor,
    )));
    Ok(())
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

fn unregister_drep(drep_state: &mut DrepState, credential: StakeCredential) -> Result<(), LedgerError> {
    let drep = drep_from_credential(credential);
    if drep_state.unregister(&drep).is_none() {
        return Err(LedgerError::DrepNotRegistered(drep));
    }

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
) -> Result<(), LedgerError> {
    let Some(state) = stake_credentials.get_mut(&credential) else {
        return Err(LedgerError::StakeCredentialNotRegistered(credential));
    };

    if !is_builtin_drep(drep) && !drep_state.is_registered(&drep) {
        return Err(LedgerError::DrepNotRegistered(drep));
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
fn touch_drep_activity_for_certs(
    certificates: Option<&[DCert]>,
    drep_state: &mut DrepState,
    current_epoch: EpochNo,
) {
    let Some(certs) = certificates else {
        return;
    };
    for cert in certs {
        let credential = match cert {
            DCert::DrepRegistration(c, _, _)
            | DCert::DrepUpdate(c, _) => *c,
            _ => continue,
        };
        let drep = drep_from_credential(credential);
        if let Some(entry) = drep_state.get_mut(&drep) {
            entry.touch_activity(current_epoch);
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
    collateral_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
    total_ex_units: Option<&crate::eras::alonzo::ExUnits>,
    collateral_return: Option<&MultiEraTxOut>,
    total_collateral: Option<u64>,
    has_redeemers: bool,
) -> Result<(), LedgerError> {
    crate::fees::validate_tx_size(params, tx_body_size)?;
    crate::fees::validate_fee(params, tx_body_size, total_ex_units, declared_fee)?;
    if let Some(eu) = total_ex_units {
        crate::fees::validate_tx_ex_units(params, eu)?;
    }
    crate::min_utxo::validate_all_outputs_min_utxo(params, outputs)?;
    crate::min_utxo::validate_output_not_too_big(params, outputs)?;
    crate::min_utxo::validate_output_boot_addr_attrs(outputs)?;

    // When the transaction carries phase-2 scripts (redeemers ≠ ∅),
    // collateral is mandatory.
    // Reference: Cardano.Ledger.Alonzo.Rules.Utxo — feesOK Part 2.
    if has_redeemers {
        let has_collateral = collateral_inputs
            .map_or(false, |c| !c.is_empty());
        if !has_collateral {
            return Err(LedgerError::MissingCollateralForScripts);
        }
    }

    if let Some(collateral) = collateral_inputs {
        if !collateral.is_empty() {
            crate::collateral::validate_collateral(
                params, utxo, collateral, declared_fee,
                collateral_return, total_collateral,
            )?;
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
    let vkey_hashes = crate::witnesses::witness_vkey_hash_set(&ws.vkey_witnesses);
    crate::witnesses::validate_vkey_witnesses(required, &vkey_hashes)?;
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
    let vkey_hashes = crate::witnesses::witness_vkey_hash_set(&ws.vkey_witnesses);
    crate::witnesses::validate_vkey_witnesses(required, &vkey_hashes)?;
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

/// Validates that no scripts in the witness set are extraneous — every
/// provided script must be required by an input, certificate, withdrawal,
/// mint, vote, or proposal in the transaction.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.extraneousScriptWitnessesUTXOW`.
fn validate_no_extraneous_script_witnesses(
    witness_bytes: Option<&[u8]>,
    required_script_hashes: &HashSet<[u8; 28]>,
) -> Result<(), LedgerError> {
    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    let provided = provided_script_hashes_from_witnesses(&ws);
    for hash in &provided {
        if !required_script_hashes.contains(hash) {
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
) -> Result<(), LedgerError> {
    let provided = provided_script_hashes_from_witnesses(ws);
    for hash in &provided {
        if !required_script_hashes.contains(hash) {
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
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `validateAuxiliaryData`.
fn validate_auxiliary_data(
    declared_hash: Option<&[u8; 32]>,
    auxiliary_data: Option<&[u8]>,
) -> Result<(), LedgerError> {
    match (declared_hash, auxiliary_data) {
        (Some(declared), Some(data)) => {
            let computed = yggdrasil_crypto::hash_bytes_256(data).0;
            if *declared != computed {
                Err(LedgerError::AuxiliaryDataHashMismatch {
                    declared: *declared,
                    computed,
                })
            } else {
                Ok(())
            }
        }
        (Some(_), None) => Err(LedgerError::AuxiliaryDataMissing),
        // Upstream `validateMissingTxBodyMetadataHash`: if auxiliary data is
        // present in the transaction, the body MUST declare its hash.
        (None, Some(_)) => Err(LedgerError::MissingTxBodyMetadataHash),
        // Neither hash nor data — nothing to validate.
        (None, None) => Ok(()),
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
fn validate_output_network_ids(
    expected: u8,
    outputs: &[MultiEraTxOut],
) -> Result<(), LedgerError> {
    for output in outputs {
        let addr_bytes = output.address();
        if let Some(net) = shelley_address_network_id(addr_bytes) {
            if net != expected {
                return Err(LedgerError::WrongNetwork { expected, found: net });
            }
        }
    }
    Ok(())
}

/// Validates that all withdrawal reward accounts have the expected network
/// ID.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `WrongNetworkWithdrawal`.
fn validate_withdrawal_network_ids<'a, I>(
    expected: u8,
    withdrawals: I,
) -> Result<(), LedgerError>
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
fn validate_tx_body_network_id(
    expected: u8,
    declared: Option<u8>,
) -> Result<(), LedgerError> {
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
//
// Epoch-boundary hookup (iterating proposals, calling `enact_gov_action`,
// and pruning enacted proposals) is a subsequent slice.

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
            // All eligible voters abstained — action is accepted per upstream
            // convention (vacuous quorum).
            return true;
        }
        // yes * denominator >= threshold_numerator * active
        (self.yes as u128) * (threshold.denominator as u128)
            >= (threshold.numerator as u128) * (active as u128)
    }
}

/// Tally constitutional-committee votes for a governance action.
///
/// Each non-resigned committee member has equal weight (1).
/// Resigned members are excluded from the total.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `ccVotesSatisfied`.
pub(crate) fn tally_committee_votes(
    action: &GovernanceActionState,
    committee_state: &CommitteeState,
) -> VoteTally {
    use crate::eras::conway::{Vote, Voter};

    let mut yes: u64 = 0;
    let mut no: u64 = 0;
    let mut abstain: u64 = 0;
    let mut eligible: u64 = 0;

    for (cold_cred, member_state) in committee_state.iter() {
        // Resigned members do not count.
        if member_state.is_resigned() {
            continue;
        }
        eligible += 1;

        // Find whether this committee member voted.
        // Votes are keyed by Voter; committee votes use cold-credential tags.
        let voter = match cold_cred {
            StakeCredential::AddrKeyHash(h) => Voter::CommitteeKeyHash(*h),
            StakeCredential::ScriptHash(h) => Voter::CommitteeScript(*h),
        };

        match action.votes.get(&voter) {
            Some(Vote::Yes) => yes += 1,
            Some(Vote::No) => no += 1,
            Some(Vote::Abstain) => abstain += 1,
            None => {} // did not vote — counted in eligible but not tallied
        }
    }

    VoteTally { yes, no, abstain, total: eligible }
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
        if reg.last_active_epoch.is_some_and(|e| {
            e.0.saturating_add(drep_activity) < current_epoch.0
        }) {
            continue; // inactive — excluded from quorum
        }

        total = total.saturating_add(*stake);

        // Find vote keyed by DRep voter tag.
        let voter = match drep {
            DRep::KeyHash(h) => Voter::DRepKeyHash(*h),
            DRep::ScriptHash(h) => Voter::DRepScript(*h),
            DRep::AlwaysAbstain | DRep::AlwaysNoConfidence => unreachable!(),
        };

        match action.votes.get(&voter) {
            Some(Vote::Yes) => yes = yes.saturating_add(*stake),
            Some(Vote::No) => no = no.saturating_add(*stake),
            Some(Vote::Abstain) => abstain = abstain.saturating_add(*stake),
            None => {} // non-voting weight already in total
        }
    }

    VoteTally { yes, no, abstain, total }
}

/// Tally stake-pool operator (SPO) votes for a governance action, weighted
/// by delegated pool stake.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `spoVotesSatisfied`.
pub(crate) fn tally_spo_votes(
    action: &GovernanceActionState,
    pool_stake_dist: &PoolStakeDistribution,
) -> VoteTally {
    use crate::eras::conway::{Vote, Voter};

    let mut yes: u64 = 0;
    let mut no: u64 = 0;
    let mut abstain: u64 = 0;

    for (pool_hash, &pool_stake) in pool_stake_dist.iter() {
        let voter = Voter::StakePool(*pool_hash);
        match action.votes.get(&voter) {
            Some(Vote::Yes) => yes = yes.saturating_add(pool_stake),
            Some(Vote::No) => no = no.saturating_add(pool_stake),
            Some(Vote::Abstain) => abstain = abstain.saturating_add(pool_stake),
            None => {} // non-voting weight in total only
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
    committee_state: &CommitteeState,
    thresholds: &DRepVotingThresholds,
) -> Option<UnitInterval> {
    let committee_is_elected = conway_committee_is_elected(committee_state);

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
            Some(if committee_is_elected {
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
    committee_state: &CommitteeState,
    thresholds: &PoolVotingThresholds,
) -> Option<UnitInterval> {
    let committee_is_elected = conway_committee_is_elected(committee_state);

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
            Some(if committee_is_elected {
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

fn conway_committee_is_elected(committee_state: &CommitteeState) -> bool {
    committee_state
        .iter()
        .any(|(_, member_state)| !member_state.is_resigned())
}

/// Determines whether a governance action is accepted by the
/// constitutional committee.
///
/// The committee must meet a quorum (`committee_quorum` threshold)
/// with equal-weight per-member votes.
///
/// Returns `true` when:
/// - The action type does not require CC approval (InfoAction), or
/// - The CC tally meets the `committee_quorum` threshold.
pub(crate) fn accepted_by_committee(
    action: &GovernanceActionState,
    committee_state: &CommitteeState,
    committee_quorum: &UnitInterval,
) -> bool {
    let purpose = conway_gov_action_purpose(&action.proposal.gov_action);
    if purpose == ConwayGovActionPurpose::Info {
        return true;
    }
    let tally = tally_committee_votes(action, committee_state);
    tally.meets_threshold(committee_quorum)
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
    committee_state: &CommitteeState,
    drep_state: &DrepState,
    drep_delegated_stake: &BTreeMap<DRep, u64>,
    current_epoch: EpochNo,
    drep_activity: u64,
    thresholds: &DRepVotingThresholds,
) -> bool {
    let Some(threshold) = drep_threshold_for_action(
        &action.proposal.gov_action,
        committee_state,
        thresholds,
    ) else {
        return true; // no DRep vote required for this action type
    };

    // AlwaysNoConfidence stake counts as "Yes" for NoConfidence and
    // UpdateCommittee-in-state-of-no-confidence actions.
    let count_no_confidence_as_yes = matches!(
        &action.proposal.gov_action,
        crate::eras::conway::GovAction::NoConfidence { .. }
    ) || (
        matches!(
            &action.proposal.gov_action,
            crate::eras::conway::GovAction::UpdateCommittee { .. }
        ) && !conway_committee_is_elected(committee_state)
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
    committee_state: &CommitteeState,
    pool_stake_dist: &PoolStakeDistribution,
    thresholds: &PoolVotingThresholds,
) -> bool {
    let Some(threshold) = spo_threshold_for_action(
        &action.proposal.gov_action,
        committee_state,
        thresholds,
    ) else {
        return true; // no SPO vote required for this action type
    };
    let tally = tally_spo_votes(action, pool_stake_dist);
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
) -> bool {
    accepted_by_committee(action, committee_state, committee_quorum)
        && accepted_by_dreps(
            action,
            committee_state,
            drep_state,
            drep_delegated_stake,
            current_epoch,
            drep_activity,
            drep_thresholds,
        )
        && accepted_by_spo(action, committee_state, pool_stake_dist, pool_thresholds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eras::conway::{GovAction, Vote, Voter};
    use crate::eras::shelley::ShelleyTxOut;
    use crate::protocol_params::ProtocolParameters;
    use crate::types::{Relay, RewardAccount, UnitInterval};

    fn sample_pool_params(relays: Vec<Relay>, operator: u8) -> PoolParams {
        PoolParams {
            operator: [operator; 28],
            vrf_keyhash: [operator; 32],
            pledge: 1,
            cost: 1,
            margin: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            reward_account: RewardAccount {
                network: 1,
                credential: crate::StakeCredential::AddrKeyHash([operator; 28]),
            },
            pool_owners: vec![[operator; 28]],
            relays,
            pool_metadata: None,
        }
    }

    #[test]
    fn registered_pool_relay_access_points_skip_non_dialable_relays() {
        let pool = RegisteredPool {
            params: sample_pool_params(
                vec![
                    Relay::SingleHostAddr(Some(3001), Some([127, 0, 0, 1]), None),
                    Relay::SingleHostName(Some(3002), "relay.example".to_owned()),
                    Relay::SingleHostName(None, "missing-port.example".to_owned()),
                    Relay::MultiHostName("srv.example".to_owned()),
                ],
                1,
            ),
            retiring_epoch: None,
        };

        assert_eq!(
            pool.relay_access_points(),
            vec![
                PoolRelayAccessPoint {
                    address: "127.0.0.1".to_owned(),
                    port: 3001,
                },
                PoolRelayAccessPoint {
                    address: "relay.example".to_owned(),
                    port: 3002,
                },
            ]
        );
    }

    #[test]
    fn pool_state_relay_access_points_deduplicate_across_pools() {
        let mut pool_state = PoolState::new();
        pool_state.register(sample_pool_params(
            vec![Relay::SingleHostName(Some(3001), "shared.example".to_owned())],
            1,
        ));
        pool_state.register(sample_pool_params(
            vec![
                Relay::SingleHostName(Some(3001), "shared.example".to_owned()),
                Relay::SingleHostAddr(Some(3002), Some([127, 0, 0, 2]), None),
            ],
            2,
        ));

        assert_eq!(
            pool_state.relay_access_points(),
            vec![
                PoolRelayAccessPoint {
                    address: "shared.example".to_owned(),
                    port: 3001,
                },
                PoolRelayAccessPoint {
                    address: "127.0.0.2".to_owned(),
                    port: 3002,
                },
            ]
        );
    }

    #[test]
    fn ledger_state_checkpoint_round_trips_governance_actions() {
        let reward_account = RewardAccount {
            network: 0,
            credential: crate::StakeCredential::AddrKeyHash([0x22; 28]),
        };
        let gov_action_id = crate::eras::conway::GovActionId {
            transaction_id: [0x11; 32],
            gov_action_index: 0,
        };
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: crate::Anchor {
                url: "https://example.invalid/proposal".to_owned(),
                data_hash: [0x33; 32],
            },
        };

        let mut state = LedgerState::new(Era::Conway);
        state.governance_actions.insert(
            gov_action_id.clone(),
            GovernanceActionState::new(proposal.clone()),
        );

        let checkpoint = state.checkpoint();
        let restored = checkpoint.restore();
        assert_eq!(restored.governance_action(&gov_action_id).unwrap().proposal(), &proposal);

        let round_trip = LedgerStateCheckpoint::from_cbor_bytes(&checkpoint.to_cbor_bytes())
            .expect("checkpoint round-trip");
        assert_eq!(round_trip.restore(), state);
    }

    // -- RegisteredDrep activity tracking ---------------------------------

    #[test]
    fn test_registered_drep_new_has_no_activity() {
        let drep = RegisteredDrep::new(500_000_000, None);
        assert_eq!(drep.last_active_epoch(), None);
    }

    #[test]
    fn test_registered_drep_new_active() {
        let drep = RegisteredDrep::new_active(500_000_000, None, EpochNo(42));
        assert_eq!(drep.last_active_epoch(), Some(EpochNo(42)));
    }

    #[test]
    fn test_registered_drep_touch_activity() {
        let mut drep = RegisteredDrep::new(500_000_000, None);
        assert_eq!(drep.last_active_epoch(), None);
        drep.touch_activity(EpochNo(10));
        assert_eq!(drep.last_active_epoch(), Some(EpochNo(10)));
        drep.touch_activity(EpochNo(20));
        assert_eq!(drep.last_active_epoch(), Some(EpochNo(20)));
    }

    #[test]
    fn test_registered_drep_cbor_round_trip_with_activity() {
        let drep = RegisteredDrep::new_active(500_000_000, None, EpochNo(99));
        let bytes = drep.to_cbor_bytes();
        let mut dec = Decoder::new(&bytes);
        let restored = RegisteredDrep::decode_cbor(&mut dec).expect("decode");
        assert_eq!(restored, drep);
    }

    #[test]
    fn test_registered_drep_cbor_round_trip_without_activity() {
        let drep = RegisteredDrep::new(500_000_000, None);
        let bytes = drep.to_cbor_bytes();
        let mut dec = Decoder::new(&bytes);
        let restored = RegisteredDrep::decode_cbor(&mut dec).expect("decode");
        assert_eq!(restored, drep);
        assert_eq!(restored.last_active_epoch(), None);
    }

    #[test]
    fn test_registered_drep_cbor_backward_compat_2_element() {
        // Simulate a legacy 2-element array (no last_active_epoch).
        let mut enc = Encoder::new();
        enc.array(2);
        enc.null(); // no anchor
        enc.unsigned(500_000_000);
        let bytes = enc.into_bytes();

        let mut dec = Decoder::new(&bytes);
        let drep = RegisteredDrep::decode_cbor(&mut dec).expect("decode legacy");
        assert_eq!(drep.deposit(), 500_000_000);
        assert_eq!(drep.last_active_epoch(), None);
    }

    #[test]
    fn test_drep_state_inactive_dreps() {
        let mut ds = DrepState::new();
        let d1 = DRep::KeyHash([0x01; 28]);
        let d2 = DRep::KeyHash([0x02; 28]);
        let d3 = DRep::ScriptHash([0x03; 28]);

        // d1: active epoch 80
        ds.register(d1.clone(), RegisteredDrep::new_active(1, None, EpochNo(80)));
        // d2: active epoch 95
        ds.register(d2.clone(), RegisteredDrep::new_active(1, None, EpochNo(95)));
        // d3: no activity epoch (legacy)
        ds.register(d3.clone(), RegisteredDrep::new(1, None));

        // drep_activity=10, epoch=100: d1 (80+10=90 < 100) is expired, d2 (95+10=105 >= 100) active
        let expired = ds.inactive_dreps(EpochNo(100), 10);
        assert_eq!(expired.len(), 1);
        assert!(expired.contains(&d1));
    }

    // ------------------------------------------------------------------
    //  EnactState + enact_gov_action tests
    // ------------------------------------------------------------------

    fn sample_gov_action_id(tag: u8) -> crate::eras::conway::GovActionId {
        crate::eras::conway::GovActionId {
            transaction_id: [tag; 32],
            gov_action_index: tag as u16,
        }
    }

    fn sample_constitution(url: &str) -> crate::eras::conway::Constitution {
        crate::eras::conway::Constitution {
            anchor: crate::types::Anchor {
                url: url.to_owned(),
                data_hash: [0xAA; 32],
            },
            guardrails_script_hash: None,
        }
    }

    fn sample_reward_account(id: u8) -> RewardAccount {
        RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([id; 28]),
        }
    }

    #[test]
    fn test_enact_state_default_and_roundtrip() {
        let es = EnactState::default();
        assert!(es.prev_pparams_update().is_none());
        assert!(es.prev_hard_fork().is_none());
        assert!(es.prev_committee().is_none());
        assert!(es.prev_constitution().is_none());
        assert_eq!(es.committee_quorum().numerator, 0);
        assert_eq!(es.committee_quorum().denominator, 1);
        // CBOR round-trip
        let bytes = es.to_cbor_bytes();
        let decoded = EnactState::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(es, decoded);
    }

    #[test]
    fn test_enact_info_action_no_effect() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();

        let outcome = enact_gov_action(
            &mut es,
            sample_gov_action_id(1),
            &GovAction::InfoAction,
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome, EnactOutcome::NoEffect);
        // No lineage should be recorded.
        assert!(es.prev_pparams_update().is_none());
        assert!(es.prev_hard_fork().is_none());
        assert!(es.prev_committee().is_none());
        assert!(es.prev_constitution().is_none());
    }

    #[test]
    fn test_enact_new_constitution() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();
        let action_id = sample_gov_action_id(2);
        let new_const = sample_constitution("https://example.com/constitution");

        let outcome = enact_gov_action(
            &mut es,
            action_id.clone(),
            &GovAction::NewConstitution {
                prev_action_id: None,
                constitution: new_const.clone(),
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome, EnactOutcome::ConstitutionUpdated);
        assert_eq!(es.constitution(), &new_const);
        assert_eq!(es.prev_constitution(), Some(&action_id));
        // Other lineages untouched.
        assert!(es.prev_pparams_update().is_none());
    }

    #[test]
    fn test_enact_no_confidence() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let cred = crate::StakeCredential::AddrKeyHash([0x11; 28]);
        committee.register(cred.clone());
        assert_eq!(committee.len(), 1);

        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();
        let action_id = sample_gov_action_id(3);

        let outcome = enact_gov_action(
            &mut es,
            action_id.clone(),
            &GovAction::NoConfidence {
                prev_action_id: None,
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome, EnactOutcome::CommitteeRemoved);
        assert_eq!(committee.len(), 0);
        assert_eq!(es.prev_committee(), Some(&action_id));
        // Quorum reset to 0/1.
        assert_eq!(es.committee_quorum().numerator, 0);
    }

    #[test]
    fn test_enact_update_committee() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let existing = crate::StakeCredential::AddrKeyHash([0x01; 28]);
        let to_remove = crate::StakeCredential::AddrKeyHash([0x02; 28]);
        let new_member = crate::StakeCredential::AddrKeyHash([0x03; 28]);
        committee.register(existing.clone());
        committee.register(to_remove.clone());
        assert_eq!(committee.len(), 2);

        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();
        let action_id = sample_gov_action_id(4);

        let mut members_to_add = std::collections::BTreeMap::new();
        members_to_add.insert(new_member.clone(), 500); // term epoch

        let outcome = enact_gov_action(
            &mut es,
            action_id.clone(),
            &GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![to_remove.clone()],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 2,
                    denominator: 3,
                },
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(
            outcome,
            EnactOutcome::CommitteeUpdated {
                members_removed: 1,
                members_added: 1,
            }
        );
        assert_eq!(committee.len(), 2); // existing + new_member
        assert!(committee.is_member(&existing));
        assert!(!committee.is_member(&to_remove));
        assert!(committee.is_member(&new_member));
        assert_eq!(es.committee_quorum().numerator, 2);
        assert_eq!(es.committee_quorum().denominator, 3);
        assert_eq!(es.prev_committee(), Some(&action_id));
    }

    #[test]
    fn test_enact_update_committee_ignores_non_future_member_expirations() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let existing = crate::StakeCredential::AddrKeyHash([0x21; 28]);
        let add_past = crate::StakeCredential::AddrKeyHash([0x22; 28]);
        let add_now = crate::StakeCredential::AddrKeyHash([0x23; 28]);
        let add_future = crate::StakeCredential::AddrKeyHash([0x24; 28]);
        committee.register(existing);

        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();

        let mut members_to_add = std::collections::BTreeMap::new();
        members_to_add.insert(add_past, 9);
        members_to_add.insert(add_now, 10);
        members_to_add.insert(add_future, 11);

        let outcome = enact_gov_action_at_epoch(
            &mut es,
            EpochNo(10),
            sample_gov_action_id(41),
            &GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );

        assert_eq!(
            outcome,
            EnactOutcome::CommitteeUpdated {
                members_removed: 0,
                members_added: 1,
            }
        );
        assert!(!committee.is_member(&add_past));
        assert!(!committee.is_member(&add_now));
        assert!(committee.is_member(&add_future));
    }

    #[test]
    fn test_enact_update_committee_ignores_member_expirations_beyond_term_limit() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let existing = crate::StakeCredential::AddrKeyHash([0x31; 28]);
        let add_within_limit = crate::StakeCredential::AddrKeyHash([0x32; 28]);
        let add_beyond_limit = crate::StakeCredential::AddrKeyHash([0x33; 28]);
        committee.register(existing);

        let mut pp = Some(crate::protocol_params::ProtocolParameters {
            committee_term_limit: Some(2),
            ..Default::default()
        });
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();

        let mut members_to_add = std::collections::BTreeMap::new();
        members_to_add.insert(add_within_limit, 12); // epoch 10 + 2 => accepted
        members_to_add.insert(add_beyond_limit, 13); // beyond term limit => ignored

        let outcome = enact_gov_action_at_epoch(
            &mut es,
            EpochNo(10),
            sample_gov_action_id(43),
            &GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );

        assert_eq!(
            outcome,
            EnactOutcome::CommitteeUpdated {
                members_removed: 0,
                members_added: 1,
            }
        );
        assert!(committee.is_member(&add_within_limit));
        assert!(!committee.is_member(&add_beyond_limit));
    }

    #[test]
    fn test_enact_hard_fork_initiation() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = Some(crate::protocol_params::ProtocolParameters::alonzo_defaults());
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();
        let action_id = sample_gov_action_id(5);

        let outcome = enact_gov_action(
            &mut es,
            action_id.clone(),
            &GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(
            outcome,
            EnactOutcome::HardForkEnacted {
                new_version: (10, 0),
            }
        );
        assert_eq!(pp.unwrap().protocol_version, Some((10, 0)));
        assert_eq!(es.prev_hard_fork(), Some(&action_id));
    }

    #[test]
    fn test_enact_hard_fork_initializes_protocol_params_when_missing() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();
        let action_id = sample_gov_action_id(42);

        let outcome = enact_gov_action(
            &mut es,
            action_id.clone(),
            &GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(
            outcome,
            EnactOutcome::HardForkEnacted {
                new_version: (10, 0),
            }
        );
        assert_eq!(pp.and_then(|p| p.protocol_version), Some((10, 0)));
        assert_eq!(es.prev_hard_fork(), Some(&action_id));
    }

    #[test]
    fn test_enact_treasury_withdrawals() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = None;
        let ra1 = sample_reward_account(1);
        let ra2 = sample_reward_account(2);
        let ra_unknown = sample_reward_account(99);
        let mut ra = RewardAccounts::new();
        ra.insert(ra1.clone(), RewardAccountState::new(1000, None));
        ra.insert(ra2.clone(), RewardAccountState::new(500, None));
        let mut acc = AccountingState {
            treasury: 5000,
            reserves: 100_000,
        };
        let action_id = sample_gov_action_id(6);

        let mut withdrawals = std::collections::BTreeMap::new();
        withdrawals.insert(ra1.clone(), 200);
        withdrawals.insert(ra2.clone(), 100);
        withdrawals.insert(ra_unknown.clone(), 50); // unregistered — should be ignored

        let outcome = enact_gov_action(
            &mut es,
            action_id,
            &GovAction::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash: None,
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(
            outcome,
            EnactOutcome::TreasuryWithdrawn {
                total_withdrawn: 300,
            }
        );
        assert_eq!(ra.balance(&ra1), 1200); // 1000 + 200
        assert_eq!(ra.balance(&ra2), 600); // 500 + 100
        assert_eq!(acc.treasury, 4700); // 5000 - 300
        // No lineage tracked for treasury withdrawals.
        assert!(es.prev_pparams_update().is_none());
    }

    #[test]
    fn test_enact_parameter_change_recorded() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();
        let action_id = sample_gov_action_id(7);

        let outcome = enact_gov_action(
            &mut es,
            action_id.clone(),
            &GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_a: Some(500),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome, EnactOutcome::ParameterChangeRecorded);
        assert_eq!(es.prev_pparams_update(), Some(&action_id));
        assert_eq!(pp.as_ref().and_then(|p| Some(p.min_fee_a)), Some(500));
    }

    #[test]
    fn test_enact_lineage_chaining() {
        // Enact two constitutions in sequence — the second should
        // reference the first as prev_constitution.
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();

        let id1 = sample_gov_action_id(10);
        let id2 = sample_gov_action_id(11);

        enact_gov_action(
            &mut es,
            id1.clone(),
            &GovAction::NewConstitution {
                prev_action_id: None,
                constitution: sample_constitution("v1"),
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(es.prev_constitution(), Some(&id1));

        enact_gov_action(
            &mut es,
            id2.clone(),
            &GovAction::NewConstitution {
                prev_action_id: Some(id1.clone()),
                constitution: sample_constitution("v2"),
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(es.prev_constitution(), Some(&id2));
        assert_eq!(es.constitution().anchor.url, "v2");
    }

    #[test]
    fn test_enact_state_cbor_round_trip_with_lineage() {
        let mut es = EnactState::new();
        es.constitution = sample_constitution("https://example.com");
        es.committee_quorum = UnitInterval {
            numerator: 2,
            denominator: 3,
        };
        es.prev_pparams_update = Some(sample_gov_action_id(1));
        es.prev_hard_fork = Some(sample_gov_action_id(2));
        es.prev_committee = None;
        es.prev_constitution = Some(sample_gov_action_id(4));

        let bytes = es.to_cbor_bytes();
        let decoded = EnactState::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(es, decoded);
    }

    // ── Enactment edge-case tests ──────────────────────────────────

    #[test]
    fn test_enact_update_committee_remove_nonexistent_member() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let existing = crate::StakeCredential::AddrKeyHash([0xA1; 28]);
        let ghost = crate::StakeCredential::AddrKeyHash([0xA2; 28]);
        committee.register(existing.clone());
        assert_eq!(committee.len(), 1);

        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();

        let outcome = enact_gov_action(
            &mut es,
            sample_gov_action_id(50),
            &GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![ghost],
                members_to_add: std::collections::BTreeMap::new(),
                quorum: UnitInterval { numerator: 1, denominator: 2 },
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(
            outcome,
            EnactOutcome::CommitteeUpdated { members_removed: 0, members_added: 0 }
        );
        assert_eq!(committee.len(), 1);
        assert!(committee.is_member(&existing));
    }

    #[test]
    fn test_enact_no_confidence_on_empty_committee() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        assert_eq!(committee.len(), 0);

        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();
        let action_id = sample_gov_action_id(51);

        let outcome = enact_gov_action(
            &mut es,
            action_id.clone(),
            &GovAction::NoConfidence { prev_action_id: None },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome, EnactOutcome::CommitteeRemoved);
        assert_eq!(committee.len(), 0);
        assert_eq!(es.committee_quorum().numerator, 0);
        assert_eq!(es.committee_quorum().denominator, 1);
        assert_eq!(es.prev_committee(), Some(&action_id));
    }

    #[test]
    fn test_enact_parameter_change_multi_field() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = Some(crate::protocol_params::ProtocolParameters {
            min_fee_a: 100,
            min_fee_b: 200,
            max_tx_size: 4096,
            ..Default::default()
        });
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();
        let action_id = sample_gov_action_id(52);

        let outcome = enact_gov_action(
            &mut es,
            action_id.clone(),
            &GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_a: Some(999),
                    min_fee_b: Some(888),
                    max_tx_size: Some(8192),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome, EnactOutcome::ParameterChangeRecorded);
        let p = pp.unwrap();
        assert_eq!(p.min_fee_a, 999);
        assert_eq!(p.min_fee_b, 888);
        assert_eq!(p.max_tx_size, 8192);
        assert_eq!(es.prev_pparams_update(), Some(&action_id));
    }

    #[test]
    fn test_enact_treasury_withdrawals_zero_amount_skipped() {
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = None;
        let ra1 = sample_reward_account(10);
        let mut ra = RewardAccounts::new();
        ra.insert(ra1.clone(), RewardAccountState::new(500, None));
        let mut acc = AccountingState { treasury: 1000, reserves: 0 };

        let mut withdrawals = std::collections::BTreeMap::new();
        withdrawals.insert(ra1.clone(), 0);

        let outcome = enact_gov_action(
            &mut es,
            sample_gov_action_id(53),
            &GovAction::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash: None,
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome, EnactOutcome::TreasuryWithdrawn { total_withdrawn: 0 });
        assert_eq!(ra.balance(&ra1), 500); // unchanged
        assert_eq!(acc.treasury, 1000); // unchanged
    }

    #[test]
    fn test_enact_treasury_withdrawals_exceeds_treasury() {
        // When withdrawal amounts exceed treasury, saturating_sub
        // should bring treasury to 0 without panicking.
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = None;
        let ra1 = sample_reward_account(20);
        let ra2 = sample_reward_account(21);
        let mut ra = RewardAccounts::new();
        ra.insert(ra1.clone(), RewardAccountState::new(0, None));
        ra.insert(ra2.clone(), RewardAccountState::new(0, None));
        let mut acc = AccountingState { treasury: 100, reserves: 0 };

        let mut withdrawals = std::collections::BTreeMap::new();
        withdrawals.insert(ra1.clone(), 80);
        withdrawals.insert(ra2.clone(), 80);

        let outcome = enact_gov_action(
            &mut es,
            sample_gov_action_id(54),
            &GovAction::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash: None,
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome, EnactOutcome::TreasuryWithdrawn { total_withdrawn: 160 });
        assert_eq!(ra.balance(&ra1), 80);
        assert_eq!(ra.balance(&ra2), 80);
        assert_eq!(acc.treasury, 0); // saturated to 0
    }

    #[test]
    fn test_enact_update_committee_add_existing_member() {
        // Adding a member that already exists should NOT count as "added".
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let existing = crate::StakeCredential::AddrKeyHash([0xB1; 28]);
        committee.register(existing.clone());

        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();

        let mut members_to_add = std::collections::BTreeMap::new();
        members_to_add.insert(existing.clone(), 100); // already exists

        let outcome = enact_gov_action(
            &mut es,
            sample_gov_action_id(55),
            &GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval { numerator: 1, denominator: 1 },
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(
            outcome,
            EnactOutcome::CommitteeUpdated { members_removed: 0, members_added: 0 }
        );
        assert_eq!(committee.len(), 1);
    }

    #[test]
    fn test_enact_hard_fork_lineage_chain() {
        // Two sequential hard forks: v10 then v11.
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = Some(crate::protocol_params::ProtocolParameters::alonzo_defaults());
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();

        let id1 = sample_gov_action_id(60);
        let id2 = sample_gov_action_id(61);

        let outcome1 = enact_gov_action(
            &mut es,
            id1.clone(),
            &GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome1, EnactOutcome::HardForkEnacted { new_version: (10, 0) });
        assert_eq!(es.prev_hard_fork(), Some(&id1));

        let outcome2 = enact_gov_action(
            &mut es,
            id2.clone(),
            &GovAction::HardForkInitiation {
                prev_action_id: Some(id1),
                protocol_version: (11, 0),
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome2, EnactOutcome::HardForkEnacted { new_version: (11, 0) });
        assert_eq!(es.prev_hard_fork(), Some(&id2));
        assert_eq!(pp.unwrap().protocol_version, Some((11, 0)));
    }

    #[test]
    fn test_enact_parameter_change_lineage_chain() {
        // Two sequential parameter changes — lineage advances.
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();

        let id1 = sample_gov_action_id(70);
        let id2 = sample_gov_action_id(71);

        enact_gov_action(
            &mut es,
            id1.clone(),
            &GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_a: Some(100),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(es.prev_pparams_update(), Some(&id1));
        assert_eq!(pp.as_ref().unwrap().min_fee_a, 100);

        enact_gov_action(
            &mut es,
            id2.clone(),
            &GovAction::ParameterChange {
                prev_action_id: Some(id1),
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_b: Some(200),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(es.prev_pparams_update(), Some(&id2));
        let p = pp.unwrap();
        assert_eq!(p.min_fee_a, 100); // preserved from first
        assert_eq!(p.min_fee_b, 200); // applied from second
    }

    #[test]
    fn test_enact_parameter_change_initializes_defaults_when_none() {
        // When protocol_params is None, ParameterChange should
        // initialize defaults then apply the update.
        let mut es = EnactState::new();
        let mut committee = CommitteeState::new();
        let mut pp: Option<crate::protocol_params::ProtocolParameters> = None;
        let mut ra = RewardAccounts::new();
        let mut acc = AccountingState::default();

        let outcome = enact_gov_action(
            &mut es,
            sample_gov_action_id(72),
            &GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_a: Some(42),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            &mut committee,
            &mut pp,
            &mut ra,
            &mut acc,
        );
        assert_eq!(outcome, EnactOutcome::ParameterChangeRecorded);
        assert!(pp.is_some());
        assert_eq!(pp.as_ref().unwrap().min_fee_a, 42);
        // Other fields retain their Default::default() values.
        let defaults = crate::protocol_params::ProtocolParameters::default();
        assert_eq!(pp.as_ref().unwrap().min_fee_b, defaults.min_fee_b);
    }

    #[test]
    fn test_ledger_state_16_element_round_trip() {
        let mut ls = LedgerState::new(Era::Conway);
        ls.enact_state_mut().constitution = sample_constitution("test");
        ls.enact_state_mut().prev_hard_fork = Some(sample_gov_action_id(99));

        let bytes = ls.to_cbor_bytes();
        let restored = LedgerState::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(restored.enact_state().constitution().anchor.url, "test");
        assert!(restored.enact_state().prev_hard_fork().is_some());
    }

    #[test]
    fn test_ledger_state_15_element_backward_compat() {
        // Build a 15-element encoded LedgerState (pre-EnactState era)
        // and verify it decodes with default EnactState.
        let ls = LedgerState::new(Era::Shelley);
        // Encode with the old 15-element layout by manually encoding.
        let mut enc = Encoder::new();
        enc.array(15);
        ls.current_era.encode_cbor(&mut enc);
        ls.tip.encode_cbor(&mut enc);
        match ls.expected_network_id {
            Some(nid) => enc.unsigned(u64::from(nid)),
            None => enc.null(),
        };
        enc.map(0); // no governance actions
        ls.pool_state().encode_cbor(&mut enc);
        ls.stake_credentials().encode_cbor(&mut enc);
        ls.committee_state().encode_cbor(&mut enc);
        ls.drep_state().encode_cbor(&mut enc);
        ls.reward_accounts().encode_cbor(&mut enc);
        ls.multi_era_utxo().encode_cbor(&mut enc);
        ls.shelley_utxo.encode_cbor(&mut enc);
        enc.null(); // no protocol params
        ls.deposit_pot().encode_cbor(&mut enc);
        ls.accounting().encode_cbor(&mut enc);
        ls.current_epoch.encode_cbor(&mut enc);

        let bytes = enc.into_bytes();
        let decoded = LedgerState::from_cbor_bytes(&bytes).unwrap();
        // EnactState should be default when decoded from 15-element array.
        assert_eq!(decoded.enact_state(), &EnactState::default());
    }

    // ------------------------------------------------------------------
    //  Enacted-root prev_action_id validation tests
    // ------------------------------------------------------------------

    fn sample_proposal(
        gov_action: GovAction,
        deposit: u64,
        ra_id: u8,
    ) -> crate::eras::conway::ProposalProcedure {
        use crate::eras::conway::ProposalProcedure;
        let ra = sample_reward_account(ra_id);
        ProposalProcedure {
            deposit,
            reward_account: ra.to_bytes().to_vec(),
            gov_action,
            anchor: crate::types::Anchor {
                url: "https://example.invalid".to_owned(),
                data_hash: [0xCC; 32],
            },
        }
    }

    fn sample_governance_actions_with(
        entries: Vec<(crate::eras::conway::GovActionId, GovAction)>,
    ) -> BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState> {
        let mut map = BTreeMap::new();
        for (id, action) in entries {
            let proposal = crate::eras::conway::ProposalProcedure {
                deposit: 1,
                reward_account: sample_reward_account(1).to_bytes().to_vec(),
                gov_action: action,
                anchor: crate::types::Anchor {
                    url: "https://example.invalid/stored".to_owned(),
                    data_hash: [0xDD; 32],
                },
            };
            map.insert(
                id,
                GovernanceActionState::new(proposal),
            );
        }
        map
    }

    fn empty_stake_creds_with(ra_id: u8) -> StakeCredentials {
        let mut sc = StakeCredentials::new();
        let ra = sample_reward_account(ra_id);
        sc.register(ra.credential);
        sc
    }

    #[test]
    fn test_enacted_root_none_accepts_fresh_proposal_without_prev() {
        // EnactState has no enacted root for Committee purpose.
        // Proposal with prev_action_id = None should be accepted.
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::NoConfidence {
                prev_action_id: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_enacted_root_some_rejects_fresh_proposal_without_prev() {
        // EnactState has an enacted root for Committee purpose.
        // Proposal with prev_action_id = None should be rejected.
        let mut es = EnactState::default();
        es.prev_committee = Some(sample_gov_action_id(10));
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::NoConfidence {
                prev_action_id: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::InvalidPrevGovActionId(_))
        ));
    }

    #[test]
    fn test_enacted_root_matching_prev_accepted() {
        // EnactState has an enacted root for Constitution purpose.
        // Proposal that references the enacted root should be accepted.
        let root_id = sample_gov_action_id(20);
        let mut es = EnactState::default();
        es.prev_constitution = Some(root_id.clone());
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::NewConstitution {
                prev_action_id: Some(root_id.clone()),
                constitution: sample_constitution("v3"),
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_enacted_root_wrong_purpose_prev_rejected() {
        // EnactState has an enacted root for Constitution, but proposal
        // is ParameterChange referencing it — wrong purpose.
        let root_id = sample_gov_action_id(30);
        let mut es = EnactState::default();
        es.prev_constitution = Some(root_id.clone());
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: Some(root_id.clone()),
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_a: Some(1),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::InvalidPrevGovActionId(_))
        ));
    }

    #[test]
    fn test_enacted_root_pending_proposal_accepted() {
        // EnactState has enacted root for HardFork != prev, but a stored
        // pending proposal has the matching id and purpose.
        let enacted_id = sample_gov_action_id(40);
        let pending_id = sample_gov_action_id(41);
        let mut es = EnactState::default();
        es.prev_hard_fork = Some(enacted_id);
        let stake_creds = empty_stake_creds_with(1);
        let stored = sample_governance_actions_with(vec![(
            pending_id.clone(),
            GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (9, 1),
            },
        )]);
        let proposals = vec![sample_proposal(
            GovAction::HardForkInitiation {
                prev_action_id: Some(pending_id.clone()),
                protocol_version: (10, 0),
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &stored,
            &stake_creds,
            Some((9, 0)),
            None,
            None,
            None,
            &es,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_hard_fork_prev_enacted_root_requires_pv_follow() {
        let root_id = sample_gov_action_id(42);
        let mut es = EnactState::default();
        es.prev_hard_fork = Some(root_id.clone());
        let stake_creds = empty_stake_creds_with(1);

        let valid = vec![sample_proposal(
            GovAction::HardForkInitiation {
                prev_action_id: Some(root_id.clone()),
                protocol_version: (10, 1),
            },
            1,
            1,
        )];
        let valid_result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &valid,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            Some((10, 0)),
            None,
            None,
            None,
            &es,
        );
        assert!(valid_result.is_ok());

        let invalid = vec![sample_proposal(
            GovAction::HardForkInitiation {
                prev_action_id: Some(root_id),
                protocol_version: (10, 2),
            },
            1,
            1,
        )];
        let invalid_result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &invalid,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            Some((10, 0)),
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(invalid_result, Err(LedgerError::ProposalCantFollow { .. })));
    }

    #[test]
    fn test_enacted_root_unknown_prev_rejected() {
        // prev_action_id matches neither enacted root nor stored proposals.
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let unknown_id = sample_gov_action_id(99);
        let proposals = vec![sample_proposal(
            GovAction::NewConstitution {
                prev_action_id: Some(unknown_id),
                constitution: sample_constitution("orphan"),
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::InvalidPrevGovActionId(_))
        ));
    }

    #[test]
    fn test_enacted_root_treasury_and_info_skip_lineage() {
        // TreasuryWithdrawals and InfoAction have no lineage concept.
        // They should be accepted regardless of EnactState.
        let mut es = EnactState::default();
        es.prev_pparams_update = Some(sample_gov_action_id(50));
        es.prev_hard_fork = Some(sample_gov_action_id(51));
        es.prev_committee = Some(sample_gov_action_id(52));
        es.prev_constitution = Some(sample_gov_action_id(53));
        let stake_creds = empty_stake_creds_with(1);
        let mut withdrawals = std::collections::BTreeMap::new();
        let ra = sample_reward_account(1);
        withdrawals.insert(ra, 100);
        let proposals = vec![
            sample_proposal(
                GovAction::TreasuryWithdrawals {
                    withdrawals,
                    guardrails_script_hash: None,
                },
                1,
                1,
            ),
            sample_proposal(GovAction::InfoAction, 1, 1),
        ];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_enacted_root_update_committee_shares_committee_purpose() {
        // UpdateCommittee and NoConfidence share the Committee purpose.
        // An enacted NoConfidence root should accept an UpdateCommittee
        // referencing it.
        let root_id = sample_gov_action_id(60);
        let mut es = EnactState::default();
        es.prev_committee = Some(root_id.clone());
        let stake_creds = empty_stake_creds_with(1);
        let mut members_to_add = std::collections::BTreeMap::new();
        members_to_add.insert(
            crate::StakeCredential::AddrKeyHash([0x33; 28]),
            500, // term epoch
        );
        let proposals = vec![sample_proposal(
            GovAction::UpdateCommittee {
                prev_action_id: Some(root_id.clone()),
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 2,
                    denominator: 3,
                },
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_committee_rejects_expiration_epoch_beyond_term_limit() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let mut members_to_add = std::collections::BTreeMap::new();
        members_to_add.insert(
            crate::StakeCredential::AddrKeyHash([0x44; 28]),
            13,
        );
        let proposals = vec![sample_proposal(
            GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            1,
            1,
        )];
        let protocol_params = crate::protocol_params::ProtocolParameters {
            committee_term_limit: Some(2),
            ..Default::default()
        };

        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(10),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            Some(&protocol_params),
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::ExpirationEpochTooLarge { .. })
        ));
    }

    #[test]
    fn test_update_committee_accepts_expiration_epoch_at_term_limit() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let mut members_to_add = std::collections::BTreeMap::new();
        members_to_add.insert(
            crate::StakeCredential::AddrKeyHash([0x55; 28]),
            12,
        );
        let proposals = vec![sample_proposal(
            GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            1,
            1,
        )];
        let protocol_params = crate::protocol_params::ProtocolParameters {
            committee_term_limit: Some(2),
            ..Default::default()
        };

        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(10),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            Some(&protocol_params),
            &es,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_hard_fork_rejects_when_current_protocol_version_missing() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            1,
            1,
        )];

        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::MissingProtocolVersionForHardFork(_))
        ));
    }

    #[test]
    fn test_hard_fork_prev_enacted_root_rejects_when_current_protocol_version_missing() {
        let root_id = sample_gov_action_id(70);
        let mut es = EnactState::default();
        es.prev_hard_fork = Some(root_id.clone());
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::HardForkInitiation {
                prev_action_id: Some(root_id),
                protocol_version: (10, 1),
            },
            1,
            1,
        )];

        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::MissingProtocolVersionForHardFork(_))
        ));
    }

    #[test]
    fn test_bootstrap_rejects_non_bootstrap_proposal_action() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::NewConstitution {
                prev_action_id: None,
                constitution: sample_constitution("bootstrap-disallowed"),
            },
            1,
            1,
        )];

        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            Some((9, 0)),
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::DisallowedProposalDuringBootstrap(_))
        ));
    }

    #[test]
    fn test_bootstrap_allows_parameter_change_proposal_action() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_a: Some(1),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];

        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            Some((9, 0)),
            None,
            None,
            None,
            &es,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bootstrap_allows_info_action_proposal() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::InfoAction,
            1,
            1,
        )];

        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            Some((9, 0)),
            None,
            None,
            None,
            &es,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bootstrap_rejects_drep_vote_on_non_info_action() {
        let drep_voter = Voter::DRepKeyHash([0x66; 28]);
        let action_id = sample_gov_action_id(71);
        let governance_actions = sample_governance_actions_with(vec![
            (
                action_id.clone(),
                GovAction::HardForkInitiation {
                    prev_action_id: None,
                    protocol_version: (10, 0),
                },
            ),
        ]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id.clone(),
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(drep_voter.clone(), inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(matches!(
            result,
            Err(LedgerError::DisallowedVotesDuringBootstrap(ref entries))
                if entries == &vec![(drep_voter, action_id)]
        ));
    }

    #[test]
    fn test_bootstrap_rejects_committee_vote_on_non_bootstrap_action() {
        let committee_voter = Voter::CommitteeKeyHash([0x67; 28]);
        let action_id = sample_gov_action_id(72);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::NewConstitution {
                prev_action_id: None,
                constitution: sample_constitution("bootstrap-committee-disallowed"),
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id.clone(),
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(committee_voter.clone(), inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(matches!(
            result,
            Err(LedgerError::DisallowedVotesDuringBootstrap(ref entries))
                if entries == &vec![(committee_voter, action_id)]
        ));
    }

    #[test]
    fn test_bootstrap_rejects_spo_vote_on_non_bootstrap_action() {
        let spo_voter = Voter::StakePool([0x68; 28]);
        let action_id = sample_gov_action_id(73);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::TreasuryWithdrawals {
                withdrawals: BTreeMap::from([(sample_reward_account(7), 1)]),
                guardrails_script_hash: None,
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id.clone(),
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(spo_voter.clone(), inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(matches!(
            result,
            Err(LedgerError::DisallowedVotesDuringBootstrap(ref entries))
                if entries == &vec![(spo_voter, action_id)]
        ));
    }

    #[test]
    fn test_bootstrap_allows_drep_vote_on_info_action() {
        let drep_voter = Voter::DRepKeyHash([0x69; 28]);
        let action_id = sample_gov_action_id(74);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::InfoAction,
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id,
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(drep_voter, inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bootstrap_allows_committee_vote_on_hard_fork_action() {
        let committee_voter = Voter::CommitteeKeyHash([0x6A; 28]);
        let action_id = sample_gov_action_id(75);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id,
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(committee_voter, inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bootstrap_allows_committee_vote_on_parameter_change_action() {
        let committee_voter = Voter::CommitteeKeyHash([0x6C; 28]);
        let action_id = sample_gov_action_id(77);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_a: Some(1),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id,
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(committee_voter, inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bootstrap_allows_spo_vote_on_hard_fork_action() {
        let spo_voter = Voter::StakePool([0x6B; 28]);
        let action_id = sample_gov_action_id(76);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id,
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(spo_voter, inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bootstrap_allows_spo_vote_on_parameter_change_action() {
        let spo_voter = Voter::StakePool([0x6D; 28]);
        let action_id = sample_gov_action_id(78);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_a: Some(1),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id,
            crate::eras::conway::VotingProcedure {
                vote: Vote::No,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(spo_voter, inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bootstrap_allows_committee_vote_on_info_action() {
        let committee_voter = Voter::CommitteeKeyHash([0x6E; 28]);
        let action_id = sample_gov_action_id(79);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::InfoAction,
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id,
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(committee_voter, inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bootstrap_allows_spo_vote_on_info_action() {
        let spo_voter = Voter::StakePool([0x6F; 28]);
        let action_id = sample_gov_action_id(80);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::InfoAction,
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id,
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(spo_voter, inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            Some((9, 0)),
        );
        assert!(result.is_ok());
    }

    // --- Post-bootstrap (non-bootstrap) voter permission tests ---

    #[test]
    fn test_post_bootstrap_spo_vote_allowed_on_security_group_parameter_change() {
        let spo_voter = Voter::StakePool([0xA0; 28]);
        let action_id = sample_gov_action_id(90);
        // min_fee_a is Economic + Security group, so SPO should be allowed
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_fee_a: Some(500),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id,
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(spo_voter, inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            None, // post-bootstrap
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_post_bootstrap_spo_vote_rejected_on_non_security_parameter_change() {
        let spo_voter = Voter::StakePool([0xA1; 28]);
        let action_id = sample_gov_action_id(91);
        // key_deposit is Economic only (no security group), so SPO should be rejected
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    key_deposit: Some(2_000_000),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id.clone(),
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(spo_voter.clone(), inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            None, // post-bootstrap
        );
        assert!(matches!(
            result,
            Err(LedgerError::DisallowedVoters(ref entries))
                if entries == &vec![(spo_voter, action_id)]
        ));
    }

    #[test]
    fn test_post_bootstrap_spo_vote_rejected_on_new_constitution() {
        let spo_voter = Voter::StakePool([0xA2; 28]);
        let action_id = sample_gov_action_id(92);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::NewConstitution {
                prev_action_id: None,
                constitution: sample_constitution("post-bootstrap-constitution"),
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id.clone(),
            crate::eras::conway::VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(spo_voter.clone(), inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            None, // post-bootstrap
        );
        assert!(matches!(
            result,
            Err(LedgerError::DisallowedVoters(ref entries))
                if entries == &vec![(spo_voter, action_id)]
        ));
    }

    #[test]
    fn test_post_bootstrap_committee_vote_rejected_on_no_confidence() {
        let committee_voter = Voter::CommitteeKeyHash([0xA3; 28]);
        let action_id = sample_gov_action_id(93);
        let governance_actions = sample_governance_actions_with(vec![(
            action_id.clone(),
            GovAction::NoConfidence {
                prev_action_id: None,
            },
        )]);

        let mut inner = BTreeMap::new();
        inner.insert(
            action_id.clone(),
            crate::eras::conway::VotingProcedure {
                vote: Vote::No,
                anchor: None,
            },
        );
        let voting_procedures = crate::eras::conway::VotingProcedures {
            procedures: BTreeMap::from([(committee_voter.clone(), inner)]),
        };

        let result = validate_conway_voter_permissions(
            EpochNo(0),
            &voting_procedures,
            &governance_actions,
            None, // post-bootstrap
        );
        assert!(matches!(
            result,
            Err(LedgerError::DisallowedVoters(ref entries))
                if entries == &vec![(committee_voter, action_id)]
        ));
    }

    #[test]
    fn test_parameter_change_rejects_malformed_unit_interval() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    price_mem: Some(UnitInterval {
                        numerator: 2,
                        denominator: 1,
                    }),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn test_parameter_change_rejects_tx_size_larger_than_block_body_size() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    max_block_body_size: Some(100),
                    max_tx_size: Some(101),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn test_parameter_change_rejects_tx_size_larger_than_current_block_body_size() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    max_tx_size: Some(501),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let protocol_params = crate::protocol_params::ProtocolParameters {
            max_block_body_size: 500,
            ..Default::default()
        };
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            Some(&protocol_params),
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn test_parameter_change_rejects_protocol_version_update() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    protocol_version: Some((10, 0)),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn test_parameter_change_rejects_zero_pool_and_gov_deposits() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let pool_zero = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    pool_deposit: Some(0),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let gov_zero = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    gov_action_deposit: Some(0),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];

        let pool_result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &pool_zero,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(pool_result, Err(LedgerError::MalformedProposal(_))));

        let gov_result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &gov_zero,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(gov_result, Err(LedgerError::MalformedProposal(_))));
    }

    // -----------------------------------------------------------------------
    // Ratification tally tests
    // -----------------------------------------------------------------------

    fn test_info_action() -> GovernanceActionState {
        GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::InfoAction,
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        })
    }

    fn test_hf_action() -> GovernanceActionState {
        GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        })
    }

    fn test_treasury_action() -> GovernanceActionState {
        GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::TreasuryWithdrawals {
                withdrawals: BTreeMap::new(),
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        })
    }

    fn test_no_confidence_action() -> GovernanceActionState {
        GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::NoConfidence { prev_action_id: None },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        })
    }

    // -- VoteTally::meets_threshold ---

    #[test]
    fn tally_meets_threshold_exact() {
        let tally = VoteTally { yes: 67, no: 33, abstain: 0, total: 100 };
        let threshold = UnitInterval { numerator: 67, denominator: 100 };
        assert!(tally.meets_threshold(&threshold));
    }

    #[test]
    fn tally_below_threshold() {
        let tally = VoteTally { yes: 66, no: 34, abstain: 0, total: 100 };
        let threshold = UnitInterval { numerator: 67, denominator: 100 };
        assert!(!tally.meets_threshold(&threshold));
    }

    #[test]
    fn tally_above_threshold() {
        let tally = VoteTally { yes: 80, no: 20, abstain: 0, total: 100 };
        let threshold = UnitInterval { numerator: 67, denominator: 100 };
        assert!(tally.meets_threshold(&threshold));
    }

    #[test]
    fn tally_vacuous_quorum_all_abstain() {
        let tally = VoteTally { yes: 0, no: 0, abstain: 100, total: 100 };
        let threshold = UnitInterval { numerator: 67, denominator: 100 };
        assert!(tally.meets_threshold(&threshold));
    }

    #[test]
    fn tally_with_abstentions_excluded() {
        // 60 yes, 20 no, 20 abstain. Active = 80. 60/80 = 75% >= 67%.
        let tally = VoteTally { yes: 60, no: 20, abstain: 20, total: 100 };
        let threshold = UnitInterval { numerator: 67, denominator: 100 };
        assert!(tally.meets_threshold(&threshold));
    }

    #[test]
    fn tally_zero_total_is_vacuous() {
        let tally = VoteTally { yes: 0, no: 0, abstain: 0, total: 0 };
        let threshold = UnitInterval { numerator: 1, denominator: 2 };
        assert!(tally.meets_threshold(&threshold));
    }

    // -- Committee tally ---

    #[test]
    fn committee_tally_unanimous_yes() {
        let mut action = test_hf_action();
        let mut cs = CommitteeState::default();

        let cred_a = StakeCredential::AddrKeyHash([1; 28]);
        let cred_b = StakeCredential::AddrKeyHash([2; 28]);
        cs.register(cred_a);
        cs.register(cred_b);

        // Both vote yes.
        action.votes.insert(Voter::CommitteeKeyHash([1; 28]), Vote::Yes);
        action.votes.insert(Voter::CommitteeKeyHash([2; 28]), Vote::Yes);

        let tally = tally_committee_votes(&action, &cs);
        assert_eq!(tally.yes, 2);
        assert_eq!(tally.no, 0);
        assert_eq!(tally.total, 2);
        let quorum = UnitInterval { numerator: 2, denominator: 3 };
        assert!(tally.meets_threshold(&quorum));
    }

    #[test]
    fn committee_tally_resigned_excluded() {
        let mut action = test_hf_action();
        let mut cs = CommitteeState::default();

        let cred_a = StakeCredential::AddrKeyHash([1; 28]);
        let cred_b = StakeCredential::AddrKeyHash([2; 28]);
        cs.register(cred_a);
        cs.register(cred_b);
        // Resign member B.
        cs.get_mut(&cred_b).unwrap().set_authorization(Some(
            CommitteeAuthorization::CommitteeMemberResigned(None),
        ));

        action.votes.insert(Voter::CommitteeKeyHash([1; 28]), Vote::Yes);

        let tally = tally_committee_votes(&action, &cs);
        assert_eq!(tally.yes, 1);
        assert_eq!(tally.total, 1); // resigned excluded
    }

    #[test]
    fn committee_tally_no_votes_fails_threshold() {
        let action = test_hf_action();
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([1; 28]));
        cs.register(StakeCredential::AddrKeyHash([2; 28]));

        let tally = tally_committee_votes(&action, &cs);
        assert_eq!(tally.yes, 0);
        assert_eq!(tally.total, 2);
        let quorum = UnitInterval { numerator: 1, denominator: 2 };
        assert!(!tally.meets_threshold(&quorum));
    }

    // -- DRep tally ---

    #[test]
    fn drep_tally_weighted_by_stake() {
        let mut action = test_hf_action();
        let mut drep_state = DrepState::new();
        let drep_a = DRep::KeyHash([1; 28]);
        let drep_b = DRep::KeyHash([2; 28]);
        drep_state.register(drep_a.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        drep_state.register(drep_b.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));

        let mut stake = BTreeMap::new();
        stake.insert(drep_a.clone(), 700);
        stake.insert(drep_b.clone(), 300);

        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);
        action.votes.insert(Voter::DRepKeyHash([2; 28]), Vote::No);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
        assert_eq!(tally.yes, 700);
        assert_eq!(tally.no, 300);
        assert_eq!(tally.total, 1000);
        let threshold = UnitInterval { numerator: 67, denominator: 100 };
        assert!(tally.meets_threshold(&threshold)); // 700/1000 = 70% >= 67%
    }

    #[test]
    fn drep_tally_excludes_inactive() {
        let mut action = test_hf_action();
        let mut drep_state = DrepState::new();
        let drep_a = DRep::KeyHash([1; 28]);
        let drep_b = DRep::KeyHash([2; 28]);
        // A: active epoch 90. Activity window 10. At epoch 105: 90+10=100 < 105 → inactive.
        drep_state.register(drep_a.clone(), RegisteredDrep::new_active(0, None, EpochNo(90)));
        // B: active epoch 100. 100+10=110 >= 105 → active.
        drep_state.register(drep_b.clone(), RegisteredDrep::new_active(0, None, EpochNo(100)));

        let mut stake = BTreeMap::new();
        stake.insert(drep_a.clone(), 500);
        stake.insert(drep_b.clone(), 500);

        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes); // inactive, excluded
        action.votes.insert(Voter::DRepKeyHash([2; 28]), Vote::Yes);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(105), 10, false);
        // Only DRep B counted (active). A is inactive and excluded.
        assert_eq!(tally.yes, 500);
        assert_eq!(tally.total, 500);
        let threshold = UnitInterval { numerator: 1, denominator: 2 };
        assert!(tally.meets_threshold(&threshold));
    }

    #[test]
    fn drep_tally_unregistered_drep_excluded() {
        let action = test_hf_action();
        let drep_state = DrepState::new(); // empty — no DReps registered

        let mut stake = BTreeMap::new();
        stake.insert(DRep::KeyHash([1; 28]), 1000);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
        assert_eq!(tally.total, 0); // no registered DReps
    }

    // -- SPO tally ---

    #[test]
    fn spo_tally_weighted_by_pool_stake() {
        let mut action = test_hf_action();

        let pool_a = [1u8; 28];
        let pool_b = [2u8; 28];

        // Build pool stake distribution manually.
        let mut pool_stakes = BTreeMap::new();
        pool_stakes.insert(pool_a, 600u64);
        pool_stakes.insert(pool_b, 400u64);
        let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

        action.votes.insert(Voter::StakePool(pool_a), Vote::Yes);
        action.votes.insert(Voter::StakePool(pool_b), Vote::No);

        let tally = tally_spo_votes(&action, &pool_dist);
        assert_eq!(tally.yes, 600);
        assert_eq!(tally.no, 400);
        assert_eq!(tally.total, 1000);
        let threshold = UnitInterval { numerator: 51, denominator: 100 };
        assert!(tally.meets_threshold(&threshold)); // 600/1000 = 60% >= 51%
    }

    // -- Parameter-group classification ---

    #[test]
    fn pparam_groups_empty_update_has_no_groups() {
        let update = crate::protocol_params::ProtocolParameterUpdate::default();
        let g = conway_modified_pparam_groups(&update);
        assert!(!g.network);
        assert!(!g.economic);
        assert!(!g.technical);
        assert!(!g.gov);
        assert!(!g.security);
        assert!(!g.has_drep_group());
    }

    #[test]
    fn pparam_groups_min_fee_a_is_economic_plus_security() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            min_fee_a: Some(44),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.economic);
        assert!(g.security);
        assert!(!g.network);
        assert!(!g.technical);
        assert!(!g.gov);
    }

    #[test]
    fn pparam_groups_min_fee_b_is_economic_plus_security() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            min_fee_b: Some(155381),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.economic);
        assert!(g.security);
        assert!(!g.network);
        assert!(!g.technical);
        assert!(!g.gov);
    }

    #[test]
    fn pparam_groups_max_block_body_size_is_network_plus_security() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            max_block_body_size: Some(65536),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.network);
        assert!(g.security);
        assert!(!g.economic);
        assert!(!g.technical);
        assert!(!g.gov);
    }

    #[test]
    fn pparam_groups_max_tx_size_is_network_plus_security() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            max_tx_size: Some(16384),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.network);
        assert!(g.security);
    }

    #[test]
    fn pparam_groups_key_deposit_is_economic_only() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            key_deposit: Some(2_000_000),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.economic);
        assert!(!g.security);
        assert!(!g.network);
        assert!(!g.technical);
        assert!(!g.gov);
    }

    #[test]
    fn pparam_groups_pool_deposit_is_economic_only() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            pool_deposit: Some(500_000_000),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.economic);
        assert!(!g.security);
    }

    #[test]
    fn pparam_groups_n_opt_is_technical_only() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            n_opt: Some(500),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.technical);
        assert!(!g.security);
        assert!(!g.network);
        assert!(!g.economic);
        assert!(!g.gov);
    }

    #[test]
    fn pparam_groups_e_max_is_technical_only() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            e_max: Some(18),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.technical);
        assert!(!g.security);
    }

    #[test]
    fn pparam_groups_collateral_percentage_is_technical_only() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            collateral_percentage: Some(150),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.technical);
        assert!(!g.security);
        assert!(!g.economic);
    }

    #[test]
    fn pparam_groups_pool_voting_thresholds_is_gov_only() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            pool_voting_thresholds: Some(crate::protocol_params::PoolVotingThresholds::default()),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.gov);
        assert!(!g.security);
        assert!(!g.network);
        assert!(!g.economic);
        assert!(!g.technical);
    }

    #[test]
    fn pparam_groups_drep_activity_is_gov_only() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            drep_activity: Some(20),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.gov);
        assert!(!g.security);
    }

    #[test]
    fn pparam_groups_gov_action_deposit_is_gov_plus_security() {
        let update = crate::protocol_params::ProtocolParameterUpdate {
            gov_action_deposit: Some(100_000_000_000),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.gov);
        assert!(g.security);
        assert!(!g.network);
        assert!(!g.economic);
        assert!(!g.technical);
    }

    #[test]
    fn pparam_groups_mixed_fields_combine_correctly() {
        // min_fee_a = economic+security, n_opt = technical, drep_activity = gov
        let update = crate::protocol_params::ProtocolParameterUpdate {
            min_fee_a: Some(44),
            n_opt: Some(500),
            drep_activity: Some(20),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.economic);
        assert!(g.technical);
        assert!(g.gov);
        assert!(g.security);
        assert!(!g.network);
        assert!(g.has_drep_group());
    }

    #[test]
    fn pparam_groups_security_only_update_has_no_drep_group() {
        // protocol_version is security-only in this implementation
        let update = crate::protocol_params::ProtocolParameterUpdate {
            protocol_version: Some((10, 0)),
            ..Default::default()
        };
        let g = conway_modified_pparam_groups(&update);
        assert!(g.security);
        assert!(!g.has_drep_group());
        assert!(!g.network);
        assert!(!g.economic);
        assert!(!g.technical);
        assert!(!g.gov);
    }

    // -- Threshold dispatch ---

    #[test]
    fn drep_threshold_for_hard_fork() {
        let thresholds = DRepVotingThresholds::default();
        let committee_state = CommitteeState::default();
        let t = drep_threshold_for_action(
            &GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            &committee_state,
            &thresholds,
        );
        assert_eq!(t, Some(thresholds.hard_fork_initiation));
    }

    #[test]
    fn drep_threshold_for_info_is_none() {
        let thresholds = DRepVotingThresholds::default();
        let committee_state = CommitteeState::default();
        let t = drep_threshold_for_action(&GovAction::InfoAction, &committee_state, &thresholds);
        assert!(t.is_none());
    }

    #[test]
    fn spo_threshold_for_constitution_is_none() {
        let thresholds = PoolVotingThresholds::default();
        let committee_state = CommitteeState::default();
        let t = spo_threshold_for_action(
            &GovAction::NewConstitution {
                prev_action_id: None,
                constitution: sample_constitution("c1"),
            },
            &committee_state,
            &thresholds,
        );
        assert!(t.is_none());
    }

    #[test]
    fn spo_threshold_for_treasury_is_none() {
        let thresholds = PoolVotingThresholds::default();
        let committee_state = CommitteeState::default();
        let t = spo_threshold_for_action(
            &GovAction::TreasuryWithdrawals {
                withdrawals: BTreeMap::new(),
                guardrails_script_hash: None,
            },
            &committee_state,
            &thresholds,
        );
        assert!(t.is_none());
    }

    #[test]
    fn drep_threshold_for_no_confidence_uses_motion_threshold() {
        let thresholds = DRepVotingThresholds::default();
        let committee_state = CommitteeState::default();
        let t = drep_threshold_for_action(
            &GovAction::NoConfidence {
                prev_action_id: None,
            },
            &committee_state,
            &thresholds,
        );
        assert_eq!(t, Some(thresholds.motion_no_confidence));
    }

    #[test]
    fn spo_threshold_for_no_confidence_uses_motion_threshold() {
        let thresholds = PoolVotingThresholds::default();
        let committee_state = CommitteeState::default();
        let t = spo_threshold_for_action(
            &GovAction::NoConfidence {
                prev_action_id: None,
            },
            &committee_state,
            &thresholds,
        );
        assert_eq!(t, Some(thresholds.motion_no_confidence));
    }

    #[test]
    fn spo_threshold_for_parameter_change_requires_security_group() {
        let thresholds = PoolVotingThresholds::default();
        let committee_state = CommitteeState::default();
        let non_security = GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                n_opt: Some(99),
                ..Default::default()
            },
            guardrails_script_hash: None,
        };
        let security = GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                max_block_body_size: Some(123456),
                ..Default::default()
            },
            guardrails_script_hash: None,
        };

        assert!(spo_threshold_for_action(&non_security, &committee_state, &thresholds).is_none());
        assert_eq!(
            spo_threshold_for_action(&security, &committee_state, &thresholds),
            Some(thresholds.pp_security_group)
        );
    }

    #[test]
    fn drep_threshold_for_parameter_change_uses_max_modified_group_threshold() {
        let thresholds = DRepVotingThresholds {
            pp_network_group: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
            pp_economic_group: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            pp_technical_group: UnitInterval {
                numerator: 3,
                denominator: 4,
            },
            pp_gov_group: UnitInterval {
                numerator: 4,
                denominator: 5,
            },
            ..DRepVotingThresholds::default()
        };
        let action = GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                max_tx_size: Some(1024),
                n_opt: Some(42),
                gov_action_lifetime: Some(100),
                ..Default::default()
            },
            guardrails_script_hash: None,
        };
        let committee_state = CommitteeState::default();

        let selected = drep_threshold_for_action(&action, &committee_state, &thresholds);
        assert_eq!(selected, Some(thresholds.pp_gov_group));
    }

    #[test]
    fn drep_threshold_for_security_only_parameter_change_returns_none() {
        let thresholds = DRepVotingThresholds {
            pp_network_group: UnitInterval { numerator: 1, denominator: 2 },
            pp_economic_group: UnitInterval { numerator: 2, denominator: 3 },
            pp_technical_group: UnitInterval { numerator: 3, denominator: 4 },
            pp_gov_group: UnitInterval { numerator: 4, denominator: 5 },
            ..DRepVotingThresholds::default()
        };
        // protocol_version is security-only — no DRep group, threshold should be None
        let action = GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                protocol_version: Some((10, 0)),
                ..Default::default()
            },
            guardrails_script_hash: None,
        };
        let committee_state = CommitteeState::default();

        let selected = drep_threshold_for_action(&action, &committee_state, &thresholds);
        assert_eq!(selected, None);
    }

    #[test]
    fn drep_threshold_for_single_economic_group_returns_economic_threshold() {
        let thresholds = DRepVotingThresholds {
            pp_network_group: UnitInterval { numerator: 1, denominator: 10 },
            pp_economic_group: UnitInterval { numerator: 2, denominator: 3 },
            pp_technical_group: UnitInterval { numerator: 1, denominator: 10 },
            pp_gov_group: UnitInterval { numerator: 1, denominator: 10 },
            ..DRepVotingThresholds::default()
        };
        // key_deposit is economic-only
        let action = GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                key_deposit: Some(2_000_000),
                ..Default::default()
            },
            guardrails_script_hash: None,
        };
        let committee_state = CommitteeState::default();

        let selected = drep_threshold_for_action(&action, &committee_state, &thresholds);
        assert_eq!(selected, Some(thresholds.pp_economic_group));
    }

    #[test]
    fn drep_threshold_for_update_committee_depends_on_committee_state() {
        let thresholds = DRepVotingThresholds::default();
        let action = GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: BTreeMap::new(),
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        };

        let empty_committee = CommitteeState::default();
        let mut elected_committee = CommitteeState::default();
        elected_committee.register(StakeCredential::AddrKeyHash([0x11; 28]));

        assert_eq!(
            drep_threshold_for_action(&action, &empty_committee, &thresholds),
            Some(thresholds.committee_no_confidence)
        );
        assert_eq!(
            drep_threshold_for_action(&action, &elected_committee, &thresholds),
            Some(thresholds.committee_normal)
        );

        let mut resigned_only_committee = CommitteeState::default();
        let resigned = StakeCredential::AddrKeyHash([0x33; 28]);
        resigned_only_committee.register(resigned);
        resigned_only_committee
            .get_mut(&resigned)
            .expect("registered committee member")
            .set_authorization(Some(CommitteeAuthorization::CommitteeMemberResigned(None)));
        assert_eq!(
            drep_threshold_for_action(&action, &resigned_only_committee, &thresholds),
            Some(thresholds.committee_no_confidence)
        );
    }

    #[test]
    fn spo_threshold_for_update_committee_depends_on_committee_state() {
        let thresholds = PoolVotingThresholds::default();
        let action = GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: BTreeMap::new(),
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        };

        let empty_committee = CommitteeState::default();
        let mut elected_committee = CommitteeState::default();
        elected_committee.register(StakeCredential::AddrKeyHash([0x22; 28]));

        assert_eq!(
            spo_threshold_for_action(&action, &empty_committee, &thresholds),
            Some(thresholds.committee_no_confidence)
        );
        assert_eq!(
            spo_threshold_for_action(&action, &elected_committee, &thresholds),
            Some(thresholds.committee_normal)
        );

        let mut resigned_only_committee = CommitteeState::default();
        let resigned = StakeCredential::AddrKeyHash([0x44; 28]);
        resigned_only_committee.register(resigned);
        resigned_only_committee
            .get_mut(&resigned)
            .expect("registered committee member")
            .set_authorization(Some(CommitteeAuthorization::CommitteeMemberResigned(None)));
        assert_eq!(
            spo_threshold_for_action(&action, &resigned_only_committee, &thresholds),
            Some(thresholds.committee_no_confidence)
        );
    }

    #[test]
    fn spo_voter_permission_for_parameter_change_requires_security_group() {
        let voter = Voter::StakePool([9; 28]);
        let non_security_action = GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                drep_activity: Some(33),
                ..Default::default()
            },
            guardrails_script_hash: None,
        };
        let security_action = GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                max_block_ex_units: Some(crate::eras::alonzo::ExUnits {
                    mem: 100,
                    steps: 100,
                }),
                ..Default::default()
            },
            guardrails_script_hash: None,
        };

        assert!(!conway_voter_is_allowed_for_action(&voter, &non_security_action));
        assert!(conway_voter_is_allowed_for_action(&voter, &security_action));
    }

    // -- accepted_by_* predicates ---

    #[test]
    fn info_action_always_accepted() {
        let action = test_info_action();
        let cs = CommitteeState::default();
        let quorum = UnitInterval { numerator: 1, denominator: 1 };
        assert!(accepted_by_committee(&action, &cs, &quorum));
    }

    #[test]
    fn accepted_by_committee_happy_path() {
        let mut action = test_no_confidence_action();
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([1; 28]));
        cs.register(StakeCredential::AddrKeyHash([2; 28]));
        cs.register(StakeCredential::AddrKeyHash([3; 28]));

        action.votes.insert(Voter::CommitteeKeyHash([1; 28]), Vote::Yes);
        action.votes.insert(Voter::CommitteeKeyHash([2; 28]), Vote::Yes);
        // 3 does not vote.

        let quorum = UnitInterval { numerator: 2, denominator: 3 };
        assert!(accepted_by_committee(&action, &cs, &quorum)); // 2/3 >= 2/3
    }

    #[test]
    fn accepted_by_committee_rejected() {
        let mut action = test_no_confidence_action();
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([1; 28]));
        cs.register(StakeCredential::AddrKeyHash([2; 28]));
        cs.register(StakeCredential::AddrKeyHash([3; 28]));

        action.votes.insert(Voter::CommitteeKeyHash([1; 28]), Vote::Yes);
        // Only 1/3 yes.

        let quorum = UnitInterval { numerator: 2, denominator: 3 };
        assert!(!accepted_by_committee(&action, &cs, &quorum)); // 1/3 < 2/3
    }

    #[test]
    fn accepted_by_dreps_treasury_action() {
        let mut action = test_treasury_action();
        let committee_state = CommitteeState::default();
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));

        let mut stake = BTreeMap::new();
        stake.insert(drep, 1000);

        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

        let thresholds = DRepVotingThresholds::default();
        assert!(accepted_by_dreps(
            &action,
            &committee_state,
            &drep_state,
            &stake,
            EpochNo(5),
            100,
            &thresholds,
        )); // 100% yes >= 67%
    }

    // -- ratify_action combined ---

    #[test]
    fn ratify_info_action_always_passes() {
        let action = test_info_action();
        let cs = CommitteeState::default();
        let quorum = UnitInterval { numerator: 1, denominator: 1 };
        let drep_state = DrepState::new();
        let drep_stake = BTreeMap::new();
        let dvt = DRepVotingThresholds::default();
        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action,
            &cs,
            &quorum,
            &drep_state,
            &drep_stake,
            EpochNo(1),
            100,
            &dvt,
            &pool_dist,
            &pvt,
        ));
    }

    #[test]
    fn ratify_hf_rejected_when_dreps_insufficient() {
        let mut action = test_hf_action();
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([1; 28]));
        action.votes.insert(Voter::CommitteeKeyHash([1; 28]), Vote::Yes);

        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep, 1000);
        // DRep votes no.
        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::No);

        let dvt = DRepVotingThresholds::default();
        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let pvt = PoolVotingThresholds::default();
        let quorum = UnitInterval { numerator: 1, denominator: 2 };

        assert!(!ratify_action(
            &action,
            &cs,
            &quorum,
            &drep_state,
            &drep_stake,
            EpochNo(5),
            100,
            &dvt,
            &pool_dist,
            &pvt,
        ));
    }

    #[test]
    fn ratify_hf_accepted_when_all_roles_agree() {
        let mut action = test_hf_action();
        // CC: 1 member, votes yes.
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([1; 28]));
        action.votes.insert(Voter::CommitteeKeyHash([1; 28]), Vote::Yes);

        // DRep: 1 drep, votes yes.
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([2; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([2; 28]), Vote::Yes);

        // SPO: 1 pool, votes yes.
        let mut pool_stakes = BTreeMap::new();
        pool_stakes.insert([3u8; 28], 1000u64);
        let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);
        action.votes.insert(Voter::StakePool([3; 28]), Vote::Yes);

        let quorum = UnitInterval { numerator: 1, denominator: 2 };
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action,
            &cs,
            &quorum,
            &drep_state,
            &drep_stake,
            EpochNo(5),
            100,
            &dvt,
            &pool_dist,
            &pvt,
        ));
    }

    // -- Protocol params threshold round-trip ---

    #[test]
    fn pool_voting_thresholds_cbor_round_trip() {
        let thresholds = PoolVotingThresholds::default();
        let bytes = thresholds.to_cbor_bytes();
        let decoded = PoolVotingThresholds::from_cbor_bytes(&bytes).expect("round-trip");
        assert_eq!(thresholds, decoded);
    }

    #[test]
    fn drep_voting_thresholds_cbor_round_trip() {
        let thresholds = DRepVotingThresholds::default();
        let bytes = thresholds.to_cbor_bytes();
        let decoded = DRepVotingThresholds::from_cbor_bytes(&bytes).expect("round-trip");
        assert_eq!(thresholds, decoded);
    }

    #[test]
    fn protocol_params_with_voting_thresholds_round_trip() {
        let mut params = ProtocolParameters::alonzo_defaults();
        params.pool_voting_thresholds = Some(PoolVotingThresholds::default());
        params.drep_voting_thresholds = Some(DRepVotingThresholds::default());
        params.min_committee_size = Some(7);
        params.committee_term_limit = Some(146);
        let bytes = params.to_cbor_bytes();
        let decoded = ProtocolParameters::from_cbor_bytes(&bytes).expect("round-trip");
        assert_eq!(params, decoded);
    }

    // -----------------------------------------------------------------------
    // DRep inactivity boundary tests
    // -----------------------------------------------------------------------

    #[test]
    fn drep_tally_boundary_active_when_sum_equals_current() {
        // last_active=90, drep_activity=10, current_epoch=100.
        // 90+10 = 100. Condition: 100 < 100 → false → DRep is ACTIVE.
        let mut action = test_hf_action();
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(90)));

        let mut stake = BTreeMap::new();
        stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(100), 10, false);
        assert_eq!(tally.total, 1000, "DRep should be active at exact boundary");
        assert_eq!(tally.yes, 1000);
    }

    #[test]
    fn drep_tally_boundary_inactive_when_sum_less_than_current() {
        // last_active=90, drep_activity=10, current_epoch=101.
        // 90+10 = 100. Condition: 100 < 101 → true → DRep is INACTIVE.
        let mut action = test_hf_action();
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(90)));

        let mut stake = BTreeMap::new();
        stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(101), 10, false);
        assert_eq!(tally.total, 0, "DRep should be inactive one epoch past boundary");
        assert_eq!(tally.yes, 0);
    }

    #[test]
    fn drep_tally_no_last_active_epoch_is_active() {
        // DRep registered with no last_active_epoch (None) — should be counted.
        let mut action = test_hf_action();
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new(0, None));

        let mut stake = BTreeMap::new();
        stake.insert(drep, 500);
        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(999), 10, false);
        assert_eq!(tally.total, 500, "DRep with no last_active_epoch should be counted");
        assert_eq!(tally.yes, 500);
    }

    #[test]
    fn drep_tally_zero_activity_window() {
        // drep_activity=0. last_active=50, current=50. 50+0=50 < 50 → false → ACTIVE.
        let mut action = test_hf_action();
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(50)));

        let mut stake = BTreeMap::new();
        stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(50), 0, false);
        assert_eq!(tally.total, 1000, "DRep active when sum == current with zero window");

        // current=51: 50+0=50 < 51 → true → INACTIVE.
        let tally2 = tally_drep_votes(&action, &drep_state, &stake, EpochNo(51), 0, false);
        assert_eq!(tally2.total, 0, "DRep inactive when sum < current with zero window");
    }

    #[test]
    fn drep_tally_saturating_add_no_overflow() {
        // Ensure saturating_add prevents overflow: large last_active + large activity.
        let mut action = test_hf_action();
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(u64::MAX - 5)));

        let mut stake = BTreeMap::new();
        stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

        // (u64::MAX - 5) + 100 would overflow, saturates to u64::MAX.
        // u64::MAX < u64::MAX is false → DRep is ACTIVE.
        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(u64::MAX), 100, false);
        assert_eq!(tally.total, 1000, "saturating_add should prevent overflow");
    }

    // -----------------------------------------------------------------------
    // DRep tally: AlwaysAbstain and AlwaysNoConfidence special DReps
    // -----------------------------------------------------------------------

    #[test]
    fn drep_tally_always_abstain_excluded_from_active_vote() {
        // Stake delegated to AlwaysAbstain is not counted at all.
        let action = test_hf_action();
        let drep_state = DrepState::new();
        let mut stake = BTreeMap::new();
        stake.insert(DRep::AlwaysAbstain, 5000);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
        assert_eq!(tally.total, 0, "AlwaysAbstain stake not counted");
    }

    #[test]
    fn drep_tally_always_no_confidence_in_total_not_yes() {
        // AlwaysNoConfidence stake is included in total but NOT counted as
        // "Yes" for non-NoConfidence actions.
        let action = test_hf_action();
        let drep_state = DrepState::new();
        let mut stake = BTreeMap::new();
        stake.insert(DRep::AlwaysNoConfidence, 5000);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
        assert_eq!(tally.total, 5000, "AlwaysNoConfidence stake included in total");
        assert_eq!(tally.yes, 0, "Not counted as Yes for non-NoConfidence action");
    }

    #[test]
    fn drep_tally_non_voting_drep_counted_in_total() {
        // A registered active DRep who does NOT vote is still in the total
        // (their stake counts against the denominator).
        let action = test_hf_action(); // no DRep votes
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));

        let mut stake = BTreeMap::new();
        stake.insert(drep, 1000);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
        assert_eq!(tally.total, 1000, "non-voting DRep stake in total");
        assert_eq!(tally.yes, 0);
        assert_eq!(tally.no, 0);
        assert_eq!(tally.abstain, 0);
    }

    #[test]
    fn drep_tally_abstain_vote_counted_as_abstain() {
        let mut action = test_hf_action();
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));

        let mut stake = BTreeMap::new();
        stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Abstain);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
        assert_eq!(tally.abstain, 1000);
        assert_eq!(tally.total, 1000);
        // All abstain → vacuous quorum → passes any threshold.
        let threshold = UnitInterval { numerator: 99, denominator: 100 };
        assert!(tally.meets_threshold(&threshold));
    }

    // -----------------------------------------------------------------------
    // AlwaysNoConfidence auto-yes for NoConfidence actions
    // -----------------------------------------------------------------------

    #[test]
    fn drep_tally_always_no_confidence_auto_yes_for_no_confidence_action() {
        // AlwaysNoConfidence stake should count as auto-Yes for NoConfidence.
        let action = test_no_confidence_action();
        let drep_state = DrepState::new();
        let mut stake = BTreeMap::new();
        stake.insert(DRep::AlwaysNoConfidence, 5000);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, true);
        assert_eq!(tally.total, 5000, "AlwaysNoConfidence stake included in total");
        assert_eq!(tally.yes, 5000, "AlwaysNoConfidence stake counted as Yes");
    }

    #[test]
    fn drep_tally_always_no_confidence_not_yes_for_other_actions() {
        // For non-NoConfidence actions, AlwaysNoConfidence is in total but NOT Yes.
        let action = test_hf_action();
        let drep_state = DrepState::new();
        let mut stake = BTreeMap::new();
        stake.insert(DRep::AlwaysNoConfidence, 3000);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
        assert_eq!(tally.total, 3000);
        assert_eq!(tally.yes, 0, "Not auto-yes for non-NoConfidence action");
    }

    #[test]
    fn drep_tally_always_no_confidence_mixed_with_regular_dreps() {
        // AlwaysNoConfidence + registered DReps together.
        let mut action = test_no_confidence_action();
        let mut drep_state = DrepState::new();
        let drep_a = DRep::KeyHash([1; 28]);
        drep_state.register(drep_a.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));

        let mut stake = BTreeMap::new();
        stake.insert(DRep::AlwaysNoConfidence, 4000);
        stake.insert(drep_a, 6000);

        // DRep A votes No; AlwaysNoConfidence auto-yes.
        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::No);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, true);
        assert_eq!(tally.total, 10000);
        assert_eq!(tally.yes, 4000, "auto-yes from AlwaysNoConfidence");
        assert_eq!(tally.no, 6000, "explicit No from DRep A");

        // 4000/10000 = 40% vs threshold 67% → does NOT pass.
        let threshold = UnitInterval { numerator: 67, denominator: 100 };
        assert!(!tally.meets_threshold(&threshold));
    }

    #[test]
    fn drep_tally_always_no_confidence_pushes_no_confidence_past_threshold() {
        // AlwaysNoConfidence stake tips the balance for a NoConfidence action.
        let mut action = test_no_confidence_action();
        let mut drep_state = DrepState::new();
        let drep_a = DRep::KeyHash([1; 28]);
        drep_state.register(drep_a.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));

        let mut stake = BTreeMap::new();
        stake.insert(DRep::AlwaysNoConfidence, 5000);
        stake.insert(drep_a, 5000);

        // DRep A votes Yes; AlwaysNoConfidence also auto-yes → 10000/10000 = 100%.
        action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

        let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, true);
        assert_eq!(tally.yes, 10000);
        assert_eq!(tally.total, 10000);
        let threshold = UnitInterval { numerator: 67, denominator: 100 };
        assert!(tally.meets_threshold(&threshold));
    }

    // -----------------------------------------------------------------------
    // SPO tally edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn spo_tally_empty_pool_distribution() {
        let action = test_hf_action();
        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let tally = tally_spo_votes(&action, &pool_dist);
        assert_eq!(tally.total, 0);
        // Zero total is vacuous → meets any threshold.
        let threshold = UnitInterval { numerator: 1, denominator: 1 };
        assert!(tally.meets_threshold(&threshold));
    }

    #[test]
    fn spo_tally_non_voting_pool_in_total() {
        let action = test_hf_action(); // no SPO votes
        let mut pool_stakes = BTreeMap::new();
        pool_stakes.insert([1u8; 28], 2000u64);
        let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 2000);

        let tally = tally_spo_votes(&action, &pool_dist);
        assert_eq!(tally.total, 2000);
        assert_eq!(tally.yes, 0);
        // Non-voting pool means 0 yes out of 2000 → does NOT meet 51%.
        let threshold = UnitInterval { numerator: 51, denominator: 100 };
        assert!(!tally.meets_threshold(&threshold));
    }

    #[test]
    fn spo_tally_abstain_vote() {
        let mut action = test_hf_action();
        let mut pool_stakes = BTreeMap::new();
        pool_stakes.insert([1u8; 28], 1000u64);
        let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);
        action.votes.insert(Voter::StakePool([1; 28]), Vote::Abstain);

        let tally = tally_spo_votes(&action, &pool_dist);
        assert_eq!(tally.abstain, 1000);
        assert_eq!(tally.total, 1000);
        // All abstain → vacuous quorum.
        let threshold = UnitInterval { numerator: 99, denominator: 100 };
        assert!(tally.meets_threshold(&threshold));
    }

    // -----------------------------------------------------------------------
    // Committee tally edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn committee_tally_empty_committee_is_vacuous() {
        let action = test_hf_action();
        let cs = CommitteeState::default();

        let tally = tally_committee_votes(&action, &cs);
        assert_eq!(tally.total, 0);
        // Vacuous → passes any quorum.
        let quorum = UnitInterval { numerator: 1, denominator: 1 };
        assert!(tally.meets_threshold(&quorum));
    }

    #[test]
    fn committee_tally_all_resigned_is_vacuous() {
        let action = test_hf_action();
        let mut cs = CommitteeState::default();
        let cred = StakeCredential::AddrKeyHash([1; 28]);
        cs.register(cred);
        cs.get_mut(&cred).unwrap().set_authorization(Some(
            CommitteeAuthorization::CommitteeMemberResigned(None),
        ));

        let tally = tally_committee_votes(&action, &cs);
        assert_eq!(tally.total, 0, "all-resigned committee has zero eligible members");
    }

    #[test]
    fn committee_tally_single_member_exact_quorum() {
        let mut action = test_hf_action();
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([1; 28]));
        action.votes.insert(Voter::CommitteeKeyHash([1; 28]), Vote::Yes);

        let tally = tally_committee_votes(&action, &cs);
        assert_eq!(tally.yes, 1);
        assert_eq!(tally.total, 1);
        // 1/1 >= 100% quorum.
        let quorum = UnitInterval { numerator: 1, denominator: 1 };
        assert!(tally.meets_threshold(&quorum));
    }

    #[test]
    fn committee_member_votes_no() {
        let mut action = test_hf_action();
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([1; 28]));
        action.votes.insert(Voter::CommitteeKeyHash([1; 28]), Vote::No);

        let tally = tally_committee_votes(&action, &cs);
        assert_eq!(tally.no, 1);
        assert_eq!(tally.yes, 0);
        assert_eq!(tally.total, 1);
        let quorum = UnitInterval { numerator: 1, denominator: 2 };
        assert!(!tally.meets_threshold(&quorum));
    }

    // -----------------------------------------------------------------------
    // accepted_by_spo: actions that don't require SPO votes
    // -----------------------------------------------------------------------

    #[test]
    fn accepted_by_spo_treasury_always_true() {
        // TreasuryWithdrawals doesn't require SPO vote → always accepted.
        let action = test_treasury_action();
        let cs = CommitteeState::default();
        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let pvt = PoolVotingThresholds::default();
        assert!(accepted_by_spo(&action, &cs, &pool_dist, &pvt));
    }

    #[test]
    fn accepted_by_spo_new_constitution_always_true() {
        let action = GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::NewConstitution {
                prev_action_id: None,
                constitution: sample_constitution("test"),
            },
            anchor: crate::types::Anchor { url: String::new(), data_hash: [0; 32] },
        });
        let cs = CommitteeState::default();
        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let pvt = PoolVotingThresholds::default();
        assert!(accepted_by_spo(&action, &cs, &pool_dist, &pvt));
    }

    #[test]
    fn accepted_by_dreps_info_always_true() {
        // InfoAction has no DRep threshold → always accepted.
        let action = test_info_action();
        let cs = CommitteeState::default();
        let drep_state = DrepState::new();
        let drep_stake = BTreeMap::new();
        let dvt = DRepVotingThresholds::default();
        assert!(accepted_by_dreps(&action, &cs, &drep_state, &drep_stake, EpochNo(1), 100, &dvt));
    }

    // -----------------------------------------------------------------------
    // Ratification: NoConfidence (CC + DRep + SPO all required)
    // -----------------------------------------------------------------------

    fn test_param_change_security_action() -> GovernanceActionState {
        GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    max_block_body_size: Some(65536), // network+security group
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor { url: String::new(), data_hash: [0; 32] },
        })
    }

    fn test_param_change_economic_action() -> GovernanceActionState {
        GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    key_deposit: Some(2_000_000), // economic group only
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor { url: String::new(), data_hash: [0; 32] },
        })
    }

    fn test_update_committee_action() -> GovernanceActionState {
        GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add: BTreeMap::new(),
                quorum: UnitInterval { numerator: 1, denominator: 2 },
            },
            anchor: crate::types::Anchor { url: String::new(), data_hash: [0; 32] },
        })
    }

    fn test_new_constitution_action() -> GovernanceActionState {
        GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::NewConstitution {
                prev_action_id: None,
                constitution: sample_constitution("ratify-test"),
            },
            anchor: crate::types::Anchor { url: String::new(), data_hash: [0; 32] },
        })
    }

    /// Helper: minimal committee with one member who votes yes.
    fn setup_cc_one_yes(
        action: &mut GovernanceActionState,
    ) -> (CommitteeState, UnitInterval) {
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([0xCC; 28]));
        action.votes.insert(Voter::CommitteeKeyHash([0xCC; 28]), Vote::Yes);
        let quorum = UnitInterval { numerator: 1, denominator: 2 };
        (cs, quorum)
    }

    /// Helper: one DRep with given stake who votes yes.
    fn setup_drep_one_yes(
        action: &mut GovernanceActionState,
        drep_id: u8,
        stake_amount: u64,
    ) -> (DrepState, BTreeMap<DRep, u64>) {
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([drep_id; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        let mut stake = BTreeMap::new();
        stake.insert(drep, stake_amount);
        action.votes.insert(Voter::DRepKeyHash([drep_id; 28]), Vote::Yes);
        (drep_state, stake)
    }

    /// Helper: one pool with given stake that votes yes.
    fn setup_spo_one_yes(
        action: &mut GovernanceActionState,
        pool_id: u8,
        pool_stake: u64,
    ) -> crate::stake::PoolStakeDistribution {
        let mut pool_stakes = BTreeMap::new();
        pool_stakes.insert([pool_id; 28], pool_stake);
        action.votes.insert(Voter::StakePool([pool_id; 28]), Vote::Yes);
        crate::stake::PoolStakeDistribution::from_raw(pool_stakes, pool_stake)
    }

    #[test]
    fn ratify_no_confidence_accepted_when_all_agree() {
        let mut action = test_no_confidence_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_no_confidence_rejected_when_dreps_vote_no() {
        let mut action = test_no_confidence_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        // DRep votes no
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([0xD1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);

        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_no_confidence_rejected_when_spo_vote_no() {
        let mut action = test_no_confidence_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

        // SPO votes no
        let mut pool_stakes = BTreeMap::new();
        pool_stakes.insert([0xA1; 28], 1000u64);
        action.votes.insert(Voter::StakePool([0xA1; 28]), Vote::No);
        let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_no_confidence_rejected_when_committee_votes_no() {
        let mut action = test_no_confidence_action();
        // CC member votes no
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([0xCC; 28]));
        action.votes.insert(Voter::CommitteeKeyHash([0xCC; 28]), Vote::No);
        let quorum = UnitInterval { numerator: 1, denominator: 2 };

        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    // -----------------------------------------------------------------------
    // Ratification: ParameterChange
    // -----------------------------------------------------------------------

    #[test]
    fn ratify_param_change_security_accepted() {
        // Security-group change: requires CC + DRep + SPO.
        let mut action = test_param_change_security_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_param_change_security_rejected_without_spo() {
        // Security-group change requires SPO. If SPO votes no → rejected.
        let mut action = test_param_change_security_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

        // SPO votes no.
        let mut pool_stakes = BTreeMap::new();
        pool_stakes.insert([0xA1; 28], 1000u64);
        action.votes.insert(Voter::StakePool([0xA1; 28]), Vote::No);
        let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_param_change_economic_no_spo_needed() {
        // Economic-only change: CC + DRep required, SPO NOT required.
        let mut action = test_param_change_economic_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

        // No SPO votes, empty pool dist — should still pass.
        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_param_change_rejected_when_dreps_insufficient() {
        let mut action = test_param_change_economic_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        // DRep votes no.
        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([0xD1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);

        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    // -----------------------------------------------------------------------
    // Ratification: TreasuryWithdrawals (CC + DRep, no SPO)
    // -----------------------------------------------------------------------

    #[test]
    fn ratify_treasury_accepted_cc_and_drep() {
        let mut action = test_treasury_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

        // No SPO needed for treasury.
        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_treasury_rejected_when_dreps_vote_no() {
        let mut action = test_treasury_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([0xD1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);

        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_treasury_rejected_when_committee_fails() {
        let mut action = test_treasury_action();
        // CC votes no.
        let mut cs = CommitteeState::default();
        cs.register(StakeCredential::AddrKeyHash([0xCC; 28]));
        action.votes.insert(Voter::CommitteeKeyHash([0xCC; 28]), Vote::No);
        let quorum = UnitInterval { numerator: 1, denominator: 2 };

        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    // -----------------------------------------------------------------------
    // Ratification: NewConstitution (CC + DRep, no SPO)
    // -----------------------------------------------------------------------

    #[test]
    fn ratify_new_constitution_accepted() {
        let mut action = test_new_constitution_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_new_constitution_rejected_when_dreps_vote_no() {
        let mut action = test_new_constitution_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([0xD1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);

        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    // -----------------------------------------------------------------------
    // Ratification: UpdateCommittee (DRep + SPO, CC not required for
    // committee changes — actually CC IS required per accepted_by_committee)
    // -----------------------------------------------------------------------

    #[test]
    fn ratify_update_committee_accepted_all_agree() {
        let mut action = test_update_committee_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_update_committee_rejected_when_spo_votes_no() {
        let mut action = test_update_committee_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

        // SPO votes no.
        let mut pool_stakes = BTreeMap::new();
        pool_stakes.insert([0xA1; 28], 1000u64);
        action.votes.insert(Voter::StakePool([0xA1; 28]), Vote::No);
        let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    // -----------------------------------------------------------------------
    // Ratification: DRep inactivity affects ratification outcome
    // -----------------------------------------------------------------------

    #[test]
    fn ratify_hf_rejected_when_only_drep_is_inactive() {
        let mut action = test_hf_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        // DRep registered at epoch 10, activity window 10, current epoch 25.
        // 10 + 10 = 20 < 25 → inactive. No active DReps = vacuous → passes.
        // BUT: let's add a second DRep that is active and votes No.
        let mut drep_state = DrepState::new();
        let drep_inactive = DRep::KeyHash([0xD1; 28]);
        drep_state.register(drep_inactive.clone(), RegisteredDrep::new_active(0, None, EpochNo(10)));
        let drep_active = DRep::KeyHash([0xD2; 28]);
        drep_state.register(drep_active.clone(), RegisteredDrep::new_active(0, None, EpochNo(20)));

        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep_inactive, 1000);
        drep_stake.insert(drep_active, 1000);

        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes); // inactive, excluded
        action.votes.insert(Voter::DRepKeyHash([0xD2; 28]), Vote::No);  // active, counted

        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        // Only active DRep voted No → 0/1000 yes → fails DRep threshold.
        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(25), 10, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_hf_accepted_when_inactive_dreps_excluded_and_active_vote_yes() {
        let mut action = test_hf_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        // Inactive DRep with large NO stake is excluded; active DRep votes yes.
        let mut drep_state = DrepState::new();
        let drep_inactive = DRep::KeyHash([0xD1; 28]);
        drep_state.register(drep_inactive.clone(), RegisteredDrep::new_active(0, None, EpochNo(10)));
        let drep_active = DRep::KeyHash([0xD2; 28]);
        drep_state.register(drep_active.clone(), RegisteredDrep::new_active(0, None, EpochNo(20)));

        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep_inactive, 9000);
        drep_stake.insert(drep_active, 1000);

        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No); // inactive, excluded
        action.votes.insert(Voter::DRepKeyHash([0xD2; 28]), Vote::Yes);

        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        // Active DRep: 1000 yes / 1000 total = 100% >= 67%. Passes.
        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(25), 10, &dvt,
            &pool_dist, &pvt,
        ));
    }

    // -----------------------------------------------------------------------
    // Ratification: Multi-voter edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn ratify_hf_rejected_partial_drep_support() {
        // Two DReps: 40% yes, 60% no → fails 67% threshold.
        let mut action = test_hf_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        let mut drep_state = DrepState::new();
        let drep_a = DRep::KeyHash([0xD1; 28]);
        let drep_b = DRep::KeyHash([0xD2; 28]);
        drep_state.register(drep_a.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        drep_state.register(drep_b.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));

        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep_a, 400);
        drep_stake.insert(drep_b, 600);

        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
        action.votes.insert(Voter::DRepKeyHash([0xD2; 28]), Vote::No);

        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        // 400/1000 = 40% < 67% → fails.
        assert!(!ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_hf_accepted_with_abstentions_raising_effective_ratio() {
        // One DRep yes (500), one DRep abstain (500). Active = 500.
        // 500/500 = 100% >= 67%.
        let mut action = test_hf_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        let mut drep_state = DrepState::new();
        let drep_a = DRep::KeyHash([0xD1; 28]);
        let drep_b = DRep::KeyHash([0xD2; 28]);
        drep_state.register(drep_a.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        drep_state.register(drep_b.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));

        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep_a, 500);
        drep_stake.insert(drep_b, 500);

        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
        action.votes.insert(Voter::DRepKeyHash([0xD2; 28]), Vote::Abstain);

        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_empty_committee_is_vacuous() {
        // No CC members → accepted_by_committee returns vacuous pass for
        // non-Info actions.
        let mut action = test_hf_action();
        let cs = CommitteeState::default(); // empty
        let quorum = UnitInterval { numerator: 1, denominator: 1 };
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_all_dreps_abstain_is_vacuous_pass() {
        // All DReps abstain → vacuous quorum → DRep check passes.
        let mut action = test_hf_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([0xD1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));
        let mut drep_stake = BTreeMap::new();
        drep_stake.insert(drep, 1000);
        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Abstain);

        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_no_dreps_registered_is_vacuous() {
        // No registered DReps → total=0 → vacuous pass.
        let mut action = test_hf_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);

        let drep_state = DrepState::new();
        let drep_stake = BTreeMap::new();

        let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    #[test]
    fn ratify_no_pools_registered_is_vacuous_for_hf() {
        // HF requires SPO vote. No pools → total=0 → vacuous pass.
        let mut action = test_hf_action();
        let (cs, quorum) = setup_cc_one_yes(&mut action);
        let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

        let pool_dist = crate::stake::PoolStakeDistribution::default();
        let dvt = DRepVotingThresholds::default();
        let pvt = PoolVotingThresholds::default();

        assert!(ratify_action(
            &action, &cs, &quorum,
            &drep_state, &drep_stake, EpochNo(5), 100, &dvt,
            &pool_dist, &pvt,
        ));
    }

    // -----------------------------------------------------------------------
    // VoteTally threshold edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn tally_fractional_threshold_cross_multiply() {
        // Verify cross-multiplication works for non-trivial fractions.
        // 3 yes out of 7 active. Threshold 2/5. 3*5 = 15 >= 2*7 = 14 → passes.
        let tally = VoteTally { yes: 3, no: 4, abstain: 0, total: 7 };
        let threshold = UnitInterval { numerator: 2, denominator: 5 };
        assert!(tally.meets_threshold(&threshold));
    }

    #[test]
    fn tally_fractional_threshold_just_below() {
        // 2 yes out of 7 active. Threshold 2/5. 2*5 = 10 < 2*7 = 14 → fails.
        let tally = VoteTally { yes: 2, no: 5, abstain: 0, total: 7 };
        let threshold = UnitInterval { numerator: 2, denominator: 5 };
        assert!(!tally.meets_threshold(&threshold));
    }

    #[test]
    fn tally_100_percent_threshold_requires_unanimity() {
        let tally = VoteTally { yes: 99, no: 1, abstain: 0, total: 100 };
        let threshold = UnitInterval { numerator: 1, denominator: 1 };
        assert!(!tally.meets_threshold(&threshold));

        let tally_unanimous = VoteTally { yes: 100, no: 0, abstain: 0, total: 100 };
        assert!(tally_unanimous.meets_threshold(&threshold));
    }

    #[test]
    fn tally_zero_numerator_threshold_always_passes() {
        // 0% threshold → 0 yes suffices.
        let tally = VoteTally { yes: 0, no: 100, abstain: 0, total: 100 };
        let threshold = UnitInterval { numerator: 0, denominator: 1 };
        assert!(tally.meets_threshold(&threshold));
    }

    // -----------------------------------------------------------------------
    // Proposal validation: ParameterChange edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn proposal_rejects_empty_parameter_change() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate::default(),
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn proposal_rejects_zero_drep_deposit() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    drep_deposit: Some(0),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn proposal_rejects_zero_min_committee_size() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    min_committee_size: Some(0),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn proposal_rejects_zero_gov_action_lifetime() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    gov_action_lifetime: Some(0),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn proposal_rejects_zero_drep_activity() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    drep_activity: Some(0),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn proposal_rejects_zero_committee_term_limit() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    committee_term_limit: Some(0),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn proposal_rejects_malformed_pool_voting_thresholds() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    pool_voting_thresholds: Some(crate::protocol_params::PoolVotingThresholds {
                        // numerator > denominator → invalid
                        motion_no_confidence: UnitInterval { numerator: 3, denominator: 2 },
                        ..crate::protocol_params::PoolVotingThresholds::default()
                    }),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn proposal_rejects_malformed_drep_voting_thresholds() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    drep_voting_thresholds: Some(DRepVotingThresholds {
                        // zero denominator → invalid
                        treasury_withdrawal: UnitInterval { numerator: 0, denominator: 0 },
                        ..DRepVotingThresholds::default()
                    }),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
    }

    #[test]
    fn proposal_accepts_valid_parameter_change() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    key_deposit: Some(2_000_000),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Proposal validation: deposit and reward account checks
    // -----------------------------------------------------------------------

    #[test]
    fn proposal_rejects_incorrect_deposit() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let proposals = vec![sample_proposal(GovAction::InfoAction, 500, 1)];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            Some(1000), // expected deposit = 1000
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::ProposalDepositIncorrect { supplied: 500, expected: 1000 })
        ));
    }

    #[test]
    fn proposal_rejects_unregistered_return_account() {
        let es = EnactState::default();
        // Return account for ra_id=1 but only register ra_id=2.
        let stake_creds = empty_stake_creds_with(2);
        let proposals = vec![sample_proposal(GovAction::InfoAction, 1, 1)];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::RewardAccountNotRegistered(_))
        ));
    }

    // -----------------------------------------------------------------------
    // Proposal validation: TreasuryWithdrawals edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn proposal_rejects_zero_treasury_withdrawals() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let mut withdrawals = BTreeMap::new();
        withdrawals.insert(sample_reward_account(1), 0);
        let proposals = vec![sample_proposal(
            GovAction::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::ZeroTreasuryWithdrawals(_))
        ));
    }

    #[test]
    fn proposal_rejects_treasury_withdrawal_to_unregistered_account() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let mut withdrawals = BTreeMap::new();
        // Withdrawal target ra_id=2 is not registered.
        withdrawals.insert(sample_reward_account(2), 1_000_000);
        let proposals = vec![sample_proposal(
            GovAction::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::RewardAccountNotRegistered(_))
        ));
    }

    #[test]
    fn proposal_rejects_treasury_withdrawal_network_mismatch() {
        let es = EnactState::default();
        let mut stake_creds = StakeCredentials::new();
        // Register the return account credential (ra_id=1).
        stake_creds.register(crate::StakeCredential::AddrKeyHash([1; 28]));
        // Register the treasury withdrawal target credential.
        let cred = crate::StakeCredential::AddrKeyHash([0x77; 28]);
        stake_creds.register(cred);
        let ra = RewardAccount { network: 0, credential: cred };

        let mut withdrawals = BTreeMap::new();
        withdrawals.insert(ra, 1_000_000);

        // Use return account with network=1 (matches expected_network).
        let proposals = vec![sample_proposal(
            GovAction::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            Some(1), // expected network = 1
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::TreasuryWithdrawalsNetworkIdMismatch { .. })
        ));
    }

    // -----------------------------------------------------------------------
    // Proposal validation: UpdateCommittee edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn proposal_rejects_conflicting_committee_update() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let conflicting_cred = crate::StakeCredential::AddrKeyHash([0x99; 28]);
        let mut members_to_add = BTreeMap::new();
        members_to_add.insert(conflicting_cred, 100);
        let proposals = vec![sample_proposal(
            GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![conflicting_cred],
                members_to_add,
                quorum: UnitInterval { numerator: 1, denominator: 2 },
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::ConflictingCommitteeUpdate(_))
        ));
    }

    #[test]
    fn proposal_rejects_committee_member_expiring_at_current_epoch() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let mut members_to_add = BTreeMap::new();
        // Epoch 10 — member expiring at epoch 10 is not strictly after.
        members_to_add.insert(
            crate::StakeCredential::AddrKeyHash([0xAA; 28]),
            10,
        );
        let proposals = vec![sample_proposal(
            GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval { numerator: 1, denominator: 2 },
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            crate::types::TxId([0xAA; 32]),
            &proposals,
            EpochNo(10),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::ExpirationEpochTooSmall(_))
        ));
    }

    // -----------------------------------------------------------------------
    // Proposal validation: forward self-reference
    // -----------------------------------------------------------------------

    #[test]
    fn proposal_rejects_forward_self_reference() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let tx_id = crate::types::TxId([0xBB; 32]);
        // Proposal at index 0 references gov_action_index 0 in same tx → forward self-ref.
        let proposals = vec![sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: Some(crate::eras::conway::GovActionId {
                    transaction_id: tx_id.0,
                    gov_action_index: 0,
                }),
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    key_deposit: Some(2_000_000),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        )];
        let result = validate_conway_proposals(
            tx_id,
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::InvalidPrevGovActionId(_))
        ));
    }

    #[test]
    fn proposal_rejects_forward_reference_later_in_same_tx() {
        let es = EnactState::default();
        let stake_creds = empty_stake_creds_with(1);
        let tx_id = crate::types::TxId([0xBB; 32]);
        // Proposal at index 0 referencing index 1 (forward ref).
        let proposals = vec![
            sample_proposal(
                GovAction::ParameterChange {
                    prev_action_id: Some(crate::eras::conway::GovActionId {
                        transaction_id: tx_id.0,
                        gov_action_index: 1,
                    }),
                    protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                        key_deposit: Some(2_000_000),
                        ..Default::default()
                    },
                    guardrails_script_hash: None,
                },
                1,
                1,
            ),
            sample_proposal(
                GovAction::ParameterChange {
                    prev_action_id: None,
                    protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                        key_deposit: Some(3_000_000),
                        ..Default::default()
                    },
                    guardrails_script_hash: None,
                },
                1,
                1,
            ),
        ];
        let result = validate_conway_proposals(
            tx_id,
            &proposals,
            EpochNo(0),
            &BTreeMap::new(),
            &stake_creds,
            None,
            None,
            None,
            None,
            &es,
        );
        assert!(matches!(
            result,
            Err(LedgerError::InvalidPrevGovActionId(_))
        ));
    }

    // -----------------------------------------------------------------------
    // Vote recasting and DRep vote removal on unregistration
    // -----------------------------------------------------------------------

    #[test]
    fn vote_recast_overwrites_previous_vote() {
        let gov_id = crate::eras::conway::GovActionId {
            transaction_id: [0x01; 32],
            gov_action_index: 0,
        };
        let mut governance_actions = BTreeMap::new();
        governance_actions.insert(gov_id.clone(), test_info_action());

        let voter = Voter::DRepKeyHash([0xD1; 28]);
        let mut drep_state = DrepState::new();
        drep_state.register(DRep::KeyHash([0xD1; 28]), RegisteredDrep::new_active(0, None, EpochNo(1)));

        // First vote: Yes
        let mut procedures = crate::eras::conway::VotingProcedures { procedures: BTreeMap::new() };
        let mut votes = BTreeMap::new();
        votes.insert(gov_id.clone(), crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        });
        procedures.procedures.insert(voter.clone(), votes);
        apply_conway_votes(&procedures, &mut governance_actions, &mut drep_state, EpochNo(5));
        assert_eq!(
            governance_actions[&gov_id].votes.get(&voter),
            Some(&Vote::Yes),
        );

        // Second vote: changes to No → overwrites.
        let mut votes2 = BTreeMap::new();
        votes2.insert(gov_id.clone(), crate::eras::conway::VotingProcedure {
            vote: Vote::No,
            anchor: None,
        });
        let mut procedures2 = crate::eras::conway::VotingProcedures { procedures: BTreeMap::new() };
        procedures2.procedures.insert(voter.clone(), votes2);
        apply_conway_votes(&procedures2, &mut governance_actions, &mut drep_state, EpochNo(5));
        assert_eq!(
            governance_actions[&gov_id].votes.get(&voter),
            Some(&Vote::No),
        );
    }

    #[test]
    fn vote_casting_touches_drep_activity() {
        let gov_id = crate::eras::conway::GovActionId {
            transaction_id: [0x01; 32],
            gov_action_index: 0,
        };
        let mut governance_actions = BTreeMap::new();
        governance_actions.insert(gov_id.clone(), test_info_action());

        let mut drep_state = DrepState::new();
        let drep = DRep::KeyHash([0xD1; 28]);
        drep_state.register(drep.clone(), RegisteredDrep::new_active(0, None, EpochNo(1)));

        let mut procedures = crate::eras::conway::VotingProcedures { procedures: BTreeMap::new() };
        let mut votes = BTreeMap::new();
        votes.insert(gov_id.clone(), crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        });
        procedures.procedures.insert(Voter::DRepKeyHash([0xD1; 28]), votes);
        apply_conway_votes(&procedures, &mut governance_actions, &mut drep_state, EpochNo(42));

        assert_eq!(
            drep_state.get(&drep).unwrap().last_active_epoch(),
            Some(EpochNo(42)),
        );
    }

    #[test]
    fn drep_unregistration_removes_stored_votes() {
        let gov_id = crate::eras::conway::GovActionId {
            transaction_id: [0x01; 32],
            gov_action_index: 0,
        };
        let mut governance_actions = BTreeMap::new();
        let mut action = test_info_action();
        action.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
        action.votes.insert(Voter::DRepKeyHash([0xD2; 28]), Vote::No);
        governance_actions.insert(gov_id.clone(), action);

        // Simulate DRep [D1] unregistering.
        let unregistered = vec![Voter::DRepKeyHash([0xD1; 28])];
        remove_conway_drep_votes(&unregistered, &mut governance_actions);

        // D1's vote removed, D2's vote preserved.
        assert!(!governance_actions[&gov_id].votes.contains_key(&Voter::DRepKeyHash([0xD1; 28])));
        assert_eq!(
            governance_actions[&gov_id].votes.get(&Voter::DRepKeyHash([0xD2; 28])),
            Some(&Vote::No),
        );
    }

    #[test]
    fn drep_unregistration_removes_votes_across_multiple_actions() {
        let gov_id_1 = crate::eras::conway::GovActionId { transaction_id: [1; 32], gov_action_index: 0 };
        let gov_id_2 = crate::eras::conway::GovActionId { transaction_id: [2; 32], gov_action_index: 0 };
        let mut governance_actions = BTreeMap::new();

        let mut action_1 = test_info_action();
        action_1.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
        governance_actions.insert(gov_id_1.clone(), action_1);

        let mut action_2 = test_hf_action();
        action_2.votes.insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);
        governance_actions.insert(gov_id_2.clone(), action_2);

        let unregistered = vec![Voter::DRepKeyHash([0xD1; 28])];
        remove_conway_drep_votes(&unregistered, &mut governance_actions);

        assert!(!governance_actions[&gov_id_1].votes.contains_key(&Voter::DRepKeyHash([0xD1; 28])));
        assert!(!governance_actions[&gov_id_2].votes.contains_key(&Voter::DRepKeyHash([0xD1; 28])));
    }

    #[test]
    fn collect_unregistered_drep_voters_from_certs() {
        let certificates = vec![
            DCert::DrepUnregistration(
                StakeCredential::AddrKeyHash([0xD1; 28]),
                0,
            ),
            DCert::DrepUnregistration(
                StakeCredential::ScriptHash([0xD2; 28]),
                0,
            ),
        ];
        let unregistered = collect_conway_unregistered_drep_voters(Some(&certificates));
        assert_eq!(unregistered.len(), 2);
        assert!(unregistered.contains(&Voter::DRepKeyHash([0xD1; 28])));
        assert!(unregistered.contains(&Voter::DRepScript([0xD2; 28])));
    }

    #[test]
    fn collect_unregistered_drep_voters_deduplicates() {
        // Same DRep unregistered twice but only one entry.
        let certificates = vec![
            DCert::DrepUnregistration(StakeCredential::AddrKeyHash([0xD1; 28]), 0),
            DCert::DrepUnregistration(StakeCredential::AddrKeyHash([0xD1; 28]), 0),
        ];
        let unregistered = collect_conway_unregistered_drep_voters(Some(&certificates));
        assert_eq!(unregistered.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Voter existence checks
    // -----------------------------------------------------------------------

    #[test]
    fn voter_exists_drep_script_hash() {
        let pool_state = PoolState::new();
        let committee_state = CommitteeState::default();
        let mut drep_state = DrepState::new();
        drep_state.register(DRep::ScriptHash([0xAB; 28]), RegisteredDrep::new(0, None));

        let voter = Voter::DRepScript([0xAB; 28]);
        assert!(conway_voter_exists(&voter, &pool_state, &committee_state, &drep_state));

        let unknown_voter = Voter::DRepScript([0xCD; 28]);
        assert!(!conway_voter_exists(&unknown_voter, &pool_state, &committee_state, &drep_state));
    }

    #[test]
    fn voter_exists_committee_script_hash() {
        let pool_state = PoolState::new();
        let mut committee_state = CommitteeState::default();
        let cold_cred = StakeCredential::AddrKeyHash([0x01; 28]);
        committee_state.register(cold_cred);
        // Authorize hot key as a script hash.
        committee_state
            .get_mut(&cold_cred)
            .unwrap()
            .set_authorization(Some(CommitteeAuthorization::CommitteeHotCredential(
                StakeCredential::ScriptHash([0xEE; 28]),
            )));
        let drep_state = DrepState::new();

        let voter = Voter::CommitteeScript([0xEE; 28]);
        assert!(conway_voter_exists(&voter, &pool_state, &committee_state, &drep_state));

        let unknown_voter = Voter::CommitteeScript([0xFF; 28]);
        assert!(!conway_voter_exists(&unknown_voter, &pool_state, &committee_state, &drep_state));
    }

    #[test]
    fn voter_exists_spo() {
        let mut pool_state = PoolState::new();
        pool_state.register(
            crate::types::PoolParams {
                operator: [0x01; 28],
                vrf_keyhash: [0; 32],
                pledge: 0,
                cost: 0,
                margin: UnitInterval { numerator: 0, denominator: 1 },
                reward_account: sample_reward_account(1),
                pool_owners: vec![],
                relays: vec![],
                pool_metadata: None,
            },
        );
        let committee_state = CommitteeState::default();
        let drep_state = DrepState::new();

        let voter = Voter::StakePool([0x01; 28]);
        assert!(conway_voter_exists(&voter, &pool_state, &committee_state, &drep_state));

        let unknown_voter = Voter::StakePool([0x02; 28]);
        assert!(!conway_voter_exists(&unknown_voter, &pool_state, &committee_state, &drep_state));
    }

    // -----------------------------------------------------------------------
    // Post-bootstrap voter permission matrix (complete)
    // -----------------------------------------------------------------------

    #[test]
    fn post_bootstrap_spo_rejected_on_treasury_withdrawals() {
        let voter = Voter::StakePool([9; 28]);
        let action = GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(),
            guardrails_script_hash: None,
        };
        assert!(!conway_voter_is_allowed_for_action(&voter, &action));
    }

    #[test]
    fn post_bootstrap_spo_accepted_on_no_confidence() {
        let voter = Voter::StakePool([9; 28]);
        let action = GovAction::NoConfidence { prev_action_id: None };
        assert!(conway_voter_is_allowed_for_action(&voter, &action));
    }

    #[test]
    fn post_bootstrap_spo_accepted_on_hard_fork() {
        let voter = Voter::StakePool([9; 28]);
        let action = GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (11, 0),
        };
        assert!(conway_voter_is_allowed_for_action(&voter, &action));
    }

    #[test]
    fn post_bootstrap_spo_accepted_on_update_committee() {
        let voter = Voter::StakePool([9; 28]);
        let action = GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: BTreeMap::new(),
            quorum: UnitInterval { numerator: 1, denominator: 2 },
        };
        assert!(conway_voter_is_allowed_for_action(&voter, &action));
    }

    #[test]
    fn post_bootstrap_spo_rejected_on_new_constitution() {
        let voter = Voter::StakePool([9; 28]);
        let action = GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("spo-test"),
        };
        assert!(!conway_voter_is_allowed_for_action(&voter, &action));
    }

    #[test]
    fn post_bootstrap_committee_accepted_on_most_actions() {
        let voter = Voter::CommitteeKeyHash([9; 28]);
        // Committee can vote on everything except NoConfidence per Conway rules.
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::InfoAction));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::HardForkInitiation {
            prev_action_id: None, protocol_version: (11, 0),
        }));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(), guardrails_script_hash: None,
        }));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::NewConstitution {
            prev_action_id: None, constitution: sample_constitution("cc"),
        }));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                key_deposit: Some(1), ..Default::default()
            },
            guardrails_script_hash: None,
        }));
    }

    #[test]
    fn post_bootstrap_committee_rejected_on_no_confidence() {
        let voter = Voter::CommitteeKeyHash([9; 28]);
        let action = GovAction::NoConfidence { prev_action_id: None };
        assert!(!conway_voter_is_allowed_for_action(&voter, &action));
    }

    #[test]
    fn post_bootstrap_drep_accepted_on_all_actions() {
        let voter = Voter::DRepKeyHash([9; 28]);
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::InfoAction));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::NoConfidence {
            prev_action_id: None,
        }));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::HardForkInitiation {
            prev_action_id: None, protocol_version: (11, 0),
        }));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(), guardrails_script_hash: None,
        }));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::NewConstitution {
            prev_action_id: None, constitution: sample_constitution("drep"),
        }));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: BTreeMap::new(),
            quorum: UnitInterval { numerator: 1, denominator: 2 },
        }));
        assert!(conway_voter_is_allowed_for_action(&voter, &GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                key_deposit: Some(1), ..Default::default()
            },
            guardrails_script_hash: None,
        }));
    }

    // -----------------------------------------------------------------------
    // conway_unit_interval_well_formed
    // -----------------------------------------------------------------------

    #[test]
    fn unit_interval_well_formed_valid() {
        assert!(conway_unit_interval_well_formed(&UnitInterval { numerator: 0, denominator: 1 }));
        assert!(conway_unit_interval_well_formed(&UnitInterval { numerator: 1, denominator: 1 }));
        assert!(conway_unit_interval_well_formed(&UnitInterval { numerator: 2, denominator: 3 }));
    }

    #[test]
    fn unit_interval_well_formed_invalid() {
        // Zero denominator.
        assert!(!conway_unit_interval_well_formed(&UnitInterval { numerator: 0, denominator: 0 }));
        // Numerator > denominator.
        assert!(!conway_unit_interval_well_formed(&UnitInterval { numerator: 2, denominator: 1 }));
    }

    // ── Certificate processing unit tests ──────────────────────────

    /// Helper: default CertificateValidationContext for cert unit tests.
    fn sample_cert_ctx() -> CertificateValidationContext {
        CertificateValidationContext {
            key_deposit: 2_000_000,
            pool_deposit: 500_000_000,
            min_pool_cost: 170_000_000,
            e_max: 18,
            current_epoch: EpochNo(100),
            expected_network_id: Some(1),
        }
    }

    #[test]
    fn test_cert_account_registration_deposit() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();
        let cred = crate::StakeCredential::AddrKeyHash([0xC1; 28]);

        let certs = vec![DCert::AccountRegistrationDeposit(cred, 5_000_000)];
        let total = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
        assert_eq!(total, 0);
        assert!(sc.is_registered(&cred));
        assert_eq!(dp.key_deposits, 5_000_000);
    }

    #[test]
    fn test_cert_account_unregistration_deposit() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let cred = crate::StakeCredential::AddrKeyHash([0xC2; 28]);
        sc.register(cred);
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 5_000_000, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::AccountUnregistrationDeposit(cred, 5_000_000)];
        let total = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
        assert_eq!(total, 0);
        assert!(!sc.is_registered(&cred));
        assert_eq!(dp.key_deposits, 0);
    }

    #[test]
    fn test_cert_delegation_to_stake_pool_and_drep() {
        let mut pool = PoolState::new();
        let operator: [u8; 28] = [0xD1; 28];
        pool.register(PoolParams {
            operator,
            vrf_keyhash: [0xD1; 32],
            pledge: 0,
            cost: 170_000_000,
            margin: UnitInterval { numerator: 0, denominator: 1 },
            reward_account: RewardAccount { network: 1, credential: crate::StakeCredential::AddrKeyHash([0xD1; 28]) },
            pool_owners: vec![[0xD1; 28]],
            relays: vec![],
            pool_metadata: None,
        });
        let mut sc = StakeCredentials::new();
        let cred = crate::StakeCredential::AddrKeyHash([0xD2; 28]);
        sc.register(cred);
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        // Register a DRep for delegation target.
        let drep_cred = crate::StakeCredential::AddrKeyHash([0xD3; 28]);
        let drep = DRep::KeyHash([0xD3; 28]);
        ds.register(drep, RegisteredDrep::new(0, None));
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::DelegationToStakePoolAndDrep(cred, operator, drep)];
        let total = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
        assert_eq!(total, 0);
        let sc_state = sc.get(&cred).unwrap();
        assert_eq!(sc_state.delegated_pool(), Some(operator));
        assert_eq!(sc_state.delegated_drep(), Some(drep));
    }

    #[test]
    fn test_cert_account_reg_delegation_to_stake_pool() {
        let mut pool = PoolState::new();
        let operator: [u8; 28] = [0xE1; 28];
        pool.register(PoolParams {
            operator,
            vrf_keyhash: [0xE1; 32],
            pledge: 0,
            cost: 170_000_000,
            margin: UnitInterval { numerator: 0, denominator: 1 },
            reward_account: RewardAccount { network: 1, credential: crate::StakeCredential::AddrKeyHash([0xE1; 28]) },
            pool_owners: vec![[0xE1; 28]],
            relays: vec![],
            pool_metadata: None,
        });
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();
        let cred = crate::StakeCredential::AddrKeyHash([0xE2; 28]);

        let certs = vec![DCert::AccountRegistrationDelegationToStakePool(cred, operator, 2_000_000)];
        let total = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
        assert_eq!(total, 0);
        assert!(sc.is_registered(&cred));
        assert_eq!(sc.get(&cred).unwrap().delegated_pool(), Some(operator));
        assert_eq!(dp.key_deposits, 2_000_000);
    }

    #[test]
    fn test_cert_account_reg_delegation_to_drep() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let drep = DRep::AlwaysAbstain;
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();
        let cred = crate::StakeCredential::AddrKeyHash([0xE3; 28]);

        let certs = vec![DCert::AccountRegistrationDelegationToDrep(cred, drep, 2_000_000)];
        let total = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
        assert_eq!(total, 0);
        assert!(sc.is_registered(&cred));
        assert_eq!(sc.get(&cred).unwrap().delegated_drep(), Some(drep));
        assert_eq!(dp.key_deposits, 2_000_000);
    }

    #[test]
    fn test_cert_account_reg_delegation_to_pool_and_drep() {
        let mut pool = PoolState::new();
        let operator: [u8; 28] = [0xF1; 28];
        pool.register(PoolParams {
            operator,
            vrf_keyhash: [0xF1; 32],
            pledge: 0,
            cost: 170_000_000,
            margin: UnitInterval { numerator: 0, denominator: 1 },
            reward_account: RewardAccount { network: 1, credential: crate::StakeCredential::AddrKeyHash([0xF1; 28]) },
            pool_owners: vec![[0xF1; 28]],
            relays: vec![],
            pool_metadata: None,
        });
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let drep = DRep::AlwaysNoConfidence;
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();
        let cred = crate::StakeCredential::AddrKeyHash([0xF2; 28]);

        let certs = vec![DCert::AccountRegistrationDelegationToStakePoolAndDrep(cred, operator, drep, 3_000_000)];
        let total = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
        assert_eq!(total, 0);
        assert!(sc.is_registered(&cred));
        assert_eq!(sc.get(&cred).unwrap().delegated_pool(), Some(operator));
        assert_eq!(sc.get(&cred).unwrap().delegated_drep(), Some(drep));
        assert_eq!(dp.key_deposits, 3_000_000);
    }

    #[test]
    fn test_cert_drep_registration_and_unregistration() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();
        let cred = crate::StakeCredential::AddrKeyHash([0xA0; 28]);

        // Register DRep.
        let reg_certs = vec![DCert::DrepRegistration(cred, 500_000, None)];
        apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&reg_certs), None,
        ).unwrap();
        let drep = DRep::KeyHash([0xA0; 28]);
        assert!(ds.is_registered(&drep));
        assert_eq!(dp.drep_deposits, 500_000);

        // Unregister DRep.
        let unreg_certs = vec![DCert::DrepUnregistration(cred, 500_000)];
        apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&unreg_certs), None,
        ).unwrap();
        assert!(!ds.is_registered(&drep));
        assert_eq!(dp.drep_deposits, 0);
    }

    #[test]
    fn test_cert_drep_update() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();
        let cred = crate::StakeCredential::AddrKeyHash([0xA1; 28]);

        // Register first.
        let reg_certs = vec![DCert::DrepRegistration(cred, 500_000, None)];
        apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&reg_certs), None,
        ).unwrap();

        // Update with anchor.
        let anchor = Some(Anchor { url: "https://drep.example".to_string(), data_hash: [0xBB; 32] });
        let upd_certs = vec![DCert::DrepUpdate(cred, anchor.clone())];
        apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&upd_certs), None,
        ).unwrap();
        let drep = DRep::KeyHash([0xA1; 28]);
        assert!(ds.is_registered(&drep));
    }

    #[test]
    fn test_cert_pool_registration() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let params = PoolParams {
            operator: [0xAA; 28],
            vrf_keyhash: [0xAA; 32],
            pledge: 1_000,
            cost: 170_000_000,
            margin: UnitInterval { numerator: 1, denominator: 10 },
            reward_account: RewardAccount { network: 1, credential: crate::StakeCredential::AddrKeyHash([0xAA; 28]) },
            pool_owners: vec![[0xAA; 28]],
            relays: vec![],
            pool_metadata: None,
        };
        let certs = vec![DCert::PoolRegistration(params.clone())];
        apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
        assert!(pool.is_registered(&[0xAA; 28]));
        assert_eq!(dp.pool_deposits, 500_000_000);
    }

    #[test]
    fn test_cert_pool_registration_cost_too_low() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let params = PoolParams {
            operator: [0xBB; 28],
            vrf_keyhash: [0xBB; 32],
            pledge: 1_000,
            cost: 1_000, // below min_pool_cost (170_000_000)
            margin: UnitInterval { numerator: 1, denominator: 10 },
            reward_account: RewardAccount { network: 1, credential: crate::StakeCredential::AddrKeyHash([0xBB; 28]) },
            pool_owners: vec![[0xBB; 28]],
            relays: vec![],
            pool_metadata: None,
        };
        let certs = vec![DCert::PoolRegistration(params)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::PoolCostTooLow { .. }));
    }

    #[test]
    fn test_cert_pool_registration_invalid_margin() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let params = PoolParams {
            operator: [0xBC; 28],
            vrf_keyhash: [0xBC; 32],
            pledge: 1_000,
            cost: 170_000_000,
            margin: UnitInterval { numerator: 2, denominator: 1 }, // invalid: num > denom
            reward_account: RewardAccount { network: 1, credential: crate::StakeCredential::AddrKeyHash([0xBC; 28]) },
            pool_owners: vec![[0xBC; 28]],
            relays: vec![],
            pool_metadata: None,
        };
        let certs = vec![DCert::PoolRegistration(params)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::PoolMarginInvalid { .. }));
    }

    #[test]
    fn test_cert_pool_registration_reward_network_mismatch() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx(); // expected_network_id = Some(1)

        let params = PoolParams {
            operator: [0xBD; 28],
            vrf_keyhash: [0xBD; 32],
            pledge: 1_000,
            cost: 170_000_000,
            margin: UnitInterval { numerator: 0, denominator: 1 },
            reward_account: RewardAccount { network: 0, credential: crate::StakeCredential::AddrKeyHash([0xBD; 28]) }, // network 0 != 1
            pool_owners: vec![[0xBD; 28]],
            relays: vec![],
            pool_metadata: None,
        };
        let certs = vec![DCert::PoolRegistration(params)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::PoolRewardAccountNetworkMismatch { .. }));
    }

    #[test]
    fn test_cert_pool_registration_metadata_url_too_long() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let params = PoolParams {
            operator: [0xBE; 28],
            vrf_keyhash: [0xBE; 32],
            pledge: 1_000,
            cost: 170_000_000,
            margin: UnitInterval { numerator: 0, denominator: 1 },
            reward_account: RewardAccount { network: 1, credential: crate::StakeCredential::AddrKeyHash([0xBE; 28]) },
            pool_owners: vec![[0xBE; 28]],
            relays: vec![],
            pool_metadata: Some(crate::types::PoolMetadata {
                url: "x".repeat(65), // 65 bytes > 64
                metadata_hash: [0; 32],
            }),
        };
        let certs = vec![DCert::PoolRegistration(params)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::PoolMetadataUrlTooLong { .. }));
    }

    #[test]
    fn test_cert_pool_retirement() {
        let mut pool = PoolState::new();
        let operator: [u8; 28] = [0xCC; 28];
        pool.register(PoolParams {
            operator,
            vrf_keyhash: [0xCC; 32],
            pledge: 0,
            cost: 170_000_000,
            margin: UnitInterval { numerator: 0, denominator: 1 },
            reward_account: RewardAccount { network: 1, credential: crate::StakeCredential::AddrKeyHash([0xCC; 28]) },
            pool_owners: vec![[0xCC; 28]],
            relays: vec![],
            pool_metadata: None,
        });
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 500_000_000, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx(); // current_epoch=100, e_max=18

        // Retire at epoch 110 (within 100+18=118).
        let certs = vec![DCert::PoolRetirement(operator, EpochNo(110))];
        apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
    }

    #[test]
    fn test_cert_pool_retirement_epoch_too_far() {
        let mut pool = PoolState::new();
        let operator: [u8; 28] = [0xCD; 28];
        pool.register(PoolParams {
            operator,
            vrf_keyhash: [0xCD; 32],
            pledge: 0,
            cost: 170_000_000,
            margin: UnitInterval { numerator: 0, denominator: 1 },
            reward_account: RewardAccount { network: 1, credential: crate::StakeCredential::AddrKeyHash([0xCD; 28]) },
            pool_owners: vec![[0xCD; 28]],
            relays: vec![],
            pool_metadata: None,
        });
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 500_000_000, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx(); // current_epoch=100, e_max=18

        // Retire at epoch 200 — beyond 100+18=118.
        let certs = vec![DCert::PoolRetirement(operator, EpochNo(200))];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::PoolRetirementTooFar { .. }));
    }

    #[test]
    fn test_cert_pool_retirement_not_registered() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::PoolRetirement([0xDE; 28], EpochNo(110))];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::PoolNotRegistered(_)));
    }

    #[test]
    fn test_cert_committee_authorization() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let cold = crate::StakeCredential::AddrKeyHash([0xDA; 28]);
        let hot = crate::StakeCredential::AddrKeyHash([0xDB; 28]);
        cs.register(cold.clone());
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::CommitteeAuthorization(cold.clone(), hot.clone())];
        apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
        let ms = cs.get(&cold).unwrap();
        assert!(!ms.is_resigned());
    }

    #[test]
    fn test_cert_committee_authorization_unknown_member() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let cold = crate::StakeCredential::AddrKeyHash([0xDC; 28]);
        let hot = crate::StakeCredential::AddrKeyHash([0xDD; 28]);
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::CommitteeAuthorization(cold, hot)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::CommitteeIsUnknown(_)));
    }

    #[test]
    fn test_cert_committee_resignation() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let cold = crate::StakeCredential::AddrKeyHash([0xEA; 28]);
        cs.register(cold.clone());
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::CommitteeResignation(cold.clone(), None)];
        apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap();
        assert!(cs.get(&cold).unwrap().is_resigned());
    }

    #[test]
    fn test_cert_committee_resignation_already_resigned() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let cold = crate::StakeCredential::AddrKeyHash([0xEB; 28]);
        cs.register(cold.clone());
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        // First resign.
        let certs1 = vec![DCert::CommitteeResignation(cold.clone(), None)];
        apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs1), None,
        ).unwrap();

        // Second resign should fail.
        let certs2 = vec![DCert::CommitteeResignation(cold, None)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs2), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::CommitteeHasPreviouslyResigned(_)));
    }

    #[test]
    fn test_cert_stake_credential_already_registered() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let cred = crate::StakeCredential::AddrKeyHash([0xFA; 28]);
        sc.register(cred);
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::AccountRegistration(cred)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::StakeCredentialAlreadyRegistered(_)));
    }

    #[test]
    fn test_cert_stake_credential_unregister_not_registered() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let cred = crate::StakeCredential::AddrKeyHash([0xFB; 28]);
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::AccountUnregistration(cred)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::StakeCredentialNotRegistered(_)));
    }

    #[test]
    fn test_cert_delegate_to_unregistered_pool() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let cred = crate::StakeCredential::AddrKeyHash([0xFC; 28]);
        sc.register(cred);
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::DelegationToStakePool(cred, [0x00; 28])]; // pool not registered
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::PoolNotRegistered(_)));
    }

    #[test]
    fn test_cert_drep_already_registered() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let cred = crate::StakeCredential::AddrKeyHash([0xFD; 28]);
        let drep = DRep::KeyHash([0xFD; 28]);
        ds.register(drep, RegisteredDrep::new(500_000, None));
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let certs = vec![DCert::DrepRegistration(cred, 500_000, None)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::DrepAlreadyRegistered(_)));
    }

    #[test]
    fn test_cert_delegate_to_unregistered_drep() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let cred = crate::StakeCredential::AddrKeyHash([0xFE; 28]);
        sc.register(cred);
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        // Delegate to a DRep that is NOT registered and NOT a built-in.
        let drep = DRep::KeyHash([0x99; 28]);
        let certs = vec![DCert::DelegationToDrep(cred, drep)];
        let err = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, Some(&certs), None,
        ).unwrap_err();
        assert!(matches!(err, LedgerError::DrepNotRegistered(_)));
    }

    #[test]
    fn test_cert_withdrawals_credited_correctly() {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let cred = crate::StakeCredential::AddrKeyHash([0xAB; 28]);
        sc.register(cred);
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let ra_key = RewardAccount { network: 1, credential: cred };
        let mut ra = RewardAccounts::new();
        ra.insert(ra_key.clone(), RewardAccountState::new(100, None));
        let mut dp = DepositPot { key_deposits: 0, pool_deposits: 0, drep_deposits: 0 };
        let mut gd = std::collections::BTreeMap::new();
        let ctx = sample_cert_ctx();

        let mut withdrawals = std::collections::BTreeMap::new();
        withdrawals.insert(ra_key.clone(), 100); // withdraw entire balance

        let total = apply_certificates_and_withdrawals(
            &mut pool, &mut sc, &mut cs, &mut ds, &mut ra, &mut dp,
            &mut gd, &ctx, None, Some(&withdrawals),
        ).unwrap();
        assert_eq!(total, 100);
        assert_eq!(ra.balance(&ra_key), 0);
    }

    // -----------------------------------------------------------------------
    // conway_pv_can_follow
    // -----------------------------------------------------------------------

    #[test]
    fn pv_can_follow_major_increment() {
        assert!(conway_pv_can_follow((9, 0), (10, 0)));
    }

    #[test]
    fn pv_can_follow_minor_increment() {
        assert!(conway_pv_can_follow((9, 0), (9, 1)));
    }

    #[test]
    fn pv_can_follow_rejects_downgrade() {
        assert!(!conway_pv_can_follow((10, 0), (9, 0)));
    }

    #[test]
    fn pv_can_follow_rejects_same_version() {
        assert!(!conway_pv_can_follow((10, 0), (10, 0)));
    }

    #[test]
    fn pv_can_follow_rejects_major_jump() {
        // Major +2 is not allowed (per upstream pvCanFollow).
        assert!(!conway_pv_can_follow((9, 0), (11, 0)));
    }

    // ── validate_alonzo_plus_tx: mandatory collateral for redeemers ────

    #[test]
    fn alonzo_plus_tx_missing_collateral_with_redeemers() {
        let params = ProtocolParameters::alonzo_defaults();
        let utxo = MultiEraUtxo::new();
        let outputs = vec![];
        // has_redeemers = true, collateral_inputs = None → must fail
        let result = validate_alonzo_plus_tx(
            &params, &utxo, 200, 200_000, &outputs,
            None, None, None, None, true,
        );
        assert!(matches!(result, Err(LedgerError::MissingCollateralForScripts)));
    }

    #[test]
    fn alonzo_plus_tx_empty_collateral_with_redeemers() {
        let params = ProtocolParameters::alonzo_defaults();
        let utxo = MultiEraUtxo::new();
        let outputs = vec![];
        // has_redeemers = true, collateral_inputs = Some(&[]) → must fail
        let result = validate_alonzo_plus_tx(
            &params, &utxo, 200, 200_000, &outputs,
            Some(&[]), None, None, None, true,
        );
        assert!(matches!(result, Err(LedgerError::MissingCollateralForScripts)));
    }

    #[test]
    fn alonzo_plus_tx_no_redeemers_skips_collateral() {
        let params = ProtocolParameters::alonzo_defaults();
        let utxo = MultiEraUtxo::new();
        let outputs = vec![];
        // has_redeemers = false, collateral_inputs = None → ok (no scripts)
        let result = validate_alonzo_plus_tx(
            &params, &utxo, 200, 200_000, &outputs,
            None, None, None, None, false,
        );
        assert!(result.is_ok());
    }

    // ── Network validation tests ───────────────────────────────────────

    #[test]
    fn shelley_address_network_id_extracts_correctly() {
        // Base address, network 1 (mainnet): header byte = 0x01
        assert_eq!(shelley_address_network_id(&[0x01]), Some(1));
        // Enterprise address, network 0 (testnet): header byte = 0x60
        assert_eq!(shelley_address_network_id(&[0x60]), Some(0));
        // Reward address, network 1: header byte = 0xe1
        assert_eq!(shelley_address_network_id(&[0xe1]), Some(1));
        // Pointer address, network 0: header byte = 0x40
        assert_eq!(shelley_address_network_id(&[0x40]), Some(0));
    }

    #[test]
    fn shelley_address_network_id_returns_none_for_byron() {
        // Byron addresses have type nibble >= 8
        assert_eq!(shelley_address_network_id(&[0x82]), None);
        assert_eq!(shelley_address_network_id(&[0x83]), None);
        // Empty slice
        assert_eq!(shelley_address_network_id(&[]), None);
    }

    #[test]
    fn validate_output_network_ids_accepts_matching() {
        // Mainnet (network=1) base address
        let mut addr_bytes = vec![0x01u8]; // header: type=0, net=1
        addr_bytes.extend_from_slice(&[0xaa; 56]); // 28+28 bytes
        let output = MultiEraTxOut::Shelley(ShelleyTxOut {
            address: addr_bytes,
            amount: 1_000_000,
        });
        assert!(validate_output_network_ids(1, &[output]).is_ok());
    }

    #[test]
    fn validate_output_network_ids_rejects_mismatch() {
        // Testnet output (network=0) when mainnet (1) expected
        let mut addr_bytes = vec![0x00u8]; // header: type=0, net=0
        addr_bytes.extend_from_slice(&[0xaa; 56]);
        let output = MultiEraTxOut::Shelley(ShelleyTxOut {
            address: addr_bytes,
            amount: 1_000_000,
        });
        let result = validate_output_network_ids(1, &[output]);
        assert!(matches!(result, Err(LedgerError::WrongNetwork {
            expected: 1, found: 0,
        })));
    }

    #[test]
    fn validate_output_network_ids_skips_byron() {
        // Byron address (starts 0x82) — no network ID
        let output = MultiEraTxOut::Shelley(ShelleyTxOut {
            address: vec![0x82, 0xd8, 0x18, 0x58, 0x20],
            amount: 1_000_000,
        });
        assert!(validate_output_network_ids(1, &[output]).is_ok());
    }

    #[test]
    fn validate_withdrawal_network_ids_accepts_matching() {
        let withdrawals = std::collections::BTreeMap::from([(
            RewardAccount {
                network: 1,
                credential: crate::StakeCredential::AddrKeyHash([0xbb; 28]),
            },
            50_000u64,
        )]);
        assert!(validate_withdrawal_network_ids(1, &withdrawals).is_ok());
    }

    #[test]
    fn validate_withdrawal_network_ids_rejects_mismatch() {
        let withdrawals = std::collections::BTreeMap::from([(
            RewardAccount {
                network: 0,
                credential: crate::StakeCredential::AddrKeyHash([0xbb; 28]),
            },
            50_000u64,
        )]);
        let result = validate_withdrawal_network_ids(1, &withdrawals);
        assert!(matches!(result, Err(LedgerError::WrongNetworkWithdrawal {
            expected: 1, found: 0,
        })));
    }

    #[test]
    fn validate_tx_body_network_id_accepts_matching() {
        assert!(validate_tx_body_network_id(1, Some(1)).is_ok());
        assert!(validate_tx_body_network_id(0, Some(0)).is_ok());
    }

    #[test]
    fn validate_tx_body_network_id_accepts_absent() {
        // None means the tx body doesn't declare a network_id — always OK
        assert!(validate_tx_body_network_id(1, None).is_ok());
    }

    #[test]
    fn validate_tx_body_network_id_rejects_mismatch() {
        let result = validate_tx_body_network_id(1, Some(0));
        assert!(matches!(result, Err(LedgerError::WrongNetworkInTxBody {
            expected: 1, found: 0,
        })));
    }

    #[test]
    fn validate_output_network_ids_mixed_valid_and_invalid() {
        // Two outputs: first matching (net=1), second mismatching (net=0)
        let mut good_addr = vec![0x01u8];
        good_addr.extend_from_slice(&[0xaa; 56]);
        let mut bad_addr = vec![0x00u8];
        bad_addr.extend_from_slice(&[0xbb; 56]);
        let outputs = vec![
            MultiEraTxOut::Shelley(ShelleyTxOut {
                address: good_addr,
                amount: 1_000_000,
            }),
            MultiEraTxOut::Shelley(ShelleyTxOut {
                address: bad_addr,
                amount: 2_000_000,
            }),
        ];
        let result = validate_output_network_ids(1, &outputs);
        assert!(matches!(result, Err(LedgerError::WrongNetwork {
            expected: 1, found: 0,
        })));
    }
}
