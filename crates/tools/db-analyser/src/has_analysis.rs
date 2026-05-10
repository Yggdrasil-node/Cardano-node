//! Per-block analysis interface — trait surface used by every
//! `AnalysisName` dispatch arm.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/HasAnalysis.hs.
//!
//! Direct port of upstream's typeclass + supporting types:
//!
//! | Upstream                         | Yggdrasil                                |
//! |----------------------------------|------------------------------------------|
//! | `class HasAnalysis blk where`    | [`HasAnalysis`] trait                    |
//! | `class HasProtocolInfo blk where`| [`HasProtocolInfo`] trait + `type Args`  |
//! | `data WithLedgerState blk`       | [`WithLedgerState<Blk, State>`]          |
//! | `Ouroboros.Consensus.Storage.Serialisation.SizeInBytes` | [`SizeInBytes`] type alias |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`HasAnnTip blk` / `GetPrevHash blk` / `Condense (HeaderHash blk)`**:
//!   upstream's typeclass declaration constrains every `HasAnalysis`
//!   block to also be an instance of these protocol-level
//!   typeclasses. The Rust port keeps the trait open — concrete
//!   implementors (Byron / Shelley / Cardano blocks) will add their
//!   own bounds when era-aware ledger types are exposed at crate
//!   boundaries (per the R351 typed-config carve-out).
//! - **`Ouroboros.Consensus.Node.ProtocolInfo`**: upstream's
//!   `ProtocolInfo blk` carries era-specific protocol parameters +
//!   genesis state; Yggdrasil collapses it to an opaque associated
//!   type until the era surface lands.
//! - **`TextBuilder`**: replaced with `String` per the same carve-out
//!   documented in [`crate::csv`].

use std::collections::HashMap;

/// Block-byte-count alias, used by [`HasAnalysis::block_tx_sizes`].
///
/// Upstream: `import Ouroboros.Consensus.Storage.Serialisation (SizeInBytes)`,
/// which resolves to `Word32`. The Rust port uses `u64` for headroom
/// (modern Cardano blocks max at ~16 KiB but the type is used for
/// per-tx sizes which can be larger); narrower-int callers can
/// downcast at use site.
pub type SizeInBytes = u64;

/// A block + its ledger states immediately before and after
/// application. Mirror of upstream
/// `data WithLedgerState blk = WithLedgerState { wlsBlk, wlsStateBefore, wlsStateAfter }`.
///
/// Generic over `Blk` (the block type) and `State` (the ledger-state
/// type indexed by the same block). Upstream's
/// `LedgerState blk ValuesMK` is the values-only projection of the
/// ledger state used during block application; concrete ports will
/// instantiate `State` to a yggdrasil-ledger era-specific
/// `LedgerState` type when the era surface is exposed.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct WithLedgerState<Blk, State> {
    /// The block being analyzed.
    pub blk: Blk,
    /// Ledger state immediately before applying [`Self::blk`]. Contains
    /// only the values to be consumed by the block.
    pub state_before: State,
    /// Ledger state immediately after applying [`Self::blk`]. Contains
    /// only the values produced by the block.
    pub state_after: State,
}

impl<Blk, State> WithLedgerState<Blk, State> {
    /// Construct from the three components.
    pub fn new(blk: Blk, state_before: State, state_after: State) -> Self {
        WithLedgerState {
            blk,
            state_before,
            state_after,
        }
    }
}

/// Per-block analysis interface — the trait every era-specific block
/// implementation must satisfy for db-analyser's dispatch arms to
/// operate on it.
///
/// Upstream: `class (HasAnnTip blk, GetPrevHash blk, Condense (HeaderHash blk)) => HasAnalysis blk`.
/// Rust port keeps the trait open (era-specific implementors add
/// their own bounds per the carve-out in the module docstring).
///
/// Each method has a concrete docstring describing its role in the
/// analysis dispatch:
pub trait HasAnalysis: Sized {
    /// The header-hash type for this block.
    type HeaderHash: Eq + std::hash::Hash + Clone;
    /// The chain-hash type for this block (typically `Option<HeaderHash>`).
    type ChainHash: Clone;
    /// The ledger-state-with-values type for this block (era-specific).
    type LedgerStateValues;

    /// Count the number of transaction outputs in this block.
    /// Mirror of upstream `countTxOutputs :: blk -> Int`.
    fn count_tx_outputs(&self) -> i64;

    /// Sizes of each transaction in this block (in bytes).
    /// Mirror of upstream `blockTxSizes :: blk -> [SizeInBytes]`.
    fn block_tx_sizes(&self) -> Vec<SizeInBytes>;

    /// Map of known epoch-boundary blocks (Byron-only). Mirror of
    /// upstream `knownEBBs :: proxy blk -> Map (HeaderHash blk) (ChainHash blk)`.
    /// Returned as a `HashMap` keyed by header-hash; non-Byron eras
    /// return an empty map.
    fn known_ebbs() -> HashMap<Self::HeaderHash, Self::ChainHash>;

    /// Emit trace markers at points in processing. Mirror of upstream
    /// `emitTraces :: WithLedgerState blk -> [String]`. Used by the
    /// `TraceLedgerProcessing` analysis to mark significant events
    /// (epoch transitions, era boundaries, etc.).
    fn emit_traces(with_state: &WithLedgerState<Self, Self::LedgerStateValues>) -> Vec<String>;

    /// Per-block stats for the `BenchmarkLedgerOps` pass. Mirror of
    /// upstream `blockStats :: blk -> [TextBuilder]` (the `TextBuilder`
    /// carve-out replaces it with `String`).
    fn block_stats(&self) -> Vec<String>;

    /// CSV-emission builders for the `GetBlockApplicationMetrics`
    /// pass. Mirror of upstream
    /// `blockApplicationMetrics :: [(TextBuilder, WithLedgerState blk -> IO TextBuilder)]`.
    ///
    /// Each tuple is `(header, fn)`:
    /// - `header`: column-header string
    /// - `fn`: closure that computes the column value for a given
    ///   block-with-ledger-states; returns `Result` to handle the
    ///   IO-fallible cases upstream uses (e.g. measuring serialized
    ///   size which can fail on encoding errors).
    ///
    /// The result is consumed by [`crate::csv::compute_and_write_line_io`]
    /// at output time.
    fn block_application_metrics() -> Vec<BlockApplicationMetric<Self>>;
}

/// One column of the `BlockApplicationMetrics` CSV. The closure type
/// mirrors upstream's `WithLedgerState blk -> IO TextBuilder`.
pub type BlockApplicationMetric<Blk> = (
    &'static str,
    Box<
        dyn Fn(
                &WithLedgerState<Blk, <Blk as HasAnalysis>::LedgerStateValues>,
            ) -> Result<String, std::io::Error>
            + Send
            + Sync,
    >,
);

/// Per-block-type protocol-info construction trait. Mirror of upstream
/// `class HasProtocolInfo blk where { data Args blk; mkProtocolInfo :: Args blk -> IO (ProtocolInfo blk) }`.
///
/// The associated `Args` type carries CLI-derived arguments needed to
/// instantiate the protocol info (genesis files, network magic, etc.);
/// it's an associated type rather than a generic parameter to mirror
/// upstream's data-family declaration.
///
/// `ProtocolInfo` itself is left as an associated type on the trait
/// because upstream's `Ouroboros.Consensus.Node.ProtocolInfo blk` is
/// era-specific and depends on the consensus crate's surface (which
/// yggdrasil-ledger has not yet exposed at crate boundaries).
pub trait HasProtocolInfo: Sized {
    /// CLI-derived arguments for protocol-info construction.
    type Args;
    /// Era-specific protocol-info record (carve-out: opaque type).
    type ProtocolInfo;
    /// Errors from protocol-info construction.
    type Error: std::error::Error;

    /// Build a `ProtocolInfo` from the supplied args. Mirror of
    /// upstream `mkProtocolInfo :: Args blk -> IO (ProtocolInfo blk)`.
    fn make_protocol_info(args: Self::Args) -> Result<Self::ProtocolInfo, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial block type for trait-shape verification only.
    #[derive(Clone, Debug, Eq, PartialEq, Hash)]
    struct StubBlock {
        slot: u64,
        tx_count: i64,
        tx_sizes: Vec<SizeInBytes>,
    }

    /// A trivial state type that carries a u64 "values count" so
    /// before/after diffs are visible in tests.
    #[derive(Clone, Debug, Eq, PartialEq, Hash, Default)]
    struct StubState {
        values_count: u64,
    }

    impl HasAnalysis for StubBlock {
        type HeaderHash = u64;
        type ChainHash = Option<u64>;
        type LedgerStateValues = StubState;

        fn count_tx_outputs(&self) -> i64 {
            self.tx_count
        }

        fn block_tx_sizes(&self) -> Vec<SizeInBytes> {
            self.tx_sizes.clone()
        }

        fn known_ebbs() -> HashMap<Self::HeaderHash, Self::ChainHash> {
            HashMap::new()
        }

        fn emit_traces(with_state: &WithLedgerState<Self, Self::LedgerStateValues>) -> Vec<String> {
            vec![format!(
                "block-slot={} state_before_count={} state_after_count={}",
                with_state.blk.slot,
                with_state.state_before.values_count,
                with_state.state_after.values_count,
            )]
        }

        fn block_stats(&self) -> Vec<String> {
            vec![
                format!("slot={}", self.slot),
                format!("tx_count={}", self.tx_count),
            ]
        }

        fn block_application_metrics() -> Vec<BlockApplicationMetric<Self>> {
            vec![
                (
                    "slot",
                    Box::new(|with_state| Ok(with_state.blk.slot.to_string())),
                ),
                (
                    "tx_count",
                    Box::new(|with_state| Ok(with_state.blk.tx_count.to_string())),
                ),
                (
                    "values_delta",
                    Box::new(|with_state| {
                        let delta = with_state.state_after.values_count as i128
                            - with_state.state_before.values_count as i128;
                        Ok(delta.to_string())
                    }),
                ),
            ]
        }
    }

    fn sample_with_state() -> WithLedgerState<StubBlock, StubState> {
        WithLedgerState::new(
            StubBlock {
                slot: 100,
                tx_count: 5,
                tx_sizes: vec![32, 64, 128, 256, 512],
            },
            StubState { values_count: 10 },
            StubState { values_count: 12 },
        )
    }

    #[test]
    fn with_ledger_state_round_trips() {
        let ws = sample_with_state();
        assert_eq!(ws.blk.slot, 100);
        assert_eq!(ws.state_before.values_count, 10);
        assert_eq!(ws.state_after.values_count, 12);
    }

    #[test]
    fn count_tx_outputs_returns_block_tx_count() {
        let blk = StubBlock {
            slot: 0,
            tx_count: 42,
            tx_sizes: Vec::new(),
        };
        assert_eq!(blk.count_tx_outputs(), 42);
    }

    #[test]
    fn block_tx_sizes_round_trip() {
        let blk = StubBlock {
            slot: 0,
            tx_count: 3,
            tx_sizes: vec![100, 200, 300],
        };
        assert_eq!(blk.block_tx_sizes(), vec![100, 200, 300]);
    }

    #[test]
    fn known_ebbs_default_empty() {
        let ebbs = StubBlock::known_ebbs();
        assert!(ebbs.is_empty());
    }

    #[test]
    fn emit_traces_renders_state_diff() {
        let traces = StubBlock::emit_traces(&sample_with_state());
        assert_eq!(traces.len(), 1);
        assert!(traces[0].contains("block-slot=100"));
        assert!(traces[0].contains("state_before_count=10"));
        assert!(traces[0].contains("state_after_count=12"));
    }

    #[test]
    fn block_stats_returns_per_block_metrics() {
        let blk = StubBlock {
            slot: 200,
            tx_count: 7,
            tx_sizes: Vec::new(),
        };
        let stats = blk.block_stats();
        assert_eq!(
            stats,
            vec!["slot=200".to_string(), "tx_count=7".to_string()]
        );
    }

    #[test]
    fn block_application_metrics_drives_csv_emission() {
        let metrics = StubBlock::block_application_metrics();
        assert_eq!(metrics.len(), 3);
        assert_eq!(metrics[0].0, "slot");
        assert_eq!(metrics[1].0, "tx_count");
        assert_eq!(metrics[2].0, "values_delta");

        let ws = sample_with_state();
        let slot_value = (metrics[0].1)(&ws).expect("computes");
        let tx_count_value = (metrics[1].1)(&ws).expect("computes");
        let values_delta = (metrics[2].1)(&ws).expect("computes");
        assert_eq!(slot_value, "100");
        assert_eq!(tx_count_value, "5");
        assert_eq!(values_delta, "2");
    }

    #[test]
    fn block_application_metrics_handles_negative_delta() {
        // After-state has fewer values than before — delta is negative.
        let ws = WithLedgerState::new(
            StubBlock {
                slot: 0,
                tx_count: 0,
                tx_sizes: Vec::new(),
            },
            StubState { values_count: 100 },
            StubState { values_count: 50 },
        );
        let metrics = StubBlock::block_application_metrics();
        let values_delta = (metrics[2].1)(&ws).expect("computes");
        assert_eq!(values_delta, "-50");
    }

    /// A trivial HasProtocolInfo implementor used only for trait-shape
    /// verification.
    struct StubProtocol;

    impl HasProtocolInfo for StubProtocol {
        type Args = u32;
        type ProtocolInfo = u64;
        type Error = std::io::Error;

        fn make_protocol_info(args: Self::Args) -> Result<Self::ProtocolInfo, Self::Error> {
            // Trivial: protocol-info is just the args doubled, as a u64.
            Ok(u64::from(args) * 2)
        }
    }

    #[test]
    fn has_protocol_info_args_passes_through_to_make_protocol_info() {
        let protocol_info = StubProtocol::make_protocol_info(21).expect("constructs");
        assert_eq!(protocol_info, 42);
    }
}
