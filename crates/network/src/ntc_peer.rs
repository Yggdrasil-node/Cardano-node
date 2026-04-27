//! Node-to-client (NtC) connection lifecycle.
//!
//! Defines the node-to-client (NtC) protocol surface and version-data helpers,
//! plus the client-side ([`ntc_connect`]) and server-side ([`ntc_accept`])
//! handshake drivers.  Both sides operate over [`tokio::net::UnixStream`] using
//! the shared mini-protocol multiplexer.
//!
//! ## NtC Mini-Protocol IDs
//!
//! | Protocol ID | Name                 |
//! |-------------|----------------------|
//! | 0           | Handshake            |
//! | 5           | LocalChainSync       |
//! | 6           | LocalTxSubmission    |
//! | 7           | LocalStateQuery      |
//! | 9           | LocalTxMonitor       |
//!
//! ## NtC Version Numbers
//!
//! Supported versions (from upstream `Ouroboros.Network.NodeToClient.Version`):
//! - V9 = 9, V10 = 10, V11 = 11, V12 = 12, V13 = 13, V14 = 14, V15 = 15, V16 = 16
//!
//! Version data carries `{network_magic: u32, query: bool}`.
//!
//! Reference: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/NodeToClient>

use std::collections::HashMap;
use std::path::Path;

use tokio::net::UnixStream;
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder, vec_with_strict_capacity};

use crate::handshake::HandshakeVersion;
use crate::multiplexer::{MiniProtocolDir, MiniProtocolNum};
use crate::mux::{self, MuxHandle, ProtocolHandle};

// ---------------------------------------------------------------------------
// NtC version data
// ---------------------------------------------------------------------------

/// Version negotiation payload for node-to-client connections.
///
/// Simpler than the N2N counterpart — carries only `network_magic` and a
/// `query` flag (whether the client intends to run queries only, without sync).
///
/// Reference: `Ouroboros.Network.NodeToClient.NodeToClientVersionData`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeToClientVersionData {
    /// Network magic (1 = preprod, 2 = preview, 764824073 = mainnet).
    pub network_magic: u32,
    /// When `true` the client only queries ledger state and does not sync.
    pub query: bool,
}

/// NtC protocol version numbers.
///
/// Per upstream `Ouroboros.Network.NodeToClient.Version`, every NtC
/// version on the wire has bit 15 ([`NTC_VERSION_BIT`] = `0x8000`) set
/// to distinguish from NtN versions sharing the same handshake table.
/// `NodeToClientV_16` is encoded as the unsigned integer `32784`
/// (`0x8000 | 16`), not as `16`.  The pre-fix definitions used the
/// logical values `9..=16` directly, so cardano-cli's
/// `[V_16..V_23]` proposal (encoded `[32784..=32791]`) never matched
/// — yggdrasil's matcher saw the bit-flagged numbers as
/// "unrecognised versions" and responded `Refuse VersionMismatch`,
/// blocking every upstream `cardano-cli query tip` invocation.
/// 2026-04-27 operational rehearsal captured the on-wire bytes via
/// `YGG_NTC_DEBUG=1`; see
/// `docs/operational-runs/2026-04-27-runbook-pass.md` "Finding B".
///
/// Reference: `Ouroboros.Network.NodeToClient.Version` —
/// `nodeToClientVersionCodec`.
impl HandshakeVersion {
    /// NtC v9 (Alonzo era onwards).
    pub const NTC_V9: Self = Self(NTC_VERSION_BIT | 9);
    /// NtC v10.
    pub const NTC_V10: Self = Self(NTC_VERSION_BIT | 10);
    /// NtC v11.
    pub const NTC_V11: Self = Self(NTC_VERSION_BIT | 11);
    /// NtC v12.
    pub const NTC_V12: Self = Self(NTC_VERSION_BIT | 12);
    /// NtC v13.
    pub const NTC_V13: Self = Self(NTC_VERSION_BIT | 13);
    /// NtC v14.
    pub const NTC_V14: Self = Self(NTC_VERSION_BIT | 14);
    /// NtC v15.
    pub const NTC_V15: Self = Self(NTC_VERSION_BIT | 15);
    /// NtC v16 (Conway era, current).
    pub const NTC_V16: Self = Self(NTC_VERSION_BIT | 16);
}

/// Upstream `nodeToClientVersionBit` (`Ouroboros.Network.NodeToClient.Version`)
/// — high bit (bit 15) flagging every NtC version on the wire.  Pinned
/// here as a `pub const` so the encoding scheme is operator-visible
/// (operators reading the wire bytes can decode the version number
/// against this mask) and so a future drift in the bit convention is
/// caught by the `ntc_version_constants_have_high_bit_set` regression
/// test.
pub const NTC_VERSION_BIT: u16 = 0x8000;

/// NtC mini-protocol set including the handshake (protocol 0).
const NTC_PROTOCOLS: [MiniProtocolNum; 4] = [
    MiniProtocolNum::HANDSHAKE,
    MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION,
    MiniProtocolNum::NTC_LOCAL_STATE_QUERY,
    MiniProtocolNum::NTC_LOCAL_TX_MONITOR,
];

// ---------------------------------------------------------------------------
// NtcPeerConnection
// ---------------------------------------------------------------------------

/// An established node-to-client connection with negotiated version data and
/// per-protocol message channel handles.
pub struct NtcPeerConnection {
    /// Negotiated NtC protocol version.
    pub version: HandshakeVersion,
    /// Version data from the server.
    pub version_data: NodeToClientVersionData,
    /// Per-protocol handles keyed by `MiniProtocolNum`.
    pub protocols: HashMap<MiniProtocolNum, ProtocolHandle>,
    /// Mux handle for lifecycle management.
    pub mux: MuxHandle,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced during NtC connection setup.
#[derive(Debug, thiserror::Error)]
pub enum NtcPeerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("handshake version mismatch — no common NtC version found")]
    VersionMismatch,
    #[error("handshake refused: {0}")]
    HandshakeRefused(String),
    #[error("CBOR codec error: {0}")]
    Cbor(String),
    #[error("mux error: {0}")]
    Mux(String),
}

// ---------------------------------------------------------------------------
// NtC handshake helpers
// ---------------------------------------------------------------------------

/// Encode NtC version data as CBOR `[network_magic, query]`.
#[cfg(test)]
fn encode_ntc_version_data(magic: u32, query: bool) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(magic as u64).bool(query);
    enc.into_bytes()
}

/// Decode NtC version data from CBOR `[network_magic, query]`.
fn decode_ntc_version_data(data: &[u8]) -> Result<NodeToClientVersionData, NtcPeerError> {
    let cbor_err = |e: LedgerError| NtcPeerError::Cbor(e.to_string());
    let mut dec = Decoder::new(data);
    let len = dec.array().map_err(cbor_err)?;
    if len != 2 {
        return Err(NtcPeerError::Cbor(format!(
            "unexpected NtC version-data length {len}; expected 2"
        )));
    }
    let magic = dec.unsigned().map_err(cbor_err)?;
    let query = dec.bool().map_err(cbor_err)?;
    Ok(NodeToClientVersionData {
        network_magic: magic as u32,
        query,
    })
}

// ---------------------------------------------------------------------------
// NtC handshake CBOR helpers
// ---------------------------------------------------------------------------

/// Supported NtC versions (sorted descending for selection).
const NTC_SUPPORTED_VERSIONS: [HandshakeVersion; 8] = [
    HandshakeVersion::NTC_V16,
    HandshakeVersion::NTC_V15,
    HandshakeVersion::NTC_V14,
    HandshakeVersion::NTC_V13,
    HandshakeVersion::NTC_V12,
    HandshakeVersion::NTC_V11,
    HandshakeVersion::NTC_V10,
    HandshakeVersion::NTC_V9,
];

/// Decode `ProposeVersions [0, versionTable]` from raw CBOR.
///
/// Returns a list of `(version, version_data)` pairs.  The version data
/// values are decoded with [`decode_ntc_version_data`].
fn decode_ntc_propose_versions(
    bytes: &[u8],
) -> Result<Vec<(HandshakeVersion, NodeToClientVersionData)>, NtcPeerError> {
    let cbor_err = |e: LedgerError| NtcPeerError::Cbor(e.to_string());
    let mut dec = Decoder::new(bytes);
    let msg_len = dec.array().map_err(cbor_err)?;
    if msg_len < 2 {
        return Err(NtcPeerError::Cbor(format!(
            "NtC handshake message too short: {msg_len}"
        )));
    }
    let tag = dec.unsigned().map_err(cbor_err)?;
    if tag != 0 {
        return Err(NtcPeerError::HandshakeRefused(format!(
            "expected ProposeVersions (tag 0), got tag {tag}"
        )));
    }
    // versionTable is a map { versionNumber => versionData }
    let map_len = dec.map().map_err(cbor_err)?;
    let mut proposals = vec_with_strict_capacity(
        map_len,
        crate::protocol_size_limits::handshake::NTC_VERSION_TABLE_MAX,
    )
    .map_err(cbor_err)?;
    for _ in 0..map_len {
        let ver_num = dec.unsigned().map_err(cbor_err)? as u16;
        // Version data is encoded inline as a CBOR array.
        let vd_start = dec.position();
        // Skip one CBOR item to measure the version-data bytes.
        dec.skip().map_err(cbor_err)?;
        let vd_end = dec.position();
        let vd_bytes = &bytes[vd_start..vd_end];
        match decode_ntc_version_data(vd_bytes) {
            Ok(vd) => proposals.push((HandshakeVersion(ver_num), vd)),
            Err(err) => {
                if std::env::var("YGG_NTC_DEBUG").is_ok_and(|v| v != "0") {
                    let preview: String =
                        vd_bytes.iter().map(|b| format!("{b:02x}")).collect();
                    eprintln!(
                        "[ygg-ntc-debug] decode fail for V{ver_num}: err={err} \
                         vd_bytes_len={} preview={preview}",
                        vd_bytes.len()
                    );
                }
                // Skip undecodable version data — server just won't select
                // this version, matching upstream behavior.
                continue;
            }
        }
    }
    Ok(proposals)
}

/// Encode `AcceptVersion [1, versionNumber, versionData]` as CBOR.
fn encode_ntc_accept_version(version: HandshakeVersion, data: &NodeToClientVersionData) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3).unsigned(1).unsigned(version.0 as u64);
    // Inline version data array.
    enc.array(2)
        .unsigned(data.network_magic as u64)
        .bool(data.query);
    enc.into_bytes()
}

/// Encode `Refuse [2, [0, [*versionNumber]]]` as CBOR (VersionMismatch).
fn encode_ntc_refuse_version_mismatch(proposed: &[HandshakeVersion]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(2);
    // refuseReason: [0, [*versionNumber]]
    enc.array(2).unsigned(0);
    enc.array(proposed.len() as u64);
    for v in proposed {
        enc.unsigned(v.0 as u64);
    }
    enc.into_bytes()
}

/// Encode `ProposeVersions [0, {versionNumber => versionData}]` as CBOR.
///
/// Used by the client side of the NtC handshake to advertise our supported
/// protocol versions.  Each version proposes the same `(network_magic, query)`
/// payload as expected by the server.
///
/// Reference: `Ouroboros.Network.Protocol.Handshake.Codec` — `MsgProposeVersions`.
fn encode_ntc_propose_versions(
    versions: &[HandshakeVersion],
    data: &NodeToClientVersionData,
) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(0); // tag 0 = ProposeVersions
    enc.map(versions.len() as u64);
    for v in versions {
        enc.unsigned(v.0 as u64);
        enc.array(2)
            .unsigned(data.network_magic as u64)
            .bool(data.query);
    }
    enc.into_bytes()
}

/// Decoded server response to `ProposeVersions`.
#[derive(Clone, Debug)]
enum NtcHandshakeReply {
    /// Server accepted one of our proposed versions.
    Accept(HandshakeVersion, NodeToClientVersionData),
    /// Server refused with a textual reason (decoded best-effort).
    Refuse(String),
}

/// Decode the server's reply to a `ProposeVersions` message.
///
/// Handles both `MsgAcceptVersion [1, version, versionData]` and
/// `MsgRefuse [2, refuseReason]` (with the three upstream refuse reasons:
/// `VersionMismatch [0, [*ver]]`, `HandshakeDecodeError [1, ver, str]`, and
/// `Refused [2, ver, str]`).
fn decode_ntc_handshake_reply(bytes: &[u8]) -> Result<NtcHandshakeReply, NtcPeerError> {
    let cbor_err = |e: LedgerError| NtcPeerError::Cbor(e.to_string());
    let mut dec = Decoder::new(bytes);
    let msg_len = dec.array().map_err(cbor_err)?;
    if msg_len < 2 {
        return Err(NtcPeerError::Cbor(format!(
            "NtC handshake reply too short: {msg_len}"
        )));
    }
    let tag = dec.unsigned().map_err(cbor_err)?;
    match tag {
        1 => {
            // AcceptVersion: [1, versionNumber, versionData]
            if msg_len != 3 {
                return Err(NtcPeerError::Cbor(format!(
                    "NtC AcceptVersion expects 3 elements, got {msg_len}"
                )));
            }
            let ver = dec.unsigned().map_err(cbor_err)? as u16;
            let vd_start = dec.position();
            dec.skip().map_err(cbor_err)?;
            let vd_end = dec.position();
            let vd = decode_ntc_version_data(&bytes[vd_start..vd_end])?;
            Ok(NtcHandshakeReply::Accept(HandshakeVersion(ver), vd))
        }
        2 => {
            // Refuse: [2, refuseReason]
            let reason_len = dec.array().map_err(cbor_err)?;
            if reason_len < 1 {
                return Err(NtcPeerError::Cbor(
                    "NtC Refuse reason missing tag".to_string(),
                ));
            }
            let reason_tag = dec.unsigned().map_err(cbor_err)?;
            let msg = match reason_tag {
                0 => "version mismatch".to_string(),
                1 => "handshake decode error".to_string(),
                2 => "version refused".to_string(),
                other => format!("unknown refuse reason {other}"),
            };
            Ok(NtcHandshakeReply::Refuse(msg))
        }
        other => Err(NtcPeerError::HandshakeRefused(format!(
            "unexpected NtC handshake reply tag {other}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Connect to a Cardano node over a Unix socket and complete the NtC handshake.
///
/// Returns a [`NtcPeerConnection`] with handles for LocalTxSubmission,
/// LocalStateQuery, LocalTxMonitor, and LocalChainSync protocols.
///
/// # Arguments
/// - `socket_path` — path to the node's Unix socket (e.g. `/run/cardano-node/node.socket`)
/// - `network_magic` — network discriminant (mainnet = 764824073)
/// - `query_only` — if `true`, advertise as a query-only client (no sync)
pub async fn ntc_connect(
    socket_path: impl AsRef<Path>,
    network_magic: u32,
    query_only: bool,
) -> Result<NtcPeerConnection, NtcPeerError> {
    let stream = UnixStream::connect(socket_path.as_ref()).await?;
    ntc_connect_stream(stream, network_magic, query_only).await
}

/// Run the NtC client handshake on an already-connected Unix stream.
///
/// Used by [`ntc_connect`] and by tests that need to drive both sides over
/// an in-memory `UnixStream` pair.
pub async fn ntc_connect_stream(
    stream: UnixStream,
    network_magic: u32,
    query_only: bool,
) -> Result<NtcPeerConnection, NtcPeerError> {
    let (mut handles, mux_handle) =
        mux::start_unix(stream, MiniProtocolDir::Initiator, &NTC_PROTOCOLS, 32);

    let mut hs = handles
        .remove(&MiniProtocolNum::HANDSHAKE)
        .expect("handshake handle must be registered");

    let propose_data = NodeToClientVersionData {
        network_magic,
        query: query_only,
    };
    let propose_bytes = encode_ntc_propose_versions(&NTC_SUPPORTED_VERSIONS, &propose_data);
    hs.send(propose_bytes).await.map_err(|e| {
        NtcPeerError::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            e.to_string(),
        ))
    })?;

    let reply_bytes = hs.recv().await.ok_or_else(|| {
        NtcPeerError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection closed before NtC handshake reply",
        ))
    })?;

    match decode_ntc_handshake_reply(&reply_bytes)? {
        NtcHandshakeReply::Accept(version, vd) => {
            if vd.network_magic != network_magic {
                return Err(NtcPeerError::HandshakeRefused(format!(
                    "server returned mismatching network magic {} (expected {})",
                    vd.network_magic, network_magic
                )));
            }
            Ok(NtcPeerConnection {
                version,
                version_data: vd,
                protocols: handles,
                mux: mux_handle,
            })
        }
        NtcHandshakeReply::Refuse(reason) => Err(NtcPeerError::HandshakeRefused(reason)),
    }
}

/// Accept an inbound NtC connection from a Unix socket stream.
///
/// Runs the server side of the NtC handshake, selecting the highest common
/// protocol version whose `network_magic` matches, then returns a
/// [`NtcPeerConnection`] with handles for the NtC data mini-protocols.
///
/// Reference: `Ouroboros.Network.NodeToClient` — `NodeToClient.accept`.
///
/// # Arguments
/// - `stream` — accepted Unix socket stream
/// - `network_magic` — expected network magic (connections with different magic are refused)
pub async fn ntc_accept(
    stream: UnixStream,
    network_magic: u32,
) -> Result<NtcPeerConnection, NtcPeerError> {
    let (mut handles, mux_handle) =
        mux::start_unix(stream, MiniProtocolDir::Responder, &NTC_PROTOCOLS, 32);

    // Take the handshake handle — consumed during negotiation.
    let mut hs = handles
        .remove(&MiniProtocolNum::HANDSHAKE)
        .expect("handshake handle must be registered");

    // Receive ProposeVersions from the client.
    let propose_bytes = hs.recv().await.ok_or_else(|| {
        NtcPeerError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection closed before NtC handshake",
        ))
    })?;

    if std::env::var("YGG_NTC_DEBUG").is_ok_and(|v| v != "0") {
        let preview_len = propose_bytes.len().min(256);
        let preview: String = propose_bytes[..preview_len]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        eprintln!(
            "[ygg-ntc-debug] ProposeVersions raw_len={} preview={}",
            propose_bytes.len(),
            preview
        );
    }

    let proposed = decode_ntc_propose_versions(&propose_bytes)?;

    if std::env::var("YGG_NTC_DEBUG").is_ok_and(|v| v != "0") {
        eprintln!(
            "[ygg-ntc-debug] decoded {} version(s): {:?}",
            proposed.len(),
            proposed
                .iter()
                .map(|(v, vd)| (v.0, vd.network_magic, vd.query))
                .collect::<Vec<_>>()
        );
    }

    // Select the highest version that we support and whose network magic matches.
    for &our_ver in &NTC_SUPPORTED_VERSIONS {
        if let Some((_, vd)) = proposed
            .iter()
            .find(|(v, vd)| *v == our_ver && vd.network_magic == network_magic)
        {
            let accept_bytes = encode_ntc_accept_version(our_ver, vd);
            hs.send(accept_bytes).await.map_err(|e| {
                NtcPeerError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    e.to_string(),
                ))
            })?;

            return Ok(NtcPeerConnection {
                version: our_ver,
                version_data: vd.clone(),
                protocols: handles,
                mux: mux_handle,
            });
        }
    }

    // No compatible version — refuse.  Per upstream
    // `Ouroboros.Network.Protocol.Handshake.Codec` the `Refuse
    // VersionMismatch` reply must carry the *server's* supported version
    // table so the client can see what to renegotiate against.  The
    // pre-fix version echoed the client's `proposed` list back, which
    // upstream `cardano-cli` parses as "the server supports nothing of
    // mine", surfacing as `HandshakeError (VersionMismatch [client] [])`
    // — the empty right-hand list is the operator-observable symptom.
    // 2026-04-27 operational rehearsal in
    // `docs/operational-runs/2026-04-27-runbook-pass.md` captured this
    // against `cardano-cli 10.16.0.0`.
    let refuse_bytes = encode_ntc_refuse_version_mismatch(&NTC_SUPPORTED_VERSIONS);
    let _ = hs.send(refuse_bytes).await;

    Err(NtcPeerError::VersionMismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the full `NTC_PROTOCOLS` array against its canonical upstream
    /// content. Mirror of `peer::tests::n2n_protocols_match_canonical_six`
    /// for the NtC side — see that test's rustdoc for the failure-mode
    /// rationale.
    ///
    /// HANDSHAKE is the only mini-protocol shared between N2N and NTC;
    /// pinning both sides' arrays exactly preserves that disjointness
    /// invariant by construction (no need for an explicit cross-array
    /// intersection test, which would require exposing both private
    /// constants to a third file).
    ///
    /// Reference: `Ouroboros.Network.NodeToClient.nodeToClientProtocols`.
    #[test]
    fn ntc_protocols_match_canonical_four() {
        let expected = [
            MiniProtocolNum::HANDSHAKE,
            MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION,
            MiniProtocolNum::NTC_LOCAL_STATE_QUERY,
            MiniProtocolNum::NTC_LOCAL_TX_MONITOR,
        ];
        assert_eq!(
            NTC_PROTOCOLS, expected,
            "NTC_PROTOCOLS drifted from the canonical NtC protocol set",
        );
    }

    #[test]
    fn ntc_supported_versions_covers_v9_through_v16_descending() {
        // The NtC handshake selects "best common version" by iterating
        // `NTC_SUPPORTED_VERSIONS` in declared order. Upstream convention
        // is to list newest first so the first match wins the negotiation.
        // Pin: (1) every V9..=V16 constant is present, (2) the list is
        // strictly descending, (3) length matches the 8-variant range.
        // Drift on any of these would silently change which version a
        // client gets negotiated to (e.g. accidentally picking V9 against
        // a V16-speaking CLI would downgrade NtC features without error).
        assert_eq!(NTC_SUPPORTED_VERSIONS.len(), 8);

        // Every declared constant must appear exactly once.
        let expected = [
            HandshakeVersion::NTC_V16,
            HandshakeVersion::NTC_V15,
            HandshakeVersion::NTC_V14,
            HandshakeVersion::NTC_V13,
            HandshakeVersion::NTC_V12,
            HandshakeVersion::NTC_V11,
            HandshakeVersion::NTC_V10,
            HandshakeVersion::NTC_V9,
        ];
        assert_eq!(NTC_SUPPORTED_VERSIONS, expected);

        // Strictly descending: `v[i].0 > v[i+1].0` for every adjacent pair.
        for i in 0..NTC_SUPPORTED_VERSIONS.len() - 1 {
            assert!(
                NTC_SUPPORTED_VERSIONS[i].0 > NTC_SUPPORTED_VERSIONS[i + 1].0,
                "NTC_SUPPORTED_VERSIONS must be strictly descending, violated at index {i}: \
                 {:?} ≤ {:?}",
                NTC_SUPPORTED_VERSIONS[i].0,
                NTC_SUPPORTED_VERSIONS[i + 1].0,
            );
        }
    }

    #[test]
    fn ntc_handshake_version_constants_have_high_bit_set() {
        // Every NtC version on the wire is `NTC_VERSION_BIT | n` per
        // upstream `nodeToClientVersionCodec`.  Pin both the high-bit
        // invariant AND the literal `0x8000 | logical` value so a
        // future drift (e.g. forgetting the bit on a V17 addition)
        // fails CI cleanly with a clearly-named diagnostic.
        for (vc, logical) in [
            (HandshakeVersion::NTC_V9, 9u16),
            (HandshakeVersion::NTC_V10, 10),
            (HandshakeVersion::NTC_V11, 11),
            (HandshakeVersion::NTC_V12, 12),
            (HandshakeVersion::NTC_V13, 13),
            (HandshakeVersion::NTC_V14, 14),
            (HandshakeVersion::NTC_V15, 15),
            (HandshakeVersion::NTC_V16, 16),
        ] {
            assert_eq!(
                vc.0 & NTC_VERSION_BIT,
                NTC_VERSION_BIT,
                "NTC_V{logical} ({0:#06x}) must carry the upstream node-to-client \
                 high-bit flag (0x8000)",
                vc.0,
            );
            assert_eq!(
                vc.0 & !NTC_VERSION_BIT,
                logical,
                "NTC_V{logical} ({0:#06x}) low bits must be {logical}",
                vc.0,
            );
        }
        // Concrete pin against the upstream-canonical wire numbers.
        assert_eq!(HandshakeVersion::NTC_V16.0, 0x8010); // 32784
        assert_eq!(HandshakeVersion::NTC_V9.0, 0x8009); // 32777
    }

    #[test]
    fn ntc_version_bit_matches_upstream_constant() {
        // `NTC_VERSION_BIT` is `nodeToClientVersionBit` from
        // `Ouroboros.Network.NodeToClient.Version`.  Pinned literal
        // 0x8000 — drift here would silently change the entire NtC
        // version space and break every operator-tooling handshake.
        assert_eq!(NTC_VERSION_BIT, 0x8000);
    }

    #[test]
    fn encode_decode_ntc_version_data() {
        let vd = NodeToClientVersionData {
            network_magic: 764_824_073,
            query: false,
        };
        let encoded = encode_ntc_version_data(vd.network_magic, vd.query);
        let decoded = decode_ntc_version_data(&encoded).unwrap();
        assert_eq!(decoded, vd);
    }

    #[test]
    fn encode_decode_ntc_version_data_query_mode() {
        let vd = NodeToClientVersionData {
            network_magic: 1,
            query: true,
        };
        let encoded = encode_ntc_version_data(vd.network_magic, vd.query);
        let decoded = decode_ntc_version_data(&encoded).unwrap();
        assert_eq!(decoded, vd);
    }

    /// Build a ProposeVersions CBOR message for testing.
    fn build_propose_versions(versions: &[(u16, u32, bool)]) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.array(2).unsigned(0); // tag 0 = ProposeVersions
        enc.map(versions.len() as u64);
        for &(ver, magic, query) in versions {
            enc.unsigned(ver as u64);
            enc.array(2).unsigned(magic as u64).bool(query);
        }
        enc.into_bytes()
    }

    #[test]
    fn decode_ntc_propose_versions_roundtrip() {
        // Wire-format version numbers carry the upstream
        // `nodeToClientVersionBit` (0x8000); decoded versions must
        // round-trip to their canonical `HandshakeVersion::NTC_V*`
        // constants.  Using literal `0x8010` etc. here pins the
        // wire-format expectation explicitly so a future drift
        // (forgetting the bit somewhere in the codec) fails CI.
        let bytes = build_propose_versions(&[
            (0x8010, 764_824_073, false), // V16
            (0x800f, 764_824_073, false), // V15
            (0x8009, 764_824_073, true),  // V9
        ]);
        let proposals = decode_ntc_propose_versions(&bytes).unwrap();
        assert_eq!(proposals.len(), 3);
        assert_eq!(proposals[0].0, HandshakeVersion::NTC_V16);
        assert_eq!(proposals[0].1.network_magic, 764_824_073);
        assert!(!proposals[0].1.query);
        assert_eq!(proposals[2].0, HandshakeVersion::NTC_V9);
        assert!(proposals[2].1.query);
    }

    #[test]
    fn decode_ntc_propose_versions_wrong_magic_skipped_gracefully() {
        // All proposed versions have matching format but different magic.
        let bytes = build_propose_versions(&[(0x8010, 1, false), (0x800f, 2, false)]);
        let proposals = decode_ntc_propose_versions(&bytes).unwrap();
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0].1.network_magic, 1);
        assert_eq!(proposals[1].1.network_magic, 2);
    }

    #[test]
    fn encode_ntc_accept_version_roundtrip() {
        let vd = NodeToClientVersionData {
            network_magic: 1,
            query: true,
        };
        let bytes = encode_ntc_accept_version(HandshakeVersion::NTC_V16, &vd);
        // Decode: [1, 0x8010, [1, true]] — V16 with high-bit flag set.
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().unwrap();
        assert_eq!(len, 3);
        let tag = dec.unsigned().unwrap();
        assert_eq!(tag, 1);
        let ver = dec.unsigned().unwrap() as u16;
        assert_eq!(
            ver, 0x8010,
            "AcceptVersion must echo the wire-format V16 (0x8010), not the logical 16",
        );
        let vd_len = dec.array().unwrap();
        assert_eq!(vd_len, 2);
        let magic = dec.unsigned().unwrap() as u32;
        assert_eq!(magic, 1);
        let query = dec.bool().unwrap();
        assert!(query);
    }

    /// Operational regression for the upstream-`cardano-cli`-interop bug
    /// captured during the 2026-04-27 rehearsal — the EXACT bytes
    /// captured by `YGG_NTC_DEBUG=1` from `cardano-cli 10.16.0.0
    /// query tip --testnet-magic 1`.  A future drift in the version-bit
    /// handling (e.g. dropping the high bit, or shifting the supported
    /// set so V16 falls out) fails this test cleanly with an
    /// "expected version V16 in proposed list" diagnostic.
    #[test]
    fn decode_ntc_propose_versions_accepts_real_cardano_cli_payload() {
        // Captured wire bytes (51 bytes total):
        //   8200a8 19 8010 82 01 f4 19 8011 82 01 f4 19 8012 82 01 f4 ...
        // Decoded structure:
        //   [0,                                   // tag 0 = ProposeVersions
        //    {0x8010 -> [1, false],               // V16
        //     0x8011 -> [1, false],               // V17
        //     ...
        //     0x8017 -> [1, false]}]              // V23
        let bytes: [u8; 51] = [
            0x82, 0x00, 0xa8, 0x19, 0x80, 0x10, 0x82, 0x01, 0xf4, 0x19, 0x80, 0x11, 0x82, 0x01,
            0xf4, 0x19, 0x80, 0x12, 0x82, 0x01, 0xf4, 0x19, 0x80, 0x13, 0x82, 0x01, 0xf4, 0x19,
            0x80, 0x14, 0x82, 0x01, 0xf4, 0x19, 0x80, 0x15, 0x82, 0x01, 0xf4, 0x19, 0x80, 0x16,
            0x82, 0x01, 0xf4, 0x19, 0x80, 0x17, 0x82, 0x01, 0xf4,
        ];
        let proposals = decode_ntc_propose_versions(&bytes).unwrap();
        assert_eq!(
            proposals.len(),
            8,
            "cardano-cli proposes V_16..V_23 — yggdrasil must decode all 8 entries",
        );
        let versions: Vec<u16> = proposals.iter().map(|(v, _)| v.0).collect();
        assert!(
            versions.contains(&HandshakeVersion::NTC_V16.0),
            "cardano-cli's V_16 (0x8010) must round-trip to NTC_V16 — \
             pre-fix this was the empty-overlap bug surfacing as \
             `HandshakeError (VersionMismatch [V_16..V_23] [])`",
        );
        for (_, vd) in &proposals {
            assert_eq!(vd.network_magic, 1, "captured payload was --testnet-magic 1");
            assert!(!vd.query, "cardano-cli query tip uses query=false handshake");
        }
    }

    #[test]
    fn encode_ntc_refuse_version_mismatch_is_valid_cbor() {
        let bytes = encode_ntc_refuse_version_mismatch(&[
            HandshakeVersion::NTC_V16,
            HandshakeVersion::NTC_V15,
        ]);
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().unwrap();
        assert_eq!(len, 2);
        let tag = dec.unsigned().unwrap();
        assert_eq!(tag, 2); // Refuse
        let reason_len = dec.array().unwrap();
        assert_eq!(reason_len, 2);
        let reason_tag = dec.unsigned().unwrap();
        assert_eq!(reason_tag, 0); // VersionMismatch
        let versions_len = dec.array().unwrap();
        assert_eq!(versions_len, 2);
    }

    /// Round 145 regression — the `Refuse VersionMismatch` reply must
    /// carry the *server's* supported version table, not echo back the
    /// client's proposed list.  Pre-fix, `ntc_accept` called
    /// `encode_ntc_refuse_version_mismatch` with `proposed.iter().map(|(v, _)| *v)`
    /// which produced the symptom captured during the 2026-04-27
    /// operational rehearsal: upstream `cardano-cli 10.16.0.0` parsed
    /// the reply and surfaced
    /// `HandshakeError (VersionMismatch [V_16..V_23] [])` — empty
    /// right-hand list because the encoded versions were ALL outside
    /// the client's supported range.  After the fix, the reply
    /// contains `NTC_SUPPORTED_VERSIONS` (V9..V16) and a client whose
    /// own supported range is V17..V23 sees a meaningful
    /// "no overlap; server supports up to V16" diagnosis.
    #[test]
    fn ntc_accept_refuse_payload_carries_server_supported_versions() {
        let bytes = encode_ntc_refuse_version_mismatch(&NTC_SUPPORTED_VERSIONS);
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.array().unwrap(), 2);
        assert_eq!(dec.unsigned().unwrap(), 2); // Refuse
        assert_eq!(dec.array().unwrap(), 2);
        assert_eq!(dec.unsigned().unwrap(), 0); // VersionMismatch
        let versions_len = dec.array().unwrap();
        assert_eq!(
            versions_len as usize,
            NTC_SUPPORTED_VERSIONS.len(),
            "Refuse VersionMismatch must list every server-supported version, \
             not echo the client's proposed list",
        );
        let mut server_versions = Vec::with_capacity(versions_len as usize);
        for _ in 0..versions_len {
            server_versions.push(HandshakeVersion(dec.unsigned().unwrap() as u16));
        }
        assert_eq!(
            server_versions,
            NTC_SUPPORTED_VERSIONS.to_vec(),
            "encoded versions must match NTC_SUPPORTED_VERSIONS in declared order",
        );
    }

    #[test]
    fn encode_ntc_propose_versions_roundtrip() {
        let vd = NodeToClientVersionData {
            network_magic: 764_824_073,
            query: false,
        };
        let bytes = encode_ntc_propose_versions(
            &[HandshakeVersion::NTC_V16, HandshakeVersion::NTC_V15],
            &vd,
        );
        let proposals = decode_ntc_propose_versions(&bytes).unwrap();
        assert_eq!(proposals.len(), 2);
        for (_, p_vd) in &proposals {
            assert_eq!(p_vd.network_magic, vd.network_magic);
            assert_eq!(p_vd.query, vd.query);
        }
        let versions: Vec<_> = proposals.iter().map(|(v, _)| *v).collect();
        assert!(versions.contains(&HandshakeVersion::NTC_V16));
        assert!(versions.contains(&HandshakeVersion::NTC_V15));
    }

    #[test]
    fn decode_ntc_handshake_reply_accept_roundtrip() {
        let vd = NodeToClientVersionData {
            network_magic: 1,
            query: true,
        };
        let bytes = encode_ntc_accept_version(HandshakeVersion::NTC_V14, &vd);
        match decode_ntc_handshake_reply(&bytes).unwrap() {
            NtcHandshakeReply::Accept(ver, decoded) => {
                assert_eq!(ver, HandshakeVersion::NTC_V14);
                assert_eq!(decoded, vd);
            }
            NtcHandshakeReply::Refuse(_) => panic!("expected accept"),
        }
    }

    #[test]
    fn decode_ntc_handshake_reply_refuse_version_mismatch() {
        let bytes = encode_ntc_refuse_version_mismatch(&[HandshakeVersion::NTC_V16]);
        match decode_ntc_handshake_reply(&bytes).unwrap() {
            NtcHandshakeReply::Refuse(reason) => {
                assert!(reason.contains("version mismatch"), "got: {reason}");
            }
            NtcHandshakeReply::Accept(_, _) => panic!("expected refuse"),
        }
    }

    #[tokio::test]
    async fn ntc_connect_and_accept_handshake_succeeds() {
        let (client_stream, server_stream) = tokio::net::UnixStream::pair().unwrap();
        let magic = 1;
        let server = tokio::spawn(async move { ntc_accept(server_stream, magic).await });
        let client =
            tokio::spawn(async move { ntc_connect_stream(client_stream, magic, true).await });

        let server_conn = server.await.unwrap().expect("server handshake");
        let client_conn = client.await.unwrap().expect("client handshake");

        assert_eq!(server_conn.version, HandshakeVersion::NTC_V16);
        assert_eq!(client_conn.version, HandshakeVersion::NTC_V16);
        assert_eq!(server_conn.version_data.network_magic, magic);
        assert_eq!(client_conn.version_data.network_magic, magic);
        assert!(client_conn.version_data.query);
    }

    #[tokio::test]
    async fn ntc_connect_rejects_wrong_magic() {
        let (client_stream, server_stream) = tokio::net::UnixStream::pair().unwrap();
        let server = tokio::spawn(async move { ntc_accept(server_stream, 1).await });
        let client =
            tokio::spawn(async move { ntc_connect_stream(client_stream, 999, false).await });

        let server_res = server.await.unwrap();
        let client_res = client.await.unwrap();
        assert!(matches!(server_res, Err(NtcPeerError::VersionMismatch)));
        assert!(matches!(client_res, Err(NtcPeerError::HandshakeRefused(_))));
    }
}
