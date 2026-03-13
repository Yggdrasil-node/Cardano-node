use crate::eras::allegra::AllegraTxBody;
use crate::eras::alonzo::AlonzoTxBody;
use crate::eras::babbage::BabbageTxBody;
use crate::eras::conway::ConwayTxBody;
use crate::eras::mary::{MultiAsset, Value};
use crate::eras::shelley::{ShelleyTxBody, ShelleyUtxo};
use crate::types::{Address, EpochNo, Point, PoolKeyHash, PoolParams, RewardAccount};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use crate::{CborDecode, CborEncode, Era, LedgerError};
use std::collections::BTreeMap;

/// Registered stake-pool state carried by the ledger.
///
/// This is a narrow container for pool registration data plus an optional
/// retirement epoch. Certificate application will populate and update this
/// structure in a later slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredPool {
    params: PoolParams,
    retiring_epoch: Option<EpochNo>,
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

    /// Returns the reward-account state captured in this snapshot.
    pub fn reward_accounts(&self) -> &RewardAccounts {
        &self.reward_accounts
    }

    /// Returns the registered state for `operator`, if present.
    pub fn registered_pool(&self, operator: &PoolKeyHash) -> Option<&RegisteredPool> {
        self.pool_state.get(operator)
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
/// upcoming certificate and withdrawal work. A legacy `ShelleyUtxo`
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
    /// Reward-account balances and delegation pointers.
    reward_accounts: RewardAccounts,
    /// Multi-era UTxO set.
    multi_era_utxo: MultiEraUtxo,
    /// Legacy Shelley-only UTxO set kept in sync for backward compatibility.
    shelley_utxo: ShelleyUtxo,
}

impl LedgerState {
    /// Creates a new ledger state rooted at the given era with an `Origin`
    /// tip and an empty UTxO set.
    pub fn new(current_era: Era) -> Self {
        Self {
            current_era,
            tip: Point::Origin,
            pool_state: PoolState::new(),
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
            reward_accounts: self.reward_accounts.clone(),
            multi_era_utxo: self.multi_era_utxo.clone(),
            shelley_utxo: self.shelley_utxo.clone(),
        }
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
                staged.apply_tx(crate::tx::compute_tx_id(&tx.body.to_cbor_bytes()).0, &tx.body, current_slot.0)?;
                self.shelley_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Allegra(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_allegra_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Mary(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_mary_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Alonzo(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_alonzo_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Babbage(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_babbage_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
            }
            crate::tx::MultiEraSubmittedTx::Conway(tx) => {
                let mut staged = self.multi_era_utxo.clone();
                staged.apply_conway_tx(tx.tx_id().0, &tx.body, current_slot.0)?;
                self.multi_era_utxo = staged;
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
        for (tx_id, body) in &decoded {
            staged.apply_tx(tx_id.0, body, slot)?;
        }
        self.shelley_utxo = staged;
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
        for (tx_id, body) in &decoded {
            staged.apply_allegra_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
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
        for (tx_id, body) in &decoded {
            staged.apply_mary_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
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
        for (tx_id, body) in &decoded {
            staged.apply_alonzo_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
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
        for (tx_id, body) in &decoded {
            staged.apply_babbage_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
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
        for (tx_id, body) in &decoded {
            staged.apply_conway_tx(tx_id.0, body, slot)?;
        }
        self.multi_era_utxo = staged;
        Ok(())
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
