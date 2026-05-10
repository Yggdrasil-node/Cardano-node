//! Output-format dispatch + CSV/JSON writer entry points used by the
//! BenchmarkLedgerOps analysis.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/Analysis/BenchmarkLedgerOps/FileWriting.hs.
//!
//! Direct port of the upstream file-writing surface that ties
//! together [`super::slot_data_point::SlotDataPoint`],
//! [`super::metadata::Metadata`], and [`crate::csv`]. Each writer
//! takes an `impl Write` sink (mirroring upstream's `IO.Handle`),
//! plus an [`OutputFormat`] selector, and emits either tab-separated
//! CSV or JSON output matching upstream byte-for-byte.
//!
//! Mapping summary:
//!
//! | Upstream                                   | Yggdrasil                              |
//! |--------------------------------------------|----------------------------------------|
//! | `data OutputFormat = CSV \| JSON`           | [`OutputFormat::Csv`] / [`OutputFormat::Json`] |
//! | `getOutputFormat :: Maybe FilePath -> IO OutputFormat` | [`get_output_format`] (test-friendly + IO variant) |
//! | `csvSeparator`                             | [`csv_separator`]                      |
//! | `writeHeader`                              | [`write_header`]                       |
//! | `writeDataPoint`                           | [`write_data_point`]                   |
//! | `writeMetadata`                            | [`write_metadata`]                     |
//! | `dataPointCsvBuilder`                      | [`data_point_csv_builder`]             |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Data.ByteString.Lazy.hPut + Aeson.encode`**: upstream emits
//!   JSON via `BSL.hPut h (Aeson.encode x)`, which writes a single
//!   JSON value with no trailing newline. The Rust port uses
//!   `serde_json::to_writer` for the same effect — byte-identical
//!   modulo Aeson's vs serde_json's whitespace handling (both emit
//!   compact JSON with no extra whitespace between tokens by default
//!   for the types in scope).
//! - **`takeExtension :: FilePath -> String`**: upstream's
//!   `System.FilePath.Posix.takeExtension` returns `".csv"` /
//!   `".json"` / `""` (with leading dot). The Rust port uses
//!   `Path::extension()` which returns just the extension *without*
//!   the leading dot, so the dispatch matches on `"csv"` / `"json"`.
//!   Both produce byte-equivalent OutputFormat selection.

use std::io::{self, Write};
use std::path::Path;

use crate::csv::{Separator, compute_and_write_line_pure, write_header_line};
use crate::types::LedgerApplicationMode;

use super::metadata::Metadata;
use super::slot_data_point::SlotDataPoint;

/// Output format for the BenchmarkLedgerOps analysis. Mirror of
/// upstream `data OutputFormat = CSV | JSON`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum OutputFormat {
    /// Tab-separated CSV output. Default when no path is supplied.
    #[default]
    Csv,
    /// JSON output, one value per write call (no separator between
    /// successive writes — same as upstream's
    /// `BSL.hPut h (Aeson.encode x)` semantics).
    Json,
}

/// Tab character used as the CSV column separator. Mirror of
/// upstream `csvSeparator :: TextBuilder; csvSeparator = "\t"`.
pub fn csv_separator() -> Separator {
    Separator::tab()
}

/// Decide the [`OutputFormat`] from a path's extension. Mirror of
/// upstream `getOutputFormat :: Maybe FilePath -> IO OutputFormat`,
/// but with a [`Write`]-based stderr sink injected so the function
/// stays testable. See [`get_output_format_io`] for the IO-driven
/// counterpart that writes to `std::io::stderr()` directly (matching
/// upstream's `IO.hPutStr IO.stderr ...` warning path).
///
/// Dispatch:
/// - `Some(path)` with extension `csv` → [`OutputFormat::Csv`]
/// - `Some(path)` with extension `json` → [`OutputFormat::Json`]
/// - `Some(path)` with any other extension → [`OutputFormat::Csv`]
///   plus a warning emitted to `stderr_sink`
/// - `None` → [`OutputFormat::Csv`] (no warning)
pub fn get_output_format<W: Write>(path: Option<&Path>, stderr_sink: &mut W) -> OutputFormat {
    let Some(path) = path else {
        return OutputFormat::Csv;
    };
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("csv") => OutputFormat::Csv,
        Some("json") => OutputFormat::Json,
        Some(other) => {
            // Mirror upstream's exact warning string so any user
            // tooling that grep's stderr for the warning continues
            // to find it. Upstream prints with a leading `'.'` on
            // the extension; we add it here because Rust's
            // `Path::extension()` strips it.
            let _ = write!(
                stderr_sink,
                "Unsupported extension '.{other}'. Defaulting to CSV.",
            );
            OutputFormat::Csv
        }
        None => {
            let _ = write!(stderr_sink, "Unsupported extension ''. Defaulting to CSV.",);
            OutputFormat::Csv
        }
    }
}

/// IO-driven [`get_output_format`] mirroring upstream's
/// `getOutputFormat :: Maybe FilePath -> IO OutputFormat` byte-for-byte
/// (warning goes to `std::io::stderr()`).
pub fn get_output_format_io(path: Option<&Path>) -> OutputFormat {
    get_output_format(path, &mut io::stderr())
}

/// Write the header row for the BenchmarkLedgerOps output. Mirror of
/// upstream `writeHeader`.
///
/// In CSV mode emits a tab-separated row of column names from
/// [`data_point_csv_builder`]; in JSON mode is a no-op (matching
/// upstream's `writeHeader _ JSON = pure ()`).
pub fn write_header<W: Write>(writer: &mut W, format: OutputFormat) -> io::Result<()> {
    match format {
        OutputFormat::Csv => {
            let headers: Vec<&str> = data_point_csv_builder()
                .iter()
                .map(|(name, _)| *name)
                .collect();
            write_header_line(writer, &csv_separator(), &headers)
        }
        OutputFormat::Json => Ok(()),
    }
}

/// Write a single [`SlotDataPoint`] row. Mirror of upstream
/// `writeDataPoint` (which is documented as "not thread safe").
///
/// In CSV mode emits the 15 columns from [`data_point_csv_builder`];
/// in JSON mode emits a single JSON value with no trailing newline
/// (matching upstream's `BSL.hPut h (Aeson.encode slotDataPoint)`).
pub fn write_data_point<W: Write>(
    writer: &mut W,
    format: OutputFormat,
    data_point: &SlotDataPoint,
) -> io::Result<()> {
    match format {
        OutputFormat::Csv => {
            let builders = data_point_csv_builder();
            compute_and_write_line_pure(writer, &csv_separator(), &builders, data_point)
        }
        OutputFormat::Json => serde_json::to_writer(writer, data_point).map_err(io::Error::other),
    }
}

/// Write the [`Metadata`] preamble. Mirror of upstream
/// `writeMetadata`.
///
/// In CSV mode is a no-op (matching upstream's
/// `writeMetadata _ CSV _ = pure ()`); in JSON mode emits a single
/// JSON-encoded `Metadata` value with no trailing newline.
pub fn write_metadata<W: Write>(
    writer: &mut W,
    format: OutputFormat,
    ledger_application_mode: LedgerApplicationMode,
) -> io::Result<()> {
    match format {
        OutputFormat::Csv => Ok(()),
        OutputFormat::Json => {
            let metadata = Metadata::collect(ledger_application_mode);
            serde_json::to_writer(writer, &metadata).map_err(io::Error::other)
        }
    }
}

/// One column-builder entry: `(header, fn(&SlotDataPoint) -> String)`.
/// Mirror of upstream's `(TextBuilder, SlotDataPoint -> TextBuilder)`
/// pair element of `dataPointCsvBuilder`.
pub type DataPointCsvBuilder = (&'static str, fn(&SlotDataPoint) -> String);

/// CSV column-builder list mirroring upstream's
/// `dataPointCsvBuilder :: [(TextBuilder, SlotDataPoint -> TextBuilder)]`.
///
/// Each entry is `(header, fn)`; the header column-list is the same
/// list consumed by [`write_header`]. The 15 fields are emitted in
/// upstream's exact column order so any tooling grading
/// BenchmarkLedgerOps output by column position continues to work.
pub fn data_point_csv_builder() -> Vec<DataPointCsvBuilder> {
    vec![
        ("slot", |dp| dp.slot.0.to_string()),
        ("slotGap", |dp| dp.slot_gap.to_string()),
        ("totalTime", |dp| dp.total_time.to_string()),
        ("mut", |dp| dp.mut_.to_string()),
        ("gc", |dp| dp.gc.to_string()),
        ("majGcCount", |dp| dp.maj_gc_count.to_string()),
        ("minGcCount", |dp| dp.min_gc_count.to_string()),
        ("allocatedBytes", |dp| dp.allocated_bytes.to_string()),
        ("mut_forecast", |dp| dp.mut_forecast.to_string()),
        ("mut_headerTick", |dp| dp.mut_header_tick.to_string()),
        ("mut_headerApply", |dp| dp.mut_header_apply.to_string()),
        ("mut_blockTick", |dp| dp.mut_block_tick.to_string()),
        ("mut_blockApply", |dp| dp.mut_block_apply.to_string()),
        ("blockBytes", |dp| dp.block_byte_size.to_string()),
        ("...era-specific stats", |dp| {
            // Mirror upstream's
            // `Builder.intercalate csvSeparator . unBlockStats . blockStats`.
            dp.block_stats.as_slice().join("\t")
        }),
    ]
}

#[cfg(test)]
#[allow(clippy::type_complexity)]
mod tests {
    use super::*;
    use crate::analysis::benchmark_ledger_ops::slot_data_point::BlockStats;
    use yggdrasil_ledger::SlotNo;

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
    fn output_format_default_is_csv() {
        assert_eq!(OutputFormat::default(), OutputFormat::Csv);
    }

    #[test]
    fn csv_separator_is_tab() {
        assert_eq!(csv_separator().as_str(), "\t");
    }

    #[test]
    fn get_output_format_none_defaults_to_csv() {
        let mut stderr = Vec::new();
        let format = get_output_format(None, &mut stderr);
        assert_eq!(format, OutputFormat::Csv);
        assert!(stderr.is_empty(), "no warning expected for None");
    }

    #[test]
    fn get_output_format_csv_extension() {
        let mut stderr = Vec::new();
        let format = get_output_format(Some(Path::new("out/data.csv")), &mut stderr);
        assert_eq!(format, OutputFormat::Csv);
        assert!(stderr.is_empty());
    }

    #[test]
    fn get_output_format_json_extension() {
        let mut stderr = Vec::new();
        let format = get_output_format(Some(Path::new("out/data.json")), &mut stderr);
        assert_eq!(format, OutputFormat::Json);
        assert!(stderr.is_empty());
    }

    #[test]
    fn get_output_format_unsupported_extension_warns_and_defaults_to_csv() {
        let mut stderr = Vec::new();
        let format = get_output_format(Some(Path::new("out/data.tsv")), &mut stderr);
        assert_eq!(format, OutputFormat::Csv);
        let warning = String::from_utf8(stderr).expect("utf8");
        assert_eq!(warning, "Unsupported extension '.tsv'. Defaulting to CSV.");
    }

    #[test]
    fn get_output_format_no_extension_warns_and_defaults_to_csv() {
        let mut stderr = Vec::new();
        let format = get_output_format(Some(Path::new("out/data")), &mut stderr);
        assert_eq!(format, OutputFormat::Csv);
        let warning = String::from_utf8(stderr).expect("utf8");
        assert_eq!(warning, "Unsupported extension ''. Defaulting to CSV.");
    }

    #[test]
    fn write_header_csv_emits_15_columns_separated_by_tabs() {
        let mut buf = Vec::new();
        write_header(&mut buf, OutputFormat::Csv).expect("writes");
        let line = String::from_utf8(buf).expect("utf8");
        assert!(line.ends_with('\n'));
        let header_line = line.trim_end_matches('\n');
        let columns: Vec<&str> = header_line.split('\t').collect();
        assert_eq!(columns.len(), 15);
        assert_eq!(columns[0], "slot");
        assert_eq!(columns[1], "slotGap");
        assert_eq!(columns[14], "...era-specific stats");
    }

    #[test]
    fn write_header_json_is_a_no_op() {
        let mut buf = Vec::new();
        write_header(&mut buf, OutputFormat::Json).expect("writes");
        assert!(buf.is_empty());
    }

    #[test]
    fn write_data_point_csv_emits_tab_separated_row() {
        let mut buf = Vec::new();
        write_data_point(&mut buf, OutputFormat::Csv, &sample()).expect("writes");
        let line = String::from_utf8(buf).expect("utf8");
        let row = line.trim_end_matches('\n');
        let columns: Vec<&str> = row.split('\t').collect();
        // The first 14 columns are fixed; the last builder expands
        // its era-specific BlockStats list into one column per stats
        // entry (intercalated with the same tab separator). Sample
        // has 2 stats entries, so total columns = 14 + 2 = 16.
        // This matches upstream's exact CSV format for era-specific
        // stats — the trailing column-count is variable.
        assert_eq!(columns.len(), 16);
        assert_eq!(columns[0], "12345"); // slot
        assert_eq!(columns[1], "1"); // slotGap
        assert_eq!(columns[2], "1000000"); // totalTime
        assert_eq!(columns[3], "950000"); // mut
        assert_eq!(columns[4], "50000"); // gc
        assert_eq!(columns[7], "10485760"); // allocatedBytes
        assert_eq!(columns[13], "8192"); // blockBytes
        // Era-specific stats expand into trailing columns.
        assert_eq!(columns[14], "era=Babbage");
        assert_eq!(columns[15], "txs=42");
    }

    #[test]
    fn write_data_point_csv_with_empty_block_stats_emits_14_plus_one_empty_column() {
        let mut dp = sample();
        dp.block_stats = BlockStats::empty();
        let mut buf = Vec::new();
        write_data_point(&mut buf, OutputFormat::Csv, &dp).expect("writes");
        let line = String::from_utf8(buf).expect("utf8");
        let row = line.trim_end_matches('\n');
        let columns: Vec<&str> = row.split('\t').collect();
        // 14 fixed + 1 empty trailing column from the empty stats
        // list (which renders as an empty string).
        assert_eq!(columns.len(), 15);
        assert_eq!(columns[14], "");
    }

    #[test]
    fn write_data_point_json_emits_full_object() {
        let mut buf = Vec::new();
        write_data_point(&mut buf, OutputFormat::Json, &sample()).expect("writes");
        let json = String::from_utf8(buf).expect("utf8");
        // Single JSON object with no trailing newline (matches
        // upstream BSL.hPut behavior).
        assert!(!json.ends_with('\n'));
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
        // Round-trip through serde_json::Value to verify shape.
        let value: serde_json::Value = serde_json::from_str(&json).expect("parses");
        assert_eq!(value["slot"], 12345);
        assert_eq!(value["slotGap"], 1);
        assert_eq!(
            value["blockStats"],
            serde_json::json!(["era=Babbage", "txs=42"])
        );
    }

    #[test]
    fn write_metadata_csv_is_a_no_op() {
        let mut buf = Vec::new();
        write_metadata(
            &mut buf,
            OutputFormat::Csv,
            LedgerApplicationMode::LedgerApply,
        )
        .expect("writes");
        assert!(buf.is_empty());
    }

    #[test]
    fn write_metadata_json_emits_full_object() {
        let mut buf = Vec::new();
        write_metadata(
            &mut buf,
            OutputFormat::Json,
            LedgerApplicationMode::LedgerApply,
        )
        .expect("writes");
        let json = String::from_utf8(buf).expect("utf8");
        assert!(!json.is_empty());
        let value: serde_json::Value = serde_json::from_str(&json).expect("parses");
        // Spot-check: the upstream-typo'd key is preserved, ledger
        // application mode is rendered correctly.
        assert_eq!(value["ledgerApplicationMode"], "full-application");
        assert!(value["gitRevison"].is_string());
    }

    #[test]
    fn data_point_csv_builder_has_15_columns_in_upstream_order() {
        let builders = data_point_csv_builder();
        let names: Vec<&str> = builders.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "slot",
                "slotGap",
                "totalTime",
                "mut",
                "gc",
                "majGcCount",
                "minGcCount",
                "allocatedBytes",
                "mut_forecast",
                "mut_headerTick",
                "mut_headerApply",
                "mut_blockTick",
                "mut_blockApply",
                "blockBytes",
                "...era-specific stats",
            ],
        );
    }

    #[test]
    fn data_point_csv_builder_renders_each_column_correctly() {
        let dp = sample();
        let builders = data_point_csv_builder();
        let cols: Vec<String> = builders.iter().map(|(_, f)| f(&dp)).collect();
        assert_eq!(cols[0], "12345"); // slot
        assert_eq!(cols[3], "950000"); // mut
        assert_eq!(cols[7], "10485760"); // allocatedBytes
        assert_eq!(cols[14], "era=Babbage\ttxs=42"); // joined stats
    }

    #[test]
    fn round_trip_csv_header_then_data_yields_two_lines() {
        let mut buf = Vec::new();
        write_header(&mut buf, OutputFormat::Csv).expect("writes header");
        write_data_point(&mut buf, OutputFormat::Csv, &sample()).expect("writes data");
        let output = String::from_utf8(buf).expect("utf8");
        let lines: Vec<&str> = output.split('\n').filter(|s| !s.is_empty()).collect();
        assert_eq!(lines.len(), 2);
    }
}
