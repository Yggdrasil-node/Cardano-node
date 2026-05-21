//! DMQ node-to-client protocol version.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/dmq-node/dmq-node/src/DMQ/NodeToClient/Version.hs.
//!
//! Ports the full `NodeToClient/Version.hs` surface: `NodeToClientVersion`
//! (the version enum, its `nodeToClientVersionCodec` CBOR-term codec,
//! JSON rendering) and `NodeToClientVersionData` (the handshake version
//! data, `stdVersionDataNTC` / `NodeToClientVersionData::standard`, the
//! `Acceptable` negotiation `accept`, the `nodeToClientCodecCBORTerm`
//! CBOR-term codec `encode_term` / `decode_term`, and the JSON
//! rendering).

use crate::types::NetworkMagic;
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

/// The bit (the 12th) set on every node-to-client version's wire tag
/// to distinguish it from a `NodeToNodeVersion`.
///
/// Mirror of upstream `nodeToClientVersionBit = 12`. Per the upstream
/// comment, this differs from the bit `ouroboros-network` uses.
const NODE_TO_CLIENT_VERSION_BIT: u64 = 12;

/// The DMQ node-to-client protocol version.
///
/// Upstream `data NodeToClientVersion = NodeToClientV_1`
/// (`deriving (Eq, Ord, Enum, Bounded, Show)`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum NodeToClientVersion {
    /// `NodeToClientV_1`.
    V1,
}

impl NodeToClientVersion {
    /// Every version, low to high — upstream's `[minBound .. maxBound]`.
    pub const ALL: [NodeToClientVersion; 1] = [NodeToClientVersion::V1];

    /// The logical integer tag — `NodeToClientV_1` is 1. This is the
    /// tag *before* the distinguishing version bit is set; the wire
    /// form ([`Self::encode`]) ORs in `NODE_TO_CLIENT_VERSION_BIT`.
    pub fn to_int(self) -> u64 {
        match self {
            NodeToClientVersion::V1 => 1,
        }
    }

    /// The version for a logical integer tag, or `None` if unknown.
    pub fn from_int(tag: u64) -> Option<NodeToClientVersion> {
        match tag {
            1 => Some(NodeToClientVersion::V1),
            _ => None,
        }
    }

    /// Encode as a CBOR integer term — the logical tag with
    /// `NODE_TO_CLIENT_VERSION_BIT` set.
    ///
    /// Mirror of upstream `nodeToClientVersionCodec`'s `encodeTerm`
    /// (`TInt . (\`setBit\` nodeToClientVersionBit)`).
    pub fn encode(self, enc: &mut Encoder) {
        enc.unsigned(self.to_int() | (1u64 << NODE_TO_CLIENT_VERSION_BIT));
    }

    /// Decode from a CBOR integer term.
    ///
    /// Mirror of upstream `nodeToClientVersionCodec`'s `decodeTerm`:
    /// the distinguishing bit must be set; it is then cleared and the
    /// remaining logical tag resolved. A missing bit or an unknown
    /// tag is a decode error.
    pub fn decode(dec: &mut Decoder) -> Result<NodeToClientVersion, LedgerError> {
        let raw = dec.unsigned()?;
        let bit = 1u64 << NODE_TO_CLIENT_VERSION_BIT;
        if raw & bit == 0 {
            return Err(LedgerError::CborDecodeError(format!(
                "decode NodeToClientVersion: unknown tag: {raw}"
            )));
        }
        let tag = raw & !bit;
        NodeToClientVersion::from_int(tag).ok_or_else(|| {
            LedgerError::CborDecodeError(format!("decode NodeToClientVersion: unknown tag: {tag}"))
        })
    }

    /// Render as JSON — the bare logical integer tag.
    ///
    /// Mirror of upstream `instance ToJSON NodeToClientVersion`
    /// (`NodeToClientV_1` is `1`).
    pub fn to_json(self) -> serde_json::Value {
        serde_json::json!(self.to_int())
    }
}

/// Node-to-client version data exchanged during the handshake.
///
/// Upstream `data NodeToClientVersionData` (`NodeToClient/Version.hs`)
/// — simpler than its node-to-node sibling: just the network magic
/// and a query flag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeToClientVersionData {
    /// The network magic — must match between peers.
    pub network_magic: NetworkMagic,
    /// Whether this is a version query.
    pub query: bool,
}

impl NodeToClientVersionData {
    /// The standard version data for a network — `query` is `false`.
    ///
    /// Mirror of upstream `stdVersionDataNTC`.
    pub fn standard(network_magic: NetworkMagic) -> NodeToClientVersionData {
        NodeToClientVersionData {
            network_magic,
            query: false,
        }
    }

    /// Negotiate version data with a remote peer.
    ///
    /// Mirror of upstream `instance Acceptable NodeToClientVersionData`:
    /// the network magic must match (a mismatch refuses); `query` is
    /// the OR of the two.
    pub fn accept(
        &self,
        remote: &NodeToClientVersionData,
    ) -> Result<NodeToClientVersionData, String> {
        if self.network_magic != remote.network_magic {
            return Err(format!(
                "version data mismatch: network magic {} /= {}",
                self.network_magic.0, remote.network_magic.0
            ));
        }
        Ok(NodeToClientVersionData {
            network_magic: self.network_magic,
            query: self.query || remote.query,
        })
    }

    /// Encode as a CBOR term — a 2-element array `[networkMagic,
    /// query]`.
    ///
    /// Mirror of upstream `nodeToClientCodecCBORTerm`'s `encodeTerm`.
    pub fn encode_term(&self, enc: &mut Encoder) {
        enc.array(2)
            .unsigned(u64::from(self.network_magic.0))
            .bool(self.query);
    }

    /// Decode a CBOR-term-encoded version data.
    ///
    /// Mirror of upstream `nodeToClientCodecCBORTerm`'s `decodeTerm` —
    /// rejects an out-of-range network magic.
    pub fn decode_term(dec: &mut Decoder) -> Result<NodeToClientVersionData, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let magic = dec.unsigned()?;
        if magic > u64::from(u32::MAX) {
            return Err(LedgerError::CborDecodeError(format!(
                "networkMagic out of bound: {magic}"
            )));
        }
        let query = dec.bool()?;
        Ok(NodeToClientVersionData {
            network_magic: NetworkMagic(magic as u32),
            query,
        })
    }

    /// Render as JSON.
    ///
    /// Mirror of upstream `instance ToJSON NodeToClientVersionData` —
    /// `{ "NetworkMagic": <magic>, "Query": <query> }`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "NetworkMagic": self.network_magic.0, "Query": self.query })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_int_and_from_int_round_trip() {
        for v in NodeToClientVersion::ALL {
            assert_eq!(NodeToClientVersion::from_int(v.to_int()), Some(v));
        }
        assert_eq!(NodeToClientVersion::V1.to_int(), 1);
        assert_eq!(NodeToClientVersion::from_int(2), None);
    }

    #[test]
    fn encode_sets_the_distinguishing_bit() {
        let mut enc = Encoder::new();
        NodeToClientVersion::V1.encode(&mut enc);
        let encoded = enc.into_bytes();
        // Decoding the raw CBOR uint yields the bit-tagged value
        // 1 | (1 << 12) = 4097.
        let mut dec = Decoder::new(&encoded);
        assert_eq!(dec.unsigned().unwrap(), 4097);
    }

    #[test]
    fn codec_round_trips() {
        for v in NodeToClientVersion::ALL {
            let mut enc = Encoder::new();
            v.encode(&mut enc);
            let encoded = enc.into_bytes();
            let mut dec = Decoder::new(&encoded);
            assert_eq!(NodeToClientVersion::decode(&mut dec).expect("decodes"), v);
        }
    }

    #[test]
    fn decode_rejects_tag_without_the_distinguishing_bit() {
        // A bare `1` (the bit-12 distinguisher absent) is rejected.
        let mut enc = Encoder::new();
        enc.unsigned(1);
        let encoded = enc.into_bytes();
        let mut dec = Decoder::new(&encoded);
        let err = NodeToClientVersion::decode(&mut dec).expect_err("rejects");
        assert!(
            matches!(&err, LedgerError::CborDecodeError(msg) if msg.contains("unknown tag")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn to_json_is_the_logical_tag() {
        assert_eq!(NodeToClientVersion::V1.to_json(), serde_json::json!(1));
    }

    #[test]
    fn version_data_standard_has_query_false() {
        let data = NodeToClientVersionData::standard(NetworkMagic(764));
        assert_eq!(data.network_magic, NetworkMagic(764));
        assert!(!data.query);
    }

    #[test]
    fn version_data_accept_matches_magic_and_ors_query() {
        let local = NodeToClientVersionData {
            network_magic: NetworkMagic(7),
            query: false,
        };
        let remote = NodeToClientVersionData {
            network_magic: NetworkMagic(7),
            query: true,
        };
        let agreed = local.accept(&remote).expect("magic matches");
        assert!(agreed.query);
        // A magic mismatch refuses.
        let other = NodeToClientVersionData {
            network_magic: NetworkMagic(9),
            query: false,
        };
        assert!(local.accept(&other).is_err());
    }

    #[test]
    fn version_data_codec_round_trips() {
        for data in [
            NodeToClientVersionData {
                network_magic: NetworkMagic(764),
                query: true,
            },
            NodeToClientVersionData::standard(NetworkMagic(1)),
        ] {
            let mut enc = Encoder::new();
            data.encode_term(&mut enc);
            let encoded = enc.into_bytes();
            assert_eq!(encoded[0], 0x82, "a CBOR 2-element array");
            let mut dec = Decoder::new(&encoded);
            assert_eq!(
                NodeToClientVersionData::decode_term(&mut dec).expect("decodes"),
                data
            );
        }
    }

    #[test]
    fn version_data_to_json_matches_upstream_shape() {
        let json = NodeToClientVersionData {
            network_magic: NetworkMagic(42),
            query: true,
        }
        .to_json();
        assert_eq!(json["NetworkMagic"], serde_json::json!(42));
        assert_eq!(json["Query"], serde_json::json!(true));
    }
}
