//! Per-block forge loop for the `db-synthesizer` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Forging.hs.
//!
//! Direct port of upstream's `runForge` driver loop and its
//! `ForgeState` accumulator. Upstream `runForge`:
//!
//! 1. Walks slot-by-slot from `nextSlot`.
//! 2. Per slot, derives a `BlockContext` (prev block + next `BlockNo`)
//!    from the current ChainDB chain fragment.
//! 3. Runs `checkShouldForge` against every `BlockForging` credential;
//!    if a forger is slot-leader it produces a block, adds it to the
//!    ChainDB, and verifies adoption.
//! 4. Stops when [`ForgeLimit`] is reached
//!    (`ForgeLimitSlot`/`ForgeLimitBlock`/`ForgeLimitEpoch`).
//!
//! ## Slice boundary (post Phase 4 R3c-5)
//!
//! R3c-4 consumes the typed [`BlockProducerCredentials`] set and uses
//! the shared node block-producer's Praos leader check + KES-signed
//! `forgeBlock` surface. R3c-5 wires the leader-check stake fraction
//! to the same rotating ledger snapshots (`mark`/`set`/`go`) used by
//! the node's forecast ledger view.
//!
//! What this slice DOES port: the `runForge` *control loop* shape,
//! the `ForgeState` accumulator, the `ForgeLimit`-driven
//! `forgingDone` predicate, the `nextForgeState` slot/epoch advance
//! arithmetic, the prev-hash-threaded block-context derivation, and
//! R3c-3's state threading: a genesis-seeded [`LedgerState`] plus
//! [`NonceEvolutionState`] are replayed through any existing ChainDB
//! prefix and advanced for each newly adopted block. Production forging
//! now calls `checkShouldForge`, skips non-leader slots, calls
//! `forgeBlock`, and appends the KES-signed Praos block only after the
//! ledger/nonce state transition succeeds.

use yggdrasil_consensus::{
    ActiveSlotCoeff, EpochSize, NonceDerivation, NonceEvolutionConfig, NonceEvolutionState,
};
use yggdrasil_crypto::blake2b::hash_bytes_224;
use yggdrasil_ledger::{
    Block, BlockHeader, BlockNo, EpochNo, Era, HeaderHash, LedgerState, Nonce, PoolKeyHash, SlotNo,
    StakeSnapshots, Tx, apply_epoch_boundary, compute_stake_snapshot,
};
use yggdrasil_ledger::{CborDecode, ConwayBlock, Decoder};
use yggdrasil_node_block_producer::{
    BlockContext as ProducerBlockContext, BlockProducerCredentials, BlockProducerError,
    ShouldForge, check_should_forge, forge_block, forged_block_to_storage_block,
    make_block_context,
};
use yggdrasil_storage::{ImmutableStore, StorageError};

use std::collections::BTreeMap;

use crate::types::{ForgeLimit, ForgeResult};

/// Accumulator threaded through the forge loop.
///
/// Mirror of upstream `data ForgeState = ForgeState { currentSlot,
/// forged, currentEpoch, processed }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForgeState {
    /// Slot the loop is currently considering.
    pub current_slot: SlotNo,
    /// Number of blocks forged + adopted so far.
    pub forged: u64,
    /// Epoch counter (advances every `epoch_size` processed slots).
    pub current_epoch: u64,
    /// Number of slots processed so far.
    pub processed: SlotNo,
    /// Ledger state at the current forge tip.
    pub ledger_state: LedgerState,
    /// Praos nonce-evolution state at the current forge tip.
    pub nonce_evolution: NonceEvolutionState,
    /// Rotating active stake snapshots used by Praos leader election.
    pub stake_snapshots: StakeSnapshots,
}

impl ForgeState {
    /// Mirror of upstream `initialForgeState = ForgeState 0 0 0 0`,
    /// extended with the genesis-seeded ledger and nonce state that
    /// Haskell supplies through `pInfoInitLedger` / `ChainDepState`.
    pub fn initial(ledger_state: LedgerState, nonce_evolution: NonceEvolutionState) -> Self {
        let stake_snapshots = stake_snapshots_from_ledger_state(&ledger_state);
        Self::initial_with_stake_snapshots(ledger_state, nonce_evolution, stake_snapshots)
    }

    /// Builds the initial forge state with an explicit ledger-view stake
    /// snapshot. The db-synthesizer production path uses this for the
    /// Shelley-genesis forecast snapshot before the first synthetic block
    /// has activated the pending genesis stake inside [`LedgerState`].
    pub fn initial_with_stake_snapshots(
        ledger_state: LedgerState,
        nonce_evolution: NonceEvolutionState,
        stake_snapshots: StakeSnapshots,
    ) -> Self {
        ForgeState {
            current_slot: SlotNo(0),
            forged: 0,
            current_epoch: 0,
            processed: SlotNo(0),
            ledger_state,
            nonce_evolution,
            stake_snapshots,
        }
    }
}

fn stake_snapshots_from_ledger_state(ledger_state: &LedgerState) -> StakeSnapshots {
    let current = compute_stake_snapshot(
        ledger_state.multi_era_utxo(),
        ledger_state.stake_credentials(),
        ledger_state.reward_accounts(),
        ledger_state.pool_state(),
    );
    StakeSnapshots {
        mark: current.clone(),
        set: current.clone(),
        go: current,
        fee_pot: 0,
        previous_fee_pot: 0,
    }
}

fn structural_nonce_input(block: &Block) -> Vec<u8> {
    block.header.hash.0.to_vec()
}

fn conway_praos_nonce_input_from_raw(raw: &[u8]) -> Result<Option<Vec<u8>>, ForgeError> {
    let mut dec = Decoder::new(raw);
    let envelope_len = dec.array()?;
    if envelope_len != 2 {
        return Ok(None);
    }
    let era_tag = dec.unsigned()?;
    if era_tag != 7 {
        return Ok(None);
    }
    let block = ConwayBlock::decode_cbor(&mut dec)?;
    Ok(Some(block.header.body.vrf_result.output))
}

fn nonce_input_for_replayed_block(block: &Block) -> Result<Vec<u8>, ForgeError> {
    if block.era == Era::Conway
        && let Some(raw) = block.raw_cbor.as_deref()
        && let Some(nonce_input) = conway_praos_nonce_input_from_raw(raw)?
    {
        return Ok(nonce_input);
    }
    Ok(structural_nonce_input(block))
}

fn predecessor_nonce_hash(block: &Block) -> Option<HeaderHash> {
    if block.header.block_no == BlockNo(0) {
        None
    } else {
        Some(block.header.prev_hash)
    }
}

fn apply_block_to_state(
    state: &mut ForgeState,
    block: &Block,
    nonce_input: &[u8],
    nonce_config: &NonceEvolutionConfig,
    nonce_derivation: NonceDerivation,
) -> Result<(), ForgeError> {
    state.ledger_state.apply_block(block)?;
    state.nonce_evolution.apply_block(
        block.header.slot_no,
        nonce_input,
        predecessor_nonce_hash(block),
        nonce_config,
        nonce_derivation,
    );
    Ok(())
}

fn replay_existing_chain<S: ImmutableStore>(
    store: &S,
    state: &mut ForgeState,
    nonce_config: &NonceEvolutionConfig,
    nonce_derivation: NonceDerivation,
) -> Result<(), ForgeError> {
    for block in store.suffix_after(&yggdrasil_ledger::Point::Origin)? {
        let nonce_input = nonce_input_for_replayed_block(&block)?;
        apply_block_to_state(state, &block, &nonce_input, nonce_config, nonce_derivation)?;
    }
    Ok(())
}

fn replay_existing_chain_with_epoch_boundaries<S: ImmutableStore>(
    store: &S,
    epoch_size: u64,
    state: &mut ForgeState,
    nonce_config: &NonceEvolutionConfig,
    nonce_derivation: NonceDerivation,
) -> Result<(), ForgeError> {
    for block in store.suffix_after(&yggdrasil_ledger::Point::Origin)? {
        apply_epoch_boundaries_through_slot(epoch_size, state, block.header.slot_no)?;
        let nonce_input = nonce_input_for_replayed_block(&block)?;
        apply_block_to_state(state, &block, &nonce_input, nonce_config, nonce_derivation)?;
    }
    Ok(())
}

/// Errors from the forge loop.
#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    /// Underlying immutable-store failure.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    /// Ledger rejected a replayed or newly forged structural block.
    #[error("ledger error: {0}")]
    Ledger(#[from] yggdrasil_ledger::LedgerError),
    /// Block-producer failure while checking leadership or forging.
    #[error("block-producer error: {0}")]
    BlockProducer(#[from] BlockProducerError),
}

/// Stop predicate — mirror of upstream `forgingDone`.
///
/// ```haskell
/// forgingDone = case opts of
///   ForgeLimitSlot s  -> (s ==) . processed
///   ForgeLimitBlock b -> (b ==) . forged
///   ForgeLimitEpoch e -> (e ==) . currentEpoch
/// ```
fn forging_done(limit: ForgeLimit, state: &ForgeState) -> bool {
    match limit {
        ForgeLimit::Slot(s) => state.processed == s,
        ForgeLimit::Block(b) => state.forged == b,
        ForgeLimit::Epoch(e) => state.current_epoch == e,
    }
}

/// Advance the accumulator after a slot — mirror of upstream
/// `nextForgeState`.
///
/// ```haskell
/// processed' = processed + 1
/// epoch'     = currentEpoch + if unSlotNo processed' `rem` epochSize == 0 then 1 else 0
/// ```
fn next_forge_state(epoch_size: u64, state: &mut ForgeState, did_forge: bool) {
    let processed = SlotNo(state.processed.0 + 1);
    let current_epoch = if epoch_size != 0 && processed.0.is_multiple_of(epoch_size) {
        state.current_epoch + 1
    } else {
        state.current_epoch
    };
    state.current_slot = SlotNo(state.current_slot.0 + 1);
    state.forged += u64::from(did_forge);
    state.current_epoch = current_epoch;
    state.processed = processed;
}

fn apply_epoch_boundaries_through_slot(
    epoch_size: u64,
    state: &mut ForgeState,
    slot: SlotNo,
) -> Result<(), ForgeError> {
    if epoch_size == 0 {
        return Ok(());
    }

    let target_epoch = slot.0 / epoch_size;
    while state.ledger_state.current_epoch().0 < target_epoch {
        let new_epoch = EpochNo(state.ledger_state.current_epoch().0 + 1);
        apply_epoch_boundary(
            &mut state.ledger_state,
            new_epoch,
            &mut state.stake_snapshots,
            &BTreeMap::new(),
        )?;
    }
    Ok(())
}

fn advance_praos_forge_state(
    epoch_size: u64,
    state: &mut ForgeState,
    did_forge: bool,
) -> Result<(), ForgeError> {
    next_forge_state(epoch_size, state, did_forge);
    apply_epoch_boundaries_through_slot(epoch_size, state, state.current_slot)
}

/// Context for the block about to be forged — mirror of upstream
/// `data BlockContext = BlockContext { bcBlockNo, bcPrevPoint }`.
///
/// Yggdrasil's `FileImmutable` is a flat append-only store, so the
/// prev-point is reduced to the prev header hash (the field the
/// synthesized `Block.header.prev_hash` needs).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BlockContext {
    /// Block number to stamp on the block about to be forged.
    pub block_no: BlockNo,
    /// Header hash of the predecessor block (`[0; 32]` at genesis).
    pub prev_hash: HeaderHash,
}

/// Derive the [`BlockContext`] from the current immutable chain tip.
///
/// Mirror of upstream `mkCurrentBlockContext` collapsed onto the flat
/// `ImmutableStore`: the next `BlockNo` is `tip_block_no + 1` (or `0`
/// at genesis) and the prev-hash is the tip header hash (or the
/// genesis predecessor `[0; 32]`).
fn current_block_context<S: ImmutableStore>(store: &S) -> Result<BlockContext, ForgeError> {
    match store
        .suffix_after(&yggdrasil_ledger::Point::Origin)?
        .last()
        .cloned()
    {
        Some(tip) => Ok(BlockContext {
            block_no: BlockNo(tip.header.block_no.0 + 1),
            prev_hash: tip.header.hash,
        }),
        None => Ok(BlockContext {
            block_no: BlockNo(0),
            prev_hash: HeaderHash([0u8; 32]),
        }),
    }
}

fn current_producer_block_context<S: ImmutableStore>(
    store: &S,
    current_slot: SlotNo,
) -> Result<Option<ProducerBlockContext>, ForgeError> {
    let blocks = store.suffix_after(&yggdrasil_ledger::Point::Origin)?;
    let (tip_slot, tip_block_no, tip_hash) = match blocks.last() {
        Some(tip) => (
            Some(tip.header.slot_no),
            Some(tip.header.block_no),
            Some(tip.header.hash),
        ),
        None => (None, None, None),
    };
    Ok(make_block_context(
        current_slot,
        tip_slot,
        tip_block_no,
        tip_hash,
    ))
}

/// Deterministically derive a structurally-valid (non-Praos) block.
///
/// The header hash is `Blake2b-256(prev_hash || slot_no_le ||
/// block_no_le)` — a stable, collision-resistant identity that lets
/// `FileImmutable` index the block without a real KES-signed header.
///
/// This is the explicit carve-out boundary for this slice: a real
/// Praos block carries a VRF cert, a KES signature, an operational
/// certificate, and the upstream CBOR header layout. None of that is
/// present here. The block IS, however, genuinely chained: `prev_hash`
/// points at the real predecessor, so a reader that walks the chain
/// (yggdrasil's `db-analyser`, `FileImmutable::get_tip`) sees a
/// consistent, non-branching sequence.
pub fn synth_structural_block(era: Era, ctx: BlockContext, slot: SlotNo) -> Block {
    let mut preimage = Vec::with_capacity(32 + 8 + 8);
    preimage.extend_from_slice(&ctx.prev_hash.0);
    preimage.extend_from_slice(&slot.0.to_le_bytes());
    preimage.extend_from_slice(&ctx.block_no.0.to_le_bytes());
    let hash = HeaderHash(yggdrasil_crypto::blake2b::hash_bytes_256(&preimage).0);
    Block {
        era,
        header: BlockHeader {
            hash,
            prev_hash: ctx.prev_hash,
            slot_no: slot,
            block_no: ctx.block_no,
            issuer_vkey: [0u8; 32],
            protocol_version: None,
        },
        transactions: Vec::<Tx>::new(),
        raw_cbor: None,
        header_cbor_size: None,
    }
}

/// Outcome of [`run_forge`] — the [`ForgeResult`] plus the final
/// [`ForgeState`] so callers can report the slot/epoch reached.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForgeRunOutcome {
    /// Upstream `ForgeResult` — number of blocks forged + adopted.
    pub result: ForgeResult,
    /// Terminal forge-loop accumulator.
    pub final_state: ForgeState,
}

/// Runtime parameters needed to advance the forge state.
#[derive(Clone, Debug)]
pub struct ForgeRuntimeConfig {
    /// Network / era nonce-evolution parameters.
    pub nonce_config: NonceEvolutionConfig,
    /// TPraos vs Praos nonce derivation for the current structural era.
    pub nonce_derivation: NonceDerivation,
    /// Active slot coefficient `f` used by Praos leader election.
    pub active_slot_coeff: ActiveSlotCoeff,
    /// Maximum block body size for mempool prefix selection.
    pub max_block_body_size: u32,
    /// Protocol version embedded into forged Praos headers.
    pub protocol_version: (u64, u64),
}

/// Drive the forge loop against an [`ImmutableStore`].
///
/// Mirror of upstream `runForge epochSize nextSlot opts chainDB
/// blockForging cfg genTxs` — minus the Praos crypto path (see the
/// module-level carve-out note). `epoch_size` is the Shelley-genesis
/// `epochLength`; in this slice it is supplied by the caller (stubbed
/// to a mainnet-shaped constant until genesis loading lands).
///
/// Returns [`ForgeRunOutcome`] or the first [`StorageError`] from
/// `append_block`.
pub fn run_structural_forge<S: ImmutableStore>(
    era: Era,
    epoch_size: u64,
    next_slot: SlotNo,
    limit: ForgeLimit,
    store: &mut S,
) -> Result<ForgeRunOutcome, ForgeError> {
    let initial_state = ForgeState::initial(
        LedgerState::new(Era::Byron),
        NonceEvolutionState::new(Nonce::Neutral),
    );
    let nonce_config = NonceEvolutionConfig {
        epoch_size: EpochSize(epoch_size),
        stability_window: 0,
        extra_entropy: Nonce::Neutral,
        byron_shelley_transition: None,
    };
    let active_slot_coeff = ActiveSlotCoeff::new(1.0)
        .expect("1.0 is a valid active slot coefficient for structural tests");
    run_structural_forge_with_state(
        era,
        epoch_size,
        next_slot,
        limit,
        store,
        initial_state,
        ForgeRuntimeConfig {
            nonce_config,
            nonce_derivation: NonceDerivation::TPraos,
            active_slot_coeff,
            max_block_body_size: u32::MAX,
            protocol_version: (0, 0),
        },
    )
}

/// Drive the forge loop with a caller-supplied ledger / nonce state.
///
/// Existing blocks in `store` are replayed first so append-mode runs
/// start from the same ledger and nonce cursor as their ChainDB tip.
/// The new-block path applies each candidate block to cloned state
/// before appending, committing the state transition only after the
/// store accepts the block.
pub fn run_structural_forge_with_state<S: ImmutableStore>(
    era: Era,
    epoch_size: u64,
    next_slot: SlotNo,
    limit: ForgeLimit,
    store: &mut S,
    mut state: ForgeState,
    runtime_config: ForgeRuntimeConfig,
) -> Result<ForgeRunOutcome, ForgeError> {
    replay_existing_chain(
        store,
        &mut state,
        &runtime_config.nonce_config,
        runtime_config.nonce_derivation,
    )?;
    state.current_slot = next_slot;
    state.forged = 0;
    state.current_epoch = 0;
    state.processed = SlotNo(0);

    while !forging_done(limit, &state) {
        // This slice always "forges" — there is no leader check, so
        // every processed slot produces exactly one block. Upstream's
        // `goSlot` may return `NoLeader` and skip; that branch lives in
        // the production Praos forge path below.
        let ctx = current_block_context(store)?;
        let block = synth_structural_block(era, ctx, state.current_slot);
        let nonce_input = structural_nonce_input(&block);
        let mut next_state = state.clone();
        apply_block_to_state(
            &mut next_state,
            &block,
            &nonce_input,
            &runtime_config.nonce_config,
            runtime_config.nonce_derivation,
        )?;
        store.append_block(block)?;
        state.ledger_state = next_state.ledger_state;
        state.nonce_evolution = next_state.nonce_evolution;
        next_forge_state(epoch_size, &mut state, true);
    }

    Ok(ForgeRunOutcome {
        result: ForgeResult {
            forged: i64::try_from(state.forged).unwrap_or(i64::MAX),
        },
        final_state: state,
    })
}

fn check_forgers_for_slot(
    forgers: &mut [BlockProducerCredentials],
    slot: SlotNo,
    epoch_nonce: Nonce,
    stake_snapshots: &StakeSnapshots,
    runtime_config: &ForgeRuntimeConfig,
) -> Option<(usize, yggdrasil_node_block_producer::LeaderElectionResult)> {
    for (index, forger) in forgers.iter_mut().enumerate() {
        let (sigma_num, sigma_den) = relative_stake_for_forger(stake_snapshots, forger);
        match check_should_forge(
            forger,
            slot,
            epoch_nonce,
            sigma_num,
            sigma_den,
            &runtime_config.active_slot_coeff,
        ) {
            ShouldForge::ShouldForge(election) => return Some((index, election)),
            ShouldForge::NotLeader
            | ShouldForge::CannotForge(_)
            | ShouldForge::ForgeStateUpdateError(_) => {}
        }
    }
    None
}

fn pool_key_hash_for_forger(forger: &BlockProducerCredentials) -> PoolKeyHash {
    hash_bytes_224(&forger.issuer_vkey.to_bytes()).0
}

fn relative_stake_for_forger(
    stake_snapshots: &StakeSnapshots,
    forger: &BlockProducerCredentials,
) -> (u64, u64) {
    let pool = pool_key_hash_for_forger(forger);
    stake_snapshots
        .set
        .pool_stake_distribution()
        .relative_stake(&pool)
}

/// Drive the upstream-shaped Praos forge loop with caller-supplied
/// ledger / nonce state and block-producer credentials.
///
/// Existing blocks in `store` are replayed first. For each processed
/// slot, the first credential that returns [`ShouldForge::ShouldForge`]
/// wins; non-leader slots advance `processed` but do not append a block
/// or increment `forged`, matching upstream `go . nextForgeState ...
/// . isRight =<< goSlot`.
pub fn run_forge<S: ImmutableStore>(
    epoch_size: u64,
    next_slot: SlotNo,
    limit: ForgeLimit,
    store: &mut S,
    mut state: ForgeState,
    runtime_config: ForgeRuntimeConfig,
    forgers: &mut [BlockProducerCredentials],
) -> Result<ForgeRunOutcome, ForgeError> {
    if forgers.is_empty() {
        state.current_slot = next_slot;
        state.forged = 0;
        state.current_epoch = 0;
        state.processed = SlotNo(0);
        return Ok(ForgeRunOutcome {
            result: ForgeResult { forged: 0 },
            final_state: state,
        });
    }

    replay_existing_chain_with_epoch_boundaries(
        store,
        epoch_size,
        &mut state,
        &runtime_config.nonce_config,
        runtime_config.nonce_derivation,
    )?;
    state.current_slot = next_slot;
    state.forged = 0;
    state.current_epoch = 0;
    state.processed = SlotNo(0);

    while !forging_done(limit, &state) {
        let epoch_nonce = state.nonce_evolution.epoch_nonce;
        let Some(context) = current_producer_block_context(store, state.current_slot)? else {
            advance_praos_forge_state(epoch_size, &mut state, false)?;
            continue;
        };
        let Some((forger_index, election)) = check_forgers_for_slot(
            forgers,
            state.current_slot,
            epoch_nonce,
            &state.stake_snapshots,
            &runtime_config,
        ) else {
            advance_praos_forge_state(epoch_size, &mut state, false)?;
            continue;
        };

        let forger = &forgers[forger_index];
        let forged = forge_block(
            forger,
            &election,
            &context,
            state.current_slot,
            &[],
            runtime_config.max_block_body_size,
            forger.issuer_vkey.clone(),
            runtime_config.protocol_version,
        )?;
        let nonce_input = forged.header.header_body.leader_vrf_output.clone();
        let block = forged_block_to_storage_block(&forged);

        let mut next_state = state.clone();
        apply_block_to_state(
            &mut next_state,
            &block,
            &nonce_input,
            &runtime_config.nonce_config,
            runtime_config.nonce_derivation,
        )?;
        store.append_block(block)?;
        state.ledger_state = next_state.ledger_state;
        state.nonce_evolution = next_state.nonce_evolution;
        advance_praos_forge_state(epoch_size, &mut state, true)?;
    }

    Ok(ForgeRunOutcome {
        result: ForgeResult {
            forged: i64::try_from(state.forged).unwrap_or(i64::MAX),
        },
        final_state: state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_consensus::OpCert;
    use yggdrasil_crypto::blake2b::hash_bytes_256;
    use yggdrasil_crypto::ed25519::SigningKey;
    use yggdrasil_crypto::sum_kes::{derive_sum_kes_vk, gen_sum_kes_signing_key};
    use yggdrasil_crypto::vrf::VrfSecretKey;
    use yggdrasil_ledger::{
        Delegations, IndividualStake, PoolParams, RewardAccount, StakeCredential, StakeSnapshot,
        UnitInterval,
    };
    use yggdrasil_storage::InMemoryImmutable;

    fn test_forge_state() -> ForgeState {
        ForgeState::initial(
            LedgerState::new(Era::Byron),
            NonceEvolutionState::new(Nonce::Neutral),
        )
    }

    fn test_runtime_config(epoch_size: u64) -> ForgeRuntimeConfig {
        ForgeRuntimeConfig {
            nonce_config: NonceEvolutionConfig {
                epoch_size: EpochSize(epoch_size),
                stability_window: 0,
                extra_entropy: Nonce::Neutral,
                byron_shelley_transition: None,
            },
            nonce_derivation: NonceDerivation::TPraos,
            active_slot_coeff: ActiveSlotCoeff::new(1.0).unwrap(),
            max_block_body_size: 65_536,
            protocol_version: (9, 0),
        }
    }

    fn test_praos_runtime_config(epoch_size: u64) -> ForgeRuntimeConfig {
        ForgeRuntimeConfig {
            nonce_derivation: NonceDerivation::Praos,
            ..test_runtime_config(epoch_size)
        }
    }

    fn test_pool_params(forger: &BlockProducerCredentials) -> PoolParams {
        let pool = pool_key_hash_for_forger(forger);
        PoolParams {
            operator: pool,
            vrf_keyhash: hash_bytes_256(&forger.vrf_verification_key.to_bytes()).0,
            pledge: 0,
            cost: 0,
            margin: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash(pool),
            },
            pool_owners: vec![pool],
            relays: Vec::new(),
            pool_metadata: None,
        }
    }

    fn stake_snapshots_for(entries: &[(&BlockProducerCredentials, u64)]) -> StakeSnapshots {
        let mut stake = IndividualStake::new();
        let mut delegations = Delegations::new();
        let mut pool_params = BTreeMap::new();

        for (forger, amount) in entries {
            let pool = pool_key_hash_for_forger(forger);
            let credential = StakeCredential::AddrKeyHash(pool);
            stake.add(credential, *amount);
            delegations.insert(credential, pool);
            pool_params.insert(pool, test_pool_params(forger));
        }

        let snapshot = StakeSnapshot {
            stake,
            delegations,
            pool_params,
        };
        StakeSnapshots {
            mark: snapshot.clone(),
            set: snapshot.clone(),
            go: snapshot,
            fee_pot: 0,
            previous_fee_pot: 0,
        }
    }

    fn test_forge_state_with_stake(forgers: &[(&BlockProducerCredentials, u64)]) -> ForgeState {
        ForgeState::initial_with_stake_snapshots(
            LedgerState::new(Era::Byron),
            NonceEvolutionState::new(Nonce::Neutral),
            stake_snapshots_for(forgers),
        )
    }

    fn test_praos_credentials(seed: u8) -> BlockProducerCredentials {
        let cold_sk = SigningKey::from_bytes([seed; 32]);
        let cold_vk = cold_sk.verification_key().unwrap();
        let kes_seed = [seed.wrapping_add(1); 32];
        let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).unwrap();
        let kes_vk = derive_sum_kes_vk(&kes_sk).unwrap();
        let opcert_signable = {
            let mut buf = [0u8; 48];
            buf[..32].copy_from_slice(&kes_vk.to_bytes());
            buf[32..40].copy_from_slice(&0u64.to_be_bytes());
            buf[40..48].copy_from_slice(&0u64.to_be_bytes());
            buf
        };
        let opcert_sigma = cold_sk.sign(&opcert_signable).unwrap();
        let vrf_sk = VrfSecretKey::from_seed([seed.wrapping_add(2); 32]);
        let vrf_vk = vrf_sk.verification_key();

        BlockProducerCredentials {
            vrf_signing_key: vrf_sk,
            vrf_verification_key: vrf_vk,
            kes_signing_key: kes_sk,
            kes_current_period: 0,
            operational_cert: OpCert {
                hot_vkey: kes_vk,
                sequence_number: 0,
                kes_period: 0,
                sigma: opcert_sigma,
            },
            issuer_vkey: cold_vk,
            slots_per_kes_period: 1_000_000,
            max_kes_evolutions: 1,
        }
    }

    #[test]
    fn forging_done_slot_limit() {
        let limit = ForgeLimit::Slot(SlotNo(10));
        let mut s = test_forge_state();
        assert!(!forging_done(limit, &s));
        s.processed = SlotNo(10);
        assert!(forging_done(limit, &s));
    }

    #[test]
    fn forging_done_block_limit() {
        let limit = ForgeLimit::Block(5);
        let mut s = test_forge_state();
        assert!(!forging_done(limit, &s));
        s.forged = 5;
        assert!(forging_done(limit, &s));
    }

    #[test]
    fn forging_done_epoch_limit() {
        let limit = ForgeLimit::Epoch(2);
        let mut s = test_forge_state();
        assert!(!forging_done(limit, &s));
        s.current_epoch = 2;
        assert!(forging_done(limit, &s));
    }

    #[test]
    fn next_forge_state_advances_slot_and_processed() {
        let mut s = test_forge_state();
        next_forge_state(100, &mut s, true);
        assert_eq!(s.current_slot, SlotNo(1));
        assert_eq!(s.processed, SlotNo(1));
        assert_eq!(s.forged, 1);
        assert_eq!(s.current_epoch, 0);
    }

    #[test]
    fn next_forge_state_no_forge_keeps_forged_count() {
        let mut s = test_forge_state();
        next_forge_state(100, &mut s, false);
        assert_eq!(s.forged, 0);
        assert_eq!(s.processed, SlotNo(1));
    }

    #[test]
    fn next_forge_state_rolls_epoch_on_boundary() {
        // epoch_size = 4 → processed reaching 4 bumps the epoch.
        let mut s = test_forge_state();
        for _ in 0..3 {
            next_forge_state(4, &mut s, true);
            assert_eq!(s.current_epoch, 0);
        }
        next_forge_state(4, &mut s, true);
        assert_eq!(s.processed, SlotNo(4));
        assert_eq!(s.current_epoch, 1);
    }

    #[test]
    fn synth_structural_block_is_deterministic() {
        let ctx = BlockContext {
            block_no: BlockNo(3),
            prev_hash: HeaderHash([7u8; 32]),
        };
        let a = synth_structural_block(Era::Shelley, ctx, SlotNo(42));
        let b = synth_structural_block(Era::Shelley, ctx, SlotNo(42));
        assert_eq!(a, b);
    }

    #[test]
    fn synth_structural_block_threads_prev_hash() {
        let ctx = BlockContext {
            block_no: BlockNo(1),
            prev_hash: HeaderHash([9u8; 32]),
        };
        let block = synth_structural_block(Era::Shelley, ctx, SlotNo(1));
        assert_eq!(block.header.prev_hash, HeaderHash([9u8; 32]));
        assert_eq!(block.header.block_no, BlockNo(1));
        assert_eq!(block.header.slot_no, SlotNo(1));
        assert!(block.transactions.is_empty());
    }

    #[test]
    fn synth_structural_block_distinct_slots_distinct_hashes() {
        let ctx = BlockContext {
            block_no: BlockNo(0),
            prev_hash: HeaderHash([0u8; 32]),
        };
        let a = synth_structural_block(Era::Shelley, ctx, SlotNo(1));
        let b = synth_structural_block(Era::Shelley, ctx, SlotNo(2));
        assert_ne!(a.header.hash, b.header.hash);
    }

    #[test]
    fn run_forge_block_limit_produces_exactly_n_blocks() {
        let mut store = InMemoryImmutable::default();
        let outcome = run_structural_forge(
            Era::Shelley,
            100,
            SlotNo(0),
            ForgeLimit::Block(5),
            &mut store,
        )
        .unwrap();
        assert_eq!(outcome.result.forged, 5);
        assert_eq!(store.len(), 5);
    }

    #[test]
    fn run_forge_slot_limit_produces_exactly_n_blocks() {
        let mut store = InMemoryImmutable::default();
        let outcome = run_structural_forge(
            Era::Shelley,
            100,
            SlotNo(0),
            ForgeLimit::Slot(SlotNo(8)),
            &mut store,
        )
        .unwrap();
        // Every slot forges in this slice → processed == forged.
        assert_eq!(outcome.result.forged, 8);
        assert_eq!(store.len(), 8);
    }

    #[test]
    fn run_forge_epoch_limit_produces_epoch_size_blocks() {
        let mut store = InMemoryImmutable::default();
        // epoch_size = 4, limit = 1 epoch → 4 slots processed.
        let outcome =
            run_structural_forge(Era::Shelley, 4, SlotNo(0), ForgeLimit::Epoch(1), &mut store)
                .unwrap();
        assert_eq!(outcome.result.forged, 4);
        assert_eq!(store.len(), 4);
    }

    #[test]
    fn run_forge_chains_blocks_consistently() {
        let mut store = InMemoryImmutable::default();
        run_structural_forge(
            Era::Shelley,
            100,
            SlotNo(0),
            ForgeLimit::Block(4),
            &mut store,
        )
        .unwrap();
        let blocks = store
            .suffix_after(&yggdrasil_ledger::Point::Origin)
            .unwrap();
        assert_eq!(blocks.len(), 4);
        // Genesis successor has the all-zero prev-hash.
        assert_eq!(blocks[0].header.prev_hash, HeaderHash([0u8; 32]));
        // Each subsequent block points at its real predecessor.
        for w in blocks.windows(2) {
            assert_eq!(w[1].header.prev_hash, w[0].header.hash);
            assert_eq!(w[1].header.block_no.0, w[0].header.block_no.0 + 1);
        }
    }

    #[test]
    fn run_forge_resumes_from_next_slot() {
        let mut store = InMemoryImmutable::default();
        run_structural_forge(
            Era::Shelley,
            100,
            SlotNo(0),
            ForgeLimit::Block(3),
            &mut store,
        )
        .unwrap();
        // Append-style resume: continue from slot 3 for 2 more blocks.
        let outcome = run_structural_forge(
            Era::Shelley,
            100,
            SlotNo(3),
            ForgeLimit::Block(2),
            &mut store,
        )
        .unwrap();
        assert_eq!(outcome.result.forged, 2);
        assert_eq!(store.len(), 5);
        let blocks = store
            .suffix_after(&yggdrasil_ledger::Point::Origin)
            .unwrap();
        assert_eq!(blocks[4].header.slot_no, SlotNo(4));
        assert_eq!(blocks[4].header.block_no, BlockNo(4));
    }

    #[test]
    fn run_structural_forge_with_state_advances_ledger_tip_and_nonce() {
        let mut store = InMemoryImmutable::default();
        let initial_nonce = Nonce::Hash([1u8; 32]);
        let initial_state = ForgeState::initial(
            LedgerState::new(Era::Byron),
            NonceEvolutionState::new(initial_nonce),
        );
        let outcome = run_structural_forge_with_state(
            Era::Shelley,
            100,
            SlotNo(0),
            ForgeLimit::Block(3),
            &mut store,
            initial_state,
            test_runtime_config(100),
        )
        .unwrap();

        let blocks = store
            .suffix_after(&yggdrasil_ledger::Point::Origin)
            .unwrap();
        let tip = blocks.last().unwrap();
        assert_eq!(
            outcome.final_state.ledger_state.tip,
            yggdrasil_ledger::Point::BlockPoint(tip.header.slot_no, tip.header.hash),
        );
        assert_eq!(
            outcome.final_state.ledger_state.tip_block_no,
            Some(tip.header.block_no),
        );
        assert_ne!(
            outcome.final_state.nonce_evolution.evolving_nonce, initial_nonce,
            "structural block adoption must advance the nonce cursor",
        );
    }

    #[test]
    fn run_structural_forge_with_state_replays_existing_chain_before_append() {
        let mut one_shot = InMemoryImmutable::default();
        let one_shot_outcome = run_structural_forge_with_state(
            Era::Shelley,
            100,
            SlotNo(0),
            ForgeLimit::Block(5),
            &mut one_shot,
            test_forge_state(),
            test_runtime_config(100),
        )
        .unwrap();

        let mut appended = InMemoryImmutable::default();
        run_structural_forge_with_state(
            Era::Shelley,
            100,
            SlotNo(0),
            ForgeLimit::Block(3),
            &mut appended,
            test_forge_state(),
            test_runtime_config(100),
        )
        .unwrap();
        let append_outcome = run_structural_forge_with_state(
            Era::Shelley,
            100,
            SlotNo(3),
            ForgeLimit::Block(2),
            &mut appended,
            test_forge_state(),
            test_runtime_config(100),
        )
        .unwrap();

        assert_eq!(appended.len(), 5);
        assert_eq!(
            append_outcome.final_state.ledger_state.tip,
            one_shot_outcome.final_state.ledger_state.tip,
        );
        assert_eq!(
            append_outcome.final_state.nonce_evolution,
            one_shot_outcome.final_state.nonce_evolution,
        );
    }

    #[test]
    fn run_forge_praos_block_limit_uses_leader_check_and_signed_blocks() {
        let mut store = InMemoryImmutable::default();
        let mut forgers = vec![test_praos_credentials(0x31)];
        let expected_issuer = forgers[0].issuer_vkey.to_bytes();
        let outcome = run_forge(
            100,
            SlotNo(0),
            ForgeLimit::Block(2),
            &mut store,
            test_forge_state_with_stake(&[(&forgers[0], 1)]),
            test_praos_runtime_config(100),
            &mut forgers,
        )
        .unwrap();

        assert_eq!(outcome.result.forged, 2);
        assert_eq!(store.len(), 2);
        let blocks = store
            .suffix_after(&yggdrasil_ledger::Point::Origin)
            .unwrap();
        assert!(blocks.iter().all(|block| block.era == Era::Conway));
        assert!(blocks.iter().all(|block| block.raw_cbor.is_some()));
        assert_eq!(blocks[0].header.issuer_vkey, expected_issuer);
        assert_eq!(blocks[0].header.protocol_version, Some((9, 0)));
        assert_eq!(
            outcome.final_state.ledger_state.tip,
            yggdrasil_ledger::Point::BlockPoint(blocks[1].header.slot_no, blocks[1].header.hash),
        );
    }

    #[test]
    fn run_forge_praos_not_leader_advances_processed_without_appending() {
        let mut store = InMemoryImmutable::default();
        let mut forgers = vec![test_praos_credentials(0x41)];

        let outcome = run_forge(
            10,
            SlotNo(0),
            ForgeLimit::Slot(SlotNo(3)),
            &mut store,
            test_forge_state(),
            test_praos_runtime_config(10),
            &mut forgers,
        )
        .unwrap();

        assert_eq!(outcome.result.forged, 0);
        assert_eq!(outcome.final_state.processed, SlotNo(3));
        assert_eq!(outcome.final_state.current_slot, SlotNo(3));
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn run_forge_praos_replays_raw_cbor_nonce_state_before_append() {
        let mut one_shot = InMemoryImmutable::default();
        let mut one_shot_forgers = vec![test_praos_credentials(0x51)];
        let one_shot_outcome = run_forge(
            100,
            SlotNo(0),
            ForgeLimit::Block(3),
            &mut one_shot,
            test_forge_state_with_stake(&[(&one_shot_forgers[0], 1)]),
            test_praos_runtime_config(100),
            &mut one_shot_forgers,
        )
        .unwrap();

        let mut appended = InMemoryImmutable::default();
        let mut first_forgers = vec![test_praos_credentials(0x51)];
        run_forge(
            100,
            SlotNo(0),
            ForgeLimit::Block(2),
            &mut appended,
            test_forge_state_with_stake(&[(&first_forgers[0], 1)]),
            test_praos_runtime_config(100),
            &mut first_forgers,
        )
        .unwrap();
        let mut second_forgers = vec![test_praos_credentials(0x51)];
        let append_outcome = run_forge(
            100,
            SlotNo(2),
            ForgeLimit::Block(1),
            &mut appended,
            test_forge_state_with_stake(&[(&second_forgers[0], 1)]),
            test_praos_runtime_config(100),
            &mut second_forgers,
        )
        .unwrap();

        assert_eq!(appended.len(), 3);
        assert_eq!(
            append_outcome.final_state.ledger_state.tip,
            one_shot_outcome.final_state.ledger_state.tip,
        );
        assert_eq!(
            append_outcome.final_state.nonce_evolution,
            one_shot_outcome.final_state.nonce_evolution,
        );
    }
}
