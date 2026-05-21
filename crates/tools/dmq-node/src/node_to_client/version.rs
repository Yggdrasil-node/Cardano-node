//! DMQ node-to-client protocol version.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/dmq-node/dmq-node/src/DMQ/NodeToClient/Version.hs.
//!
//! This slice ports `NodeToClientVersion` — the version enum, its
//! CBOR-term codec (`nodeToClientVersionCodec`), and its JSON
//! rendering. `NodeToClientVersionData`, `stdVersionDataNTC`, and the
//! `Acceptable` / `Queryable` version-negotiation instances depend on
//! the `ouroboros-network-api` `NetworkMagic` type and land with the
//! diffusion sub-arc.

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
}
