//! CSV output writers for the `db-analyser` benchmark + metrics
//! analyses.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/CSV.hs.
//!
//! Direct port of the small CSV-emission helpers used by the
//! `BenchmarkLedgerOps` and `GetBlockApplicationMetrics` analyses.
//! Each row is computed from a value `a` via a parallel list of
//! `(header, a -> column-string)` builder pairs; the helpers
//! intercalate columns with a configurable separator.
//!
//! Mapping summary:
//!
//! | Upstream                                         | Yggdrasil                              |
//! |--------------------------------------------------|----------------------------------------|
//! | `newtype Separator = Separator { unSeparator :: TextBuilder }` | [`Separator`] (newtype around `String`) |
//! | `writeHeaderLine`                                | [`write_header_line`]                  |
//! | `writeLine`                                      | [`write_line`]                         |
//! | `computeAndWriteLinePure`                        | [`compute_and_write_line_pure`]        |
//! | `computeAndWriteLine` (IO actions)               | [`compute_and_write_line_io`]          |
//! | `computeColumnsPure`                             | [`compute_columns_pure`]               |
//! | `computeColumns` (IO actions)                    | [`compute_columns_io`]                 |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`TextBuilder`**: upstream uses the `text-builder` ecosystem
//!   crate for amortized intercalation. The Rust port uses plain
//!   `String` — adequate for the analyzers' output volume (one
//!   row per block; ~hundreds of thousands max). If a future round
//!   needs higher-throughput output, a `bytes::BytesMut` or
//!   `std::fmt::Write` hot path can be added without changing the
//!   public API.

use std::io::Write;

/// Column separator for CSV rows. Mirror of upstream
/// `newtype Separator = Separator { unSeparator :: TextBuilder }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct Separator(pub String);

impl Separator {
    /// Construct from any string-like value.
    pub fn new(separator: impl Into<String>) -> Self {
        Separator(separator.into())
    }

    /// The canonical `,` (comma) separator.
    pub fn comma() -> Self {
        Separator::new(",")
    }

    /// The canonical `\t` (tab) separator.
    pub fn tab() -> Self {
        Separator::new("\t")
    }

    /// Borrow the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S: Into<String>> From<S> for Separator {
    fn from(value: S) -> Self {
        Separator(value.into())
    }
}

/// Write the header row to an arbitrary writer. Mirror of upstream
/// `writeHeaderLine`.
///
/// `headers` is the list of header names; each maps 1:1 with a
/// column in the subsequent data rows.
pub fn write_header_line<W: Write>(
    writer: &mut W,
    separator: &Separator,
    headers: &[&str],
) -> std::io::Result<()> {
    writeln!(writer, "{}", headers.join(separator.as_str()))
}

/// Write a pre-computed data row to an arbitrary writer. Mirror of
/// upstream `writeLine`.
pub fn write_line<W: Write>(
    writer: &mut W,
    separator: &Separator,
    columns: &[String],
) -> std::io::Result<()> {
    writeln!(writer, "{}", columns.join(separator.as_str()))
}

/// Compute each column from `value` via the supplied pure
/// `(header, value -> column-string)` builder list, then write the
/// row. Mirror of upstream `computeAndWriteLinePure`.
///
/// The `header` halves are unused at this entrypoint (they're only
/// used by [`write_header_line`] when emitting the file's first
/// row); they're kept in the builder list so a single
/// `Vec<(header, fn)>` can drive both header-emit and per-row-emit
/// paths in the call site, exactly mirroring upstream's API shape.
pub fn compute_and_write_line_pure<W, A, F>(
    writer: &mut W,
    separator: &Separator,
    builders: &[(&str, F)],
    value: &A,
) -> std::io::Result<()>
where
    W: Write,
    F: Fn(&A) -> String,
{
    let columns = compute_columns_pure(builders, value);
    write_line(writer, separator, &columns)
}

/// Compute each column from `value` via the supplied fallible
/// `(header, value -> Result<column-string>)` builder list, then
/// write the row. Mirror of upstream `computeAndWriteLine` (which
/// runs in `IO`); the Rust port short-circuits on the first
/// fallible-builder failure.
pub fn compute_and_write_line_io<W, A, F, E>(
    writer: &mut W,
    separator: &Separator,
    builders: &[(&str, F)],
    value: &A,
) -> Result<(), CsvWriteError<E>>
where
    W: Write,
    F: Fn(&A) -> Result<String, E>,
    E: std::error::Error,
{
    let columns = compute_columns_io(builders, value).map_err(CsvWriteError::Builder)?;
    write_line(writer, separator, &columns).map_err(CsvWriteError::Io)
}

/// Apply each pure column-builder to `value` and collect the
/// results. Mirror of upstream `computeColumnsPure`.
pub fn compute_columns_pure<A, F>(builders: &[(&str, F)], value: &A) -> Vec<String>
where
    F: Fn(&A) -> String,
{
    builders.iter().map(|(_, f)| f(value)).collect()
}

/// Apply each fallible column-builder to `value` and collect the
/// results, short-circuiting on the first error. Mirror of
/// upstream `computeColumns` (which runs in `IO`).
pub fn compute_columns_io<A, F, E>(builders: &[(&str, F)], value: &A) -> Result<Vec<String>, E>
where
    F: Fn(&A) -> Result<String, E>,
{
    builders.iter().map(|(_, f)| f(value)).collect()
}

/// Errors from the fallible CSV emit path.
#[derive(Debug, thiserror::Error)]
pub enum CsvWriteError<E: std::error::Error> {
    /// A column-builder closure returned `Err`.
    #[error("CSV column-builder failed: {0}")]
    Builder(E),
    /// The underlying I/O writer failed.
    #[error("CSV write failed: {0}")]
    Io(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separator_default_is_empty_string() {
        assert_eq!(Separator::default().as_str(), "");
    }

    #[test]
    fn separator_comma_round_trip() {
        let s = Separator::comma();
        assert_eq!(s.as_str(), ",");
    }

    #[test]
    fn separator_tab_round_trip() {
        let s = Separator::tab();
        assert_eq!(s.as_str(), "\t");
    }

    #[test]
    fn separator_from_str_round_trip() {
        let s: Separator = ";".into();
        assert_eq!(s.as_str(), ";");
    }

    #[test]
    fn write_header_line_emits_csv() {
        let mut buf = Vec::new();
        write_header_line(
            &mut buf,
            &Separator::comma(),
            &["slot", "block", "tx_count"],
        )
        .expect("writes");
        assert_eq!(
            String::from_utf8(buf).expect("utf8"),
            "slot,block,tx_count\n"
        );
    }

    #[test]
    fn write_header_line_with_tab_separator() {
        let mut buf = Vec::new();
        write_header_line(&mut buf, &Separator::tab(), &["a", "b", "c"]).expect("writes");
        assert_eq!(String::from_utf8(buf).expect("utf8"), "a\tb\tc\n");
    }

    #[test]
    fn write_line_emits_data_row() {
        let mut buf = Vec::new();
        write_line(
            &mut buf,
            &Separator::comma(),
            &["100".to_string(), "deadbeef".to_string(), "5".to_string()],
        )
        .expect("writes");
        assert_eq!(String::from_utf8(buf).expect("utf8"), "100,deadbeef,5\n");
    }

    #[test]
    fn compute_columns_pure_applies_each_builder() {
        struct Row {
            slot: u64,
            block: u64,
        }
        let builders: Vec<(&str, fn(&Row) -> String)> = vec![
            ("slot", |r| r.slot.to_string()),
            ("block", |r| r.block.to_string()),
        ];
        let cols = compute_columns_pure(
            &builders,
            &Row {
                slot: 100,
                block: 5,
            },
        );
        assert_eq!(cols, vec!["100".to_string(), "5".to_string()]);
    }

    #[test]
    fn compute_and_write_line_pure_emits_complete_row() {
        struct Row {
            slot: u64,
            block: u64,
            txs: u32,
        }
        let builders: Vec<(&str, fn(&Row) -> String)> = vec![
            ("slot", |r| r.slot.to_string()),
            ("block", |r| r.block.to_string()),
            ("txs", |r| r.txs.to_string()),
        ];
        let mut buf = Vec::new();
        compute_and_write_line_pure(
            &mut buf,
            &Separator::comma(),
            &builders,
            &Row {
                slot: 1234,
                block: 56,
                txs: 7,
            },
        )
        .expect("writes");
        assert_eq!(String::from_utf8(buf).expect("utf8"), "1234,56,7\n");
    }

    #[test]
    fn compute_columns_io_short_circuits_on_first_error() {
        struct Row {
            value: i32,
        }

        #[derive(Debug, thiserror::Error)]
        #[error("synthetic builder failure: {0}")]
        struct BuildErr(String);

        let builders: Vec<(&str, fn(&Row) -> Result<String, BuildErr>)> = vec![
            ("ok", |r| Ok(r.value.to_string())),
            ("fail", |_| Err(BuildErr("nope".to_string()))),
            ("never_called", |r| Ok(r.value.to_string())),
        ];
        let result = compute_columns_io(&builders, &Row { value: 42 });
        assert!(result.is_err());
        // Only the first column was computed; the failing column
        // short-circuited the rest.
        match result {
            Err(BuildErr(msg)) => assert_eq!(msg, "nope"),
            Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn compute_and_write_line_io_propagates_builder_error() {
        struct Row {
            _value: i32,
        }

        #[derive(Debug, thiserror::Error)]
        #[error("err")]
        struct BuildErr;

        let builders: Vec<(&str, fn(&Row) -> Result<String, BuildErr>)> =
            vec![("col", |_| Err(BuildErr))];
        let mut buf = Vec::new();
        let result =
            compute_and_write_line_io(&mut buf, &Separator::comma(), &builders, &Row { _value: 0 });
        assert!(matches!(result, Err(CsvWriteError::Builder(_))));
        // No row was written.
        assert!(buf.is_empty());
    }

    #[test]
    fn write_line_handles_empty_columns_list() {
        let mut buf = Vec::new();
        write_line(&mut buf, &Separator::comma(), &[]).expect("writes");
        assert_eq!(String::from_utf8(buf).expect("utf8"), "\n");
    }

    #[test]
    fn write_header_handles_single_column() {
        let mut buf = Vec::new();
        write_header_line(&mut buf, &Separator::comma(), &["only"]).expect("writes");
        assert_eq!(String::from_utf8(buf).expect("utf8"), "only\n");
    }
}
