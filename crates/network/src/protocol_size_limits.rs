//! Per-protocol upper bounds on peer-supplied count fields, used by the
//! CBOR decoders to gate `Vec`/`HashMap` pre-allocation against
//! attacker-controlled `count: u64` values.
//!
//! These bounds are *defensive* sanity caps. They must be ≥ the largest
//! count any legitimate upstream peer can produce — they delay or reject
//! pathological allocations, never legitimate messages.
//!
//! Reference: per-protocol codec modules in `ouroboros-network-protocols`
//! and CBOR fragments in `cardano-cddl`.
//!
//! See [`yggdrasil_ledger::vec_with_safe_capacity`] for the soft helper
//! and [`yggdrasil_ledger::vec_with_strict_capacity`] for the hard one.

// ---------------------------------------------------------------------------
// Handshake bounds
// ---------------------------------------------------------------------------

/// Upper bounds for the node-to-node handshake.
///
/// Reference: `Ouroboros.Network.Protocol.Handshake.Codec` and
/// `Ouroboros.Network.Handshake.Acceptable`.
pub mod handshake {
    /// Maximum number of `(version, versionData)` pairs in a
    /// `MsgProposeVersions` / `MsgQueryReply` table.
    ///
    /// Upstream `acceptableVersion` accepts a small handful of versions
    /// at any time (currently ~10 NtN versions). 64 leaves ≥ 6× headroom
    /// for any future upstream growth.
    pub const VERSION_TABLE_MAX: usize = 64;

    /// Maximum number of versions a peer may list in
    /// `RefuseReason::VersionMismatch`.
    pub const REFUSE_VERSION_LIST_MAX: usize = 64;

    /// NtC handshake — fewer versions in flight than NtN.
    pub const NTC_VERSION_TABLE_MAX: usize = 32;
}

// ---------------------------------------------------------------------------
// ChainSync bounds
// ---------------------------------------------------------------------------

/// Upper bounds for the ChainSync mini-protocol.
///
/// Reference: `Ouroboros.Network.Protocol.ChainSync.Codec`.
pub mod chainsync {
    /// Maximum number of points the client may include in
    /// `MsgFindIntersect`. Upstream uses a logarithmic suffix of the
    /// candidate chain plus a fixed sparse prefix; in practice this is
    /// always under a few hundred.
    pub const INTERSECT_POINTS_MAX: usize = 1024;
}

// ---------------------------------------------------------------------------
// TxSubmission bounds
// ---------------------------------------------------------------------------

/// Upper bounds for the TxSubmission2 mini-protocol.
///
/// Reference: `Ouroboros.Network.Protocol.TxSubmission2.Codec` and
/// `Ouroboros.Network.TxSubmission.Inbound.Decision` (the `numTxIdsToReq`
/// budget is a `Word16`).
pub mod txsubmission {
    /// Maximum number of txids in a single `MsgReplyTxIds` reply.
    /// Upstream wire format uses `Word16`, so the natural cap is `u16::MAX`.
    pub const TXIDS_MAX: usize = u16::MAX as usize;

    /// Maximum number of transaction bodies in a single `MsgReplyTxs`
    /// reply. Upstream wire format uses `Word16`.
    pub const TXS_MAX: usize = u16::MAX as usize;
}

// ---------------------------------------------------------------------------
// PeerSharing bounds
// ---------------------------------------------------------------------------

/// Upper bounds for the PeerSharing mini-protocol.
///
/// Reference: `Ouroboros.Network.Protocol.PeerSharing.Codec`.
pub mod peersharing {
    /// Maximum number of peers a peer may share in a single
    /// `MsgSharePeers` reply.  Upstream wire format historically used
    /// `Word8`; current versions widen to `Word16`.
    pub const PEERS_MAX: usize = u16::MAX as usize;
}

// ---------------------------------------------------------------------------
// Block-body bounds (era decoders)
// ---------------------------------------------------------------------------

/// Soft caps for block-body element counts.
///
/// These are enforced by the implicit `max_block_body_size` ledger
/// parameter (≤ 90 112 bytes on mainnet). The cap here is purely an
/// allocator-overflow guard — the decoder loop still pushes `count`
/// items, so legitimate large blocks are never rejected.
pub mod block_body {
    /// Sanity cap for any per-element vector inside a block body
    /// (inputs, outputs, certs, withdrawals, etc.).
    pub const ELEMENTS_MAX: usize = 1_048_576;
}

// ---------------------------------------------------------------------------
// Generic default — only for incidental decoder allocations where no
// per-protocol bound applies.
// ---------------------------------------------------------------------------

/// Default soft cap for incidental decoder allocations.
pub const ALLOC_CAP_DEFAULT: usize = u16::MAX as usize;

// Compile-time sanity checks: clippy rejects `assert!(const_expr)` in
// runtime tests, so we use static_assertions-style const-evaluated
// invariants instead.  These confirm at build time that the caps chosen
// here are large enough not to reject any legitimate upstream message.

const _: () = assert!(handshake::VERSION_TABLE_MAX >= 32);
const _: () = assert!(handshake::NTC_VERSION_TABLE_MAX >= 16);
const _: () = assert!(txsubmission::TXIDS_MAX == u16::MAX as usize);
const _: () = assert!(txsubmission::TXS_MAX == u16::MAX as usize);
// Mainnet max_block_body_size is 90 112 bytes; the element count of any
// per-block vector is strictly bounded below that.  A 1 MiB element cap
// is comfortable headroom.
const _: () = assert!(block_body::ELEMENTS_MAX > 90_112);
