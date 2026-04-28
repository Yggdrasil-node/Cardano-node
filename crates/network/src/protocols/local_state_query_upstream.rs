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

/// Era-specific inner query under [`HardForkBlockQuery::QueryIfCurrent`].
///
/// Each Cardano era exposes its own `BlockQuery era` sum type; the
/// HFC layer wraps this in `[era_index, era_specific_query]` per
/// upstream `Cardano.Consensus.HardFork.Combinator.Ledger.Query`.
///
/// This enum recognises the era_index plus a small, frequently-used
/// subset of era-specific query tags shared across the Shelley
/// family (Shelley/Allegra/Mary/Alonzo/Babbage/Conway).  Other tags
/// remain opaque via [`Self::Unknown`].
///
/// Reference: tag values from
/// `Cardano.Ledger.Shelley.LedgerStateQuery` and successor era
/// modules.  Tags are stable across the Shelley family — newer
/// eras add tags but don't renumber existing ones.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EraSpecificQuery {
    /// `[1]` — `GetEpochNo`.  Returns the current epoch number
    /// (CBOR uint).  Used by `cardano-cli query slot-number /
    /// utxo --epoch`.
    GetEpochNo,
    /// `[3]` — `GetCurrentPParams`.  Returns the active protocol
    /// parameters in the era's native PP shape (a 17-element CBOR
    /// list for Shelley).  Used by every wallet, tx-builder, and
    /// `cardano-cli query protocol-parameters` invocation.
    GetCurrentPParams,
    /// `[6, addresses]` — `GetUTxOByAddress`.  Returns the UTxO
    /// entries for the supplied set of addresses.  Used by
    /// `cardano-cli query utxo --address`.  Carries the raw
    /// CBOR-encoded address-set (a CBOR set/array of address
    /// bytestrings) so the dispatcher can filter without
    /// re-decoding.
    GetUTxOByAddress { address_set_cbor: Vec<u8> },
    /// `[7]` — `GetWholeUTxO`.  Returns the entire UTxO map.
    /// Used by `cardano-cli query utxo --whole-utxo`.
    GetWholeUTxO,
    /// `[15, txin_set]` — `GetUTxOByTxIn`.  Returns the UTxO
    /// entries for the supplied set of TxIns.  Used by
    /// `cardano-cli query utxo --tx-in`.  Captured wire tag 15
    /// from the 2026-04-28 cardano-cli rehearsal.
    GetUTxOByTxIn { txin_set_cbor: Vec<u8> },
    /// Any era-specific query whose tag this codec doesn't yet
    /// recognise.  Carries the raw inner CBOR so the dispatcher can
    /// fall through to `null_response()` without losing the bytes.
    Unknown { tag: u64, raw_inner: Vec<u8> },
}

/// Decode the `[era_index, era_specific_query]` inner payload of a
/// [`HardForkBlockQuery::QueryIfCurrent`].  Returns the era_index
/// (0=Byron, 1=Shelley, 2=Allegra, 3=Mary, 4=Alonzo, 5=Babbage,
/// 6=Conway) plus the recognised [`EraSpecificQuery`] variant.
///
/// Reference: `Cardano.Consensus.HardFork.Combinator.Ledger.Query`
/// — `decodeQueryIfCurrent`.
pub fn decode_query_if_current(inner_cbor: &[u8]) -> Result<(u32, EraSpecificQuery), LedgerError> {
    let mut dec = Decoder::new(inner_cbor);
    let outer_len = dec.array()?;
    if outer_len != 2 {
        return Err(LedgerError::CborDecodeError(format!(
            "QueryIfCurrent inner must be a 2-element list \
             [era_index, era_query]; got len={outer_len}"
        )));
    }
    let era_index = dec.unsigned()? as u32;
    // Era-specific query: a singleton (`[tag]`) for tag-only queries
    // like GetCurrentPParams, or a multi-element list for queries
    // with parameters.  We capture the whole sub-list as raw bytes
    // and inspect the leading tag to classify.
    let q_start = dec.position();
    let q_len = dec.array()?;
    let q_tag = dec.unsigned()?;
    let q_end_after_tag = dec.position();
    // Skip remaining elements (if any) so the slice is the full
    // era-specific query CBOR.
    for _ in 1..q_len {
        dec.skip()?;
    }
    let q_end = dec.position();
    let raw_inner = inner_cbor[q_start..q_end].to_vec();
    let kind = match (q_len, q_tag) {
        (1, 1) => EraSpecificQuery::GetEpochNo,
        (1, 3) => EraSpecificQuery::GetCurrentPParams,
        (1, 7) => EraSpecificQuery::GetWholeUTxO,
        (2, 6) => {
            // `[6, address_set_cbor]` — captured the address-set
            // payload between `q_end_after_tag` and `q_end`.
            EraSpecificQuery::GetUTxOByAddress {
                address_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
            }
        }
        (2, 15) => EraSpecificQuery::GetUTxOByTxIn {
            txin_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        _ => EraSpecificQuery::Unknown {
            tag: q_tag,
            raw_inner,
        },
    };
    Ok((era_index, kind))
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

/// Per-network era-history selector.  Distinguishes the live
/// Cardano networks whose vendored `shelley-genesis.json` shapes
/// drive the [`encode_interpreter_for_network`] /
/// [`encode_system_start_for_network`] outputs.  Per-network
/// constants come from
/// [`node/configuration/<network>/shelley-genesis.json`](../../../../node/configuration/).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkKind {
    /// Preprod: `epochLength=432_000` (5-day epochs),
    /// Byron→Shelley at slot 86_400 / epoch 4, system start
    /// 2022-06-01.
    Preprod,
    /// Preview: `epochLength=86_400` (1-day epochs), every hard
    /// fork at epoch 0 (no Byron blocks), system start
    /// 2022-10-25.
    Preview,
    /// Mainnet: `epochLength=432_000`, Byron→Shelley at slot
    /// 4_492_800 / epoch 208, system start 2017-09-23.
    Mainnet,
}

/// Encode the `Interpreter` (era-history summary) tailored to the
/// supplied [`NetworkKind`].  cardano-cli's `query tip` walks the
/// summary list to convert the queried slot to `(epoch,
/// slotInEpoch, slotsToEpochEnd)`; the wrong shape leads to either
/// nonsense values or silent fall-through to genesis-shape display.
pub fn encode_interpreter_for_network(network: NetworkKind) -> Vec<u8> {
    match network {
        NetworkKind::Preprod => encode_interpreter_preprod(),
        NetworkKind::Preview => encode_interpreter_preview(),
        NetworkKind::Mainnet => encode_interpreter_mainnet(),
    }
}

/// Encode `SystemStart` (genesis wall-clock anchor) tailored to
/// the supplied [`NetworkKind`].  cardano-cli's `query tip` uses
/// it together with the `Interpreter` and the queried slot to
/// compute the `syncProgress` percentage.
pub fn encode_system_start_for_network(network: NetworkKind) -> Vec<u8> {
    match network {
        // Preprod: 2022-06-01 = year 2022, day-of-year 152.
        NetworkKind::Preprod => encode_system_start(2022, 152, 0),
        // Preview: 2022-10-25 = year 2022, day-of-year 298.
        NetworkKind::Preview => encode_system_start(2022, 298, 0),
        // Mainnet: 2017-09-23 = year 2017, day-of-year 266.
        NetworkKind::Mainnet => encode_system_start(2017, 266, 0),
    }
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
    const SHELLEY_END_PICOS: u64 =
        BYRON_END_PICOS + ((SHELLEY_END_SLOT - BYRON_END_SLOT) * 1_000_000_000_000_u64);
    // Shelley epochSize = 432_000 slots (5 days × 86_400 s/day).
    const SHELLEY_END_EPOCH: u64 = BYRON_END_EPOCH + (SHELLEY_END_SLOT - BYRON_END_SLOT) / 432_000;

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

/// Encode the preview `Interpreter`.
///
/// Preview's `config.json` sets every `Test*HardForkAtEpoch=0`,
/// meaning all hard forks occurred at epoch 0 and no Byron blocks
/// were ever produced.  The on-disk
/// [`shelley-genesis.json`](../../../../node/configuration/preview/shelley-genesis.json)
/// pins `epochLength=86_400` (1-day epochs at 1s/slot).
///
/// Emits a single open-ended Shelley-shape summary anchored at slot
/// 0 with synthetic far-future end at slot 10_000_000 (well past
/// the current preview tip — 1-day epochs over ~3.6 years gives
/// ~314M slots; the synthetic end caps slot↔epoch math at 10M and
/// is documented as a Phase-3 follow-up to extend coverage).
fn encode_interpreter_preview() -> Vec<u8> {
    const EPOCH_LENGTH: u64 = 86_400;
    const SHELLEY_END_SLOT: u64 = 10_000_000;
    const SHELLEY_END_PICOS: u64 = SHELLEY_END_SLOT * 1_000_000_000_000_u64;
    const SHELLEY_END_EPOCH: u64 = SHELLEY_END_SLOT / EPOCH_LENGTH;

    let mut enc = Encoder::new();
    enc.raw(&[0x9f]);

    enc.array(3);
    enc.array(3);
    encode_relative_time(&mut enc, 0);
    enc.unsigned(0);
    enc.unsigned(0);
    enc.array(3);
    encode_relative_time(&mut enc, SHELLEY_END_PICOS);
    enc.unsigned(SHELLEY_END_SLOT);
    enc.unsigned(SHELLEY_END_EPOCH);
    enc.array(4);
    enc.unsigned(EPOCH_LENGTH); // 86_400
    enc.unsigned(1_000); // slotLength ms
    enc.array(3);
    enc.unsigned(0);
    enc.unsigned(EPOCH_LENGTH * 3); // safeZone slots ≈ 3k/f
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(EPOCH_LENGTH); // genesisWindow

    enc.raw(&[0xff]);
    enc.into_bytes()
}

/// Encode the mainnet `Interpreter`.
///
/// Mainnet Byron→Shelley transitioned at epoch 208 (slot
/// 4_492_800 = 208 epochs × 21_600 Byron-slots).  Byron uses 20s
/// slots; Shelley onwards uses 1s slots at `epochLength=432_000`
/// (5-day epochs).
///
/// Phase-3 follow-up: emit explicit Allegra/Mary/Alonzo/Babbage/
/// Conway summaries when consensus reports the current era past
/// Shelley.  For now a single open Shelley summary with synthetic
/// far-future end at slot 4_492_800 + 10_000_000 keeps
/// `relativeTime` in u64 range and gives correct slot↔epoch math
/// for any slot in the first ~115 days post-Byron.
fn encode_interpreter_mainnet() -> Vec<u8> {
    const BYRON_END_SLOT: u64 = 4_492_800;
    const BYRON_END_EPOCH: u64 = 208;
    // Byron: 4_492_800 slots × 20s = 89_856_000s = 8.9856e19 ps —
    // exceeds u64.  Cap at u64::MAX-aware encoding: use slot-
    // boundary picoseconds up to u64 max instead.  Real value:
    // 4_492_800 × 20 × 1e12 = 8.9856e19; u64 max = 1.844e19.
    //
    // Workaround: scale relativeTime down to a representable
    // value by treating slotLength as 1s for relativeTime
    // purposes only (cardano-cli uses Bound.slot for slot↔epoch
    // math, not relativeTime).  Set Byron eraEnd relativeTime to
    // BYRON_END_SLOT * 1e12 (=4.4928e18 ps, fits u64).
    const BYRON_END_PICOS: u64 = BYRON_END_SLOT * 1_000_000_000_000_u64;
    const SHELLEY_END_SLOT: u64 = BYRON_END_SLOT + 10_000_000;
    const SHELLEY_END_PICOS: u64 = SHELLEY_END_SLOT * 1_000_000_000_000_u64;
    const SHELLEY_END_EPOCH: u64 = BYRON_END_EPOCH + (SHELLEY_END_SLOT - BYRON_END_SLOT) / 432_000;

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
    enc.unsigned(21_600);
    enc.unsigned(20_000);
    enc.array(3);
    enc.unsigned(0);
    enc.unsigned(4_320);
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(4_320);

    // Shelley summary
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
    enc.unsigned(432_000);
    enc.unsigned(1_000);
    enc.array(3);
    enc.unsigned(0);
    enc.unsigned(129_600);
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(129_600);

    enc.raw(&[0xff]);
    enc.into_bytes()
}

/// Wrap an era-specific `QueryIfCurrent` result in the upstream
/// `Either (MismatchEraInfo xs) r` envelope per
/// `Cardano.Consensus.HardFork.Combinator.Serialisation.Common.encodeEitherMismatch`.
///
/// HFC NodeToClient uses **list-length discrimination** between
/// `Right` and `Left` — there's no leading variant tag:
/// - `Right a` (era matches): `[encoded_a]` — **1-element list**.
/// - `Left mismatch`: `[era1_ns, era2_ns]` — 2-element list of
///   `NS`-encoded era names.
///
/// This helper emits the `Right` (matching) form.  Source:
///
/// ```text
/// (HardForkNodeToClientEnabled{}, Right a) ->
///   mconcat [ Enc.encodeListLen 1, enc a ]
/// ```
pub fn encode_query_if_current_match(result_cbor: &[u8]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1);
    enc.raw(result_cbor);
    enc.into_bytes()
}

/// Encode `MismatchEraInfo` for the `Left` case when the requested
/// era doesn't match the snapshot's active era.
///
/// Per `encodeEitherMismatch`:
///
/// ```text
/// (HardForkNodeToClientEnabled{}, Left (MismatchEraInfo err)) ->
///   mconcat [ Enc.encodeListLen 2
///           , encodeNS (hpure (fn encodeName)) era1
///           , encodeNS (hpure (fn (encodeName . getLedgerEraInfo))) era2
///           ]
/// ```
///
/// `encodeNS` for a non-empty era list emits `[ns_index, payload]`
/// where `ns_index` selects the era and `payload` is the era's
/// `SingleEraInfo`/`LedgerEraInfo` (a text-string era name).
pub fn encode_query_if_current_mismatch(ledger_era_idx: u32, query_era_idx: u32) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2);
    encode_ns_era_name(&mut enc, query_era_idx);
    encode_ns_era_name(&mut enc, ledger_era_idx);
    enc.into_bytes()
}

fn encode_ns_era_name(enc: &mut Encoder, era_idx: u32) {
    // NS-encoded era: `[ns_index, era_name_text]` per
    // `Cardano.Consensus.HardFork.Combinator.Util.SOP.encodeNS`.
    enc.array(2);
    enc.unsigned(era_idx as u64);
    enc.text(era_ordinal_to_upstream_name(era_idx));
}

fn era_ordinal_to_upstream_name(ordinal: u32) -> &'static str {
    match ordinal {
        0 => "Byron",
        1 => "Shelley",
        2 => "Allegra",
        3 => "Mary",
        4 => "Alonzo",
        5 => "Babbage",
        6 => "Conway",
        _ => "Unknown",
    }
}

/// Encode Shelley-era `PParams` in the upstream `GetCurrentPParams`
/// response shape: a 17-element CBOR list (NOT the map-based
/// update-proposal shape).
///
/// Upstream `Cardano.Ledger.Shelley.PParams.encCBOR`:
///
/// ```text
/// encodeListLen 17
///   <> minfeeA <> minfeeB <> maxBBSize <> maxTxSize <> maxBHSize
///   <> keyDeposit <> poolDeposit <> eMax <> nOpt
///   <> a0 <> rho <> tau <> d <> extraEntropy
///   <> protocolVersion <> minUTxOValue <> minPoolCost
/// ```
///
/// Field types:
/// - `a0`: `NonNegativeInterval` = CBOR tag 30 + `[num, den]`.
/// - `rho`/`tau`/`d`: `UnitInterval` = CBOR tag 30 + `[num, den]`.
/// - `extraEntropy`: `Nonce` = `[0]` (Neutral) or `[1, hash]` (Hash).
/// - `protocolVersion`: `[major, minor]`.
///
/// Defaults applied when the snapshot's optional Shelley fields are
/// `None`: `d = 1.0` (fully decentralised), `extraEntropy = Neutral`,
/// `protocolVersion = (2, 0)` (Shelley genesis), `minUTxOValue = 0`.
pub fn encode_shelley_pparams_for_lsq(params: &yggdrasil_ledger::ProtocolParameters) -> Vec<u8> {
    use yggdrasil_ledger::CborEncode;
    let mut enc = Encoder::new();
    enc.array(17);
    enc.unsigned(params.min_fee_a);
    enc.unsigned(params.min_fee_b);
    enc.unsigned(params.max_block_body_size as u64);
    enc.unsigned(params.max_tx_size as u64);
    enc.unsigned(params.max_block_header_size as u64);
    enc.unsigned(params.key_deposit);
    enc.unsigned(params.pool_deposit);
    enc.unsigned(params.e_max);
    enc.unsigned(params.n_opt);
    params.a0.encode_cbor(&mut enc);
    params.rho.encode_cbor(&mut enc);
    params.tau.encode_cbor(&mut enc);
    let d = params.d.unwrap_or(yggdrasil_ledger::types::UnitInterval {
        numerator: 1,
        denominator: 1,
    });
    d.encode_cbor(&mut enc);
    encode_shelley_nonce(&mut enc, params.extra_entropy.as_ref());
    let (pv_major, pv_minor) = params.protocol_version.unwrap_or((2, 0));
    enc.array(2);
    enc.unsigned(pv_major);
    enc.unsigned(pv_minor);
    enc.unsigned(params.min_utxo_value.unwrap_or(0));
    enc.unsigned(params.min_pool_cost);
    enc.into_bytes()
}

/// Encode upstream's `Nonce` per
/// `Cardano.Ledger.BaseTypes.Nonce.encCBOR`:
/// - `NeutralNonce` → `[0]` (1-element list with value 0)
/// - `Nonce h` → `[1, h]` (2-element list)
fn encode_shelley_nonce(enc: &mut Encoder, nonce: Option<&yggdrasil_ledger::types::Nonce>) {
    use yggdrasil_ledger::types::Nonce;
    match nonce {
        Some(Nonce::Hash(h)) => {
            enc.array(2);
            enc.unsigned(1);
            enc.bytes(h);
        }
        _ => {
            enc.array(1);
            enc.unsigned(0);
        }
    }
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

    /// Round 152 — pin the preprod Interpreter Byron prefix
    /// byte-for-byte against upstream `cardano-node 10.7.1`'s
    /// captured wire bytes.  When this regresses, cardano-cli's
    /// `query tip` against yggdrasil silently falls back to
    /// displaying origin (`slot=0/epoch=0/syncProgress=0.00`) and
    /// operator visibility into chain progress is lost.  Capture
    /// source: `/tmp/ygg-runbook/haskell-traffic.bin` (socat -x -v
    /// proxy of `cardano-cli 10.16.0.0 query tip --testnet-magic 1`
    /// against upstream Haskell preprod).
    #[test]
    fn preprod_interpreter_byron_prefix_matches_upstream_capture() {
        let bytes = encode_interpreter_minimal(21_600, 1);
        // Wire shape (37 bytes for Byron summary):
        //   9f                                 — indef-array opener
        //   83                                 — Byron summary [eraStart, eraEnd, params]
        //     83 00 00 00                      — eraStart [relativeTime=0, slot=0, epoch=0]
        //     83 1b 17fb16d83be00000 1a 00015180 04
        //                                      — eraEnd [1.728e18 ps, 86400, 4]
        //     84 195460 194e20 83 00 1910e0 81 00 1910e0
        //                                      — params [21600, 20000, [0,4320,[0]], 4320]
        let expected_byron_prefix: [u8; 39] = [
            0x9f, // indef-array opener
            0x83, // Byron summary header
            0x83, 0x00, 0x00, 0x00, // eraStart [0,0,0]
            0x83, // eraEnd opener
            0x1b, 0x17, 0xfb, 0x16, 0xd8, 0x3b, 0xe0, 0x00,
            0x00, // relativeTime u64 (NOT bignum)
            0x1a, 0x00, 0x01, 0x51, 0x80, // slot 86400
            0x04, // epoch 4
            0x84, // eraParams opener (4 fields)
            0x19, 0x54, 0x60, // epochSize=21600
            0x19, 0x4e, 0x20, // slotLength=20000ms
            0x83, 0x00, 0x19, 0x10, 0xe0, 0x81, 0x00, // safeZone=[0,4320,[0]]
            0x19, 0x10, 0xe0, // genesisWindow=4320
        ];
        assert!(
            bytes.starts_with(&expected_byron_prefix),
            "Byron summary prefix must match upstream capture verbatim — \
             relativeTime is CBOR uint (0x1b prefix), NOT bignum (0xc2 0x48 …); \
             when this drifts, cardano-cli silently falls back to origin tip",
        );
    }

    /// Round 152 — pin the Shelley summary's `epochSize=432000` and
    /// `slotLength=1000ms` against the socat capture.  Earlier
    /// drafts used Shelley `epochSize=21600` (Byron-shape) which
    /// caused cardano-cli to compute the wrong epoch boundaries
    /// (and ultimately fall back to origin display because the
    /// Shelley summary failed downstream validation).
    #[test]
    fn preprod_interpreter_shelley_uses_captured_epoch_size_and_genesis_window() {
        let bytes = encode_interpreter_minimal(21_600, 1);
        // Shelley summary's params block: locate by walking past
        // the Byron summary (38 bytes) then past the Shelley
        // start+end Bound headers.  The Shelley `eraParams` starts
        // with `84 1a 00069780 1903e8 …` — the `0x69780` (432000)
        // is the load-bearing value.
        let shelley_params_marker = [0x84u8, 0x1a, 0x00, 0x06, 0x97, 0x80, 0x19, 0x03, 0xe8];
        assert!(
            bytes
                .windows(shelley_params_marker.len())
                .any(|w| w == shelley_params_marker),
            "Shelley eraParams must start with `84 1a 00069780 1903e8` \
             (epochSize=432000, slotLength=1000ms) — captured from \
             upstream `cardano-node 10.7.1`; using Byron-shape values \
             (21600/20000) here breaks cardano-cli's slot↔epoch \
             conversion",
        );
        // Shelley genesisWindow=129600 (0x1fa40) and
        // safeZone=[0, 129600, [0]] both reuse the same 4-byte literal.
        let shelley_genesis_window = [0x1au8, 0x00, 0x01, 0xfa, 0x40];
        let occurrences = bytes
            .windows(shelley_genesis_window.len())
            .filter(|w| *w == shelley_genesis_window)
            .count();
        assert!(
            occurrences >= 2,
            "Shelley summary must encode 0x1fa40 (=129600) for both \
             safeZone-slots and genesisWindow",
        );
    }

    /// Round 153 — preview testnet's vendored `shelley-genesis.json`
    /// pins `epochLength=86_400` (1-day epochs at 1s/slot) and
    /// `config.json` sets every `Test*HardForkAtEpoch=0` (no Byron
    /// blocks).  This test pins the resulting wire shape so a future
    /// drift in either constant fails CI rather than silently
    /// regressing operator-visible cardano-cli output.
    #[test]
    fn preview_interpreter_emits_single_shelley_summary_with_1day_epochs() {
        let bytes = encode_interpreter_for_network(NetworkKind::Preview);
        // Indef-array opener
        assert_eq!(bytes[0], 0x9f, "Summary indef-array opener");
        assert_eq!(bytes[1], 0x83, "Single EraSummary header (array len 3)");
        // eraStart [0, 0, 0]
        assert_eq!(
            &bytes[2..6],
            &[0x83, 0x00, 0x00, 0x00],
            "Preview eraStart=[0,0,0]"
        );
        // Critical: Preview eraParams use epochSize=86_400 (NOT
        // 432_000 as preprod), encoded as `1a 00 01 51 80`.
        let expected_preview_params_marker =
            [0x84u8, 0x1a, 0x00, 0x01, 0x51, 0x80, 0x19, 0x03, 0xe8];
        assert!(
            bytes
                .windows(expected_preview_params_marker.len())
                .any(|w| w == expected_preview_params_marker),
            "Preview eraParams must start with `84 1a 00015180 1903e8` \
             (epochSize=86_400, slotLength=1000ms) — preprod's `0x69780` \
             (=432_000) must NOT appear in preview output",
        );
        // Confirm preprod's signature `0x69780` (=432_000) is NOT in
        // the preview output — guards against accidentally falling
        // through to the preprod encoder.
        let preprod_marker = [0x1au8, 0x00, 0x06, 0x97, 0x80];
        assert!(
            !bytes
                .windows(preprod_marker.len())
                .any(|w| w == preprod_marker),
            "Preview must NOT emit preprod's epochSize=432_000",
        );
    }

    /// Round 153 — preview's `systemStart` is 2022-10-25 (day-of-year
    /// 298).  Pin the encoding so a regression in the date constant
    /// fails CI cleanly.
    #[test]
    fn preview_system_start_is_2022_day_298() {
        let bytes = encode_system_start_for_network(NetworkKind::Preview);
        // [year=2022, dayOfYear=298, picosecondsOfDay=0]
        // 2022 = uint16 0x07e6, 298 = uint16 0x012a.
        assert_eq!(bytes, [0x83, 0x19, 0x07, 0xe6, 0x19, 0x01, 0x2a, 0x00]);
    }

    /// Round 153 — preprod `systemStart` baseline pinned alongside
    /// the per-network selector to guard against accidental swap.
    #[test]
    fn preprod_system_start_is_2022_day_152() {
        let bytes = encode_system_start_for_network(NetworkKind::Preprod);
        // 2022 = 0x07e6, 152 = uint8 0x18 0x98.
        assert_eq!(bytes, [0x83, 0x19, 0x07, 0xe6, 0x18, 0x98, 0x00]);
    }

    /// Round 156 — captured upstream `cardano-cli 10.16.0.0 query
    /// protocol-parameters --testnet-magic 1` payload:
    /// `82 03 82 00 82 00 82 01 81 03` =
    /// `MsgQuery [BlockQuery [QueryIfCurrent [era_index=1, [GetCurrentPParams=3]]]]`.
    /// Pin the decoder so a future drift in any layer fails CI cleanly.
    #[test]
    fn decode_real_cardano_cli_get_current_pparams_payload() {
        // The full MsgQuery wraps the UpstreamQuery; extract the
        // UpstreamQuery payload (skip the leading `82 03` MsgQuery
        // wrapper which is the LSQ codec's responsibility).
        let upstream_query_bytes = [0x82, 0x00, 0x82, 0x00, 0x82, 0x01, 0x81, 0x03];
        let q = UpstreamQuery::decode(&upstream_query_bytes).expect("must decode");
        let inner = match q {
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryIfCurrent { inner_cbor }) => {
                inner_cbor
            }
            other => panic!("expected QueryIfCurrent, got {other:?}"),
        };
        let (era_idx, era_query) =
            decode_query_if_current(&inner).expect("inner decode must succeed");
        assert_eq!(era_idx, 1, "era_index must be Shelley=1");
        assert!(matches!(era_query, EraSpecificQuery::GetCurrentPParams));
    }

    /// Round 157 — pin the captured `query utxo --whole-utxo`
    /// payload `82 00 82 00 82 01 81 07` so a future drift in
    /// QueryIfCurrent or `GetWholeUTxO` (era-specific tag 7)
    /// fails CI cleanly.
    #[test]
    fn decode_real_cardano_cli_get_whole_utxo_payload() {
        let bytes = [0x82, 0x00, 0x82, 0x00, 0x82, 0x01, 0x81, 0x07];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        let inner = match q {
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryIfCurrent { inner_cbor }) => {
                inner_cbor
            }
            other => panic!("expected QueryIfCurrent, got {other:?}"),
        };
        let (era_idx, era_query) = decode_query_if_current(&inner).expect("decode");
        assert_eq!(era_idx, 1);
        assert!(matches!(era_query, EraSpecificQuery::GetWholeUTxO));
    }

    /// Round 157 — pin the captured `query utxo --tx-in` payload
    /// shape: `[era_idx=1, [15, txin_set]]`.  Tag **15** (NOT 14) is
    /// the load-bearing fact captured from the 2026-04-28
    /// cardano-cli rehearsal.
    #[test]
    fn decode_real_cardano_cli_get_utxo_by_tx_in_payload() {
        // Inner: `82 01 82 0f 81 82 58 20 <32 bytes txid> 00`.
        let mut inner = vec![0x82, 0x01, 0x82, 0x0f, 0x81, 0x82, 0x58, 0x20];
        inner.extend_from_slice(&[0xa0u8; 32]);
        inner.push(0x00); // index 0
        let (era_idx, era_query) = decode_query_if_current(&inner).expect("decode");
        assert_eq!(era_idx, 1);
        match era_query {
            EraSpecificQuery::GetUTxOByTxIn { txin_set_cbor } => {
                // First byte must be the array length-1 marker (0x81).
                assert_eq!(txin_set_cbor[0], 0x81, "txin_set is array len 1");
            }
            other => panic!("expected GetUTxOByTxIn, got {other:?}"),
        }
    }

    /// Round 157 — `GetUTxOByAddress` is era-specific tag 6.  Pin
    /// the decoder so a future drift in tag assignment fails CI.
    #[test]
    fn decode_get_utxo_by_address_recognises_tag_6() {
        // Inner: `[1, [6, [<addr_bytes>]]]`.
        let mut inner = vec![0x82, 0x01, 0x82, 0x06, 0x81, 0x58, 0x1d];
        inner.extend_from_slice(&[0xab; 29]); // 29-byte addr
        let (era_idx, era_query) = decode_query_if_current(&inner).expect("decode");
        assert_eq!(era_idx, 1);
        assert!(matches!(
            era_query,
            EraSpecificQuery::GetUTxOByAddress { .. }
        ));
    }

    /// Round 156 — encode_query_if_current_match must produce a
    /// **1-element** CBOR list (not 2-element with tag) per upstream
    /// `encodeEitherMismatch`.  This is the load-bearing wire-shape
    /// fact: cardano-cli's decoder uses list-len discrimination
    /// between Right (len=1) and Left (len=2) — there is NO leading
    /// variant tag for Right.
    #[test]
    fn encode_query_if_current_match_is_one_element_list_no_tag() {
        let result_payload = [0x91u8, 0x01]; // sentinel inner result
        let envelope = encode_query_if_current_match(&result_payload);
        // 0x81 = array(1), then the inner result bytes verbatim.
        assert_eq!(envelope, [0x81, 0x91, 0x01]);
        assert_ne!(
            envelope[0], 0x82,
            "must NOT be 2-element list — that's the Left/mismatch shape, \
             not Right/match",
        );
    }

    /// Round 156 — encode_query_if_current_mismatch must produce a
    /// 2-element CBOR list of NS-encoded era names per upstream
    /// `encodeEitherMismatch` `Left` case.  The order matches
    /// upstream: `era1` (the query's requested era) first, then
    /// `era2` (the ledger's actual era).
    #[test]
    fn encode_query_if_current_mismatch_is_two_element_ns_list() {
        // ledger=Shelley(1), query=Babbage(5)
        let bytes = encode_query_if_current_mismatch(1, 5);
        // 0x82 array(2), then `[5, "Babbage"]`, then `[1, "Shelley"]`.
        assert_eq!(bytes[0], 0x82, "outer list len 2");
        assert_eq!(bytes[1], 0x82, "first NS-era is a 2-element list");
        assert_eq!(bytes[2], 0x05, "first NS-era index = 5 (Babbage)");
    }

    /// Round 156 — encode_shelley_pparams_for_lsq emits the upstream
    /// 17-element PParams list with preprod-genesis-shape values.
    #[test]
    fn shelley_pparams_emit_17_element_list_with_preprod_values() {
        use yggdrasil_ledger::ProtocolParameters;
        let params = ProtocolParameters {
            min_fee_a: 44,
            min_fee_b: 155381,
            max_block_body_size: 65536,
            max_tx_size: 16384,
            max_block_header_size: 1100,
            key_deposit: 2_000_000,
            pool_deposit: 500_000_000,
            e_max: 18,
            n_opt: 150,
            min_utxo_value: Some(1_000_000),
            min_pool_cost: 340_000_000,
            protocol_version: Some((2, 0)),
            ..ProtocolParameters::default()
        };
        let bytes = encode_shelley_pparams_for_lsq(&params);
        // 0x91 = array(17).
        assert_eq!(bytes[0], 0x91, "must be 17-element list");
        // First element: minFeeA = 44 = 0x18 0x2c.
        assert_eq!(&bytes[1..3], &[0x18, 0x2c]);
        // Second: minFeeB = 155381 = 0x1a 0x00 0x02 0x5e 0xf5.
        assert_eq!(&bytes[3..8], &[0x1a, 0x00, 0x02, 0x5e, 0xf5]);
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
