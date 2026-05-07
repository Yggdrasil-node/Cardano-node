//! Epoch nonce evolution state machine.
//!
//! Tracks the evolving and candidate nonces per block and computes the
//! new epoch nonce at epoch boundaries. Implements the combined
//! UPDN (Update Nonce) and TICKN rules from `cardano-protocol-tpraos`.
//!
//! Two public types:
//!
//! - `NonceEvolutionConfig` — per-network/per-era config (epoch size,
//!   stability window, extra entropy, Byron→Shelley transition).
//! - `NonceEvolutionState` — per-block mutable state (evolving / candidate
//!   / epoch / lab / prev-hash nonces) plus CBOR encode/decode for the
//!   chain-dep-state sidecar.
//!
//! Mirrors upstream `Cardano.Protocol.TPraos.Rules.Updn` (UPDN rule) +
//! `Cardano.Protocol.TPraos.Rules.Tickn` (TICKN rule) +
//! `Cardano.Protocol.TPraos.API::tickChainDepState` /
//! `updateChainDepState`.
//!
//! Extracted from `nonce.rs` in R273b (Phase γ §R273 second slice).

use yggdrasil_ledger::{EpochNo, HeaderHash, Nonce, SlotNo};

use super::derivation::{NonceDerivation, derive_vrf_nonce};
use crate::epoch::{EpochSize, epoch_first_slot, slot_to_epoch};

/// Configuration parameters governing nonce evolution and stability window.
///
/// These values are fixed per network and era (they come from the genesis
/// configuration).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NonceEvolutionConfig {
    /// Number of slots per epoch (mainnet: 432000) — Shelley-era length.
    pub epoch_size: EpochSize,
    /// Stability window in slots: `3k/f` where `k` = `SecurityParam` and
    /// `f` = `ActiveSlotCoeff` (mainnet: 129600).
    pub stability_window: u64,
    /// Extra entropy injected at epoch transitions (protocol parameter;
    /// `NeutralNonce` from Babbage onward).
    pub extra_entropy: Nonce,
    /// Byron→Shelley transition `(boundary_slot, first_shelley_epoch)`.
    /// `None` for Shelley-only chains (preview, fixed-length test
    /// fixtures); `Some` for chains with a Byron prefix (mainnet,
    /// preprod).
    ///
    /// When set, `apply_block`'s slot→epoch and epoch→first_slot math
    /// uses the era-aware schedule so block at slot
    /// `boundary_slot + N * epoch_size` is correctly classified as
    /// epoch `first_shelley_epoch + N` instead of `EpochNo(N + 1)`
    /// from fixed-length math anchored at slot 0. Without this, the
    /// `tick_epoch_transition` rule fires at the wrong slots for any
    /// chain with a Byron prefix — producing a divergent active
    /// `epoch_nonce` and a downstream `InvalidVrfProof` failure
    /// (R262 root cause).
    ///
    /// Reference:
    /// `crates/consensus/src/epoch.rs::EpochSchedule::with_byron_shelley`.
    pub byron_shelley_transition: Option<(u64, u64)>,
}

impl NonceEvolutionConfig {
    /// Era-aware `slot → epoch`. Mirrors
    /// `EpochSchedule::slot_to_epoch` so nonce evolution uses the
    /// same epoch boundaries as `apply_block_validated` and overlay
    /// classification.
    pub(crate) fn slot_to_epoch(&self, slot: SlotNo) -> EpochNo {
        match self.byron_shelley_transition {
            Some((boundary_slot, first_shelley_epoch)) if slot.0 >= boundary_slot => {
                let post = slot.0 - boundary_slot;
                EpochNo(first_shelley_epoch + post / self.epoch_size.0)
            }
            // Note: Byron-prefix epochs are 21600 slots upstream, but
            // for nonce-evolution purposes Byron blocks don't
            // contribute (`apply_nonce_evolution` skips them), so the
            // exact pre-boundary epoch label only matters for
            // change-detection. Treat the entire Byron prefix as a
            // single conceptual epoch (label 0) so `tick_epoch_transition`
            // does not fire spuriously inside Byron.
            Some(_) => EpochNo(0),
            None => slot_to_epoch(slot, self.epoch_size),
        }
    }

    /// Era-aware `epoch → first_slot`. Mirrors
    /// `EpochSchedule::epoch_first_slot`.
    pub(crate) fn epoch_first_slot(&self, epoch: EpochNo) -> SlotNo {
        match self.byron_shelley_transition {
            Some((boundary_slot, first_shelley_epoch)) if epoch.0 >= first_shelley_epoch => {
                let post_epoch = epoch.0 - first_shelley_epoch;
                SlotNo(boundary_slot + post_epoch * self.epoch_size.0)
            }
            // Byron-prefix epochs collapse to label 0 here (matching
            // `slot_to_epoch` above); the exact first slot of Byron
            // epoch 0 is slot 0.
            Some(_) => SlotNo(0),
            None => epoch_first_slot(epoch, self.epoch_size),
        }
    }
}

/// State machine tracking epoch nonce evolution.
///
/// This tracks the five nonce components described in `ChainDepState` and
/// `PrtclState` from the upstream Haskell implementation:
///
/// - `evolving_nonce` (η_v): cumulative `Semigroup Nonce`-combine
///   (Blake2b-256(a ‖ b)) of all VRF nonce outputs seen so far in the epoch;
///   always updated per block.
/// - `candidate_nonce` (η_c): Same as `evolving_nonce` until the stability
///   window, then frozen.
/// - `epoch_nonce`: The nonce used for VRF verification in the current epoch.
/// - `prev_hash_nonce`: Hash of the last block from the previous epoch, used
///   in the next epoch transition.
/// - `lab_nonce`: "Last applied block" nonce — `prevHashToNonce` from the most
///   recently processed block.
///
/// Reference: `ChainDepState` = `PrtclState` + `TicknState` + `csLabNonce`
/// in `Cardano.Protocol.TPraos.API`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonceEvolutionState {
    /// Evolving nonce (η_v) — combined (Blake2b-256(a ‖ b)) with every
    /// block's VRF nonce output.
    pub evolving_nonce: Nonce,
    /// Candidate nonce (η_c) — tracks evolving nonce until stability window,
    /// then freezes.
    pub candidate_nonce: Nonce,
    /// Current epoch nonce used for VRF verification.
    pub epoch_nonce: Nonce,
    /// Hash of the last block from the previous epoch, combined into the
    /// epoch nonce at the next epoch transition (Blake2b-256(a ‖ b)).
    pub prev_hash_nonce: Nonce,
    /// Nonce from the `prev_hash` field of the last applied block header.
    pub lab_nonce: Nonce,
    /// Current epoch number (used to detect epoch transitions).
    pub current_epoch: EpochNo,
}

impl NonceEvolutionState {
    /// Creates a new state from an initial epoch nonce.
    ///
    /// Reference: `initialChainDepState` in `Cardano.Protocol.TPraos.API`.
    pub fn new(initial_nonce: Nonce) -> Self {
        Self {
            evolving_nonce: initial_nonce,
            candidate_nonce: initial_nonce,
            epoch_nonce: initial_nonce,
            prev_hash_nonce: Nonce::Neutral,
            lab_nonce: Nonce::Neutral,
            current_epoch: EpochNo(0),
        }
    }

    /// Creates a new state starting from a known epoch with a given epoch nonce.
    ///
    /// Useful for resuming from a snapshot or starting from a non-genesis state.
    pub fn from_epoch(epoch: EpochNo, epoch_nonce: Nonce) -> Self {
        Self {
            evolving_nonce: epoch_nonce,
            candidate_nonce: epoch_nonce,
            epoch_nonce,
            prev_hash_nonce: Nonce::Neutral,
            lab_nonce: Nonce::Neutral,
            current_epoch: epoch,
        }
    }

    /// Processes a block, updating nonce state.
    ///
    /// This implements the UPDN rule (per-block nonce update) and the TICKN
    /// rule (epoch transition) in a single call.
    ///
    /// ## Parameters
    ///
    /// - `slot`: The block's slot number.
    /// - `vrf_nonce_output`: The VRF nonce contribution from this block.
    ///   For TPraos (Shelley–Alonzo) this is the `nonce_vrf` output; for
    ///   Praos (Babbage/Conway) this is the single `vrf_result` output.
    ///   Pass the raw VRF output bytes (64 bytes typically); they will be
    ///   hashed to produce a 32-byte nonce using the era-appropriate
    ///   derivation.
    /// - `prev_hash`: The `prev_hash` from the block header (`None` for
    ///   genesis successor blocks).
    /// - `config`: Network and era parameters.
    /// - `derivation`: Era-specific VRF-to-nonce hashing scheme.
    pub fn apply_block(
        &mut self,
        slot: SlotNo,
        vrf_nonce_output: &[u8],
        prev_hash: Option<HeaderHash>,
        config: &NonceEvolutionConfig,
        derivation: NonceDerivation,
    ) {
        // R262 fix: use era-aware slot→epoch so chains with a Byron
        // prefix (mainnet, preprod) classify slot `boundary +
        // N*epoch_size` as Shelley epoch `first_shelley_epoch + N`
        // instead of fixed-length `EpochNo(N + 1)` anchored at slot 0.
        // Without this, `tick_epoch_transition` fires at slots that
        // are mid-epoch upstream (e.g. preprod's slot 432000 sits in
        // Shelley epoch 4, NOT at an epoch-5 boundary), producing a
        // divergent active `epoch_nonce` and `InvalidVrfProof`.
        let block_epoch = config.slot_to_epoch(slot);

        // Detect epoch transition and apply TICKN rule.
        if block_epoch > self.current_epoch {
            self.tick_epoch_transition(config);
            self.current_epoch = block_epoch;
        }

        // Derive nonce from VRF output using era-appropriate derivation.
        let eta = derive_vrf_nonce(vrf_nonce_output, derivation);

        // UPDN rule:
        // evolving_nonce' = evolving_nonce ⭒ η
        self.evolving_nonce = self.evolving_nonce.combine(eta);

        // candidate_nonce update: freeze in stability window.
        let next_epoch = EpochNo(block_epoch.0 + 1);
        let first_slot_next = config.epoch_first_slot(next_epoch);
        let in_stability_window = slot.0 + config.stability_window >= first_slot_next.0;

        if !in_stability_window {
            // Not in stability window — update candidate to match evolving.
            self.candidate_nonce = self.evolving_nonce;
        }
        // else: candidate nonce stays frozen.

        // Update lab_nonce (prevHashToNonce of this block).
        self.lab_nonce = match prev_hash {
            Some(h) => Nonce::from_header_hash(h),
            None => Nonce::Neutral,
        };
    }

    /// Applies the TICKN epoch transition rule.
    ///
    /// This is called when we detect that we've entered a new epoch.
    ///
    /// ```text
    /// epoch_nonce' = candidate_nonce ⭒ prev_hash_nonce ⭒ extra_entropy
    /// prev_hash_nonce' = lab_nonce
    /// ```
    ///
    /// Reference: `tickTransition` in `Cardano.Protocol.TPraos.Rules.Tickn`.
    fn tick_epoch_transition(&mut self, config: &NonceEvolutionConfig) {
        // New epoch nonce = candidate ⭒ prev_hash ⭒ extra_entropy
        self.epoch_nonce = self
            .candidate_nonce
            .combine(self.prev_hash_nonce)
            .combine(config.extra_entropy);

        // The lab_nonce (hash of last applied block) becomes the new
        // prev_hash_nonce for the next epoch transition.
        self.prev_hash_nonce = self.lab_nonce;
    }
}

/// CBOR codec for `NonceEvolutionState` as embedded in node-owned
/// ChainDepState sidecar bundles. Encoded as a 6-element CBOR list in
/// field-order:
///
/// ```text
/// [evolving, candidate, epoch, prev_hash, lab, current_epoch]
/// ```
///
/// Each `Nonce` uses upstream `Cardano.Ledger.Crypto.Nonce` wire
/// shape (`NeutralNonce → [0]`, `Nonce h → [1, h]`).  Mirrors
/// `Ouroboros.Consensus.Protocol.Praos.PraosState`'s persistent
/// nonce sub-record so the LSQ `query protocol-state` response
/// can surface the live values across node restarts.
impl yggdrasil_ledger::cbor::CborEncode for NonceEvolutionState {
    fn encode_cbor(&self, enc: &mut yggdrasil_ledger::cbor::Encoder) {
        enc.array(6);
        encode_nonce(enc, &self.evolving_nonce);
        encode_nonce(enc, &self.candidate_nonce);
        encode_nonce(enc, &self.epoch_nonce);
        encode_nonce(enc, &self.prev_hash_nonce);
        encode_nonce(enc, &self.lab_nonce);
        enc.unsigned(self.current_epoch.0);
    }
}

impl yggdrasil_ledger::cbor::CborDecode for NonceEvolutionState {
    fn decode_cbor(
        dec: &mut yggdrasil_ledger::cbor::Decoder<'_>,
    ) -> Result<Self, yggdrasil_ledger::LedgerError> {
        let len = dec.array()?;
        if len != 6 {
            return Err(yggdrasil_ledger::LedgerError::CborInvalidLength {
                expected: 6,
                actual: len as usize,
            });
        }
        let evolving_nonce = decode_nonce(dec)?;
        let candidate_nonce = decode_nonce(dec)?;
        let epoch_nonce = decode_nonce(dec)?;
        let prev_hash_nonce = decode_nonce(dec)?;
        let lab_nonce = decode_nonce(dec)?;
        let current_epoch = EpochNo(dec.unsigned()?);
        Ok(Self {
            evolving_nonce,
            candidate_nonce,
            epoch_nonce,
            prev_hash_nonce,
            lab_nonce,
            current_epoch,
        })
    }
}

fn encode_nonce(enc: &mut yggdrasil_ledger::cbor::Encoder, nonce: &Nonce) {
    match nonce {
        Nonce::Neutral => {
            enc.array(1);
            enc.unsigned(0);
        }
        Nonce::Hash(h) => {
            enc.array(2);
            enc.unsigned(1);
            enc.bytes(h);
        }
    }
}

fn decode_nonce(
    dec: &mut yggdrasil_ledger::cbor::Decoder<'_>,
) -> Result<Nonce, yggdrasil_ledger::LedgerError> {
    let len = dec.array()?;
    let tag = dec.unsigned()?;
    match (len, tag) {
        (1, 0) => Ok(Nonce::Neutral),
        (2, 1) => {
            let bytes = dec.bytes()?;
            let arr: [u8; 32] =
                bytes
                    .try_into()
                    .map_err(|_| yggdrasil_ledger::LedgerError::CborInvalidLength {
                        expected: 32,
                        actual: bytes.len(),
                    })?;
            Ok(Nonce::Hash(arr))
        }
        _ => Err(yggdrasil_ledger::LedgerError::CborDecodeError(format!(
            "Nonce: unrecognised (len={len}, tag={tag})"
        ))),
    }
}
