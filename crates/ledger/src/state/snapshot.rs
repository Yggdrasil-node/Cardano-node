//! `LedgerStateSnapshot` — read-only capture of ledger-visible state.
//!
//! Mirrors the read-only query view used by upstream
//! [`Ouroboros.Consensus.Shelley.Ledger.Query`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus-cardano/src/Ouroboros/Consensus/Shelley/Ledger/Query.hs)
//! when answering NtC LSQ requests. The snapshot preserves the current era,
//! tip, stake-pool state, reward-account state, and both UTxO views (legacy
//! Shelley + multi-era) so callers can query ledger-visible data without
//! mutating [`super::LedgerState`].
//!
//! Companion `ChainDepStateContext` and `StakeSnapshots` are attached lazily
//! by the consensus runtime via `with_chain_dep_state` / `with_stake_snapshots`.
//!
//! Extracted from `state.rs` in R269 thirteenth slice as part of the strict
//! 1:1 filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269m-state-snapshot-extraction.md`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side LSQ-friendly read-only
//! capture aggregating fields from `LedgerState` for
//! `Ouroboros.Consensus.Shelley.Ledger.Query` answer paths. **Name
//! clashes with** upstream's
//! `Ouroboros.Consensus.Storage.LedgerDB.Snapshots` (on-disk codec,
//! different concept). Yggdrasil's filename intentionally diverges
//! from upstream to surface the LSQ-side role; the on-disk codec
//! lives in `crates/storage/src/file_ledger.rs` and
//! `ocert_sidecar.rs`.

use super::{
    AccountingState, ChainDepStateContext, CommitteeMemberState, CommitteeState, DepositPot,
    DrepState, EnactState, GenesisDelegationState, GovernanceActionState, PoolState,
    RegisteredDrep, RegisteredPool, RewardAccountState, RewardAccounts, StakeCredentialState,
    StakeCredentials, phase1_validation::accumulate_multi_asset,
};
use crate::Era;
use crate::eras::mary::{MultiAsset, Value};
use crate::eras::shelley::ShelleyUtxo;
use crate::types::{
    Address, BlockNo, DRep, EpochNo, GenesisHash, Point, PoolKeyHash, RewardAccount,
    StakeCredential,
};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use std::collections::BTreeMap;

/// Read-only snapshot of ledger-visible state.
///
/// This snapshot preserves the current era, tip, stake-pool state,
/// reward-account state, and both UTxO views so callers can query
/// ledger-visible data without mutating `LedgerState`. The dual UTxO
/// representation is retained because Shelley-only state is still stored
/// separately for backward compatibility.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerStateSnapshot {
    pub(super) current_era: Era,
    pub(super) tip: Point,
    pub(super) latest_block_protocol_version: Option<(u64, u64)>,
    pub(super) tip_block_no: Option<BlockNo>,
    pub(super) current_epoch: EpochNo,
    pub(super) expected_network_id: Option<u8>,
    pub(super) governance_actions:
        BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState>,
    pub(super) pool_state: PoolState,
    pub(super) stake_credentials: StakeCredentials,
    pub(super) committee_state: CommitteeState,
    pub(super) drep_state: DrepState,
    pub(super) reward_accounts: RewardAccounts,
    pub(super) shelley_utxo: ShelleyUtxo,
    pub(super) multi_era_utxo: MultiEraUtxo,
    pub(super) protocol_params: Option<crate::protocol_params::ProtocolParameters>,
    pub(super) deposit_pot: DepositPot,
    pub(super) accounting: AccountingState,
    pub(super) enact_state: EnactState,
    pub(super) gen_delegs: BTreeMap<GenesisHash, GenesisDelegationState>,
    pub(super) stability_window: Option<u64>,
    pub(super) num_dormant_epochs: u64,
    /// Round 192 — optional consensus-side `ChainDepState` mirror
    /// for serving `query protocol-state` with live nonces + OCert
    /// counters.  `None` when the runtime hasn't populated it yet
    /// (test fakes, very early bootstrap); LSQ dispatchers fall back
    /// to neutral placeholders in that case.
    pub(super) chain_dep_state: Option<ChainDepStateContext>,
    /// Round 202 — optional active stake-snapshot rotation
    /// (`mark`/`set`/`go`) for serving `query stake-snapshot` and
    /// stake-distribution queries with live per-pool totals.
    /// `None` when the runtime hasn't populated it (test fakes,
    /// pre-epoch-boundary bootstrap, or sync paths without a
    /// stake-snapshot tracker).
    pub(super) stake_snapshots: Option<crate::stake::StakeSnapshots>,
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
