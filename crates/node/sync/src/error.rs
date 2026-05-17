//! `SyncError` aggregate error type for node sync orchestration.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side aggregate error type for
//! the node-to-node sync orchestration runtime in `crates/node/sync/src/lib.rs`.
//! Wraps upstream protocol-client errors from
//! `Ouroboros.Network.{ChainSync,BlockFetch,KeepAlive}.Client` plus
//! `Ouroboros.Consensus` validation errors, and surfaces the
//! Yggdrasil-side per-era validation failure variants used by the
//! verified multi-era sync pipeline.
//!
//! `is_peer_attributable` mirrors upstream `InvalidBlockPunishment`
//! peer-attribution semantics from
//! `Ouroboros.Consensus.Storage.ChainDB.API.Types.InvalidBlockPunishment`
//! — errors that result in `throwTo PeerSentAnInvalidBlockException`.
//!
//! Extracted from `crates/node/sync/src/lib.rs` in R498 (sync.rs R-arc, 1st
//! slice). See `docs/operational-runs/2026-05-12-round-498-plan-sync-rs-split-arc.md`
//! for the multi-round plan.

use yggdrasil_consensus::ConsensusError;
use yggdrasil_ledger::{Era, LedgerError, Nonce};
use yggdrasil_network::{
    BlockFetchClientError, ChainSyncClientError, KeepAliveClientError, PeerError,
};
use yggdrasil_storage::StorageError;

/// Error type for sync orchestration operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Peer bootstrap or handshake error before protocol sync begins.
    #[error("peer error: {0}")]
    Peer(#[from] PeerError),

    /// ChainSync protocol error while requesting next chain update.
    #[error("chainsync error: {0}")]
    ChainSync(#[from] ChainSyncClientError),

    /// BlockFetch protocol error while fetching blocks for a roll-forward.
    #[error("blockfetch error: {0}")]
    BlockFetch(#[from] BlockFetchClientError),

    /// Ledger decode error while deserializing fetched block bytes.
    #[error("ledger decode error: {0}")]
    LedgerDecode(#[from] LedgerError),

    /// Storage error while applying decoded sync results.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    /// KeepAlive protocol error during heartbeat.
    #[error("keepalive error: {0}")]
    KeepAlive(#[from] KeepAliveClientError),

    /// Consensus validation error (header verification failure).
    #[error("consensus error: {0}")]
    Consensus(#[from] ConsensusError),

    /// VRF verification failed for a concrete block header.
    #[error(
        "VRF verification failed at slot {slot} in {era} using epoch nonce {epoch_nonce:?}: {source}"
    )]
    VrfVerification {
        /// Slot of the block whose VRF proof failed.
        slot: u64,
        /// Era decoder branch used for this block.
        era: &'static str,
        /// Epoch nonce supplied to the VRF input constructor.
        epoch_nonce: Nonce,
        /// Underlying consensus-layer VRF error.
        #[source]
        source: ConsensusError,
    },

    /// Recovery failed because the available storage state could not be
    /// reconstructed into a usable ledger tip.
    #[error("recovery error: {0}")]
    Recovery(String),

    /// Block body hash in the header does not match the actual block body.
    #[error("block body hash mismatch")]
    BlockBodyHashMismatch,

    /// A received block's slot is beyond the tolerable clock skew.
    ///
    /// Reference: `InFutureHeaderExceedsClockSkew` in
    /// `Ouroboros.Consensus.MiniProtocol.ChainSync.Client.InFutureCheck`.
    #[error("block from far future: slot {slot} is {excess_slots} slots ahead of wall clock")]
    BlockFromFuture {
        /// The block's slot number.
        slot: u64,
        /// How many slots ahead of the wall-clock the block is.
        excess_slots: u64,
    },

    /// The declared block body size in the header does not match the actual
    /// serialized body size.
    ///
    /// Reference: `WrongBlockBodySizeBBODY` in
    /// `Cardano.Ledger.Shelley.Rules.Bbody`.
    #[error(
        "wrong block body size: header declares {declared} bytes, \
         actual body is {actual} bytes"
    )]
    WrongBlockBodySize {
        /// The `block_body_size` field from the block header.
        declared: u32,
        /// The actual serialized size of the block body.
        actual: u32,
    },

    /// The block header's protocol version is outside the acceptable range
    /// for the era it claims to be in.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Chain` — era/protocol
    /// version consistency check.
    #[error(
        "protocol version mismatch: block in era {era:?} carries version \
         ({major}, {minor}), expected major in {expected_range}"
    )]
    ProtocolVersionMismatch {
        /// The era of the block.
        era: Era,
        /// Declared major version.
        major: u64,
        /// Declared minor version.
        minor: u64,
        /// Human-readable expected range string.
        expected_range: String,
    },

    /// The block header's major protocol version exceeds the maximum
    /// major version configured for this node.
    ///
    /// Reference: `MaxMajorProtVer` in
    /// `Ouroboros.Consensus.Shelley.Ledger.Block`.
    #[error(
        "protocol version too high: block major version {major} exceeds \
         node maximum {max}"
    )]
    ProtocolVersionTooHigh {
        /// Declared major version from the block header.
        major: u64,
        /// The node's configured `MaxMajorProtVer`.
        max: u64,
    },

    /// Block header major protocol version exceeds
    /// `pp.protocolVersion.major + 1`.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Bbody` —
    /// `HeaderProtVerTooHigh`.
    #[error(
        "header protocol version too high: major {header_major} > pp major \
         {pp_major} + 1"
    )]
    HeaderProtVerTooHigh {
        /// Major version declared in the block header.
        header_major: u64,
        /// Current protocol-parameter major version.
        pp_major: u64,
    },
}

impl SyncError {
    /// Returns `true` when the error is attributable to the remote peer
    /// sending data that fails validation (invalid block body hash,
    /// consensus header verification failure, or a block that breaks
    /// ledger rules).
    ///
    /// These errors indicate a misbehaving or broken peer and should be
    /// handled by reconnecting to a different peer rather than stopping
    /// the sync service.  Local infrastructure errors (`Storage`) and
    /// protocol framing errors (`ChainSync`, `BlockFetch`) are not
    /// peer-attributable validation failures.
    ///
    /// Reference: upstream `InvalidBlockPunishment` in
    /// `Ouroboros.Consensus.Storage.ChainDB.API.Types.InvalidBlockPunishment`
    /// — errors that result in `throwTo PeerSentAnInvalidBlockException`.
    pub fn is_peer_attributable(&self) -> bool {
        matches!(
            self,
            SyncError::Consensus(_)
                | SyncError::BlockBodyHashMismatch
                | SyncError::LedgerDecode(_)
                | SyncError::VrfVerification { .. }
                | SyncError::BlockFromFuture { .. }
                | SyncError::WrongBlockBodySize { .. }
                | SyncError::ProtocolVersionMismatch { .. }
                | SyncError::ProtocolVersionTooHigh { .. }
                | SyncError::HeaderProtVerTooHigh { .. }
        )
    }
}
