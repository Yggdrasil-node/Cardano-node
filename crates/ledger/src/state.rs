use crate::eras::allegra::AllegraTxBody;
use crate::eras::alonzo::AlonzoTxBody;
use crate::eras::babbage::BabbageTxBody;
use crate::eras::conway::ConwayTxBody;
use crate::eras::mary::{MultiAsset, Value};
use crate::eras::shelley::{ShelleyTxBody, ShelleyUtxo};
use crate::types::{
    Address, Anchor, DCert, DRep, EpochNo, Point, PoolKeyHash, PoolParams, RewardAccount,
    StakeCredential,
};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use crate::{CborDecode, CborEncode, Decoder, Encoder, Era, LedgerError};
use std::collections::BTreeMap;

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
        enc.array(9);
        self.current_era.encode_cbor(enc);
        self.tip.encode_cbor(enc);
        self.pool_state.encode_cbor(enc);
        self.stake_credentials.encode_cbor(enc);
        self.committee_state.encode_cbor(enc);
        self.drep_state.encode_cbor(enc);
        self.reward_accounts.encode_cbor(enc);
        self.multi_era_utxo.encode_cbor(enc);
        self.shelley_utxo.encode_cbor(enc);
    }
}

impl CborDecode for LedgerState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 9 {
            return Err(LedgerError::CborInvalidLength {
                expected: 9,
                actual: len as usize,
            });
        }

        Ok(Self {
            current_era: Era::decode_cbor(dec)?,
            tip: Point::decode_cbor(dec)?,
            pool_state: PoolState::decode_cbor(dec)?,
            stake_credentials: StakeCredentials::decode_cbor(dec)?,
            committee_state: CommitteeState::decode_cbor(dec)?,
            drep_state: DrepState::decode_cbor(dec)?,
            reward_accounts: RewardAccounts::decode_cbor(dec)?,
            multi_era_utxo: MultiEraUtxo::decode_cbor(dec)?,
            shelley_utxo: ShelleyUtxo::decode_cbor(dec)?,
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

    /// Captures a read-only snapshot of the current ledger state.
    pub fn snapshot(&self) -> LedgerStateSnapshot {
        LedgerStateSnapshot {
            current_era: self.current_era,
            tip: self.tip.clone(),
            pool_state: self.pool_state.clone(),
            stake_credentials: self.stake_credentials.clone(),
            committee_state: self.committee_state.clone(),
            drep_state: self.drep_state.clone(),
            reward_accounts: self.reward_accounts.clone(),
            multi_era_utxo: self.multi_era_utxo.clone(),
            shelley_utxo: self.shelley_utxo.clone(),
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
        let slot = block.header.slot_no.0;

        match block.era {
            Era::Shelley => self.apply_shelley_block(block, slot)?,
            Era::Allegra => self.apply_allegra_block(block, slot)?,
            Era::Mary => self.apply_mary_block(block, slot)?,
            Era::Alonzo => self.apply_alonzo_block(block, slot)?,
            Era::Babbage => self.apply_babbage_block(block, slot)?,
            Era::Conway => self.apply_conway_block(block, slot)?,
            era => return Err(LedgerError::UnsupportedEra(era)),
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
                let mut staged = self.shelley_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
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
            }
            crate::tx::MultiEraSubmittedTx::Allegra(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
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
            }
            crate::tx::MultiEraSubmittedTx::Mary(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
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
            }
            crate::tx::MultiEraSubmittedTx::Alonzo(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
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
            }
            crate::tx::MultiEraSubmittedTx::Babbage(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
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
            }
            crate::tx::MultiEraSubmittedTx::Conway(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                let mut staged_pool_state = self.pool_state.clone();
                let mut staged_stake_credentials = self.stake_credentials.clone();
                let mut staged_committee_state = self.committee_state.clone();
                let mut staged_drep_state = self.drep_state.clone();
                let mut staged_reward_accounts = self.reward_accounts.clone();
                let withdrawal_total = apply_certificates_and_withdrawals(
                    &mut staged_pool_state,
                    &mut staged_stake_credentials,
                    &mut staged_committee_state,
                    &mut staged_drep_state,
                    &mut staged_reward_accounts,
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
            }
        }

        Ok(())
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

        let decoded: Vec<(crate::types::TxId, ShelleyTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ShelleyTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
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
        for (tx_id, body) in &decoded {
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
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

        let decoded: Vec<(crate::types::TxId, AllegraTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AllegraTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        for (tx_id, body) in &decoded {
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
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

        let decoded: Vec<(crate::types::TxId, crate::eras::mary::MaryTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = crate::eras::mary::MaryTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        for (tx_id, body) in &decoded {
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
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
        Ok(())
    }

    fn apply_alonzo_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, AlonzoTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = AlonzoTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        for (tx_id, body) in &decoded {
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
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
        Ok(())
    }

    fn apply_babbage_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, BabbageTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = BabbageTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        for (tx_id, body) in &decoded {
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
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
        Ok(())
    }

    fn apply_conway_block(
        &mut self,
        block: &crate::tx::Block,
        slot: u64,
    ) -> Result<(), LedgerError> {
        if block.transactions.is_empty() {
            return Ok(());
        }

        let decoded: Vec<(crate::types::TxId, ConwayTxBody)> = block
            .transactions
            .iter()
            .map(|tx| {
                let body = ConwayTxBody::from_cbor_bytes(&tx.body)?;
                Ok((tx.id, body))
            })
            .collect::<Result<Vec<_>, LedgerError>>()?;

        let mut staged = self.multi_era_utxo.clone();
        let mut staged_pool_state = self.pool_state.clone();
        let mut staged_stake_credentials = self.stake_credentials.clone();
        let mut staged_committee_state = self.committee_state.clone();
        let mut staged_drep_state = self.drep_state.clone();
        let mut staged_reward_accounts = self.reward_accounts.clone();
        for (tx_id, body) in &decoded {
            let withdrawal_total = apply_certificates_and_withdrawals(
                &mut staged_pool_state,
                &mut staged_stake_credentials,
                &mut staged_committee_state,
                &mut staged_drep_state,
                &mut staged_reward_accounts,
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
        Ok(())
    }
}

fn apply_certificates_and_withdrawals(
    pool_state: &mut PoolState,
    stake_credentials: &mut StakeCredentials,
    committee_state: &mut CommitteeState,
    drep_state: &mut DrepState,
    reward_accounts: &mut RewardAccounts,
    certificates: Option<&[DCert]>,
    withdrawals: Option<&BTreeMap<RewardAccount, u64>>,
) -> Result<u64, LedgerError> {
    if let Some(certs) = certificates {
        for cert in certs {
            match cert {
                DCert::AccountRegistration(credential)
                | DCert::AccountRegistrationDeposit(credential, _) => {
                    register_stake_credential(stake_credentials, *credential)?;
                }
                DCert::AccountUnregistration(credential)
                | DCert::AccountUnregistrationDeposit(credential, _) => {
                    unregister_stake_credential(stake_credentials, reward_accounts, *credential)?;
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
                DCert::AccountRegistrationDelegationToStakePool(credential, pool, _) => {
                    register_stake_credential(stake_credentials, *credential)?;
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
                DCert::AccountRegistrationDelegationToDrep(credential, drep, _) => {
                    register_stake_credential(stake_credentials, *credential)?;
                    delegate_drep(stake_credentials, drep_state, *credential, *drep)?;
                }
                DCert::AccountRegistrationDelegationToStakePoolAndDrep(credential, pool, drep, _) => {
                    register_stake_credential(stake_credentials, *credential)?;
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
                DCert::PoolRegistration(params) => pool_state.register(params.clone()),
                DCert::PoolRetirement(pool, epoch) => {
                    if !pool_state.retire(*pool, *epoch) {
                        return Err(LedgerError::PoolNotRegistered(*pool));
                    }
                }
                DCert::DrepRegistration(credential, deposit, anchor) => {
                    register_drep(drep_state, *credential, *deposit, anchor.clone())?;
                }
                DCert::DrepUnregistration(credential, _) => {
                    unregister_drep(drep_state, *credential)?;
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

fn accumulate_multi_asset(total: &mut MultiAsset, assets: &MultiAsset) {
    for (policy, entries) in assets {
        let policy_total = total.entry(*policy).or_default();
        for (asset_name, quantity) in entries {
            let entry = policy_total.entry(asset_name.clone()).or_default();
            *entry = entry.saturating_add(*quantity);
        }
    }
}
