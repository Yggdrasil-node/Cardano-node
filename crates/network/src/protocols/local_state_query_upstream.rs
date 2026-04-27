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
/// Upstream `encodePoint` per `Cardano.Slotting.Block`:
///   - `Origin`         = `[]`             (empty CBOR array)
///   - `BlockPoint(s,h)` = `[slot, hash]`   (length-2 array)
///
/// 2026-04-27 operator capture confirms: upstream `cardano-node 10.7.1`
/// at NtC V_23 sent `82 04 82 1a 00 09 4e f8 58 20 ec 4a ...` —
/// MsgResult `[4, [610040, h'ec4a...']]` for `GetChainPoint`.  No
/// leading constructor tag; the `[slot, hash]` array IS the Point
/// itself.
pub fn encode_chain_point(point: &Point) -> Vec<u8> {
    let mut enc = Encoder::new();
    match point {
        Point::Origin => {
            enc.array(0);
        }
        Point::BlockPoint(slot, hash) => {
            enc.array(2);
            enc.unsigned(slot.0);
            enc.bytes(&hash.0);
        }
    }
    enc.into_bytes()
}

/// Encode a minimal valid `Interpreter` (era-history summary) result
/// for `BlockQuery (QueryHardFork GetInterpreter)`.
///
/// Upstream `Interpreter xs = Interpreter (Summary xs)` encodes the
/// `Summary` as an indefinite-length CBOR array of `EraSummary`
/// records — non-empty (at least one era).  Each EraSummary is
/// `[eraStart :: Bound, eraEnd :: EraEnd, eraParams :: EraParams]`
/// where:
///
///   - `Bound = [relativeTime :: Word64, slot :: Word64, epoch :: Word64]`
///     (3-element array — `relativeTime` is whole + fractional
///     picoseconds packed as a single bignum).
///   - `EraEnd = EraUnbounded | EraEnd Bound` — represented as a
///     1-tuple.  An unbounded era is just `[Bound{...}]` per the
///     2026-04-27 operator capture.
///   - `EraParams = [epochSize, slotLength, safeZone, genesisWindow]`
///     where `slotLength` is encoded as picoseconds and `safeZone`
///     is `[0]` (StandardSafeZone) or `[1, slots]` (UnsafeIndefiniteSafeZone).
///
/// This encoder emits a SINGLE open-ended era anchored at slot 0 with
/// preprod-shape parameters (epochSize=21600 slots, slotLength=1
/// second, safeZone=129600 slots).  cardano-cli's slot-to-time
/// conversion will be wrong for non-Byron slots, but `query tip` only
/// needs the Interpreter to deserialise — the displayed `slot`/`hash`
/// come from `GetChainPoint` directly.  Phase-3 follow-up: derive
/// the real era summaries from the loaded `ShelleyGenesis`/`AlonzoGenesis`/
/// `ConwayGenesis` hard-fork transition epochs threaded through the
/// `LedgerStateSnapshot`.
pub fn encode_interpreter_minimal(_epoch_size: u64, _slot_length_secs: u64) -> Vec<u8> {
    encode_interpreter_preprod()
}

/// Encode CBOR positive bignum (tag 2).  Used as a fallback for
/// values that exceed u64 range.  Upstream uses plain CBOR uint
/// (major type 0) for `relativeTime` whenever the value fits in
/// u64 (which is true for all realistic preprod/mainnet slots), so
/// this helper is only needed for synthetic far-future bounds we
/// don't currently emit.
#[allow(dead_code)]
fn encode_bignum_u128(enc: &mut Encoder, value: u128) {
    enc.tag(2);
    if value == 0 {
        enc.bytes(&[]);
        return;
    }
    let bytes = value.to_be_bytes();
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(15);
    enc.bytes(&bytes[first_nonzero..]);
}

/// Encode `relativeTime :: NominalDiffTimeMicro` per upstream
/// `Ouroboros.Consensus.HardFork.History.Summary` which serialises
/// it as a plain CBOR uint when it fits in u64.  Captured wire
/// bytes from `cardano-node 10.7.1` at NtC V_23 confirm: Byron
/// eraEnd encoded as `1b 17fb16d83be00000` (CBOR uint8 prefix +
/// 8-byte big-endian = 1.728e18), not `c2 48 17fb16d83be00000`
/// (bignum tag 2 + byte string).
fn encode_relative_time(enc: &mut Encoder, picoseconds: u64) {
    enc.unsigned(picoseconds);
}

/// Encode an upstream-faithful preprod `Interpreter` with two era
/// summaries — Byron (closed) and Shelley (synthetic far-future end)
/// — so cardano-cli's slot-to-epoch conversion produces the right
/// `epoch` / `slotInEpoch` values for any post-genesis preprod slot.
///
/// Upstream emits one summary per transitioned era (Byron, Shelley,
/// Allegra, Mary, Alonzo, Babbage, Conway), with the latest era's
/// `eraEnd` synthetic-far-future and the rest closed at the
/// successor era's start.  Allegra-onwards share Shelley's
/// `epochSize=21600` and `slotLength=1000ms`, so a single Shelley
/// summary spanning all post-Byron slots gives cardano-cli the right
/// arithmetic for `query tip` purposes.  `current_era` reported via
/// `GetCurrentEra` (separate query) is what produces the displayed
/// `era` field — the interpreter only feeds the slot↔time
/// conversion.
///
/// Phase-3 follow-up: derive era boundaries from the loaded
/// `ShelleyGenesis`/`AlonzoGenesis`/`ConwayGenesis` hard-fork
/// transition epochs threaded through `LedgerStateSnapshot`, and emit
/// all 7 summaries when current era is Conway so the per-era
/// `eraStart`/`eraEnd` align with real preprod boundaries.
fn encode_interpreter_preprod() -> Vec<u8> {
    // Preprod Byron→Shelley boundary captured from `cardano-node 10.7.1`
    // socat -x -v at NtC V_23: epoch 4, slot 86_400, relativeTime
    // 1.728e18 picoseconds = 1.728e6 seconds = 20 days
    // (4 epochs × 21_600 Byron-slots × 20_000ms/slot).
    const BYRON_END_SLOT: u64 = 86_400;
    const BYRON_END_EPOCH: u64 = 4;
    const BYRON_END_PICOS: u64 = 0x17fb_16d8_3be0_0000;

    // Shelley→Allegra boundary captured from same upstream socat:
    // epoch 5, slot 0x7e900 = 518_400.  Allegra inherits Shelley's
    // params shape so we don't need to emit Allegra explicitly until
    // a node progresses past slot 518_400 — Phase-3 follow-up.
    //
    // Synthetic far-future Shelley end at slot=2^36 covers all
    // realistic preprod slots (years past current tip) and keeps
    // relativeTime in u64 range:
    //   2^36 slots × 1e12 ps/slot = 6.87e22 — overflows u64.
    // So cap synthetic end at slot=10_000_000 (≈ 116 days post
    // Byron at 1s/slot, well past current preprod test tip):
    //   relativeTime = 1.728e18 + (10_000_000 - 86400) * 1e12
    //                = 1.0099e19 ps  (fits in u64).
    const SHELLEY_END_SLOT: u64 = 10_000_000;
    const SHELLEY_END_PICOS: u64 = BYRON_END_PICOS
        + ((SHELLEY_END_SLOT - BYRON_END_SLOT) * 1_000_000_000_000_u64);
    // Shelley epochSize = 432_000 slots (5 days × 86_400 s/day).
    const SHELLEY_END_EPOCH: u64 =
        BYRON_END_EPOCH + (SHELLEY_END_SLOT - BYRON_END_SLOT) / 432_000;

    let mut enc = Encoder::new();
    enc.raw(&[0x9f]);

    // Byron summary
    enc.array(3);
    enc.array(3);
    encode_relative_time(&mut enc, 0);
    enc.unsigned(0);
    enc.unsigned(0);
    enc.array(3);
    encode_relative_time(&mut enc, BYRON_END_PICOS);
    enc.unsigned(BYRON_END_SLOT);
    enc.unsigned(BYRON_END_EPOCH);
    enc.array(4);
    enc.unsigned(21_600); // epochSize
    enc.unsigned(20_000); // slotLength ms
    enc.array(3); // safeZone
    enc.unsigned(0);
    enc.unsigned(4_320);
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(4_320); // genesisWindow

    // Shelley summary (open era — synthetic far-future end)
    enc.array(3);
    enc.array(3);
    encode_relative_time(&mut enc, BYRON_END_PICOS);
    enc.unsigned(BYRON_END_SLOT);
    enc.unsigned(BYRON_END_EPOCH);
    enc.array(3);
    encode_relative_time(&mut enc, SHELLEY_END_PICOS);
    enc.unsigned(SHELLEY_END_SLOT);
    enc.unsigned(SHELLEY_END_EPOCH);
    enc.array(4);
    enc.unsigned(432_000); // epochSize captured from upstream
    enc.unsigned(1_000); // slotLength ms
    enc.array(3); // safeZone
    enc.unsigned(0);
    enc.unsigned(129_600);
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(129_600); // genesisWindow captured from upstream

    enc.raw(&[0xff]);
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
/// Upstream's Serialise instance for UTCTime is a 3-element CBOR
/// list `[year, dayOfYear, picosecondsOfDay]` per the
/// 2026-04-27 cardano-cli decoder error message
/// `Size mismatch when decoding UTCTime. Expected 3, but found 2.`:
///   - year: integer Gregorian year (e.g. 2022)
///   - dayOfYear: integer day of year `[1, 366]`
///   - picosecondsOfDay: integer in `[0, 86400*10^12)`
pub fn encode_system_start(year: u64, day_of_year: u64, picoseconds_of_day: u64) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    enc.unsigned(year);
    enc.unsigned(day_of_year);
    enc.unsigned(picoseconds_of_day);
    enc.into_bytes()
}

/// Encode the result of `BlockQuery (QueryHardFork GetCurrentEra)`.
///
/// Upstream NtC V_23 emits `EraIndex` as a bare CBOR unsigned per the
/// 2026-04-27 operator capture (`socat -x -v` proxy on
/// `cardano-node 10.7.1`'s NtC socket): `MsgResult` for
/// `BlockQuery (QueryHardFork GetCurrentEra)` is `82 04 02` —
/// `[4, 2]` with `2` (Allegra ordinal) as a bare uint.  Round 149
/// extends yggdrasil to advertise NtC V_17..V_23 so the negotiated
/// version against upstream `cardano-cli 10.16.0.0` is V_23, matching
/// the canonical V_23 wire format.
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

    /// Diagnostic dump of the preprod Interpreter bytes — used to
    /// confirm the wire shape against an upstream `socat -x -v` capture
    /// when cardano-cli silently rejects the interpreter and falls
    /// back to displaying origin tip.  Print-only test.
    #[test]
    fn dump_preprod_interpreter_bytes_for_diagnostic() {
        let bytes = encode_interpreter_minimal(21_600, 1);
        eprintln!("preprod_interpreter_bytes_len={}", bytes.len());
        eprintln!(
            "preprod_interpreter_bytes_hex={}",
            bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join("")
        );
    }

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
    fn encode_chain_point_origin_is_empty_array() {
        let bytes = encode_chain_point(&Point::Origin);
        assert_eq!(bytes, vec![0x80]);
    }

    #[test]
    fn encode_chain_point_block_point_is_slot_hash_pair() {
        use yggdrasil_ledger::{HeaderHash, SlotNo};
        let p = Point::BlockPoint(SlotNo(42), HeaderHash([0xab; 32]));
        let bytes = encode_chain_point(&p);
        // [42, h'<32 bytes>'] — length 2, no constructor tag.
        assert_eq!(bytes[0], 0x82); // array len 2
        assert_eq!(bytes[1], 0x18); // CBOR uint8 escape
        assert_eq!(bytes[2], 0x2a); // 42
        assert_eq!(bytes[3], 0x58); // bytes uint8 length follows
        assert_eq!(bytes[4], 0x20); // length 32
        // Remaining 32 bytes are the hash payload.
    }

    /// Operator capture from `cardano-node 10.7.1` at NtC V_23 — Allegra era,
    /// slot 610040, hash `ec4a816d...12`.  Inner MsgResult after the [4, ...]
    /// wrapper is `82 1a 00 09 4e f8 58 20 ec 4a 81 6d ...` =
    /// `[610040, h'<32-byte hash>']`.
    #[test]
    fn encode_chain_point_matches_real_haskell_capture_block_point() {
        use yggdrasil_ledger::{HeaderHash, SlotNo};
        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(
            &hex::decode("ec4a816d939b1999386ffcda5d0df3d96a535c282c59edefdec20a9cd841cf12")
                .expect("valid hex"),
        );
        let p = Point::BlockPoint(SlotNo(610040), HeaderHash(hash_bytes));
        let bytes = encode_chain_point(&p);
        let expected = hex::decode(
            "821a00094ef85820\
             ec4a816d939b1999386ffcda5d0df3d96a535c282c59edefdec20a9cd841cf12",
        )
        .expect("valid hex");
        assert_eq!(bytes, expected);
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
    fn encode_era_index_bare_unsigned_v23_shape() {
        // NtC V_23 (negotiated against modern upstream cardano-cli)
        // emits EraIndex as bare CBOR uint, per the 2026-04-27
        // socat-proxy capture from `cardano-node 10.7.1`.
        assert_eq!(encode_era_index(7), vec![0x07]);
        assert_eq!(encode_era_index(0), vec![0x00]);
        assert_eq!(encode_era_index(23), vec![0x17]); // boundary: still 1 byte
        assert_eq!(encode_era_index(24), vec![0x18, 0x18]); // CBOR uint8 escape
    }

    /// The exact bytes captured from `cardano-node 10.7.1` at NtC V_23 in
    /// response to `BlockQuery (QueryHardFork GetCurrentEra)` while in
    /// Allegra era — `MsgResult [4, 2]` = `82 04 02`.  `encode_era_index(2)`
    /// must match the inner result `02`.
    #[test]
    fn encode_era_index_matches_real_haskell_capture() {
        assert_eq!(encode_era_index(2), vec![0x02]);
    }
}
