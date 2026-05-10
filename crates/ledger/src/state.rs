//! state - module-level docstring.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `LedgerState`
//! orchestrator + per-era apply-path entry points. The
//! structural state lives in sibling sub-modules under `state/`
//! (see e.g. `state/treasury.rs`, `state/pool_state.rs`, etc.)
//! and the per-era apply paths live in `state/eras/*.rs`.
//! Upstream's `Cardano.Ledger.Shelley.LedgerState.hs` is a
//! single ~3000-line module that Yggdrasil splits along
//! structural seams; this file is the orchestrator shell that
//! ties everything together.

use crate::eras::mary::Value;
use crate::eras::shelley::{ShelleyTxIn, ShelleyUtxo};
use crate::protocol_params::DRepVotingThresholds;
use crate::types::{
    Address, Anchor, BlockNo, DCert, DRep, EpochNo, GenesisDelegateHash, GenesisHash, MirPot,
    MirTarget, Point, PoolKeyHash, RewardAccount, StakeCredential, UnitInterval, VrfKeyHash,
};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};
use crate::{CborDecode, CborEncode, Decoder, Encoder, Era, LedgerError};
use std::collections::BTreeMap;
use std::collections::HashSet;

// R269 first slice — `InstantaneousRewards` (MIR accumulation state) lives in
// its own file mirroring upstream `Cardano.Ledger.Shelley.LedgerState`'s MIR
// types and the `Cardano.Ledger.Shelley.Rules.Mir` per-epoch processing rule.
pub mod mir;
pub use mir::InstantaneousRewards;

// R269 second slice — Conway RATIFY rule tally engine lives in its own file
// mirroring upstream `Cardano.Ledger.Conway.Rules.Ratify` /
// `Cardano.Ledger.Conway.Governance.DRepPulser`.
pub mod ratify;
#[allow(unused_imports)]
pub(crate) use ratify::{
    DefaultVote, VoteTally, accepted_by_committee, accepted_by_dreps, accepted_by_spo,
    default_stake_pool_vote, drep_threshold_for_action, ratify_action, spo_threshold_for_action,
    tally_committee_votes, tally_drep_votes, tally_spo_votes,
};

// R269 third slice — Conway ENACT rule (`enact_gov_action` family +
// `EnactState` / `EnactOutcome`) lives in its own file mirroring upstream
// `Cardano.Ledger.Conway.Rules.Enact` / `Cardano.Ledger.Conway.Governance`.
pub mod enact;
pub use enact::{EnactOutcome, EnactState, enact_gov_action};

// R269 fourth slice — `DepositPot` (aggregate deposit tracking) lives in its
// own file mirroring upstream `Obligations` from
// `Cardano.Ledger.State.CertState` and `utxosDeposited` from
// `Cardano.Ledger.Shelley.LedgerState`.
pub mod deposit_pot;
pub use deposit_pot::DepositPot;

// R269 fifth slice — Phase-1 transaction validation helpers (pre-CEK
// predicate gates: tx size, fee minima, min-UTxO, ExUnits, witnesses,
// native scripts, script-witness coverage, auxiliary data hash, network ID)
// live in their own file mirroring upstream `Cardano.Ledger.*.Rules.Utxo` /
// `Cardano.Ledger.*.Rules.Utxow`. Internal `pub(super) fn` visibility plus
// the glob `use phase1_validation::*` below keeps the 100+ in-state.rs call
// sites unqualified.
pub(super) mod phase1_validation;
use phase1_validation::*;

// R269 sixth slice — Stake-pool registry types (`RegisteredPool`,
// `PoolRelayAccessPoint`, `PoolState`) live in their own file mirroring
// upstream `Cardano.Ledger.State.PoolState` / Shelley `PState`.
pub mod pool_state;
pub use pool_state::{PoolRelayAccessPoint, PoolState, RegisteredPool};

// R269 seventh slice — Reward-account state (`RewardAccountState` +
// `RewardAccounts`) lives in its own file mirroring upstream
// `Cardano.Ledger.State.AccountState` / DState reward-account portion.
pub mod reward_accounts;
pub use reward_accounts::{RewardAccountState, RewardAccounts};

// R269 eighth slice — Stake-credential registry (`StakeCredentialState` +
// `StakeCredentials`) lives in its own file mirroring upstream
// `Cardano.Ledger.Shelley.LedgerState::DState`'s `dsUnified` map.
pub mod stake_credentials;
pub use stake_credentials::{StakeCredentialState, StakeCredentials};

// R269 ninth slice — Conway DRep registry (`RegisteredDrep` + `DrepState`)
// lives in its own file mirroring upstream
// `Cardano.Ledger.Conway.Governance::DRepState` / `VState::vsDReps`.
pub mod drep_state;
pub use drep_state::{DrepState, RegisteredDrep};

// R269 tenth slice — Conway `GovernanceActionState` (stored proposal +
// votes + lifetime) lives in its own file mirroring upstream
// `Cardano.Ledger.Conway.Governance::GovActionState`.
pub mod governance_action_state;
pub use governance_action_state::GovernanceActionState;

// R269 eleventh slice — Constitutional-committee state
// (`CommitteeAuthorization`, `CommitteeMemberState`, `CommitteeState`)
// lives in its own file mirroring upstream
// `Cardano.Ledger.Conway.Governance.Committee` + `csCommitteeCreds`.
pub mod committee_state;
pub use committee_state::{CommitteeAuthorization, CommitteeMemberState, CommitteeState};

// R269 twelfth slice — Sidecar nonce + OCert counter mirror
// (`ChainDepStateContext`) attached to `LedgerStateSnapshot` for LSQ
// `query protocol-state`. Mirrors upstream
// `Ouroboros.Consensus.Protocol.Praos.PraosState`.
pub mod chain_dep;
pub use chain_dep::ChainDepStateContext;

// R269 twelfth slice (cont.) — Treasury + reserves accounting
// (`AccountingState`) mirroring upstream
// `Cardano.Ledger.Shelley.LedgerState::esAccountState`.
pub mod treasury;
pub use treasury::AccountingState;

// R269 thirteenth slice — `LedgerStateSnapshot` (read-only LSQ capture
// view) lives in its own file mirroring upstream
// `Ouroboros.Consensus.Shelley.Ledger.Query` query view.
pub mod snapshot;
pub use snapshot::LedgerStateSnapshot;

// R269 fourteenth slice — `LedgerStateCheckpoint` (full-state restorable
// rollback seam used by `crates/storage`) lives in its own file. Companion
// sidecar to `LedgerState` distinct from `LedgerStateSnapshot`.
pub mod checkpoint;
pub use checkpoint::LedgerStateCheckpoint;

// R269 fifteenth slice — PPUP (Protocol Parameter Update Proposal) helpers
// (`PpupSlotContext`, `pv_can_follow`, `overlay_step`,
// `is_overlay_slot_for_blocks_made`) live in their own file mirroring
// upstream `Cardano.Ledger.Shelley.Rules.Ppup` /
// `Cardano.Ledger.Shelley.PParams::pvCanFollow`.
pub mod ppup;
pub use ppup::{PpupSlotContext, pv_can_follow};
// `overlay_step` + `is_overlay_slot_for_blocks_made` re-exposed for the
// in-state.rs blocks-made counting paths and the existing
// `state/tests.rs::*overlay_slot_*` regressions which call them unqualified
// via `use super::*;`. Promoted to `pub fn` in `ppup.rs` because
// `pub(super) fn` cannot be re-exported across module boundaries.
pub use ppup::{is_overlay_slot_for_blocks_made, overlay_step};

// R269 sixteenth slice — `LedgerState` CBOR codec lives in its own file
// (mechanical 24-element array encoder + length-conditional decoder).
// `state/cbor.rs` is a descendant of `state.rs`, so it can access
// `LedgerState`'s private fields directly without `pub(super)` promotions
// (per Rust's "private items visible to defining module AND ITS DESCENDANTS"
// rule).
pub(super) mod cbor;

// R269 seventeenth slice — per-era `LedgerState` apply implementations.
// Each era's `apply_<era>_block` method moves to `state/eras/<era>.rs` and
// is exposed via `pub(super) fn` so `apply_block_validated` (orchestrator)
// can dispatch across module boundaries. R269q extracts Byron only as a
// validation slice; R269r–R269w follow with the larger per-era apply
// blocks (Shelley, Allegra, Mary, Alonzo, Babbage, Conway).
pub(super) mod eras;

type FutureGenesisDelegKey = (u64, GenesisHash);

pub(super) fn encode_optional_epoch_no(value: Option<EpochNo>, enc: &mut Encoder) {
    match value {
        Some(epoch) => epoch.encode_cbor(enc),
        None => {
            enc.null();
        }
    }
}

pub(super) fn decode_optional_epoch_no(
    dec: &mut Decoder<'_>,
) -> Result<Option<EpochNo>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(EpochNo::decode_cbor(dec)?))
    }
}

pub(super) fn encode_optional_pool_key_hash(value: Option<PoolKeyHash>, enc: &mut Encoder) {
    match value {
        Some(hash) => {
            enc.bytes(&hash);
        }
        None => {
            enc.null();
        }
    }
}

pub(super) fn decode_optional_pool_key_hash(
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

pub(super) fn encode_optional_anchor(value: Option<&Anchor>, enc: &mut Encoder) {
    match value {
        Some(anchor) => anchor.encode_cbor(enc),
        None => {
            enc.null();
        }
    }
}

pub(super) fn decode_optional_anchor(dec: &mut Decoder<'_>) -> Result<Option<Anchor>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(Anchor::decode_cbor(dec)?))
    }
}

pub(super) fn encode_optional_drep(value: Option<&DRep>, enc: &mut Encoder) {
    match value {
        Some(drep) => drep.encode_cbor(enc),
        None => {
            enc.null();
        }
    }
}

pub(super) fn decode_optional_drep(dec: &mut Decoder<'_>) -> Result<Option<DRep>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(DRep::decode_cbor(dec)?))
    }
}

pub(super) fn encode_optional_gov_action_id(
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

pub(super) fn decode_optional_gov_action_id(
    dec: &mut Decoder<'_>,
) -> Result<Option<crate::eras::conway::GovActionId>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(crate::eras::conway::GovActionId::decode_cbor(dec)?))
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
        enact::enact_gov_action_at_epoch(
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
                    0, // tx_index: submitted tx; ptr tracking follows block application
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
                    0, // tx_index: submitted tx; ptr tracking follows block application
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
                    0, // tx_index: submitted tx; ptr tracking follows block application
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
                    0, // tx_index: submitted tx; ptr tracking follows block application
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
                    0, // tx_index: submitted tx; ptr tracking follows block application
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
                    0, // tx_index: submitted tx; ptr tracking follows block application
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
    //
    // R269q extracted `apply_byron_block` to `state/eras/byron.rs`.
    // Subsequent rounds R269r–R269w will move Shelley, Allegra, Mary,
    // Alonzo, Babbage, and Conway respectively.
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

pub(super) fn conway_parameter_change_has_spo_security_vote_group(
    update: &crate::protocol_params::ProtocolParameterUpdate,
) -> bool {
    conway_modified_pparam_groups(update).security
}

pub(super) fn conway_drep_parameter_change_threshold(
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
        0, // current_slot: simulation context, no block slot
        0, // tx_index: simulation context, ptr tracking not needed
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
    tx_index: u64,
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
        for (cert_index, cert) in certs.iter().enumerate() {
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
                    register_stake_credential(
                        stake_credentials,
                        *credential,
                        key_deposit,
                        Some((current_slot, tx_index, cert_index as u64)),
                    )?;
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
                        register_stake_credential(
                            stake_credentials,
                            *credential,
                            *deposit,
                            Some((current_slot, tx_index, cert_index as u64)),
                        )?;
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
                        register_stake_credential(
                            stake_credentials,
                            *credential,
                            *deposit,
                            Some((current_slot, tx_index, cert_index as u64)),
                        )?;
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
                        register_stake_credential(
                            stake_credentials,
                            *credential,
                            *deposit,
                            Some((current_slot, tx_index, cert_index as u64)),
                        )?;
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
                        register_stake_credential(
                            stake_credentials,
                            *credential,
                            *deposit,
                            Some((current_slot, tx_index, cert_index as u64)),
                        )?;
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
    ptr: Option<(u64, u64, u64)>,
) -> Result<(), LedgerError> {
    if !stake_credentials.register_with_ptr(credential, deposit, ptr) {
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

pub(super) fn is_builtin_drep(drep: DRep) -> bool {
    matches!(drep, DRep::AlwaysAbstain | DRep::AlwaysNoConfidence)
}

#[cfg(test)]
mod tests;
