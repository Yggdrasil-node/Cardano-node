//! DMQ node-to-node protocol version.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/dmq-node/dmq-node/src/DMQ/NodeToNode/Version.hs.
//!
//! Ports `NodeToNodeVersion` (the version enum, its CBOR-term codec
//! `nodeToNodeVersionCodec`, and its JSON rendering) and
//! `NodeToNodeVersionData` with the `Acceptable` version-negotiation
//! instance (`NodeToNodeVersionData::accept`). The
//! `NodeToNodeVersionData` CBOR-term codec (`nodeToNodeCodecCBORTerm`)
//! lands with a subsequent dmq-node-arc round.

use crate::types::NetworkMagic;
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

/// The DMQ node-to-node protocol version.
///
/// Upstream `data NodeToNodeVersion = NodeToNodeV_1 | NodeToNodeV_2`
/// (`deriving (Eq, Ord, Enum, Bounded, Show)`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum NodeToNodeVersion {
    /// `NodeToNodeV_1`.
    V1,
    /// `NodeToNodeV_2`.
    V2,
}

impl NodeToNodeVersion {
    /// Every version, low to high — upstream's `[minBound .. maxBound]`.
    pub const ALL: [NodeToNodeVersion; 2] = [NodeToNodeVersion::V1, NodeToNodeVersion::V2];

    /// The on-the-wire integer tag — `NodeToNodeV_1` is 1, `_2` is 2.
    pub fn to_int(self) -> u64 {
        match self {
            NodeToNodeVersion::V1 => 1,
            NodeToNodeVersion::V2 => 2,
        }
    }

    /// The version for an integer tag, or `None` for an unknown tag.
    pub fn from_int(tag: u64) -> Option<NodeToNodeVersion> {
        match tag {
            1 => Some(NodeToNodeVersion::V1),
            2 => Some(NodeToNodeVersion::V2),
            _ => None,
        }
    }

    /// Encode as a CBOR integer term.
    ///
    /// Mirror of upstream `nodeToNodeVersionCodec`'s `encodeTerm`
    /// (`NodeToNodeV_1` is `TInt 1`, `_2` is `TInt 2`).
    pub fn encode(self, enc: &mut Encoder) {
        enc.unsigned(self.to_int());
    }

    /// Decode from a CBOR integer term.
    ///
    /// Mirror of upstream `nodeToNodeVersionCodec`'s `decodeTerm` —
    /// an unknown tag is a decode error.
    pub fn decode(dec: &mut Decoder) -> Result<NodeToNodeVersion, LedgerError> {
        let tag = dec.unsigned()?;
        NodeToNodeVersion::from_int(tag).ok_or_else(|| {
            LedgerError::CborDecodeError(format!("decode NodeToNodeVersion: unknown tag: {tag}"))
        })
    }

    /// Render as JSON — the bare integer tag.
    ///
    /// Mirror of upstream `instance ToJSON NodeToNodeVersion`
    /// (`NodeToNodeV_1` is `1`, `_2` is `2`).
    pub fn to_json(self) -> serde_json::Value {
        serde_json::json!(self.to_int())
    }
}

/// The diffusion mode a node runs in.
///
/// Upstream `Ouroboros.Network.DiffusionMode`. `Ord` so that version
/// negotiation can pick the more restrictive (smaller) of two modes —
/// `InitiatorOnly` is the smaller.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DiffusionMode {
    /// `InitiatorOnlyDiffusionMode` — the node only initiates connections.
    InitiatorOnly,
    /// `InitiatorAndResponderDiffusionMode` — the node also responds.
    InitiatorAndResponder,
}

/// Whether peer sharing is enabled on a connection.
///
/// Upstream `Ouroboros.Network.PeerSelection.PeerSharing`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerSharing {
    /// `PeerSharingDisabled`.
    Disabled,
    /// `PeerSharingEnabled`.
    Enabled,
}

impl PeerSharing {
    /// Combine two peer-sharing settings — sharing is agreed only when
    /// both peers enable it (upstream `Semigroup PeerSharing`).
    fn combine(self, other: PeerSharing) -> PeerSharing {
        match (self, other) {
            (PeerSharing::Enabled, PeerSharing::Enabled) => PeerSharing::Enabled,
            _ => PeerSharing::Disabled,
        }
    }
}

/// Node-to-node version data exchanged during the handshake.
///
/// Upstream `data NodeToNodeVersionData` (`NodeToNode/Version.hs`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeToNodeVersionData {
    /// The network magic — must match between peers.
    pub network_magic: NetworkMagic,
    /// The diffusion mode.
    pub diffusion_mode: DiffusionMode,
    /// The peer-sharing setting.
    pub peer_sharing: PeerSharing,
    /// Whether this is a version query.
    pub query: bool,
}

impl NodeToNodeVersionData {
    /// Negotiate version data with a remote peer.
    ///
    /// Mirror of upstream `instance Acceptable NodeToNodeVersionData`:
    /// the network magic must match; the accepted diffusion mode is
    /// the more restrictive (`min`) of the two; peer sharing is agreed
    /// only when the accepted mode is `InitiatorAndResponder` and both
    /// peers enabled it; `query` is the OR of the two. A network-magic
    /// mismatch is a refusal.
    pub fn accept(&self, remote: &NodeToNodeVersionData) -> Result<NodeToNodeVersionData, String> {
        if self.network_magic != remote.network_magic {
            return Err(format!(
                "version data mismatch: network magic {} /= {}",
                self.network_magic.0, remote.network_magic.0
            ));
        }
        let diffusion_mode = self.diffusion_mode.min(remote.diffusion_mode);
        let peer_sharing = match diffusion_mode {
            DiffusionMode::InitiatorAndResponder => self.peer_sharing.combine(remote.peer_sharing),
            DiffusionMode::InitiatorOnly => PeerSharing::Disabled,
        };
        Ok(NodeToNodeVersionData {
            network_magic: self.network_magic,
            diffusion_mode,
            peer_sharing,
            query: self.query || remote.query,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_int_and_from_int_round_trip() {
        for v in NodeToNodeVersion::ALL {
            assert_eq!(NodeToNodeVersion::from_int(v.to_int()), Some(v));
        }
        assert_eq!(NodeToNodeVersion::V1.to_int(), 1);
        assert_eq!(NodeToNodeVersion::V2.to_int(), 2);
        assert_eq!(NodeToNodeVersion::from_int(3), None);
    }

    #[test]
    fn all_is_ordered_low_to_high() {
        assert_eq!(
            NodeToNodeVersion::ALL,
            [NodeToNodeVersion::V1, NodeToNodeVersion::V2]
        );
        assert!(NodeToNodeVersion::V1 < NodeToNodeVersion::V2);
    }

    #[test]
    fn codec_round_trips() {
        for v in NodeToNodeVersion::ALL {
            let mut enc = Encoder::new();
            v.encode(&mut enc);
            let encoded = enc.into_bytes();
            let mut dec = Decoder::new(&encoded);
            assert_eq!(NodeToNodeVersion::decode(&mut dec).expect("decodes"), v);
        }
    }

    #[test]
    fn decode_rejects_unknown_tag() {
        let mut enc = Encoder::new();
        enc.unsigned(99);
        let encoded = enc.into_bytes();
        let mut dec = Decoder::new(&encoded);
        let err = NodeToNodeVersion::decode(&mut dec).expect_err("rejects");
        assert!(
            matches!(&err, LedgerError::CborDecodeError(msg) if msg.contains("unknown tag: 99")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn to_json_is_the_integer_tag() {
        assert_eq!(NodeToNodeVersion::V1.to_json(), serde_json::json!(1));
        assert_eq!(NodeToNodeVersion::V2.to_json(), serde_json::json!(2));
    }

    fn version_data(
        magic: u32,
        mode: DiffusionMode,
        sharing: PeerSharing,
        query: bool,
    ) -> NodeToNodeVersionData {
        NodeToNodeVersionData {
            network_magic: NetworkMagic(magic),
            diffusion_mode: mode,
            peer_sharing: sharing,
            query,
        }
    }

    #[test]
    fn accept_rejects_a_network_magic_mismatch() {
        let local = version_data(
            764,
            DiffusionMode::InitiatorAndResponder,
            PeerSharing::Enabled,
            false,
        );
        let remote = version_data(
            999,
            DiffusionMode::InitiatorAndResponder,
            PeerSharing::Enabled,
            false,
        );
        let err = local.accept(&remote).expect_err("magic mismatch refuses");
        assert!(err.contains("network magic"), "got: {err}");
    }

    #[test]
    fn accept_picks_the_more_restrictive_diffusion_mode() {
        let local = version_data(
            7,
            DiffusionMode::InitiatorAndResponder,
            PeerSharing::Enabled,
            false,
        );
        let remote = version_data(7, DiffusionMode::InitiatorOnly, PeerSharing::Enabled, false);
        let agreed = local.accept(&remote).expect("magic matches");
        // The more restrictive InitiatorOnly wins.
        assert_eq!(agreed.diffusion_mode, DiffusionMode::InitiatorOnly);
        // Peer sharing is disabled outside InitiatorAndResponder.
        assert_eq!(agreed.peer_sharing, PeerSharing::Disabled);
    }

    #[test]
    fn accept_agrees_peer_sharing_only_when_both_enable_it() {
        let mk = |sharing| version_data(7, DiffusionMode::InitiatorAndResponder, sharing, false);
        assert_eq!(
            mk(PeerSharing::Enabled)
                .accept(&mk(PeerSharing::Enabled))
                .unwrap()
                .peer_sharing,
            PeerSharing::Enabled
        );
        assert_eq!(
            mk(PeerSharing::Enabled)
                .accept(&mk(PeerSharing::Disabled))
                .unwrap()
                .peer_sharing,
            PeerSharing::Disabled
        );
    }

    #[test]
    fn accept_ors_the_query_flag() {
        let mk = |query| {
            version_data(
                7,
                DiffusionMode::InitiatorOnly,
                PeerSharing::Disabled,
                query,
            )
        };
        assert!(mk(false).accept(&mk(true)).unwrap().query);
        assert!(!mk(false).accept(&mk(false)).unwrap().query);
    }
}
