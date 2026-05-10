//! Per-block ledger-op timing data point — fed into the
//! `BenchmarkLedgerOps` analysis CSV/JSON output streams.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/Analysis/BenchmarkLedgerOps/SlotDataPoint.hs.
//!
//! Direct port of the data-record + JSON-instance surface used by
//! upstream's BenchmarkLedgerOps analysis. Each [`SlotDataPoint`]
//! captures wall-clock + GC + allocation timings for the five major
//! ledger operations applied at a single slot:
//!
//! 0. Forecast.
//! 1. Header tick.
//! 2. Header application.
//! 3. Block tick.
//! 4. Block application.
//!
//! Mapping summary:
//!
//! | Upstream                                          | Yggdrasil                          |
//! |---------------------------------------------------|------------------------------------|
//! | `data SlotDataPoint = SlotDataPoint { slot, slotGap, totalTime, mut, gc, ... }` | [`SlotDataPoint`] (15-field struct) |
//! | `newtype BlockStats = BlockStats { unBlockStats :: [TextBuilder] }` | [`BlockStats`] (newtype around `Vec<String>`) |
//! | `instance ToJSON BlockStats`                      | `serde::Serialize` impl on [`BlockStats`] |
//! | `instance ToJSON SlotDataPoint`                   | `serde::Serialize` derive on [`SlotDataPoint`] |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`TextBuilder`**: same carve-out as [`crate::csv`] — replaced
//!   with `String`. The block-stats list is written either to CSV
//!   (intercalated with the file's separator) or to JSON (as a
//!   string-vector); both representations are byte-identical to
//!   upstream after the `TextBuilder → Text → JSON-string` chain.
//! - **`Cardano.Slotting.Slot.SlotNo`**: the upstream record uses
//!   the consensus-layer slot-number type. Yggdrasil reuses
//!   [`yggdrasil_ledger::SlotNo`], the same type already pinned by
//!   [`crate::types::DBAnalyserConfig`].
//! - **`Aeson.genericToEncoding`**: upstream's JSON-encoding
//!   derivation produces field-order-preserving output. Yggdrasil
//!   uses `serde_json` with `#[derive(Serialize)]`, which preserves
//!   declaration order — byte-equivalent for the field set, modulo
//!   numeric formatting of `Int64` (upstream emits decimal integers,
//!   matching serde_json's default).

use serde::Serialize;
use yggdrasil_ledger::SlotNo;

/// Free-form per-block stats list, written either as
/// `TAB`-separated text columns in CSV mode or as a JSON string
/// array in JSON mode.
///
/// Mirror of upstream
/// `newtype BlockStats = BlockStats { unBlockStats :: [TextBuilder] }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Default, Serialize)]
#[serde(transparent)]
pub struct BlockStats {
    /// The underlying string list. Each entry is one stats line
    /// produced by the era-specific [`crate::has_analysis::HasAnalysis::block_stats`]
    /// implementation.
    pub un_block_stats: Vec<String>,
}

impl BlockStats {
    /// Construct an empty `BlockStats`.
    pub fn empty() -> Self {
        BlockStats {
            un_block_stats: Vec::new(),
        }
    }

    /// Construct from a list of stats strings.
    pub fn from_strings<I: IntoIterator<Item = S>, S: Into<String>>(iter: I) -> Self {
        BlockStats {
            un_block_stats: iter.into_iter().map(Into::into).collect(),
        }
    }

    /// Borrow the underlying string list.
    pub fn as_slice(&self) -> &[String] {
        &self.un_block_stats
    }

    /// Number of stats entries.
    pub fn len(&self) -> usize {
        self.un_block_stats.len()
    }

    /// Whether the stats list is empty.
    pub fn is_empty(&self) -> bool {
        self.un_block_stats.is_empty()
    }
}

/// Per-slot timing/allocation data point. Mirror of upstream
/// `data SlotDataPoint = SlotDataPoint { ... }`.
///
/// Fields are ordered identically to upstream's record-syntax
/// declaration so JSON-encoded output preserves field order. It is
/// up to the user of a slot data point to decide which units the
/// data represent (eg milliseconds, nanoseconds, etc).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
pub struct SlotDataPoint {
    /// Slot in which the 5 ledger operations were applied.
    pub slot: SlotNo,
    /// Gap to the previous slot.
    #[serde(rename = "slotGap")]
    pub slot_gap: u64,
    /// Total time spent in the 5 ledger operations at `slot`.
    #[serde(rename = "totalTime")]
    pub total_time: i64,
    /// Time spent by the mutator while performing the 5 ledger
    /// operations at `slot`.
    pub mut_: i64,
    /// Time spent in garbage collection while performing the 5 ledger
    /// operations at `slot`.
    pub gc: i64,
    /// Total number of __major__ garbage collections that took place
    /// while performing the 5 ledger operations at `slot`.
    #[serde(rename = "majGcCount")]
    pub maj_gc_count: u32,
    /// Total number of __minor__ garbage collections that took place
    /// while performing the 5 ledger operations at `slot`.
    #[serde(rename = "minGcCount")]
    pub min_gc_count: u32,
    /// Allocated bytes while performing the 5 ledger operations at
    /// `slot`.
    #[serde(rename = "allocatedBytes")]
    pub allocated_bytes: u64,
    /// Difference of the GC.mutator_elapsed_ns field when computing
    /// the forecast.
    pub mut_forecast: i64,
    /// Difference of the mutator-elapsed time across the header-tick
    /// rule.
    #[serde(rename = "mut_headerTick")]
    pub mut_header_tick: i64,
    /// Difference of the mutator-elapsed time across the
    /// header-application rule.
    #[serde(rename = "mut_headerApply")]
    pub mut_header_apply: i64,
    /// Difference of the mutator-elapsed time across the block-tick
    /// rule.
    #[serde(rename = "mut_blockTick")]
    pub mut_block_tick: i64,
    /// Difference of the mutator-elapsed time across the
    /// block-application rule.
    #[serde(rename = "mut_blockApply")]
    pub mut_block_apply: i64,
    /// Block byte size — feeds the `blockBytes` CSV column.
    #[serde(rename = "blockByteSize")]
    pub block_byte_size: u32,
    /// Free-form information about the block (era-specific stats).
    #[serde(rename = "blockStats")]
    pub block_stats: BlockStats,
}

impl SlotDataPoint {
    /// Construct an empty `SlotDataPoint` at slot 0 with all timings
    /// and allocations set to zero.
    pub fn empty(slot: SlotNo) -> Self {
        SlotDataPoint {
            slot,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SlotDataPoint {
        SlotDataPoint {
            slot: SlotNo(12345),
            slot_gap: 1,
            total_time: 1_000_000,
            mut_: 950_000,
            gc: 50_000,
            maj_gc_count: 1,
            min_gc_count: 4,
            allocated_bytes: 10_485_760,
            mut_forecast: 100,
            mut_header_tick: 200,
            mut_header_apply: 300,
            mut_block_tick: 400,
            mut_block_apply: 8_000,
            block_byte_size: 8192,
            block_stats: BlockStats::from_strings(vec!["era=Babbage", "txs=42"]),
        }
    }

    #[test]
    fn block_stats_default_is_empty() {
        let stats = BlockStats::default();
        assert!(stats.is_empty());
        assert_eq!(stats.len(), 0);
    }

    #[test]
    fn block_stats_empty_helper() {
        assert_eq!(BlockStats::empty(), BlockStats::default());
    }

    #[test]
    fn block_stats_from_iter_round_trip() {
        let stats = BlockStats::from_strings(vec!["a", "b", "c"]);
        assert_eq!(stats.len(), 3);
        assert_eq!(
            stats.as_slice(),
            ["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn block_stats_serializes_as_string_array() {
        let stats = BlockStats::from_strings(vec!["era=Babbage", "txs=42"]);
        let json = serde_json::to_string(&stats).expect("serializes");
        assert_eq!(json, r#"["era=Babbage","txs=42"]"#);
    }

    #[test]
    fn empty_block_stats_serializes_as_empty_array() {
        let json = serde_json::to_string(&BlockStats::empty()).expect("serializes");
        assert_eq!(json, "[]");
    }

    #[test]
    fn slot_data_point_default_zeroes_all_fields() {
        let dp = SlotDataPoint::default();
        assert_eq!(dp.slot, SlotNo(0));
        assert_eq!(dp.slot_gap, 0);
        assert_eq!(dp.total_time, 0);
        assert_eq!(dp.mut_, 0);
        assert_eq!(dp.gc, 0);
        assert_eq!(dp.maj_gc_count, 0);
        assert_eq!(dp.min_gc_count, 0);
        assert_eq!(dp.allocated_bytes, 0);
        assert_eq!(dp.mut_forecast, 0);
        assert_eq!(dp.mut_header_tick, 0);
        assert_eq!(dp.mut_header_apply, 0);
        assert_eq!(dp.mut_block_tick, 0);
        assert_eq!(dp.mut_block_apply, 0);
        assert_eq!(dp.block_byte_size, 0);
        assert!(dp.block_stats.is_empty());
    }

    #[test]
    fn slot_data_point_empty_helper_zeroes_at_slot() {
        let dp = SlotDataPoint::empty(SlotNo(7));
        assert_eq!(dp.slot, SlotNo(7));
        assert_eq!(dp.total_time, 0);
        assert!(dp.block_stats.is_empty());
    }

    #[test]
    fn slot_data_point_round_trip_through_struct() {
        let dp = sample();
        assert_eq!(dp.slot, SlotNo(12345));
        assert_eq!(dp.allocated_bytes, 10_485_760);
        assert_eq!(dp.block_stats.len(), 2);
    }

    #[test]
    fn slot_data_point_serializes_to_json_with_camel_case_renames() {
        let dp = sample();
        let json = serde_json::to_value(&dp).expect("serializes");
        assert_eq!(json["slot"], 12345);
        assert_eq!(json["slotGap"], 1);
        assert_eq!(json["totalTime"], 1_000_000);
        assert_eq!(json["mut_"], 950_000);
        assert_eq!(json["gc"], 50_000);
        assert_eq!(json["majGcCount"], 1);
        assert_eq!(json["minGcCount"], 4);
        assert_eq!(json["allocatedBytes"], 10_485_760_u64);
        assert_eq!(json["mut_forecast"], 100);
        assert_eq!(json["mut_headerTick"], 200);
        assert_eq!(json["mut_headerApply"], 300);
        assert_eq!(json["mut_blockTick"], 400);
        assert_eq!(json["mut_blockApply"], 8_000);
        assert_eq!(json["blockByteSize"], 8192);
        assert_eq!(
            json["blockStats"],
            serde_json::json!(["era=Babbage", "txs=42"])
        );
    }

    #[test]
    fn slot_data_point_negative_timings_serialize_as_signed_integers() {
        let mut dp = SlotDataPoint::empty(SlotNo(0));
        dp.mut_forecast = -42;
        dp.total_time = -1;
        let json = serde_json::to_value(&dp).expect("serializes");
        assert_eq!(json["mut_forecast"], -42);
        assert_eq!(json["totalTime"], -1);
    }
}
