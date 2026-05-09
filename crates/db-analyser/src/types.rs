//! Typed configuration surface for the `db-analyser` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/Types.hs.
//!
//! Direct ports of upstream's data declarations:
//!
//! - [`SelectDB`] — `data SelectDB = SelectImmutableDB (WithOrigin SlotNo)`.
//! - [`DBAnalyserConfig`] — full 7-field record (dbDir, verbose,
//!   selectDB, validation, analysis, confLimit, ldbBackend).
//! - [`AnalysisName`] — 13-variant sum covering every analysis mode
//!   the upstream binary exposes.
//! - [`AnalysisResult`] — 2-variant sum returned from analysis runs.
//! - [`NumberOfBlocks`] — newtype `Word64` for batch sizing.
//! - [`Limit`] — `Limit Int | Unlimited` for capping analysis runs.
//! - [`LedgerDBBackend`] — `V2InMem | V2LSM`.
//! - [`ValidateBlocks`] — `ValidateAllBlocks | MinimumBlockValidation`.
//! - [`LedgerApplicationMode`] — `LedgerReapply | LedgerApply`.
//! - [`WithOrigin`] — `Origin | At a` (mirrors upstream's
//!   `Ouroboros.Consensus.Block.WithOrigin`).
//!
//! `SlotNo` is reused from `yggdrasil_ledger::types`.

use std::path::PathBuf;

use yggdrasil_ledger::SlotNo;

/// `Origin | At a` — mirrors upstream `Ouroboros.Consensus.Block.WithOrigin`.
///
/// Used by [`SelectDB::SelectImmutableDB`] to allow the operator to
/// pick "the entire chain" (`Origin`) or "from a specific slot
/// onwards" (`At slot`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WithOrigin<A> {
    /// Pre-genesis pseudo-tip.
    Origin,
    /// Concrete value at the given position.
    At(A),
}

/// Which slice of the ChainDB to operate on.
///
/// Upstream: `data SelectDB = SelectImmutableDB (WithOrigin SlotNo)`.
/// Currently the only variant is `SelectImmutableDB` (the operator
/// always points at the immutable DB; the volatile DB is not
/// addressable by db-analyser).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SelectDB {
    /// Run analysis against the immutable DB starting at the given
    /// slot (or from origin).
    SelectImmutableDB(WithOrigin<SlotNo>),
}

/// Number of blocks per BenchmarkLedgerOps batch / GetBlockApplicationMetrics
/// reporting interval.
///
/// Upstream: `newtype NumberOfBlocks = NumberOfBlocks Word64`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct NumberOfBlocks(pub u64);

/// Cap on how many blocks to process before stopping.
///
/// Upstream: `data Limit = Limit Int | Unlimited`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Limit {
    /// Stop after processing this many blocks.
    Limit(u64),
    /// Process the entire selected slice.
    Unlimited,
}

/// Which LedgerDB backend to use during analysis.
///
/// Upstream: `data LedgerDBBackend = V2InMem | V2LSM`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum LedgerDBBackend {
    /// In-memory ledger DB (faster, OOM-bounded).
    #[default]
    V2InMem,
    /// LSM-backed ledger DB (slower, disk-bounded).
    V2LSM,
}

/// Extent of the ChainDB on-disk file validation.
///
/// Upstream: `data ValidateBlocks = ValidateAllBlocks | MinimumBlockValidation`.
/// **Unrelated to ledger-rule validation.**
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum ValidateBlocks {
    /// Validate every block before processing.
    ValidateAllBlocks,
    /// Validate only the minimum required to walk the chain.
    #[default]
    MinimumBlockValidation,
}

/// Whether to apply blocks to a ledger state via /reapplication/
/// (skipping signature checks + Plutus scripts) or full /application/
/// (much slower but covers every protocol rule).
///
/// Upstream: `data LedgerApplicationMode = LedgerReapply | LedgerApply`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum LedgerApplicationMode {
    /// Skip witness + Plutus checks; trust the chain.
    #[default]
    LedgerReapply,
    /// Run the full apply pipeline.
    LedgerApply,
}

/// What kind of analysis to run.
///
/// Upstream: `data AnalysisName = ShowSlotBlockNo | CountTxOutputs | ... | GetBlockApplicationMetrics ...`
/// (13 variants).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisName {
    /// Print `<slot> <blockNo>` per block.
    ShowSlotBlockNo,
    /// Count tx outputs per block.
    CountTxOutputs,
    /// Print header size per block.
    ShowBlockHeaderSize,
    /// Print transaction sizes per block.
    ShowBlockTxsSize,
    /// Print Epoch Boundary Block (Byron) markers.
    ShowEBBs,
    /// Validate the immutable DB without producing per-block output.
    OnlyValidation,
    /// Persist the ledger state at a specific slot.
    StoreLedgerStateAt(SlotNo, LedgerApplicationMode),
    /// Count blocks; emit total at end.
    CountBlocks,
    /// Run NoThunks-style ledger-state inspection every N blocks.
    CheckNoThunksEvery(u64),
    /// Trace ledger-rule events to stderr.
    TraceLedgerProcessing,
    /// Run benchmark on ledger-apply hot path; optional output file.
    BenchmarkLedgerOps(Option<PathBuf>, LedgerApplicationMode),
    /// Reproduce mempool-and-forge behavior at sustained rate.
    ReproMempoolAndForge(i64),
    /// Compute block-application metrics every N blocks; optional output.
    GetBlockApplicationMetrics(NumberOfBlocks, Option<PathBuf>),
}

/// Result of an analysis run.
///
/// Upstream: `data AnalysisResult = ResultCountBlock Int | ResultMaxHeaderSize Word16`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AnalysisResult {
    /// Total block count from `CountBlocks`.
    ResultCountBlock(i64),
    /// Maximum header size in bytes from `ShowBlockHeaderSize`.
    ResultMaxHeaderSize(u16),
}

/// Operator-supplied configuration for the `db-analyser` binary.
///
/// Upstream:
/// ```haskell
/// data DBAnalyserConfig = DBAnalyserConfig
///   { dbDir :: FilePath
///   , verbose :: Bool
///   , selectDB :: SelectDB
///   , validation :: Maybe ValidateBlocks
///   , analysis :: AnalysisName
///   , confLimit :: Limit
///   , ldbBackend :: LedgerDBBackend
///   }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DBAnalyserConfig {
    /// Path to the chain DB. Mirrors upstream `dbDir :: FilePath`.
    pub db_dir: PathBuf,
    /// Verbose logging flag. Mirrors upstream `verbose :: Bool`.
    pub verbose: bool,
    /// Slice of the chain DB to analyze.
    pub select_db: SelectDB,
    /// Optional on-disk validation extent. Mirrors upstream
    /// `validation :: Maybe ValidateBlocks`.
    pub validation: Option<ValidateBlocks>,
    /// What kind of analysis to run.
    pub analysis: AnalysisName,
    /// Cap on number of blocks processed.
    pub conf_limit: Limit,
    /// LedgerDB backend selection.
    pub ldb_backend: LedgerDBBackend,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_origin_round_trip() {
        let origin: WithOrigin<SlotNo> = WithOrigin::Origin;
        let at: WithOrigin<SlotNo> = WithOrigin::At(SlotNo(42));
        assert!(matches!(origin, WithOrigin::Origin));
        match at {
            WithOrigin::At(slot) => assert_eq!(slot, SlotNo(42)),
            WithOrigin::Origin => panic!("expected At"),
        }
    }

    #[test]
    fn select_db_immutable_db_round_trip() {
        let s = SelectDB::SelectImmutableDB(WithOrigin::At(SlotNo(100)));
        match s {
            SelectDB::SelectImmutableDB(WithOrigin::At(slot)) => assert_eq!(slot, SlotNo(100)),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn limit_round_trip() {
        let l = Limit::Limit(1000);
        let u = Limit::Unlimited;
        match l {
            Limit::Limit(n) => assert_eq!(n, 1000),
            Limit::Unlimited => panic!("wrong variant"),
        }
        assert!(matches!(u, Limit::Unlimited));
    }

    #[test]
    fn ledger_db_backend_default_is_v2_in_mem() {
        assert_eq!(LedgerDBBackend::default(), LedgerDBBackend::V2InMem);
    }

    #[test]
    fn validate_blocks_default_is_minimum() {
        assert_eq!(
            ValidateBlocks::default(),
            ValidateBlocks::MinimumBlockValidation
        );
    }

    #[test]
    fn ledger_application_mode_default_is_reapply() {
        assert_eq!(
            LedgerApplicationMode::default(),
            LedgerApplicationMode::LedgerReapply
        );
    }

    #[test]
    fn analysis_name_count_blocks_round_trip() {
        let name = AnalysisName::CountBlocks;
        assert!(matches!(name, AnalysisName::CountBlocks));
    }

    #[test]
    fn analysis_name_store_ledger_state_carries_payload() {
        let name =
            AnalysisName::StoreLedgerStateAt(SlotNo(500), LedgerApplicationMode::LedgerApply);
        match name {
            AnalysisName::StoreLedgerStateAt(slot, mode) => {
                assert_eq!(slot, SlotNo(500));
                assert_eq!(mode, LedgerApplicationMode::LedgerApply);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn analysis_name_benchmark_ledger_ops_with_path_carries_payload() {
        let name = AnalysisName::BenchmarkLedgerOps(
            Some(PathBuf::from("/tmp/out.csv")),
            LedgerApplicationMode::LedgerReapply,
        );
        match name {
            AnalysisName::BenchmarkLedgerOps(Some(p), LedgerApplicationMode::LedgerReapply) => {
                assert_eq!(p.to_str(), Some("/tmp/out.csv"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn analysis_name_get_block_application_metrics_carries_count() {
        let name = AnalysisName::GetBlockApplicationMetrics(NumberOfBlocks(1000), None);
        match name {
            AnalysisName::GetBlockApplicationMetrics(n, p) => {
                assert_eq!(n, NumberOfBlocks(1000));
                assert!(p.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn analysis_result_count_block_carries_total() {
        let r = AnalysisResult::ResultCountBlock(42);
        match r {
            AnalysisResult::ResultCountBlock(n) => assert_eq!(n, 42),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn analysis_result_max_header_size_carries_bytes() {
        let r = AnalysisResult::ResultMaxHeaderSize(2048);
        match r {
            AnalysisResult::ResultMaxHeaderSize(n) => assert_eq!(n, 2048),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn db_analyser_config_construction_with_defaults() {
        let config = DBAnalyserConfig {
            db_dir: PathBuf::from("/var/lib/cardano-node/db"),
            verbose: false,
            select_db: SelectDB::SelectImmutableDB(WithOrigin::Origin),
            validation: None,
            analysis: AnalysisName::CountBlocks,
            conf_limit: Limit::Unlimited,
            ldb_backend: LedgerDBBackend::default(),
        };
        assert_eq!(config.db_dir.to_str(), Some("/var/lib/cardano-node/db"));
        assert!(!config.verbose);
        assert!(config.validation.is_none());
        assert!(matches!(config.analysis, AnalysisName::CountBlocks));
        assert!(matches!(config.conf_limit, Limit::Unlimited));
    }

    #[test]
    fn number_of_blocks_ord() {
        assert!(NumberOfBlocks(100) < NumberOfBlocks(200));
    }
}
