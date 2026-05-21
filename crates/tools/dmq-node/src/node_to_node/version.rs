//! DMQ node-to-node protocol version.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/dmq-node/dmq-node/src/DMQ/NodeToNode/Version.hs.
//!
//! This slice ports `NodeToNodeVersion` — the version enum, its
//! CBOR-term codec (`nodeToNodeVersionCodec`), and its JSON
//! rendering. `NodeToNodeVersionData` plus the `Acceptable` /
//! `Queryable` version-negotiation instances depend on the
//! `ouroboros-network-api` `NetworkMagic` / `DiffusionMode` /
//! `PeerSharing` types and land with the diffusion sub-arc.

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
}
