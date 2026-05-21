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
use std::path::PathBuf;

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

/// CLI-derived arguments for constructing the Cardano-block protocol
/// info — the `HasProtocolInfo` `Args` data-family instance.
///
/// Upstream `Block/Cardano.hs`:
/// ```haskell
/// data Args (CardanoBlock StandardCrypto) = CardanoBlockArgs
///   { configFile :: FilePath
///   , threshold  :: Maybe PBftSignatureThreshold
///   }
/// ```
///
/// db-analyser collapses the three per-era `Block/{Byron,Shelley,Cardano}.hs`
/// modules into this module — see the [`HasAnalysis`] impl docstring — so
/// the `Args (CardanoBlock StandardCrypto)` data-family instance lives
/// here rather than in a `block/cardano.rs` mirror. `config_file` is the
/// operator's node `config.json`; `threshold` is the optional Byron PBFT
/// signature threshold (upstream `PBftSignatureThreshold` is a `Double`
/// newtype → `f64`).
#[derive(Clone, Debug, PartialEq)]
pub struct CardanoBlockArgs {
    /// Path to the operator's node `config.json`.
    pub config_file: PathBuf,
    /// Optional Byron PBFT signature threshold.
    pub threshold: Option<f64>,
}

// ---------------------------------------------------------------------------
// HasAnalysis impl for Yggdrasil's unified Block (R476)
// ---------------------------------------------------------------------------

/// Per-block ledger-state values associated with [`yggdrasil_ledger::Block`]
/// for the [`HasAnalysis::LedgerStateValues`] slot.
///
/// Mirror of upstream's `LedgerState (CardanoBlock c) ValuesMK` —
/// the values-only projection of the consensus ledger-state used
/// during block application. Yggdrasil ships a placeholder unit
/// struct because the analyses that consume non-trivial state
/// (`StoreLedgerStateAt`, `TraceLedgerProcessing`, `BenchmarkLedgerOps`,
/// `ReproMempoolAndForge`, `CheckNoThunksEvery`,
/// `GetBlockApplicationMetrics`) are deferred to a future ledger-state
/// apply-loop arc — R475-R481 lands only the block-iteration-only
/// analyses.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct CardanoLedgerStateValues;

/// HasAnalysis surface for the unified [`yggdrasil_ledger::Block`].
///
/// ## Naming parity
///
/// **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/Block/Cardano.hs.
///
/// Upstream ships three per-era typeclass instances under
/// `DBAnalyser/Block/{Byron,Shelley,Cardano}.hs` — one per
/// upstream-side block newtype. Yggdrasil collapses the three into
/// a single impl because `yggdrasil_ledger::Block` is a unified
/// struct carrying an `era: Era` discriminator. Per-era logic
/// dispatches through that discriminator (mirror of the Haskell
/// typeclass-dispatch shape).
///
/// **Byron known-EBB registry** lives at [`crate::byron_ebbs::known_ebbs`]
/// (R476 — a direct port of upstream `Ouroboros.Consensus.Byron.EBBs::knownEBBs`).
///
/// **Ledger-state-dependent methods** ([`Self::emit_traces`],
/// [`Self::block_stats`], [`Self::block_application_metrics`])
/// currently return minimal/empty values — they're filled in by the
/// future ledger-state apply-loop arc per the carve-out documented
/// in [`crate::status::analysis_dispatch_status`].
impl HasAnalysis for yggdrasil_ledger::Block {
    type HeaderHash = yggdrasil_ledger::HeaderHash;
    type ChainHash = Option<yggdrasil_ledger::HeaderHash>;
    type LedgerStateValues = CardanoLedgerStateValues;

    /// Sum of per-tx output counts across all transactions in the block.
    /// Mirror of upstream `countTxOutputs (Block { blkTxs = txs }) =
    /// sum (map countTxOutputs txs)` per-era dispatch.
    ///
    /// Per-tx body-decode errors are treated as zero (mirror of
    /// upstream's behavior when a body fails to decode — the chain
    /// rule would have rejected the block at apply time, so a
    /// successful chain-walk encountering a decode error here is a
    /// bug, not a real-data condition).
    fn count_tx_outputs(&self) -> i64 {
        let mut total: i64 = 0;
        for tx in &self.transactions {
            let n = tx.output_count(self.era).unwrap_or(0);
            total = total.saturating_add(n as i64);
        }
        total
    }

    /// Per-transaction serialized sizes. Mirror of upstream
    /// `blockTxSizes (Block { blkTxs = txs }) = map txSize txs`.
    fn block_tx_sizes(&self) -> Vec<SizeInBytes> {
        self.transactions
            .iter()
            .map(|tx| tx.serialized_size() as SizeInBytes)
            .collect()
    }

    /// Byron known-EBB registry. Returns the full registry across
    /// all networks (Mainnet + Staging + Testnet) — callers filter
    /// by chain context at dispatch time.
    ///
    /// Mirror of upstream `knownEBBs = const Byron.knownEBBs` from
    /// `DBAnalyser/Block/Byron.hs`. Non-Byron upstream block types
    /// return `Map.empty`; the Cardano combinator at upstream
    /// `Block/Cardano.hs::knownEBBs` unions the Byron registry with
    /// empty per-era maps, so the union is identical to the Byron
    /// registry alone.
    fn known_ebbs() -> HashMap<Self::HeaderHash, Self::ChainHash> {
        crate::byron_ebbs::known_ebbs()
    }

    /// Trace markers emitted during ledger-state apply.
    ///
    /// **R496 expansion:** R476 shipped an empty placeholder; R496
    /// emits block-iteration-derivable per-block trace strings —
    /// era, slot, block_no, tx_count, EBB marker when applicable,
    /// and the previous-hash relation. Each string is a stable
    /// `key=value` pair so downstream tooling can grep / parse.
    /// Ledger-state-derived traces (stake delta, reward delta,
    /// epoch-boundary processing) still require a configured
    /// genesis state — those land in a follow-on arc.
    fn emit_traces(with_state: &WithLedgerState<Self, Self::LedgerStateValues>) -> Vec<String> {
        let blk = &with_state.blk;
        let mut traces = vec![
            format!("event=block_apply"),
            format!("slot={}", blk.header.slot_no.0),
            format!("block_no={}", blk.header.block_no.0),
            format!("era={:?}", blk.era),
            format!("tx_count={}", blk.transactions.len()),
        ];
        // EBB marker — Byron-era blocks whose hash matches a known
        // EBB entry; useful for ShowEBBs cross-reference at apply
        // time.
        let registry = crate::byron_ebbs::known_ebbs();
        if registry.contains_key(&blk.header.hash) {
            traces.push("ebb=true".to_string());
        }
        // Origin-successor marker — first block of the chain has
        // prev_hash = all-zeros sentinel.
        if blk.header.prev_hash.0 == [0u8; 32] {
            traces.push("prev=<origin>".to_string());
        }
        traces
    }

    /// Per-block stats for the `BenchmarkLedgerOps` analysis.
    ///
    /// Yggdrasil emits the block-iteration-only stats (slot, block_no,
    /// era, tx_count). Upstream emits additional ledger-state-derived
    /// stats which are deferred per the R476 carve-out.
    fn block_stats(&self) -> Vec<String> {
        vec![
            format!("slot={}", self.header.slot_no.0),
            format!("block_no={}", self.header.block_no.0),
            format!("era={:?}", self.era),
            format!("tx_count={}", self.transactions.len()),
        ]
    }

    /// Per-block CSV columns for the `GetBlockApplicationMetrics`
    /// analysis. Each tuple is `(header, closure)`.
    ///
    /// Yggdrasil ships the block-iteration-only columns (slot,
    /// block_no, era, tx_count). Upstream ships ledger-state-derived
    /// columns (mempool-fee-totals, utxo-delta, etc.) which are
    /// deferred per the R476 carve-out.
    fn block_application_metrics() -> Vec<BlockApplicationMetric<Self>> {
        vec![
            (
                "slot",
                Box::new(|with_state| Ok(with_state.blk.header.slot_no.0.to_string())),
            ),
            (
                "block_no",
                Box::new(|with_state| Ok(with_state.blk.header.block_no.0.to_string())),
            ),
            (
                "era",
                Box::new(|with_state| Ok(format!("{:?}", with_state.blk.era))),
            ),
            (
                "tx_count",
                Box::new(|with_state| Ok(with_state.blk.transactions.len().to_string())),
            ),
        ]
    }
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

    #[test]
    fn cardano_block_args_constructs_without_threshold() {
        let args = CardanoBlockArgs {
            config_file: PathBuf::from("/etc/cardano/config.json"),
            threshold: None,
        };
        assert_eq!(args.config_file, PathBuf::from("/etc/cardano/config.json"));
        assert_eq!(args.threshold, None);
    }

    #[test]
    fn cardano_block_args_carries_pbft_threshold() {
        let args = CardanoBlockArgs {
            config_file: PathBuf::from("config.json"),
            threshold: Some(0.22),
        };
        assert_eq!(args.threshold, Some(0.22));
        // The data-family instance derives structural equality, mirror
        // of upstream's `CardanoBlockArgs` record being comparable.
        assert_eq!(args.clone(), args);
    }

    // ── HasAnalysis for yggdrasil_ledger::Block (R476) ─────────────────

    use yggdrasil_ledger::{
        Block, BlockHeader, BlockNo, Era, HeaderHash, SlotNo, Tx, compute_tx_id,
    };

    fn mk_block_header(slot: u64, block_no: u64) -> BlockHeader {
        BlockHeader {
            hash: HeaderHash([0x01; 32]),
            prev_hash: HeaderHash([0x00; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x00; 32],
            protocol_version: None,
        }
    }

    fn mk_empty_tx_with_body(body: Vec<u8>) -> Tx {
        Tx {
            id: compute_tx_id(&body),
            body,
            witnesses: None,
            auxiliary_data: None,
            is_valid: None,
        }
    }

    fn mk_shelley_body_cbor() -> Vec<u8> {
        use yggdrasil_ledger::CborEncode;
        use yggdrasil_ledger::{ShelleyTxBody, ShelleyTxIn, ShelleyTxOut};
        let body = ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xAA; 32],
                index: 0,
            }],
            outputs: vec![
                ShelleyTxOut {
                    address: vec![0x61; 29],
                    amount: 1_000_000,
                },
                ShelleyTxOut {
                    address: vec![0x62; 29],
                    amount: 2_000_000,
                },
            ],
            fee: 1_000,
            ttl: 100,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        body.to_cbor_bytes()
    }

    #[test]
    fn block_count_tx_outputs_empty_block_is_zero() {
        let blk = Block {
            era: Era::Shelley,
            header: mk_block_header(0, 0),
            transactions: vec![],
            raw_cbor: None,
            header_cbor_size: None,
        };
        assert_eq!(blk.count_tx_outputs(), 0);
    }

    #[test]
    fn block_count_tx_outputs_shelley_sums_per_tx() {
        let body = mk_shelley_body_cbor();
        let blk = Block {
            era: Era::Shelley,
            header: mk_block_header(10, 5),
            // Three transactions, each with 2 outputs → expect 6.
            transactions: vec![
                mk_empty_tx_with_body(body.clone()),
                mk_empty_tx_with_body(body.clone()),
                mk_empty_tx_with_body(body),
            ],
            raw_cbor: None,
            header_cbor_size: None,
        };
        assert_eq!(blk.count_tx_outputs(), 6);
    }

    #[test]
    fn block_count_tx_outputs_treats_decode_error_as_zero() {
        // Block carries a tx with garbage body bytes — count is 0
        // (the chain rule would have rejected the block, so the
        // decode-error is a forensic-only condition; we don't crash).
        let blk = Block {
            era: Era::Shelley,
            header: mk_block_header(0, 0),
            transactions: vec![mk_empty_tx_with_body(vec![0xFF, 0xFF])],
            raw_cbor: None,
            header_cbor_size: None,
        };
        assert_eq!(blk.count_tx_outputs(), 0);
    }

    #[test]
    fn block_count_tx_outputs_byron_dispatch() {
        use yggdrasil_ledger::CborEncode;
        use yggdrasil_ledger::{ByronTx, ByronTxIn, ByronTxOut};
        let mut enc = yggdrasil_ledger::cbor::Encoder::new();
        enc.map(0);
        let attrs = enc.into_bytes();
        let byron_tx = ByronTx {
            inputs: vec![ByronTxIn {
                txid: [0xCC; 32],
                index: 0,
            }],
            outputs: vec![ByronTxOut {
                address: vec![0x82, 0x80, 0xA0],
                amount: 500,
            }],
            attributes: attrs,
        };
        let body = byron_tx.to_cbor_bytes();
        let blk = Block {
            era: Era::Byron,
            header: mk_block_header(0, 0),
            transactions: vec![mk_empty_tx_with_body(body)],
            raw_cbor: None,
            header_cbor_size: None,
        };
        assert_eq!(blk.count_tx_outputs(), 1);
    }

    #[test]
    fn block_tx_sizes_returns_per_tx_serialized_sizes() {
        let body_a = vec![0x80]; // CBOR empty array
        let body_b = mk_shelley_body_cbor();
        let blk = Block {
            era: Era::Shelley,
            header: mk_block_header(0, 0),
            transactions: vec![
                mk_empty_tx_with_body(body_a.clone()),
                mk_empty_tx_with_body(body_b.clone()),
            ],
            raw_cbor: None,
            header_cbor_size: None,
        };
        let sizes = blk.block_tx_sizes();
        assert_eq!(sizes.len(), 2);
        // Each size should match Tx::serialized_size() cast to u64.
        assert_eq!(
            sizes[0],
            blk.transactions[0].serialized_size() as SizeInBytes
        );
        assert_eq!(
            sizes[1],
            blk.transactions[1].serialized_size() as SizeInBytes
        );
    }

    #[test]
    fn block_known_ebbs_returns_byron_registry() {
        // The registry is populated from upstream's EBBs table —
        // 325 entries total.
        let ebbs = <Block as HasAnalysis>::known_ebbs();
        assert_eq!(ebbs.len(), 325);
        // Byron genesis successor is in the registry with no
        // previous hash (the first Mainnet entry in EBBs.hs).
        let genesis_succ = HeaderHash(crate::byron_ebbs::parse_hex32(
            "89d9b5a5b8ddc8d7e5a6795e9774d97faf1efea59b2caf7eaf9f8c5b32059df4",
        ));
        assert!(ebbs.contains_key(&genesis_succ));
    }

    #[test]
    fn block_emit_traces_returns_block_iteration_traces_r496() {
        // R496: emit_traces now emits block-iteration-derived
        // strings (event/slot/block_no/era/tx_count + optional
        // origin / ebb markers). Was empty at R476.
        let blk = Block {
            era: Era::Shelley,
            header: mk_block_header(123, 456),
            transactions: vec![],
            raw_cbor: None,
            header_cbor_size: None,
        };
        let with_state =
            WithLedgerState::new(blk, CardanoLedgerStateValues, CardanoLedgerStateValues);
        let traces = Block::emit_traces(&with_state);
        assert!(!traces.is_empty(), "emit_traces should emit ≥1 string");
        assert!(traces.iter().any(|s| s == "event=block_apply"));
        assert!(traces.iter().any(|s| s == "slot=123"));
        assert!(traces.iter().any(|s| s == "block_no=456"));
        assert!(traces.iter().any(|s| s == "era=Shelley"));
        assert!(traces.iter().any(|s| s == "tx_count=0"));
    }

    #[test]
    fn block_stats_renders_block_iteration_only_columns() {
        let blk = Block {
            era: Era::Conway,
            header: mk_block_header(42, 17),
            transactions: vec![mk_empty_tx_with_body(vec![]), mk_empty_tx_with_body(vec![])],
            raw_cbor: None,
            header_cbor_size: None,
        };
        let stats = blk.block_stats();
        assert_eq!(stats.len(), 4);
        assert_eq!(stats[0], "slot=42");
        assert_eq!(stats[1], "block_no=17");
        assert_eq!(stats[2], "era=Conway");
        assert_eq!(stats[3], "tx_count=2");
    }

    #[test]
    fn block_application_metrics_for_yggdrasil_block() {
        let metrics = <Block as HasAnalysis>::block_application_metrics();
        assert_eq!(metrics.len(), 4);
        assert_eq!(metrics[0].0, "slot");
        assert_eq!(metrics[1].0, "block_no");
        assert_eq!(metrics[2].0, "era");
        assert_eq!(metrics[3].0, "tx_count");

        let blk = Block {
            era: Era::Babbage,
            header: mk_block_header(100, 50),
            transactions: vec![mk_empty_tx_with_body(vec![])],
            raw_cbor: None,
            header_cbor_size: None,
        };
        let with_state =
            WithLedgerState::new(blk, CardanoLedgerStateValues, CardanoLedgerStateValues);
        assert_eq!((metrics[0].1)(&with_state).unwrap(), "100");
        assert_eq!((metrics[1].1)(&with_state).unwrap(), "50");
        assert_eq!((metrics[2].1)(&with_state).unwrap(), "Babbage");
        assert_eq!((metrics[3].1)(&with_state).unwrap(), "1");
    }

    // ── per-era dispatch coverage: Allegra / Mary / Alonzo (R477) ──────

    fn mk_alonzo_body_cbor() -> Vec<u8> {
        use yggdrasil_ledger::CborEncode;
        use yggdrasil_ledger::{AlonzoTxBody, AlonzoTxOut, ShelleyTxIn, Value};
        let body = AlonzoTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xBB; 32],
                index: 0,
            }],
            outputs: vec![
                AlonzoTxOut {
                    address: vec![0x61; 29],
                    amount: Value::Coin(5_000_000),
                    datum_hash: None,
                },
                AlonzoTxOut {
                    address: vec![0x62; 29],
                    amount: Value::Coin(7_500_000),
                    datum_hash: Some([0xCC; 32]),
                },
                AlonzoTxOut {
                    address: vec![0x63; 29],
                    amount: Value::Coin(10_000_000),
                    datum_hash: None,
                },
            ],
            fee: 1_000,
            ttl: Some(100),
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
            script_data_hash: None,
            collateral: None,
            required_signers: None,
            network_id: None,
        };
        body.to_cbor_bytes()
    }

    #[test]
    fn block_count_tx_outputs_allegra_dispatch() {
        // Allegra reuses ShelleyTxBody — same wire format. The
        // R475 dispatcher maps Allegra → ShelleyTxBody decoder.
        let body = mk_shelley_body_cbor();
        let blk = Block {
            era: Era::Allegra,
            header: mk_block_header(208, 0),
            transactions: vec![
                mk_empty_tx_with_body(body.clone()),
                mk_empty_tx_with_body(body),
            ],
            raw_cbor: None,
            header_cbor_size: None,
        };
        // Two transactions × two outputs each = 4.
        assert_eq!(blk.count_tx_outputs(), 4);
    }

    #[test]
    fn block_count_tx_outputs_mary_dispatch() {
        // Mary reuses ShelleyTxBody at the wire-format level (Value
        // changes are encoded inside TxOut but tx-output counting
        // walks the outer array shape, which is unchanged).
        let body = mk_shelley_body_cbor();
        let blk = Block {
            era: Era::Mary,
            header: mk_block_header(300, 0),
            transactions: vec![mk_empty_tx_with_body(body)],
            raw_cbor: None,
            header_cbor_size: None,
        };
        assert_eq!(blk.count_tx_outputs(), 2);
    }

    #[test]
    fn block_count_tx_outputs_alonzo_dispatch() {
        let body = mk_alonzo_body_cbor();
        let blk = Block {
            era: Era::Alonzo,
            header: mk_block_header(400, 0),
            transactions: vec![mk_empty_tx_with_body(body)],
            raw_cbor: None,
            header_cbor_size: None,
        };
        // Single Alonzo tx with 3 outputs.
        assert_eq!(blk.count_tx_outputs(), 3);
    }

    #[test]
    fn block_count_tx_outputs_alonzo_multi_tx() {
        let body = mk_alonzo_body_cbor();
        let blk = Block {
            era: Era::Alonzo,
            header: mk_block_header(401, 0),
            transactions: vec![
                mk_empty_tx_with_body(body.clone()),
                mk_empty_tx_with_body(body.clone()),
                mk_empty_tx_with_body(body),
            ],
            raw_cbor: None,
            header_cbor_size: None,
        };
        // 3 txs × 3 outputs = 9.
        assert_eq!(blk.count_tx_outputs(), 9);
    }

    #[test]
    fn block_count_tx_outputs_alonzo_decoder_accepts_shelley_body() {
        // Alonzo's TxBody wire format is a superset of Shelley's
        // (same map keys 0..6, plus optional Alonzo-only keys
        // 7..15). When the era is Alonzo but the body is shaped
        // like a Shelley body (no Alonzo-extension fields set),
        // the Alonzo decoder accepts it and the output count is
        // the Shelley body's output count.
        //
        // This is a *property* of the wire format, not a chain-
        // validity claim — real Alonzo blocks always carry full
        // Alonzo bodies; this test documents the dispatcher's
        // backward-compat-by-decoder-design behavior.
        let shelley_body = mk_shelley_body_cbor();
        let blk = Block {
            era: Era::Alonzo,
            header: mk_block_header(402, 0),
            transactions: vec![mk_empty_tx_with_body(shelley_body)],
            raw_cbor: None,
            header_cbor_size: None,
        };
        // Shelley body has 2 outputs; Alonzo decoder reads them.
        assert_eq!(blk.count_tx_outputs(), 2);
    }

    #[test]
    fn block_stats_renders_each_era() {
        for (era, name) in [
            (Era::Allegra, "Allegra"),
            (Era::Mary, "Mary"),
            (Era::Alonzo, "Alonzo"),
        ] {
            let blk = Block {
                era,
                header: mk_block_header(1, 1),
                transactions: vec![],
                raw_cbor: None,
                header_cbor_size: None,
            };
            let stats = blk.block_stats();
            assert!(
                stats[2].contains(name),
                "era={era:?} expected 'era={name}' got {:?}",
                stats[2]
            );
        }
    }

    // ── per-era dispatch coverage: Babbage / Conway (R478) ─────────────

    fn mk_babbage_body_cbor() -> Vec<u8> {
        use yggdrasil_ledger::CborEncode;
        use yggdrasil_ledger::{BabbageTxBody, BabbageTxOut, ShelleyTxIn, Value};
        let body = BabbageTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xDD; 32],
                index: 0,
            }],
            outputs: vec![
                BabbageTxOut {
                    address: vec![0x61; 29],
                    amount: Value::Coin(1_000_000),
                    datum_option: None,
                    script_ref: None,
                },
                BabbageTxOut {
                    address: vec![0x62; 29],
                    amount: Value::Coin(2_000_000),
                    datum_option: None,
                    script_ref: None,
                },
            ],
            fee: 1_000,
            ttl: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
            script_data_hash: None,
            collateral: None,
            required_signers: None,
            network_id: None,
            collateral_return: None,
            total_collateral: None,
            reference_inputs: None,
        };
        body.to_cbor_bytes()
    }

    fn mk_conway_body_cbor() -> Vec<u8> {
        use yggdrasil_ledger::CborEncode;
        use yggdrasil_ledger::{BabbageTxOut, ConwayTxBody, ShelleyTxIn, Value};
        let body = ConwayTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xEE; 32],
                index: 0,
            }],
            outputs: vec![
                BabbageTxOut {
                    address: vec![0x61; 29],
                    amount: Value::Coin(3_000_000),
                    datum_option: None,
                    script_ref: None,
                },
                BabbageTxOut {
                    address: vec![0x62; 29],
                    amount: Value::Coin(5_000_000),
                    datum_option: None,
                    script_ref: None,
                },
                BabbageTxOut {
                    address: vec![0x63; 29],
                    amount: Value::Coin(7_000_000),
                    datum_option: None,
                    script_ref: None,
                },
                BabbageTxOut {
                    address: vec![0x64; 29],
                    amount: Value::Coin(11_000_000),
                    datum_option: None,
                    script_ref: None,
                },
            ],
            fee: 2_000,
            ttl: None,
            certificates: None,
            withdrawals: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
            script_data_hash: None,
            collateral: None,
            required_signers: None,
            network_id: None,
            collateral_return: None,
            total_collateral: None,
            reference_inputs: None,
            voting_procedures: None,
            proposal_procedures: None,
            current_treasury_value: None,
            treasury_donation: None,
        };
        body.to_cbor_bytes()
    }

    #[test]
    fn block_count_tx_outputs_babbage_dispatch() {
        let body = mk_babbage_body_cbor();
        let blk = Block {
            era: Era::Babbage,
            header: mk_block_header(500, 0),
            transactions: vec![mk_empty_tx_with_body(body)],
            raw_cbor: None,
            header_cbor_size: None,
        };
        // Single Babbage tx with 2 outputs.
        assert_eq!(blk.count_tx_outputs(), 2);
    }

    #[test]
    fn block_count_tx_outputs_babbage_multi_tx() {
        let body = mk_babbage_body_cbor();
        let blk = Block {
            era: Era::Babbage,
            header: mk_block_header(501, 0),
            transactions: vec![
                mk_empty_tx_with_body(body.clone()),
                mk_empty_tx_with_body(body),
            ],
            raw_cbor: None,
            header_cbor_size: None,
        };
        // 2 txs × 2 outputs = 4.
        assert_eq!(blk.count_tx_outputs(), 4);
    }

    #[test]
    fn block_count_tx_outputs_conway_dispatch() {
        let body = mk_conway_body_cbor();
        let blk = Block {
            era: Era::Conway,
            header: mk_block_header(600, 0),
            transactions: vec![mk_empty_tx_with_body(body)],
            raw_cbor: None,
            header_cbor_size: None,
        };
        // Single Conway tx with 4 outputs.
        assert_eq!(blk.count_tx_outputs(), 4);
    }

    #[test]
    fn block_count_tx_outputs_conway_multi_tx() {
        let body = mk_conway_body_cbor();
        let blk = Block {
            era: Era::Conway,
            header: mk_block_header(601, 0),
            transactions: vec![
                mk_empty_tx_with_body(body.clone()),
                mk_empty_tx_with_body(body.clone()),
                mk_empty_tx_with_body(body),
            ],
            raw_cbor: None,
            header_cbor_size: None,
        };
        // 3 txs × 4 outputs = 12.
        assert_eq!(blk.count_tx_outputs(), 12);
    }

    #[test]
    fn block_stats_renders_babbage_and_conway() {
        for (era, name) in [(Era::Babbage, "Babbage"), (Era::Conway, "Conway")] {
            let blk = Block {
                era,
                header: mk_block_header(1, 1),
                transactions: vec![],
                raw_cbor: None,
                header_cbor_size: None,
            };
            let stats = blk.block_stats();
            assert!(
                stats[2].contains(name),
                "era={era:?} expected 'era={name}' got {:?}",
                stats[2]
            );
        }
    }

    #[test]
    fn block_application_metrics_renders_babbage_and_conway() {
        let metrics = <Block as HasAnalysis>::block_application_metrics();
        for (era, name) in [(Era::Babbage, "Babbage"), (Era::Conway, "Conway")] {
            let blk = Block {
                era,
                header: mk_block_header(1000, 100),
                transactions: vec![mk_empty_tx_with_body(vec![]), mk_empty_tx_with_body(vec![])],
                raw_cbor: None,
                header_cbor_size: None,
            };
            let with_state =
                WithLedgerState::new(blk, CardanoLedgerStateValues, CardanoLedgerStateValues);
            assert_eq!((metrics[0].1)(&with_state).unwrap(), "1000");
            assert_eq!((metrics[1].1)(&with_state).unwrap(), "100");
            assert_eq!((metrics[2].1)(&with_state).unwrap(), name);
            assert_eq!((metrics[3].1)(&with_state).unwrap(), "2");
        }
    }
}
