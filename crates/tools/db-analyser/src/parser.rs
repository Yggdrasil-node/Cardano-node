//! CLI argument parser for the `db-analyser` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/app/DBAnalyser/Parsers.hs.
//!
//! Direct port of upstream's
//! `parseDBAnalyserConfig :: Parser DBAnalyserConfig` and the
//! per-section helpers (`parseSelectDB`, `parseValidationPolicy`,
//! `parseAnalysis`, `parseLimit`, plus the per-analysis sub-parsers).
//!
//! Mandatory flags:
//!
//! - `--db PATH` — path to the Chain DB.
//!
//! Optional flags:
//!
//! - `--verbose` — boolean switch.
//! - `--analyse-from SLOT_NUMBER` — start analysis at a specific slot.
//!   Default: from origin.
//! - `--db-validation {validate-all-blocks,minimum-block-validation}`.
//! - `--num-blocks-to-process INT` — cap on processed blocks. Default
//!   [`Limit::Unlimited`].
//! - `--config PATH` — node `config.json`, populating [`CardanoBlockArgs`].
//! - `--threshold FLOAT` — Byron PBFT signature threshold (only valid
//!   together with `--config`).
//!
//! LedgerDB backend (mutually exclusive; one required):
//!
//! - `--in-mem` — V2InMem.
//! - `--lsm` — V2LSM.
//!
//! Analysis-name dispatch (mutually exclusive; default
//! [`AnalysisName::OnlyValidation`] when none supplied):
//!
//! - `--show-slot-block-no` → ShowSlotBlockNo
//! - `--count-tx-outputs` → CountTxOutputs
//! - `--show-block-header-size` → ShowBlockHeaderSize
//! - `--show-block-txs-size` → ShowBlockTxsSize
//! - `--show-ebbs` → ShowEBBs
//! - `--store-ledger SLOT [--full-ledger-validation]` → StoreLedgerStateAt
//! - `--count-blocks` → CountBlocks
//! - `--checkThunks N` → CheckNoThunksEvery
//! - `--trace-ledger` → TraceLedgerProcessing
//! - `--repro-mempool-and-forge INT` → ReproMempoolAndForge
//! - `--benchmark-ledger-ops [--out-file PATH] [--full-ledger-validation]`
//!   → BenchmarkLedgerOps
//! - `--get-block-application-metrics N [--out-file PATH]`
//!   → GetBlockApplicationMetrics
//!
//! `parseCardanoArgs` / `CardanoBlockArgs` (genesis-bootstrap arc,
//! slice 2): [`parse_cmd_line`] mirrors upstream
//! `parseCmdLine :: Parser (DBAnalyserConfig, CardanoBlockArgs)` — the
//! `--config` / `--threshold` flags populate a [`CardanoBlockArgs`].
//! Upstream makes `--config` a required `strOption`; yggdrasil keeps it
//! **optional** because `db-analyser` operates on the unified
//! [`yggdrasil_ledger::Block`], whose block-iteration analyses decode
//! without protocol info (the R475-R481 carve-out). The genesis-seeded
//! ledger bootstrap that *does* need it (the 6 ledger-applying analyses)
//! lands in genesis-bootstrap arc slices 3-4.
//!
//! `--help` / `--version` text is byte-equivalent to the upstream
//! `db-analyser` binary; fixtures captured at R335 live at
//! `crates/tools/db-analyser/tests/fixtures/upstream-{help,version}.txt`.

use std::path::PathBuf;

use yggdrasil_ledger::SlotNo;

use crate::has_analysis::CardanoBlockArgs;
use crate::types::{
    AnalysisName, DBAnalyserConfig, LedgerApplicationMode, LedgerDBBackend, Limit, NumberOfBlocks,
    SelectDB, ValidateBlocks, WithOrigin,
};

/// Byte-for-byte mirror of upstream `db-analyser --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `db-analyser --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed [`DBAnalyserConfig`] half of the command line.
///
/// The full upstream `parseCmdLine` result is the
/// `(DBAnalyserConfig, CardanoBlockArgs)` pair — see [`parse_cmd_line`].
/// This alias is the return of the [`parse_args`] convenience that drops
/// the `CardanoBlockArgs` half (every analysis that does not need a
/// genesis-seeded ledger state).
pub type Args = DBAnalyserConfig;

/// Errors from CLI argument parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen.
    #[error("(--help requested)")]
    HelpRequested,
    /// `--version` / `-v` was seen.
    #[error("(--version requested)")]
    VersionRequested,
    /// `--db PATH` was not supplied.
    #[error("missing required flag: --db")]
    MissingDb,
    /// Neither `--in-mem` nor `--lsm` was supplied.
    #[error("missing LedgerDB backend: supply --in-mem or --lsm")]
    MissingLedgerDbBackend,
    /// Both `--in-mem` and `--lsm` were supplied.
    #[error("conflicting LedgerDB backends: --in-mem and --lsm are mutually exclusive")]
    ConflictingLedgerDbBackend,
    /// More than one analysis-name flag was supplied.
    #[error("conflicting analysis-name flags supplied; pick one analysis mode")]
    ConflictingAnalysisName,
    /// `--db-validation` was supplied with an unrecognized value.
    #[error(
        "invalid --db-validation value `{0}': expected validate-all-blocks or minimum-block-validation"
    )]
    InvalidDbValidation(String),
    /// An unknown flag was passed.
    #[error("Invalid option `{0}'")]
    UnknownFlag(String),
    /// A flag requiring a value was passed without one.
    #[error("flag `{0}' requires a value")]
    MissingValue(String),
    /// A flag's value failed to parse.
    #[error("flag `{0}' has invalid value: {1}")]
    InvalidValue(String, String),
    /// `--threshold` was supplied without `--config`. The PBFT signature
    /// threshold is only meaningful as part of a [`CardanoBlockArgs`],
    /// which exists only when `--config` is given.
    #[error("flag `--threshold' requires `--config'")]
    ThresholdWithoutConfig,
}

#[derive(Clone, Debug, Default)]
struct RawArgs {
    db: Option<PathBuf>,
    verbose: bool,
    analyse_from: Option<u64>,
    db_validation: Option<ValidateBlocks>,
    num_blocks_to_process: Option<u64>,
    backend_in_mem: bool,
    backend_lsm: bool,
    out_file: Option<PathBuf>,
    full_ledger_validation: bool,
    // Cardano-block args (upstream `parseCardanoArgs`)
    config: Option<PathBuf>,
    threshold: Option<f64>,
    // Analysis-name flags (mutually exclusive)
    show_slot_block_no: bool,
    count_tx_outputs: bool,
    show_block_header_size: bool,
    show_block_txs_size: bool,
    show_ebbs: bool,
    count_blocks: bool,
    trace_ledger: bool,
    store_ledger: Option<u64>,
    check_thunks: Option<u64>,
    repro_mempool_and_forge: Option<i64>,
    benchmark_ledger_ops: bool,
    get_block_application_metrics: Option<u64>,
}

/// Parse a slice of command-line arguments into the
/// `(DBAnalyserConfig, CardanoBlockArgs)` pair.
///
/// Mirror of upstream
/// `parseCmdLine = (,) <$> parseDBAnalyserConfig <*> parseCardanoArgs`.
/// The [`CardanoBlockArgs`] half is `Some` iff `--config` was supplied
/// (see the module docstring for why yggdrasil keeps `--config`
/// optional where upstream requires it).
pub fn parse_cmd_line<I, S>(
    args: I,
) -> Result<(DBAnalyserConfig, Option<CardanoBlockArgs>), ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut raw = RawArgs::default();
    let mut iter = args.into_iter().peekable();

    while let Some(arg) = iter.next() {
        let arg = arg.as_ref().to_string();
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "--version" => return Err(ParseError::VersionRequested),
            "--db" => {
                let v = take_value(&mut iter, &arg)?;
                raw.db = Some(PathBuf::from(v));
            }
            "--verbose" => raw.verbose = true,
            "--analyse-from" => {
                let v = take_value(&mut iter, &arg)?;
                raw.analyse_from = Some(parse_u64(&arg, &v)?);
            }
            "--db-validation" => {
                let v = take_value(&mut iter, &arg)?;
                raw.db_validation = Some(match v.as_str() {
                    "validate-all-blocks" => ValidateBlocks::ValidateAllBlocks,
                    "minimum-block-validation" => ValidateBlocks::MinimumBlockValidation,
                    other => return Err(ParseError::InvalidDbValidation(other.to_string())),
                });
            }
            "--num-blocks-to-process" => {
                let v = take_value(&mut iter, &arg)?;
                raw.num_blocks_to_process = Some(parse_u64(&arg, &v)?);
            }
            "--in-mem" => raw.backend_in_mem = true,
            "--lsm" => raw.backend_lsm = true,
            "--out-file" => {
                let v = take_value(&mut iter, &arg)?;
                raw.out_file = Some(PathBuf::from(v));
            }
            "--full-ledger-validation" => raw.full_ledger_validation = true,
            "--config" => {
                let v = take_value(&mut iter, &arg)?;
                raw.config = Some(PathBuf::from(v));
            }
            "--threshold" => {
                let v = take_value(&mut iter, &arg)?;
                raw.threshold = Some(parse_f64(&arg, &v)?);
            }
            "--show-slot-block-no" => raw.show_slot_block_no = true,
            "--count-tx-outputs" => raw.count_tx_outputs = true,
            "--show-block-header-size" => raw.show_block_header_size = true,
            "--show-block-txs-size" => raw.show_block_txs_size = true,
            "--show-ebbs" => raw.show_ebbs = true,
            "--count-blocks" => raw.count_blocks = true,
            "--trace-ledger" => raw.trace_ledger = true,
            "--store-ledger" => {
                let v = take_value(&mut iter, &arg)?;
                raw.store_ledger = Some(parse_u64(&arg, &v)?);
            }
            "--checkThunks" => {
                let v = take_value(&mut iter, &arg)?;
                raw.check_thunks = Some(parse_u64(&arg, &v)?);
            }
            "--repro-mempool-and-forge" => {
                let v = take_value(&mut iter, &arg)?;
                raw.repro_mempool_and_forge = Some(parse_i64(&arg, &v)?);
            }
            "--benchmark-ledger-ops" => raw.benchmark_ledger_ops = true,
            "--get-block-application-metrics" => {
                let v = take_value(&mut iter, &arg)?;
                raw.get_block_application_metrics = Some(parse_u64(&arg, &v)?);
            }
            other => return Err(ParseError::UnknownFlag(other.to_string())),
        }
    }

    let cardano = promote_cardano(&raw)?;
    let config = promote(raw)?;
    Ok((config, cardano))
}

/// Parse a slice of command-line arguments into [`DBAnalyserConfig`],
/// dropping the [`CardanoBlockArgs`] half.
///
/// Mirror of upstream `parseDBAnalyserConfig`; a convenience over
/// [`parse_cmd_line`] for the analyses that do not need a genesis-seeded
/// ledger state.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    parse_cmd_line(args).map(|(config, _cardano)| config)
}

/// Build the [`CardanoBlockArgs`] half from the raw flags.
///
/// Mirror of upstream `parseCardanoArgs = CardanoBlockArgs <$>
/// parseConfigFile <*> parsePBftSignatureThreshold`. `--threshold`
/// without `--config` is rejected — the threshold has no
/// [`CardanoBlockArgs`] to belong to.
fn promote_cardano(raw: &RawArgs) -> Result<Option<CardanoBlockArgs>, ParseError> {
    match (&raw.config, raw.threshold) {
        (Some(config_file), threshold) => Ok(Some(CardanoBlockArgs {
            config_file: config_file.clone(),
            threshold,
        })),
        (None, Some(_)) => Err(ParseError::ThresholdWithoutConfig),
        (None, None) => Ok(None),
    }
}

fn promote(mut raw: RawArgs) -> Result<Args, ParseError> {
    let db_dir = raw.db.take().ok_or(ParseError::MissingDb)?;

    let ldb_backend = match (raw.backend_in_mem, raw.backend_lsm) {
        (true, true) => return Err(ParseError::ConflictingLedgerDbBackend),
        (true, false) => LedgerDBBackend::V2InMem,
        (false, true) => LedgerDBBackend::V2LSM,
        (false, false) => return Err(ParseError::MissingLedgerDbBackend),
    };

    let select_db = SelectDB::SelectImmutableDB(match raw.analyse_from {
        Some(slot) => WithOrigin::At(SlotNo(slot)),
        None => WithOrigin::Origin,
    });

    let conf_limit = match raw.num_blocks_to_process {
        Some(n) => Limit::Limit(n),
        None => Limit::Unlimited,
    };

    let analysis = pick_analysis(&raw)?;
    let validation = raw.db_validation;
    let verbose = raw.verbose;

    Ok(DBAnalyserConfig {
        db_dir,
        verbose,
        select_db,
        validation,
        analysis,
        conf_limit,
        ldb_backend,
    })
}

fn pick_analysis(raw: &RawArgs) -> Result<AnalysisName, ParseError> {
    let ledger_mode = if raw.full_ledger_validation {
        LedgerApplicationMode::LedgerApply
    } else {
        LedgerApplicationMode::LedgerReapply
    };

    let mut chosen: Option<AnalysisName> = None;
    let mut set_one = |candidate: AnalysisName| -> Result<(), ParseError> {
        if chosen.is_some() {
            return Err(ParseError::ConflictingAnalysisName);
        }
        chosen = Some(candidate);
        Ok(())
    };

    if raw.show_slot_block_no {
        set_one(AnalysisName::ShowSlotBlockNo)?;
    }
    if raw.count_tx_outputs {
        set_one(AnalysisName::CountTxOutputs)?;
    }
    if raw.show_block_header_size {
        set_one(AnalysisName::ShowBlockHeaderSize)?;
    }
    if raw.show_block_txs_size {
        set_one(AnalysisName::ShowBlockTxsSize)?;
    }
    if raw.show_ebbs {
        set_one(AnalysisName::ShowEBBs)?;
    }
    if raw.count_blocks {
        set_one(AnalysisName::CountBlocks)?;
    }
    if raw.trace_ledger {
        set_one(AnalysisName::TraceLedgerProcessing)?;
    }
    if let Some(slot) = raw.store_ledger {
        set_one(AnalysisName::StoreLedgerStateAt(SlotNo(slot), ledger_mode))?;
    }
    if let Some(n) = raw.check_thunks {
        set_one(AnalysisName::CheckNoThunksEvery(n))?;
    }
    if let Some(n) = raw.repro_mempool_and_forge {
        set_one(AnalysisName::ReproMempoolAndForge(n))?;
    }
    if raw.benchmark_ledger_ops {
        set_one(AnalysisName::BenchmarkLedgerOps(
            raw.out_file.clone(),
            ledger_mode,
        ))?;
    }
    if let Some(n) = raw.get_block_application_metrics {
        set_one(AnalysisName::GetBlockApplicationMetrics(
            NumberOfBlocks(n),
            raw.out_file.clone(),
        ))?;
    }

    // Default when no analysis-name flag was supplied — matches
    // upstream's `pure OnlyValidation` last-resort branch in the
    // `Foldable.asum` chain.
    Ok(chosen.unwrap_or(AnalysisName::OnlyValidation))
}

fn take_value<I, S>(iter: &mut std::iter::Peekable<I>, flag: &str) -> Result<String, ParseError>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    iter.next()
        .map(|v| v.as_ref().to_string())
        .ok_or_else(|| ParseError::MissingValue(flag.to_string()))
}

fn parse_u64(flag: &str, value: &str) -> Result<u64, ParseError> {
    value.parse().map_err(|e: std::num::ParseIntError| {
        ParseError::InvalidValue(flag.to_string(), e.to_string())
    })
}

fn parse_i64(flag: &str, value: &str) -> Result<i64, ParseError> {
    value.parse().map_err(|e: std::num::ParseIntError| {
        ParseError::InvalidValue(flag.to_string(), e.to_string())
    })
}

fn parse_f64(flag: &str, value: &str) -> Result<f64, ParseError> {
    value.parse().map_err(|e: std::num::ParseFloatError| {
        ParseError::InvalidValue(flag.to_string(), e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> Vec<&'static str> {
        vec!["--db", "/var/lib/db", "--in-mem", "--count-blocks"]
    }

    #[test]
    fn detects_help_long() {
        assert_eq!(parse_args(["--help"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_help_short() {
        assert_eq!(parse_args(["-h"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_version() {
        assert_eq!(parse_args(["--version"]), Err(ParseError::VersionRequested));
    }

    #[test]
    fn parses_minimal_count_blocks() {
        let args = parse_args(minimal()).expect("parses");
        assert_eq!(args.db_dir.to_str(), Some("/var/lib/db"));
        assert!(!args.verbose);
        assert_eq!(args.analysis, AnalysisName::CountBlocks);
        assert_eq!(args.ldb_backend, LedgerDBBackend::V2InMem);
        assert!(matches!(args.conf_limit, Limit::Unlimited));
    }

    #[test]
    fn parses_verbose_flag() {
        let args =
            parse_args(["--db", "/db", "--lsm", "--count-blocks", "--verbose"]).expect("parses");
        assert!(args.verbose);
    }

    #[test]
    fn parses_lsm_backend() {
        let args = parse_args(["--db", "/db", "--lsm", "--count-blocks"]).expect("parses");
        assert_eq!(args.ldb_backend, LedgerDBBackend::V2LSM);
    }

    #[test]
    fn parses_analyse_from_slot() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--count-blocks",
            "--analyse-from",
            "5000",
        ])
        .expect("parses");
        match args.select_db {
            SelectDB::SelectImmutableDB(WithOrigin::At(s)) => assert_eq!(s, SlotNo(5000)),
            _ => panic!("expected At slot"),
        }
    }

    #[test]
    fn analyse_from_origin_when_omitted() {
        let args = parse_args(minimal()).expect("parses");
        assert!(matches!(
            args.select_db,
            SelectDB::SelectImmutableDB(WithOrigin::Origin)
        ));
    }

    #[test]
    fn parses_db_validation_validate_all() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--count-blocks",
            "--db-validation",
            "validate-all-blocks",
        ])
        .expect("parses");
        assert_eq!(args.validation, Some(ValidateBlocks::ValidateAllBlocks));
    }

    #[test]
    fn parses_db_validation_minimum() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--count-blocks",
            "--db-validation",
            "minimum-block-validation",
        ])
        .expect("parses");
        assert_eq!(
            args.validation,
            Some(ValidateBlocks::MinimumBlockValidation)
        );
    }

    #[test]
    fn rejects_invalid_db_validation() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--count-blocks",
            "--db-validation",
            "frobnicate",
        ]);
        assert!(matches!(args, Err(ParseError::InvalidDbValidation(_))));
    }

    #[test]
    fn parses_num_blocks_to_process() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--count-blocks",
            "--num-blocks-to-process",
            "10000",
        ])
        .expect("parses");
        assert_eq!(args.conf_limit, Limit::Limit(10_000));
    }

    #[test]
    fn missing_db_rejected() {
        let args = parse_args(["--in-mem", "--count-blocks"]);
        assert_eq!(args, Err(ParseError::MissingDb));
    }

    #[test]
    fn missing_backend_rejected() {
        let args = parse_args(["--db", "/db", "--count-blocks"]);
        assert_eq!(args, Err(ParseError::MissingLedgerDbBackend));
    }

    #[test]
    fn conflicting_backends_rejected() {
        let args = parse_args(["--db", "/db", "--in-mem", "--lsm", "--count-blocks"]);
        assert_eq!(args, Err(ParseError::ConflictingLedgerDbBackend));
    }

    #[test]
    fn no_analysis_flag_defaults_to_only_validation() {
        let args = parse_args(["--db", "/db", "--in-mem"]).expect("parses");
        assert_eq!(args.analysis, AnalysisName::OnlyValidation);
    }

    #[test]
    fn parses_show_slot_block_no() {
        let args = parse_args(["--db", "/db", "--in-mem", "--show-slot-block-no"]).expect("parses");
        assert_eq!(args.analysis, AnalysisName::ShowSlotBlockNo);
    }

    #[test]
    fn parses_count_tx_outputs() {
        let args = parse_args(["--db", "/db", "--in-mem", "--count-tx-outputs"]).expect("parses");
        assert_eq!(args.analysis, AnalysisName::CountTxOutputs);
    }

    #[test]
    fn parses_show_block_header_size() {
        let args =
            parse_args(["--db", "/db", "--in-mem", "--show-block-header-size"]).expect("parses");
        assert_eq!(args.analysis, AnalysisName::ShowBlockHeaderSize);
    }

    #[test]
    fn parses_show_block_txs_size() {
        let args =
            parse_args(["--db", "/db", "--in-mem", "--show-block-txs-size"]).expect("parses");
        assert_eq!(args.analysis, AnalysisName::ShowBlockTxsSize);
    }

    #[test]
    fn parses_show_ebbs() {
        let args = parse_args(["--db", "/db", "--in-mem", "--show-ebbs"]).expect("parses");
        assert_eq!(args.analysis, AnalysisName::ShowEBBs);
    }

    #[test]
    fn parses_trace_ledger() {
        let args = parse_args(["--db", "/db", "--in-mem", "--trace-ledger"]).expect("parses");
        assert_eq!(args.analysis, AnalysisName::TraceLedgerProcessing);
    }

    #[test]
    fn parses_store_ledger_default_reapply() {
        let args =
            parse_args(["--db", "/db", "--in-mem", "--store-ledger", "5000"]).expect("parses");
        assert_eq!(
            args.analysis,
            AnalysisName::StoreLedgerStateAt(SlotNo(5000), LedgerApplicationMode::LedgerReapply)
        );
    }

    #[test]
    fn parses_store_ledger_with_full_validation() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--store-ledger",
            "5000",
            "--full-ledger-validation",
        ])
        .expect("parses");
        assert_eq!(
            args.analysis,
            AnalysisName::StoreLedgerStateAt(SlotNo(5000), LedgerApplicationMode::LedgerApply)
        );
    }

    #[test]
    fn parses_check_thunks() {
        let args =
            parse_args(["--db", "/db", "--in-mem", "--checkThunks", "1000"]).expect("parses");
        assert_eq!(args.analysis, AnalysisName::CheckNoThunksEvery(1000));
    }

    #[test]
    fn parses_repro_mempool_and_forge() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--repro-mempool-and-forge",
            "100",
        ])
        .expect("parses");
        assert_eq!(args.analysis, AnalysisName::ReproMempoolAndForge(100));
    }

    #[test]
    fn parses_benchmark_ledger_ops_no_outfile() {
        let args =
            parse_args(["--db", "/db", "--in-mem", "--benchmark-ledger-ops"]).expect("parses");
        assert_eq!(
            args.analysis,
            AnalysisName::BenchmarkLedgerOps(None, LedgerApplicationMode::LedgerReapply)
        );
    }

    #[test]
    fn parses_benchmark_ledger_ops_with_outfile_and_full_validation() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--benchmark-ledger-ops",
            "--out-file",
            "/tmp/bench.csv",
            "--full-ledger-validation",
        ])
        .expect("parses");
        assert_eq!(
            args.analysis,
            AnalysisName::BenchmarkLedgerOps(
                Some(PathBuf::from("/tmp/bench.csv")),
                LedgerApplicationMode::LedgerApply
            )
        );
    }

    #[test]
    fn parses_get_block_application_metrics() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--get-block-application-metrics",
            "500",
        ])
        .expect("parses");
        assert_eq!(
            args.analysis,
            AnalysisName::GetBlockApplicationMetrics(NumberOfBlocks(500), None)
        );
    }

    #[test]
    fn parses_get_block_application_metrics_with_outfile() {
        let args = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--get-block-application-metrics",
            "500",
            "--out-file",
            "/tmp/m.csv",
        ])
        .expect("parses");
        assert_eq!(
            args.analysis,
            AnalysisName::GetBlockApplicationMetrics(
                NumberOfBlocks(500),
                Some(PathBuf::from("/tmp/m.csv"))
            )
        );
    }

    #[test]
    fn conflicting_analysis_flags_rejected() {
        let args = parse_args(["--db", "/db", "--in-mem", "--count-blocks", "--show-ebbs"]);
        assert_eq!(args, Err(ParseError::ConflictingAnalysisName));
    }

    #[test]
    fn unknown_flag_rejected() {
        let args = parse_args(["--frobnicate"]);
        assert!(matches!(args, Err(ParseError::UnknownFlag(_))));
    }

    #[test]
    fn missing_value_rejected() {
        let args = parse_args(["--db"]);
        assert!(matches!(args, Err(ParseError::MissingValue(_))));
    }

    #[test]
    fn invalid_slot_value_rejected() {
        let args = parse_args(["--db", "/db", "--in-mem", "--analyse-from", "abc"]);
        assert!(matches!(args, Err(ParseError::InvalidValue(_, _))));
    }

    #[test]
    fn parse_cmd_line_without_config_yields_no_cardano_args() {
        let (config, cardano) = parse_cmd_line(minimal()).expect("parses");
        assert_eq!(config.db_dir.to_str(), Some("/var/lib/db"));
        assert_eq!(cardano, None);
    }

    #[test]
    fn parse_cmd_line_with_config_yields_cardano_args() {
        let (_config, cardano) = parse_cmd_line([
            "--db",
            "/db",
            "--in-mem",
            "--count-blocks",
            "--config",
            "/etc/cardano/config.json",
        ])
        .expect("parses");
        assert_eq!(
            cardano,
            Some(CardanoBlockArgs {
                config_file: PathBuf::from("/etc/cardano/config.json"),
                threshold: None,
            })
        );
    }

    #[test]
    fn parse_cmd_line_with_config_and_threshold() {
        let (_config, cardano) = parse_cmd_line([
            "--db",
            "/db",
            "--in-mem",
            "--count-blocks",
            "--config",
            "config.json",
            "--threshold",
            "0.22",
        ])
        .expect("parses");
        assert_eq!(
            cardano,
            Some(CardanoBlockArgs {
                config_file: PathBuf::from("config.json"),
                threshold: Some(0.22),
            })
        );
    }

    #[test]
    fn threshold_without_config_rejected() {
        let parsed = parse_cmd_line(["--db", "/db", "--in-mem", "--threshold", "0.5"]);
        assert_eq!(parsed, Err(ParseError::ThresholdWithoutConfig));
    }

    #[test]
    fn invalid_threshold_value_rejected() {
        let parsed = parse_cmd_line([
            "--db",
            "/db",
            "--in-mem",
            "--config",
            "c.json",
            "--threshold",
            "not-a-float",
        ]);
        assert!(matches!(parsed, Err(ParseError::InvalidValue(_, _))));
    }

    #[test]
    fn parse_args_drops_cardano_args_half() {
        // The `parse_args` convenience accepts `--config` (so it is not
        // an unknown flag) but returns only the `DBAnalyserConfig`.
        let config = parse_args([
            "--db",
            "/db",
            "--in-mem",
            "--count-blocks",
            "--config",
            "c.json",
        ])
        .expect("parses");
        assert_eq!(config.db_dir.to_str(), Some("/db"));
    }

    #[test]
    fn help_fixture_non_empty() {
        assert!(!HELP_TEXT.is_empty());
    }

    #[test]
    fn version_fixture_non_empty() {
        assert!(!VERSION_TEXT.is_empty());
    }
}
