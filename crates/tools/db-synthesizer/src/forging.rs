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
//! ## Carve-outs (NOT ported this round — Phase 4 R2 slice boundary)
//!
//! Upstream's per-slot leader check (`checkShouldForge`) is the Praos
//! VRF/KES/OpCert path: it needs a typed `BlockForging` credential
//! built from a parsed `ShelleyGenesis`, the ledger forecast
//! (`ledgerViewForecastAt`), the ticked `ChainDepState`, and
//! `Block.forgeBlock` (which signs the header with KES). That entire
//! crypto-and-genesis surface is the Praos-forging round (db-synthesizer
//! R3+) and the genesis-loading round (db-synthesizer R2). It is
//! intentionally NOT in this slice.
//!
//! What this slice DOES port: the `runForge` *control loop* shape —
//! the `ForgeState` accumulator, the `ForgeLimit`-driven `forgingDone`
//! predicate, the `nextForgeState` slot/epoch advance arithmetic, and
//! the prev-hash-threaded block-context derivation. Block bodies are
//! produced by [`synth_structural_block`]: a deterministic,
//! **non-Praos** structurally-valid [`Block`] with an empty
//! transaction list, a placeholder issuer key, and a header hash
//! derived by Blake2b-256 over `(prev_hash || slot || block_no)`.
//! Every synthesized block is genuinely chained (its `prev_hash`
//! points at the real predecessor), so the result is a structurally
//! valid ChainDB that yggdrasil's own `FileImmutable`/`db-analyser`
//! can open and walk — it is simply not a Praos-valid chain.

use yggdrasil_ledger::{Block, BlockHeader, BlockNo, Era, HeaderHash, SlotNo, Tx};
use yggdrasil_storage::{ImmutableStore, StorageError};

use crate::types::{ForgeLimit, ForgeResult};

/// Accumulator threaded through the forge loop.
///
/// Mirror of upstream `data ForgeState = ForgeState { currentSlot,
/// forged, currentEpoch, processed }`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ForgeState {
    /// Slot the loop is currently considering.
    pub current_slot: SlotNo,
    /// Number of blocks forged + adopted so far.
    pub forged: u64,
    /// Epoch counter (advances every `epoch_size` processed slots).
    pub current_epoch: u64,
    /// Number of slots processed so far.
    pub processed: SlotNo,
}

impl ForgeState {
    /// Mirror of upstream `initialForgeState = ForgeState 0 0 0 0`.
    pub fn initial() -> Self {
        ForgeState {
            current_slot: SlotNo(0),
            forged: 0,
            current_epoch: 0,
            processed: SlotNo(0),
        }
    }
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
fn next_forge_state(epoch_size: u64, state: ForgeState, did_forge: bool) -> ForgeState {
    let processed = SlotNo(state.processed.0 + 1);
    let current_epoch = if epoch_size != 0 && processed.0.is_multiple_of(epoch_size) {
        state.current_epoch + 1
    } else {
        state.current_epoch
    };
    ForgeState {
        current_slot: SlotNo(state.current_slot.0 + 1),
        forged: state.forged + u64::from(did_forge),
        current_epoch,
        processed,
    }
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
fn current_block_context<S: ImmutableStore>(store: &S) -> BlockContext {
    match store.suffix_after(&yggdrasil_ledger::Point::Origin) {
        Ok(blocks) => match blocks.last() {
            Some(tip) => BlockContext {
                block_no: BlockNo(tip.header.block_no.0 + 1),
                prev_hash: tip.header.hash,
            },
            None => BlockContext {
                block_no: BlockNo(0),
                prev_hash: HeaderHash([0u8; 32]),
            },
        },
        Err(_) => BlockContext {
            block_no: BlockNo(0),
            prev_hash: HeaderHash([0u8; 32]),
        },
    }
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ForgeRunOutcome {
    /// Upstream `ForgeResult` — number of blocks forged + adopted.
    pub result: ForgeResult,
    /// Terminal forge-loop accumulator.
    pub final_state: ForgeState,
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
pub fn run_forge<S: ImmutableStore>(
    era: Era,
    epoch_size: u64,
    next_slot: SlotNo,
    limit: ForgeLimit,
    store: &mut S,
) -> Result<ForgeRunOutcome, StorageError> {
    let mut state = ForgeState {
        current_slot: next_slot,
        ..ForgeState::initial()
    };

    while !forging_done(limit, &state) {
        // This slice always "forges" — there is no leader check, so
        // every processed slot produces exactly one block. Upstream's
        // `goSlot` may return `NoLeader` and skip; that branch is part
        // of the deferred Praos path.
        let ctx = current_block_context(store);
        let block = synth_structural_block(era, ctx, state.current_slot);
        store.append_block(block)?;
        state = next_forge_state(epoch_size, state, true);
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
    use yggdrasil_storage::InMemoryImmutable;

    #[test]
    fn forging_done_slot_limit() {
        let limit = ForgeLimit::Slot(SlotNo(10));
        let mut s = ForgeState::initial();
        assert!(!forging_done(limit, &s));
        s.processed = SlotNo(10);
        assert!(forging_done(limit, &s));
    }

    #[test]
    fn forging_done_block_limit() {
        let limit = ForgeLimit::Block(5);
        let mut s = ForgeState::initial();
        assert!(!forging_done(limit, &s));
        s.forged = 5;
        assert!(forging_done(limit, &s));
    }

    #[test]
    fn forging_done_epoch_limit() {
        let limit = ForgeLimit::Epoch(2);
        let mut s = ForgeState::initial();
        assert!(!forging_done(limit, &s));
        s.current_epoch = 2;
        assert!(forging_done(limit, &s));
    }

    #[test]
    fn next_forge_state_advances_slot_and_processed() {
        let s = next_forge_state(100, ForgeState::initial(), true);
        assert_eq!(s.current_slot, SlotNo(1));
        assert_eq!(s.processed, SlotNo(1));
        assert_eq!(s.forged, 1);
        assert_eq!(s.current_epoch, 0);
    }

    #[test]
    fn next_forge_state_no_forge_keeps_forged_count() {
        let s = next_forge_state(100, ForgeState::initial(), false);
        assert_eq!(s.forged, 0);
        assert_eq!(s.processed, SlotNo(1));
    }

    #[test]
    fn next_forge_state_rolls_epoch_on_boundary() {
        // epoch_size = 4 → processed reaching 4 bumps the epoch.
        let mut s = ForgeState::initial();
        for _ in 0..3 {
            s = next_forge_state(4, s, true);
            assert_eq!(s.current_epoch, 0);
        }
        s = next_forge_state(4, s, true);
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
        let outcome = run_forge(
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
        let outcome = run_forge(
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
            run_forge(Era::Shelley, 4, SlotNo(0), ForgeLimit::Epoch(1), &mut store).unwrap();
        assert_eq!(outcome.result.forged, 4);
        assert_eq!(store.len(), 4);
    }

    #[test]
    fn run_forge_chains_blocks_consistently() {
        let mut store = InMemoryImmutable::default();
        run_forge(
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
        run_forge(
            Era::Shelley,
            100,
            SlotNo(0),
            ForgeLimit::Block(3),
            &mut store,
        )
        .unwrap();
        // Append-style resume: continue from slot 3 for 2 more blocks.
        let outcome = run_forge(
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
}
