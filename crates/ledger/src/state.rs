use crate::eras::allegra::AllegraTxBody;
use crate::eras::alonzo::AlonzoTxBody;
use crate::eras::babbage::BabbageTxBody;
use crate::eras::conway::ConwayTxBody;
use crate::eras::mary::{MultiAsset, Value};
use crate::eras::shelley::{ShelleyTxBody, ShelleyUtxo};
use crate::types::{
    Address, Anchor, DCert, DRep, EpochNo, Point, PoolKeyHash, PoolParams, RewardAccount,
    Relay, StakeCredential,
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
}

impl CborEncode for RegisteredDrep {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        encode_optional_anchor(self.anchor.as_ref(), enc);
        enc.unsigned(self.deposit);
    }
}

impl CborDecode for RegisteredDrep {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }

        Ok(Self {
            anchor: decode_optional_anchor(dec)?,
            deposit: dec.unsigned()?,
        })
    }
}

impl RegisteredDrep {
    /// Creates registered DRep state.
    pub fn new(deposit: u64, anchor: Option<Anchor>) -> Self {
        Self { anchor, deposit }
    }

    /// Returns the current DRep anchor, if any.
    pub fn anchor(&self) -> Option<&Anchor> {
        self.anchor.as_ref()
    }

    /// Returns the current DRep deposit value.
    pub fn deposit(&self) -> u64 {
        self.deposit
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

    fn set_authorization(&mut self, authorization: Option<CommitteeAuthorization>) {
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
    pool_state: PoolState,
    stake_credentials: StakeCredentials,
    committee_state: CommitteeState,
    drep_state: DrepState,
    reward_accounts: RewardAccounts,
    multi_era_utxo: MultiEraUtxo,
    shelley_utxo: ShelleyUtxo,
    protocol_params: Option<crate::protocol_params::ProtocolParameters>,
    deposit_pot: DepositPot,
    accounting: AccountingState,
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
        enc.array(12);
        self.current_era.encode_cbor(enc);
        self.tip.encode_cbor(enc);
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
    }
}

impl CborDecode for LedgerState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        // Accept legacy 9/10-element arrays and current 12-element arrays.
        if len != 9 && len != 10 && len != 12 {
            return Err(LedgerError::CborInvalidLength {
                expected: 12,
                actual: len as usize,
            });
        }

        let current_era = Era::decode_cbor(dec)?;
        let tip = Point::decode_cbor(dec)?;
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

        Ok(Self {
            current_era,
            tip,
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

    /// Captures a read-only snapshot of the current ledger state.
    pub fn snapshot(&self) -> LedgerStateSnapshot {
        LedgerStateSnapshot {
            current_era: self.current_era,
            tip: self.tip,
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
            Era::Byron => {}
            Era::Shelley => self.apply_shelley_block(block, slot)?,
            Era::Allegra => self.apply_allegra_block(block, slot)?,
            Era::Mary => self.apply_mary_block(block, slot)?,
            Era::Alonzo => self.apply_alonzo_block(block, slot, evaluator)?,
            Era::Babbage => self.apply_babbage_block(block, slot, evaluator)?,
            Era::Conway => self.apply_conway_block(block, slot, evaluator)?,
        }

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
    ) -> Result<(), LedgerError> {
        match tx {
            crate::tx::MultiEraSubmittedTx::Shelley(tx) => {
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Shelley(o.clone()))
                        .collect();
                    let tx_size = tx.to_cbor_bytes().len();
                    validate_pre_alonzo_tx(
                        params, tx_size, tx.body.fee, &outputs,
                    )?;
                }
                let mut staged = self.shelley_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let (kd, pd) = self.deposit_amounts();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    kd, pd,
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
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
            }
            crate::tx::MultiEraSubmittedTx::Allegra(tx) => {
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Shelley(o.clone()))
                        .collect();
                    validate_pre_alonzo_tx(
                        params, tx.raw_cbor.len(), tx.body.fee, &outputs,
                    )?;
                }
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let (kd, pd) = self.deposit_amounts();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    kd, pd,
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
            }
            crate::tx::MultiEraSubmittedTx::Mary(tx) => {
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Mary(o.clone()))
                        .collect();
                    validate_pre_alonzo_tx(
                        params, tx.raw_cbor.len(), tx.body.fee, &outputs,
                    )?;
                }
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let (kd, pd) = self.deposit_amounts();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    kd, pd,
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
            }
            crate::tx::MultiEraSubmittedTx::Alonzo(tx) => {
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                        .collect();
                    let total_eu = sum_redeemer_ex_units(&tx.witness_set);
                    validate_alonzo_plus_tx(
                        params, &self.multi_era_utxo,
                        tx.raw_cbor.len(), tx.body.fee, &outputs,
                        tx.body.collateral.as_deref(), total_eu.as_ref(),
                    )?;
                }
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let (kd, pd) = self.deposit_amounts();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    kd, pd,
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
            }
            crate::tx::MultiEraSubmittedTx::Babbage(tx) => {
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    let total_eu = sum_redeemer_ex_units(&tx.witness_set);
                    validate_alonzo_plus_tx(
                        params, &self.multi_era_utxo,
                        tx.raw_cbor.len(), tx.body.fee, &outputs,
                        tx.body.collateral.as_deref(), total_eu.as_ref(),
                    )?;
                }
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let (kd, pd) = self.deposit_amounts();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    kd, pd,
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
            }
            crate::tx::MultiEraSubmittedTx::Conway(tx) => {
                if let Some(params) = &self.protocol_params {
                    let outputs: Vec<MultiEraTxOut> = tx.body.outputs.iter()
                        .map(|o| MultiEraTxOut::Babbage(o.clone()))
                        .collect();
                    let total_eu = sum_redeemer_ex_units(&tx.witness_set);
                    validate_alonzo_plus_tx(
                        params, &self.multi_era_utxo,
                        tx.raw_cbor.len(), tx.body.fee, &outputs,
                        tx.body.collateral.as_deref(), total_eu.as_ref(),
                    )?;
                }
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let mut staged_deposit_pot = self.deposit_pot.clone();
                let (kd, pd) = self.deposit_amounts();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
                    &mut staged_deposit_pot,
                    kd, pd,
                    tx.body.certificates.as_deref(),
                    tx.body.withdrawals.as_ref(),
                )?;
                staged.apply_conway_tx_withdrawals(tx.tx_id().0, &tx.body, current_slot.0, withdrawal_total)?;
                self.multi_era_utxo = staged;
                self.pool_state = staged_pool_state;
                self.stake_credentials = staged_stake_credentials;
                self.committee_state = staged_committee_state;
                self.drep_state = staged_drep_state;
                self.reward_accounts = staged_reward_accounts;
                self.deposit_pot = staged_deposit_pot;
            }
        }

        Ok(())
    }

    // -- Private helpers ------------------------------------------------------

    fn deposit_amounts(&self) -> (u64, u64) {
        match &self.protocol_params {
            Some(p) => (p.key_deposit, p.pool_deposit),
            None => (0, 0),
        }
    }

    // -- Private per-era apply helpers --------------------------------------

    fn apply_shelley_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, usize, ShelleyTxBody, Option<Vec<u8>>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ShelleyTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone()))
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
        let (kd, pd) = self.deposit_amounts();
        for (tx_id, tx_size, body, witness_bytes) in &decoded {
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
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
                kd, pd,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            staged.apply_tx_with_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
        }
        self.shelley_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
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

        let decoded: Vec<(crate::types::TxId, usize, AllegraTxBody, Option<Vec<u8>>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AllegraTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone()))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let (kd, pd) = self.deposit_amounts();
        for (tx_id, tx_size, body, witness_bytes) in &decoded {
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Shelley(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
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
            validate_native_scripts_if_present(witness_bytes.as_deref(), &required_scripts, slot)?;
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                kd, pd,
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

        let decoded: Vec<(crate::types::TxId, usize, crate::eras::mary::MaryTxBody, Option<Vec<u8>>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = crate::eras::mary::MaryTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone()))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let (kd, pd) = self.deposit_amounts();
        for (tx_id, tx_size, body, witness_bytes) in &decoded {
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Mary(o.clone()))
                    .collect();
                validate_pre_alonzo_tx(params, *tx_size, body.fee, &outputs)?;
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
            validate_native_scripts_if_present(witness_bytes.as_deref(), &required_scripts, slot)?;
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                kd, pd,
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

        let decoded: Vec<(crate::types::TxId, usize, AlonzoTxBody, Option<Vec<u8>>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AlonzoTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone()))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let (kd, pd) = self.deposit_amounts();
        for (tx_id, tx_size, body, witness_bytes) in &decoded {
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Alonzo(o.clone()))
                    .collect();
                validate_alonzo_plus_tx(
                    params, &staged, *tx_size, body.fee, &outputs,
                    body.collateral.as_deref(), total_eu.as_ref(),
                )?;
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
            validate_native_scripts_if_present(witness_bytes.as_deref(), &required_scripts, slot)?;
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
                crate::plutus_validation::validate_plutus_scripts(
                    evaluator, witness_bytes.as_deref(), &required_scripts,
                    &sorted_inputs, &sorted_policies, certs_slice, &sorted_rewards,
                )?;
            }
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                kd, pd,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            staged.apply_alonzo_tx_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
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

        let decoded: Vec<(crate::types::TxId, usize, BabbageTxBody, Option<Vec<u8>>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = BabbageTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone()))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let (kd, pd) = self.deposit_amounts();
        for (tx_id, tx_size, body, witness_bytes) in &decoded {
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                validate_alonzo_plus_tx(
                    params, &staged, *tx_size, body.fee, &outputs,
                    body.collateral.as_deref(), total_eu.as_ref(),
                )?;
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
            validate_native_scripts_if_present(witness_bytes.as_deref(), &required_scripts, slot)?;
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
                crate::plutus_validation::validate_plutus_scripts(
                    evaluator, witness_bytes.as_deref(), &required_scripts,
                    &sorted_inputs, &sorted_policies, certs_slice, &sorted_rewards,
                )?;
            }
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                kd, pd,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            staged.apply_babbage_tx_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
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

        let decoded: Vec<(crate::types::TxId, usize, ConwayTxBody, Option<Vec<u8>>)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ConwayTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, tx.body.len(), body, tx.witnesses.clone()))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        let mut staged_deposit_pot = self.deposit_pot.clone();
        let (kd, pd) = self.deposit_amounts();
        for (tx_id, tx_size, body, witness_bytes) in &decoded {
            let total_eu = sum_redeemer_ex_units_from_bytes(witness_bytes.as_deref());
            if let Some(params) = &self.protocol_params {
                let outputs: Vec<MultiEraTxOut> = body.outputs.iter()
                    .map(|o| MultiEraTxOut::Babbage(o.clone()))
                    .collect();
                validate_alonzo_plus_tx(
                    params, &staged, *tx_size, body.fee, &outputs,
                    body.collateral.as_deref(), total_eu.as_ref(),
                )?;
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
            validate_native_scripts_if_present(witness_bytes.as_deref(), &required_scripts, slot)?;
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
                crate::plutus_validation::validate_plutus_scripts(
                    evaluator, witness_bytes.as_deref(), &required_scripts,
                    &sorted_inputs, &sorted_policies, certs_slice, &sorted_rewards,
                )?;
            }
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
                &mut staged_deposit_pot,
                kd, pd,
                body.certificates.as_deref(),
                body.withdrawals.as_ref(),
            )?;
            staged.apply_conway_tx_withdrawals(tx_id.0, body, slot, withdrawal_total)?;
        }
        self.multi_era_utxo = staged;
        self.pool_state = staged_pool_state;
        self.stake_credentials = staged_stake_credentials;
        self.committee_state = staged_committee_state;
        self.drep_state = staged_drep_state;
        self.reward_accounts = staged_reward_accounts;
        self.deposit_pot = staged_deposit_pot;
        Ok(())
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
    key_deposit: u64,
    pool_deposit: u64,
    certificates: Option<&[DCert]>,
    withdrawals: Option<&BTreeMap<RewardAccount, u64>>,
) -> Result<u64, LedgerError> {
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
                other => return Err(LedgerError::UnsupportedCertificate(certificate_kind(other))),
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

fn is_builtin_drep(drep: DRep) -> bool {
    matches!(drep, DRep::AlwaysAbstain | DRep::AlwaysNoConfidence)
}

fn certificate_kind(cert: &DCert) -> &'static str {
    match cert {
        DCert::AccountRegistration(_) => "AccountRegistration",
        DCert::AccountUnregistration(_) => "AccountUnregistration",
        DCert::DelegationToStakePool(_, _) => "DelegationToStakePool",
        DCert::PoolRegistration(_) => "PoolRegistration",
        DCert::PoolRetirement(_, _) => "PoolRetirement",
        DCert::GenesisDelegation(_, _, _) => "GenesisDelegation",
        DCert::AccountRegistrationDeposit(_, _) => "AccountRegistrationDeposit",
        DCert::AccountUnregistrationDeposit(_, _) => "AccountUnregistrationDeposit",
        DCert::DelegationToDrep(_, _) => "DelegationToDrep",
        DCert::DelegationToStakePoolAndDrep(_, _, _) => "DelegationToStakePoolAndDrep",
        DCert::AccountRegistrationDelegationToStakePool(_, _, _) => {
            "AccountRegistrationDelegationToStakePool"
        }
        DCert::AccountRegistrationDelegationToDrep(_, _, _) => {
            "AccountRegistrationDelegationToDrep"
        }
        DCert::AccountRegistrationDelegationToStakePoolAndDrep(_, _, _, _) => {
            "AccountRegistrationDelegationToStakePoolAndDrep"
        }
        DCert::CommitteeAuthorization(_, _) => "CommitteeAuthorization",
        DCert::CommitteeResignation(_, _) => "CommitteeResignation",
        DCert::DrepRegistration(_, _, _) => "DrepRegistration",
        DCert::DrepUnregistration(_, _) => "DrepUnregistration",
        DCert::DrepUpdate(_, _) => "DrepUpdate",
    }
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
    Ok(())
}

/// Validates an Alonzo+ transaction against protocol parameters.
///
/// Checks: transaction size limit, fee minimum (including script costs
/// when `total_ex_units` is provided), min-UTxO per output, per-tx
/// execution-unit limits, and collateral sufficiency when collateral
/// inputs are declared.
fn validate_alonzo_plus_tx(
    params: &crate::protocol_params::ProtocolParameters,
    utxo: &MultiEraUtxo,
    tx_body_size: usize,
    declared_fee: u64,
    outputs: &[MultiEraTxOut],
    collateral_inputs: Option<&[crate::eras::shelley::ShelleyTxIn]>,
    total_ex_units: Option<&crate::eras::alonzo::ExUnits>,
) -> Result<(), LedgerError> {
    crate::fees::validate_tx_size(params, tx_body_size)?;
    crate::fees::validate_fee(params, tx_body_size, total_ex_units, declared_fee)?;
    if let Some(eu) = total_ex_units {
        crate::fees::validate_tx_ex_units(params, eu)?;
    }
    crate::min_utxo::validate_all_outputs_min_utxo(params, outputs)?;
    if let Some(collateral) = collateral_inputs {
        if !collateral.is_empty() {
            crate::collateral::validate_collateral(params, utxo, collateral, declared_fee)?;
        }
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
    crate::witnesses::verify_vkey_signatures(tx_body_hash, &ws.vkey_witnesses)
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
) -> Result<(), LedgerError> {
    if required_script_hashes.is_empty() {
        return Ok(());
    }
    let witness_bytes = match witness_bytes {
        Some(wb) => wb,
        None => return Ok(()),
    };
    let ws = crate::eras::shelley::ShelleyWitnessSet::from_cbor_bytes(witness_bytes)?;
    let vkey_hashes = crate::witnesses::witness_vkey_hash_set(&ws.vkey_witnesses);

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
        }
        // When a required script is not in the native_scripts witness
        // list, it may be a Plutus script (validated by Phase-2).
        // For now, skip missing scripts rather than fail.
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
