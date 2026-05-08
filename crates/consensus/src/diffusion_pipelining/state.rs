//! Diffusion-pipelining state machine — feature flag, tentative-tip
//! tracking, pipelining-event log, and per-peer state.
//!
//! Mirrors upstream `Ouroboros.Consensus.Block.SupportsDiffusionPipelining`
//! `DiffusionPipeliningSupport` feature flag and the
//! `Ouroboros.Consensus.HardFork.Combinator.Node.DiffusionPipelining`
//! per-peer state tracking.
//!
//! Five public types:
//!
//! - `DiffusionPipeliningSupport` — feature flag (Off / On).
//! - `TentativeState` — global tentative-tip orchestrator that
//!   serialises ChainSync tentative-header serving against the
//!   per-pool `TentativeHeaderState` rings.
//! - `TentativeHeader` — the currently-pipelined tentative header.
//! - `PipeliningEvent` — per-event trace surface (announced, retracted,
//!   confirmed, trap detected).
//! - `PeerPipeliningState` — per-peer subset of `TentativeState` used
//!   by the inbound ChainSync server.
//!
//! Extracted from `diffusion_pipelining.rs` in R273f (Phase γ §R273
//! sixth slice).
//!
//! ## Naming parity
//!
//! **Strict mirror (partial):** `Ouroboros.Consensus.Block.SupportsDiffusionPipelining.hs`
//! (`DiffusionPipeliningSupport` flag) + `Ouroboros.Consensus.HardFork.Combinator.Node.DiffusionPipelining.hs`
//! (per-peer state). Two upstream files combined here under the more
//! general `state.rs` name.

use yggdrasil_ledger::BlockNo;

use crate::header::HeaderBody;

use super::identity::{TentativeHeaderState, TentativeHeaderView};

// ---------------------------------------------------------------------------
// DiffusionPipeliningSupport — feature gate
// ---------------------------------------------------------------------------

/// Whether diffusion pipelining is enabled for this node.
///
/// Reference: `DiffusionPipeliningSupport` in
/// `Ouroboros.Consensus.Node.ProtocolInfo`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffusionPipeliningSupport {
    /// Pipelining disabled: ChainSync servers only serve the confirmed
    /// chain selection (no tentative tip).
    DiffusionPipeliningOff,
    /// Pipelining enabled: ChainSync servers may serve a tentative header
    /// at the tip before body validation completes.
    DiffusionPipeliningOn,
}

// ---------------------------------------------------------------------------
// TentativeState — combined tentative header + criterion state for ChainDb
// ---------------------------------------------------------------------------

/// Combined tentative header state maintained by ChainDb (or equivalent).
///
/// Holds the currently tentative header (if any) alongside the criterion
/// state.  The tentative header extends the current confirmed chain
/// selection and is served to ChainSync followers when pipelining is
/// enabled.
///
/// Invariants:
/// - When `tentative_header` is `Some`, the header fits on top of the
///   current confirmed chain tip and its body has not yet been validated.
/// - When `tentative_header` is `None`, the confirmed chain tip is the
///   authoritative tip for all followers.
///
/// Reference: `cdbTentativeState` + `cdbTentativeHeader` in
/// `Ouroboros.Consensus.Storage.ChainDB.Impl.Types`.
#[derive(Clone, Debug)]
pub struct TentativeState {
    /// The pipelining criterion state.
    pub criterion_state: TentativeHeaderState,
    /// The currently tentative header, if any.
    ///
    /// Stored as:
    /// - `block_no` and `slot` for identification
    /// - `header_hash` for point construction
    /// - `raw_header` so ChainSync servers can send it without re-encoding
    pub tentative_header: Option<TentativeHeader>,
}

/// A tentative header pending body validation.
#[derive(Clone, Debug)]
pub struct TentativeHeader {
    /// Block number of the tentative header.
    pub block_no: BlockNo,
    /// Slot of the tentative header.
    pub slot: yggdrasil_ledger::SlotNo,
    /// Header hash (for point construction and follower comparison).
    pub header_hash: yggdrasil_ledger::HeaderHash,
    /// The `TentativeHeaderView` extracted at pipelining time, used to
    /// update `TentativeHeaderState` if this becomes a trap header.
    pub view: TentativeHeaderView,
    /// Raw CBOR-encoded header bytes for ChainSync serving.
    pub raw_header: Vec<u8>,
}

/// Trace events emitted by the diffusion pipelining subsystem.
///
/// Reference: `TracePipeliningEvent` in
/// `Ouroboros.Consensus.Storage.ChainDB.Impl.Types`.
#[derive(Clone, Debug)]
pub enum PipeliningEvent {
    /// A tentative header was set (announced before body validation).
    ///
    /// Upstream: `SetTentativeHeader`.
    SetTentativeHeader {
        /// Block number of the tentative header.
        block_no: BlockNo,
        /// Slot of the tentative header.
        slot: yggdrasil_ledger::SlotNo,
        /// Header hash.
        header_hash: yggdrasil_ledger::HeaderHash,
    },
    /// A previously tentative header is no longer needed because the body
    /// was valid and the block is now part of the confirmed selection.
    ///
    /// Upstream: `OutdatedTentativeHeader` (cleared because adopted normally).
    TentativeHeaderAdopted {
        block_no: BlockNo,
        slot: yggdrasil_ledger::SlotNo,
        header_hash: yggdrasil_ledger::HeaderHash,
    },
    /// A tentative header's body turned out to be invalid (trap header).
    /// The announcement is rolled back.
    ///
    /// Upstream: `TrapTentativeHeader`.
    TrapTentativeHeader {
        block_no: BlockNo,
        slot: yggdrasil_ledger::SlotNo,
        header_hash: yggdrasil_ledger::HeaderHash,
    },
}

impl TentativeState {
    /// Create initial tentative state with no tentative header and clean
    /// criterion state.
    pub fn initial() -> Self {
        Self {
            criterion_state: TentativeHeaderState::initial(),
            tentative_header: None,
        }
    }

    /// Try to set a tentative header.
    ///
    /// Returns `Some(PipeliningEvent::SetTentativeHeader)` if the header
    /// passes the pipelining criterion and was set as tentative.  Returns
    /// `None` if the criterion was not met (header should not be pipelined).
    ///
    /// The caller should announce the header to ChainSync followers on
    /// `Some`.
    pub fn try_set_tentative(
        &mut self,
        header_body: &HeaderBody,
        slot: yggdrasil_ledger::SlotNo,
        header_hash: yggdrasil_ledger::HeaderHash,
        raw_header: Vec<u8>,
    ) -> Option<PipeliningEvent> {
        let view = TentativeHeaderView::from_header_body(header_body);
        // Check criterion — but don't update state yet.
        // State is only updated if this becomes a trap (body invalid).
        let _trap_state = self.criterion_state.apply_tentative_header_view(&view)?;

        let block_no = header_body.block_number;
        self.tentative_header = Some(TentativeHeader {
            block_no,
            slot,
            header_hash,
            view,
            raw_header,
        });

        Some(PipeliningEvent::SetTentativeHeader {
            block_no,
            slot,
            header_hash,
        })
    }

    /// Clear the tentative header because the body was valid and the block
    /// is now part of the confirmed chain selection.
    ///
    /// Returns the trace event, or `None` if there was no tentative header.
    pub fn clear_adopted(&mut self) -> Option<PipeliningEvent> {
        let th = self.tentative_header.take()?;
        Some(PipeliningEvent::TentativeHeaderAdopted {
            block_no: th.block_no,
            slot: th.slot,
            header_hash: th.header_hash,
        })
    }

    /// Clear the tentative header because the body was **invalid** (trap
    /// header).
    ///
    /// Updates the criterion state to record this trap, which may prevent
    /// future headers from the same issuer at the same block number from
    /// being pipelined.
    ///
    /// Returns the trace event, or `None` if there was no tentative header.
    pub fn clear_trap(&mut self) -> Option<PipeliningEvent> {
        let th = self.tentative_header.take()?;

        // Apply the view to record the trap in criterion state.
        if let Some(new_state) = self.criterion_state.apply_tentative_header_view(&th.view) {
            self.criterion_state = new_state;
        }

        Some(PipeliningEvent::TrapTentativeHeader {
            block_no: th.block_no,
            slot: th.slot,
            header_hash: th.header_hash,
        })
    }

    /// Whether there is currently a tentative header.
    pub fn has_tentative(&self) -> bool {
        self.tentative_header.is_some()
    }

    /// Access the current tentative header, if any.
    pub fn tentative(&self) -> Option<&TentativeHeader> {
        self.tentative_header.as_ref()
    }
}

impl Default for TentativeState {
    fn default() -> Self {
        Self::initial()
    }
}

// ---------------------------------------------------------------------------
// Per-peer pipelining validation
// ---------------------------------------------------------------------------

/// Per-upstream-peer pipelining state, used by BlockFetch clients to
/// validate that a peer adheres to the pipelining criterion.
///
/// Each BlockFetch upstream connection maintains its own
/// `TentativeHeaderState`.  When a block received from the peer has an
/// invalid body, we check whether the peer's behavior is consistent with
/// the criterion.  If not, we disconnect.
///
/// Reference: BlockFetch punishment logic in
/// `Ouroboros.Consensus.MiniProtocol.BlockFetch.ClientInterface`.
#[derive(Clone, Debug)]
pub struct PeerPipeliningState {
    state: TentativeHeaderState,
}

impl PeerPipeliningState {
    /// Create initial per-peer state.
    pub fn initial() -> Self {
        Self {
            state: TentativeHeaderState::initial(),
        }
    }

    /// Check whether a peer's invalid block (trap header) is consistent
    /// with the pipelining criterion.
    ///
    /// Returns `true` if the peer's behavior is allowed and we should
    /// record the trap in the per-peer state.
    ///
    /// Returns `false` if the peer violated the criterion and should be
    /// disconnected.
    pub fn check_peer_trap(&mut self, view: &TentativeHeaderView) -> bool {
        match self.state.apply_tentative_header_view(view) {
            Some(new_state) => {
                self.state = new_state;
                true
            }
            None => false,
        }
    }
}

impl Default for PeerPipeliningState {
    fn default() -> Self {
        Self::initial()
    }
}
