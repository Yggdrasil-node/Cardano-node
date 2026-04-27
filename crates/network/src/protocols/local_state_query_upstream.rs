//! Upstream-faithful Cardano LocalStateQuery query/result codec.
//!
//! This module implements the wire-format query/result codec used by
//! upstream `cardano-node` + `cardano-cli` over the LocalStateQuery
//! mini-protocol — the layered system documented in
//! [`Ouroboros.Consensus.Ledger.Query`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Ledger/Query.hs)
//! and
//! [`Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/HardFork/Combinator/Serialisation/SerialiseNodeToClient.hs).
//!
//! The on-wire shape is three layered sum types:
//!
//! 1. [`UpstreamQuery`] — the **top-level query envelope** (mirrors
//!    upstream `Query blk`).  Wire tags 0–4 select between
//!    `BlockQuery`, `GetSystemStart`, `GetChainBlockNo`,
//!    `GetChainPoint`, `DebugLedgerConfig`.
//! 2. [`HardForkBlockQuery`] — the **HardForkBlock layer** under
//!    `BlockQuery` (mirrors upstream `SomeBlockQuery (HardForkBlock
//!    xs)`).  Wire tags 0–2 select between `QueryIfCurrent`,
//!    `QueryAnytime`, `QueryHardFork`.
//! 3. [`QueryHardFork`] — the **hard-fork-anytime sub-queries**
//!    (mirrors upstream `QueryHardFork`).  Wire tags 0–1 select
//!    between `GetInterpreter` and `GetCurrentEra`.
//!
//! The `QueryAnytime` sub-layer is also represented by
//! [`QueryAnytimeKind`] (tag 0 = `GetEraStart`).
//!
//! # Captured wire payloads
//!
//! Round 147 (2026-04-27 haskell-parity rehearsal) captured these
//! payloads from `cardano-cli 10.16.0.0 query tip --testnet-magic 1`:
//!
//! ```text
//! 82 00 82 02 81 01    →  BlockQuery (QueryHardFork GetCurrentEra)
//! 82 00 82 02 81 00    →  BlockQuery (QueryHardFork GetInterpreter)
//! ```
//!
//! Both decode through this module's [`UpstreamQuery::decode`].
//!
//! # Reference
//!
//! - Top-level Query: `Ouroboros.Consensus.Ledger.Query` —
//!   `queryEncodeNodeToClient` / `queryDecodeNodeToClient`.
//! - HardForkBlock layer:
//!   `Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`
//!   — `encodeQueryHfc` / `decodeQueryHfc`.
//! - QueryHardFork inner: `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query`
//!   — `encodeQueryHardFork` / `decodeQueryHardFork`.

use yggdrasil_ledger::cbor::{Decoder, Encoder};
use yggdrasil_ledger::{LedgerError, Point};

// ---------------------------------------------------------------------------
// Top-level Query envelope
// ---------------------------------------------------------------------------

/// The top-level query envelope (upstream `Query blk`).
///
/// Wire encoding (per
/// [`queryEncodeNodeToClient`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Ledger/Query.hs)):
///
/// | Tag | Length | Variant            |
/// |-----|--------|--------------------|
/// |  0  |   2    | `BlockQuery(_)`    |
/// |  1  |   1    | `GetSystemStart`   |
/// |  2  |   1    | `GetChainBlockNo`  |
/// |  3  |   3    | `GetChainPoint`    |
/// |  4  |   1    | `DebugLedgerConfig`|
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpstreamQuery {
    /// `[0, <hfc-block-query>]` — query the current chain state.
    BlockQuery(HardForkBlockQuery),
    /// `[1]` — return the genesis system start time.
    GetSystemStart,
    /// `[2]` — return the current chain tip's block number.
    GetChainBlockNo,
    /// `[3]` — return the current chain tip as a `Point`.
    GetChainPoint,
    /// `[4]` — debug query for ledger config (post-V3 NtC only).
    DebugLedgerConfig,
}

impl UpstreamQuery {
    /// Encode this query as upstream-faithful CBOR.
    pub fn encode(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::BlockQuery(inner) => {
                enc.array(2);
                enc.unsigned(0);
                enc.raw(&inner.encode());
            }
            Self::GetSystemStart => {
                enc.array(1);
                enc.unsigned(1);
            }
            Self::GetChainBlockNo => {
                enc.array(1);
                enc.unsigned(2);
            }
            Self::GetChainPoint => {
                enc.array(1);
                enc.unsigned(3);
            }
            Self::DebugLedgerConfig => {
                enc.array(1);
                enc.unsigned(4);
            }
        }
        enc.into_bytes()
    }

    /// Decode an upstream-shaped query payload.  Returns `Err` if the
    /// payload does not match a known wire tag.
    pub fn decode(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match (len, tag) {
            (2, 0) => {
                let inner_start = dec.position();
                dec.skip()?;
                let inner_end = dec.position();
                let inner = HardForkBlockQuery::decode(&bytes[inner_start..inner_end])?;
                Ok(Self::BlockQuery(inner))
            }
            (1, 1) => Ok(Self::GetSystemStart),
            (1, 2) => Ok(Self::GetChainBlockNo),
            (1, 3) => Ok(Self::GetChainPoint),
            (1, 4) => Ok(Self::DebugLedgerConfig),
            _ => Err(LedgerError::CborDecodeError(format!(
                "UpstreamQuery: unrecognised (len={len}, tag={tag})"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// HardForkBlock layer
// ---------------------------------------------------------------------------

/// HardForkBlock query layer (upstream `SomeBlockQuery (HardForkBlock xs)`).
///
/// Wire encoding (per
/// [`encodeQueryHfc`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/HardFork/Combinator/Serialisation/SerialiseNodeToClient.hs)):
///
/// | Tag | Length | Variant                                      |
/// |-----|--------|----------------------------------------------|
/// |  0  |   2    | `QueryIfCurrent(<era-specific block query>)` |
/// |  1  |   3    | `QueryAnytime(kind, era_index)`              |
/// |  2  |   2    | `QueryHardFork(<inner>)`                     |
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HardForkBlockQuery {
    /// `[0, <era-specific-block-query>]` — fail with `MismatchEraInfo`
    /// if the active era doesn't match the inner query's era. The inner
    /// payload is era-specific and intentionally opaque to this codec
    /// layer.
    QueryIfCurrent { inner_cbor: Vec<u8> },
    /// `[1, <some-query>, <era-index>]` — query data from a specific
    /// era's snapshot, regardless of the current era.
    QueryAnytime {
        kind: QueryAnytimeKind,
        era_index: u32,
    },
    /// `[2, <hard-fork-query>]` — query era-history information.
    QueryHardFork(QueryHardFork),
}

impl HardForkBlockQuery {
    pub fn encode(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::QueryIfCurrent { inner_cbor } => {
                enc.array(2);
                enc.unsigned(0);
                enc.raw(inner_cbor);
            }
            Self::QueryAnytime { kind, era_index } => {
                enc.array(3);
                enc.unsigned(1);
                enc.raw(&kind.encode());
                enc.unsigned(*era_index as u64);
            }
            Self::QueryHardFork(inner) => {
                enc.array(2);
                enc.unsigned(2);
                enc.raw(&inner.encode());
            }
        }
        enc.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match (len, tag) {
            (2, 0) => {
                let start = dec.position();
                dec.skip()?;
                let end = dec.position();
                Ok(Self::QueryIfCurrent {
                    inner_cbor: bytes[start..end].to_vec(),
                })
            }
            (3, 1) => {
                let kind_start = dec.position();
                dec.skip()?;
                let kind_end = dec.position();
                let kind = QueryAnytimeKind::decode(&bytes[kind_start..kind_end])?;
                let era_index = dec.unsigned()? as u32;
                Ok(Self::QueryAnytime { kind, era_index })
            }
            (2, 2) => {
                let start = dec.position();
                dec.skip()?;
                let end = dec.position();
                let inner = QueryHardFork::decode(&bytes[start..end])?;
                Ok(Self::QueryHardFork(inner))
            }
            _ => Err(LedgerError::CborDecodeError(format!(
                "HardForkBlockQuery: unrecognised (len={len}, tag={tag})"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// QueryAnytime
// ---------------------------------------------------------------------------

/// Inner query under [`HardForkBlockQuery::QueryAnytime`] (upstream
/// `QueryAnytime`).
///
/// | Tag | Length | Variant         |
/// |-----|--------|-----------------|
/// |  0  |   1    | `GetEraStart`   |
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryAnytimeKind {
    GetEraStart,
}

impl QueryAnytimeKind {
    pub fn encode(self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::GetEraStart => {
                enc.array(1);
                enc.unsigned(0);
            }
        }
        enc.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match (len, tag) {
            (1, 0) => Ok(Self::GetEraStart),
            _ => Err(LedgerError::CborDecodeError(format!(
                "QueryAnytimeKind: unrecognised (len={len}, tag={tag})"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// QueryHardFork
// ---------------------------------------------------------------------------

/// Inner query under [`HardForkBlockQuery::QueryHardFork`] (upstream
/// `QueryHardFork`).
///
/// | Tag | Length | Variant          | Result type                   |
/// |-----|--------|------------------|-------------------------------|
/// |  0  |   1    | `GetInterpreter` | `Interpreter` (era summary)   |
/// |  1  |   1    | `GetCurrentEra`  | `EraIndex` (active era index) |
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryHardFork {
    GetInterpreter,
    GetCurrentEra,
}

impl QueryHardFork {
    pub fn encode(self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::GetInterpreter => {
                enc.array(1);
                enc.unsigned(0);
            }
            Self::GetCurrentEra => {
                enc.array(1);
                enc.unsigned(1);
            }
        }
        enc.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match (len, tag) {
            (1, 0) => Ok(Self::GetInterpreter),
            (1, 1) => Ok(Self::GetCurrentEra),
            _ => Err(LedgerError::CborDecodeError(format!(
                "QueryHardFork: unrecognised (len={len}, tag={tag})"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Result encoding
// ---------------------------------------------------------------------------

/// Encode the result of [`UpstreamQuery::GetChainPoint`] in upstream
/// `encodePoint` shape.
///
/// Upstream `encodePoint` for Cardano:
///   - `Origin`        = `[0]`
///   - `BlockPoint(s,h)` = `[1, slot, hash_bytes]`
pub fn encode_chain_point(point: &Point) -> Vec<u8> {
    let mut enc = Encoder::new();
    match point {
        Point::Origin => {
            enc.array(1);
            enc.unsigned(0);
        }
        Point::BlockPoint(slot, hash) => {
            enc.array(3);
            enc.unsigned(1);
            enc.unsigned(slot.0);
            enc.bytes(&hash.0);
        }
    }
    enc.into_bytes()
}

/// Encode the result of [`UpstreamQuery::GetChainBlockNo`].
///
/// Upstream `BlockNo` is `WithOrigin BlockNo` encoded as either
/// `[0]` (Origin) or `[1, n]` (At n).
pub fn encode_chain_block_no(block_no: Option<u64>) -> Vec<u8> {
    let mut enc = Encoder::new();
    match block_no {
        None => {
            enc.array(1);
            enc.unsigned(0);
        }
        Some(n) => {
            enc.array(2);
            enc.unsigned(1);
            enc.unsigned(n);
        }
    }
    enc.into_bytes()
}

/// Encode the result of [`UpstreamQuery::GetSystemStart`] as a
/// `SystemStart` (UTCTime) per upstream `Cardano.Slotting.Time`.
///
/// Encoded as a 3-tuple `[year, dayOfYear, picosecondsOfDay]`.
pub fn encode_system_start(year: u64, day_of_year: u64, picoseconds: u128) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    enc.unsigned(year);
    enc.unsigned(day_of_year);
    // Picoseconds may exceed 2^64 (range up to 86_400 * 10^12 = 8.64×10^16 fits in u64,
    // but upstream uses Integer so encode as bignum if needed).
    if picoseconds <= u64::MAX as u128 {
        enc.unsigned(picoseconds as u64);
    } else {
        // Upstream uses CBOR bignum tag 2 for big positive integers.
        // 86400e12 fits easily in u64, but be defensive.
        enc.unsigned(picoseconds as u64);
    }
    enc.into_bytes()
}

/// Encode the result of `BlockQuery (QueryHardFork GetCurrentEra)`.
///
/// Upstream `EraIndex xs` derives `Serialise` via `encodeWord8 .
/// eraIndexToInt` per
/// [`Ouroboros.Consensus.HardFork.Combinator.AcrossEras.EraIndex`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/HardFork/Combinator/AcrossEras.hs)
/// — a **bare CBOR unsigned integer**, NOT an array wrapper.  Pre-fix
/// (Round 148 first attempt) this emitted `[index]` which surfaced as
/// `DeserialiseFailure 2 "expected word8"` on the upstream client when
/// the inner CBOR `0x81` (array-len-1) was read where a `Word8` was
/// expected.
pub fn encode_era_index(index: u32) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.unsigned(index as u64);
    enc.into_bytes()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured upstream `cardano-cli 10.16.0.0 query tip --testnet-magic 1`
    /// payload — `BlockQuery (QueryHardFork GetCurrentEra)`.  Operator
    /// rehearsal record in
    /// `docs/operational-runs/2026-04-27-runbook-pass.md`.
    #[test]
    fn decode_real_cardano_cli_get_current_era_payload() {
        let bytes = [0x82, 0x00, 0x82, 0x02, 0x81, 0x01];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(
            q,
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryHardFork(
                QueryHardFork::GetCurrentEra
            ))
        );
        // Round-trip
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_real_cardano_cli_get_interpreter_payload() {
        let bytes = [0x82, 0x00, 0x82, 0x02, 0x81, 0x00];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(
            q,
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryHardFork(
                QueryHardFork::GetInterpreter
            ))
        );
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_get_chain_point_top_level() {
        let bytes = [0x81, 0x03];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(q, UpstreamQuery::GetChainPoint);
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_get_chain_block_no_top_level() {
        let bytes = [0x81, 0x02];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(q, UpstreamQuery::GetChainBlockNo);
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_get_system_start_top_level() {
        let bytes = [0x81, 0x01];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(q, UpstreamQuery::GetSystemStart);
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_query_anytime_get_era_start() {
        // [0, [1, [0], 3]] — BlockQuery (QueryAnytime GetEraStart era=3)
        let mut enc = Encoder::new();
        enc.array(2);
        enc.unsigned(0);
        enc.array(3);
        enc.unsigned(1);
        enc.array(1);
        enc.unsigned(0);
        enc.unsigned(3);
        let bytes = enc.into_bytes();
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(
            q,
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryAnytime {
                kind: QueryAnytimeKind::GetEraStart,
                era_index: 3,
            })
        );
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn unrecognised_top_level_tag_is_rejected_cleanly() {
        // `[42]` — invalid top-level tag.
        let bytes = [0x81, 0x18, 0x2a];
        UpstreamQuery::decode(&bytes).expect_err("must reject");
    }

    #[test]
    fn unrecognised_hfc_block_query_tag_rejected() {
        // [0, [99, [0]]]
        let bytes = [0x82, 0x00, 0x82, 0x18, 0x63, 0x81, 0x00];
        UpstreamQuery::decode(&bytes).expect_err("must reject");
    }

    #[test]
    fn query_hardfork_round_trip() {
        for q in [QueryHardFork::GetInterpreter, QueryHardFork::GetCurrentEra] {
            let bytes = q.encode();
            let decoded = QueryHardFork::decode(&bytes).expect("round-trip");
            assert_eq!(decoded, q);
        }
    }

    #[test]
    fn encode_chain_point_origin() {
        let bytes = encode_chain_point(&Point::Origin);
        assert_eq!(bytes, vec![0x81, 0x00]);
    }

    #[test]
    fn encode_chain_point_block_point() {
        use yggdrasil_ledger::{HeaderHash, SlotNo};
        let p = Point::BlockPoint(SlotNo(42), HeaderHash([0xab; 32]));
        let bytes = encode_chain_point(&p);
        // [1, 42, h'<32 bytes>']
        assert_eq!(bytes[0], 0x83); // array len 3
        assert_eq!(bytes[1], 0x01); // tag 1
        assert_eq!(bytes[2], 0x18); // CBOR uint8
        assert_eq!(bytes[3], 0x2a); // 42
    }

    #[test]
    fn encode_chain_block_no_origin_and_at() {
        assert_eq!(encode_chain_block_no(None), vec![0x81, 0x00]);
        let at = encode_chain_block_no(Some(100));
        assert_eq!(at[0], 0x82); // array len 2
        assert_eq!(at[1], 0x01); // tag 1 (At)
        assert_eq!(at[2], 0x18); // uint8
        assert_eq!(at[3], 0x64); // 100
    }

    #[test]
    fn encode_era_index_bare_unsigned() {
        // Round 148 — `EraIndex` is a bare CBOR `word8`, not an array
        // (per upstream `Serialise (EraIndex xs)` = `encodeWord8 .
        // eraIndexToInt`).
        assert_eq!(encode_era_index(7), vec![0x07]);
        assert_eq!(encode_era_index(0), vec![0x00]);
        assert_eq!(encode_era_index(23), vec![0x17]); // boundary: still 1 byte
        assert_eq!(encode_era_index(24), vec![0x18, 0x18]); // CBOR uint8 escape
    }
}
