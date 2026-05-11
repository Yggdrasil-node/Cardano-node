//! Programmatic-introspection helpers for the db-analyser
//! deferred surfaces.
//!
//! R442 surfaces the upstream `Cardano.Tools.DBAnalyser.{HasAnalysis, Analysis, Run}` carve-outs as a `*_status()` helper returning a structured descriptor.
//!
//! Mirrors the precedent set by cardano-tracer's R424-R429
//! carve-out inventory + R439's snapshot-converter + R440's
//! kes-agent-control + R441's db-synthesizer.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation
//! infrastructure for the db-analyser deferred carve-outs.

/// Status descriptor for the deferred per-era HasAnalysis +
/// Analysis.hs dispatch surface.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AnalysisDispatchStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// What this deferral depends on — the missing yggdrasil-side
    /// surface that needs to land first.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
    /// Pointer to the upstream Haskell entry points this surface
    /// would mirror.
    pub upstream_reference: &'static str,
}

/// Get the dispatch-status descriptor for the upstream per-era
/// HasAnalysis surface + 13-variant Analysis.hs dispatch.
///
/// R481 closes the R475-R481 arc: 7 of 13 block-iteration-only
/// analyses ship (`ShowSlotBlockNo`, `CountBlocks`,
/// `CountTxOutputs`, `ShowBlockHeaderSize`, `ShowBlockTxsSize`,
/// `ShowEBBs`, `OnlyValidation`).
///
/// R485 carves out `CheckNoThunksEvery` as fundamentally not
/// portable to Rust (Haskell-only laziness/thunks concept); it
/// now returns `AnalysisError::NotApplicableToRust` rather than
/// the ledger-state apply-loop deferral.
///
/// R488 ships `TraceLedgerProcessing` via the
/// `yggdrasil_ledger::LedgerState::apply_block` seam — per-block
/// apply Ok/Err outcomes are captured into
/// `AnalysisOutcome::TraceLedgerProcessing` (forensic semantics:
/// apply failures don't abort the run; they're observable rows in
/// the per-block trace).
///
/// R489 ships `BenchmarkLedgerOps` via the same apply-loop seam
/// with `std::time::Instant` timing instrumentation, producing
/// per-block `SlotDataPoint` records (R374) populated with the
/// portable subset of timing fields (slot, slot_gap, total_time,
/// mut_block_apply, block_byte_size, block_stats); GHC-specific
/// timing fields stay zero-filled.
///
/// The remaining 3 (`StoreLedgerStateAt`, `ReproMempoolAndForge`,
/// `GetBlockApplicationMetrics`) require either:
/// - LedgerState serialization / SnapshotEncoded codec
///   (`StoreLedgerStateAt`); OR
/// - a richer ledger-state-delta `block_application_metrics`
///   surface (`GetBlockApplicationMetrics`); OR
/// - a mempool+forge integration (`ReproMempoolAndForge`).
///
/// They return a structured `RequiresLedgerStateApplyLoop` error
/// from `crate::analysis::runner::run_analysis` pending a future
/// implementation arc.
pub fn analysis_dispatch_status() -> AnalysisDispatchStatus {
    AnalysisDispatchStatus {
        status: "9-of-13-shipped",
        depends_on: "yggdrasil's ledger-state apply-loop bootstrap. The R475-R481 arc shipped 7/13 block-iteration-only analyses through the analysis::runner dispatch core; R485 carved out CheckNoThunksEvery as a permanent NotApplicableToRust (Haskell laziness/thunks have no Rust analog); R488 shipped TraceLedgerProcessing via the LedgerState::apply_block seam (forensic per-block apply Ok/Err trace); R489 shipped BenchmarkLedgerOps via the same seam plus std::time::Instant timing instrumentation (per-block SlotDataPoint records). The remaining 3 (StoreLedgerStateAt, ReproMempoolAndForge, GetBlockApplicationMetrics) need LedgerState snapshot serialization, mempool+forge integration, or a richer block_application_metrics body.",
        deferred_round: "R489",
        upstream_reference: ".reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/{HasAnalysis, Analysis, Run, Block/Byron, Block/Shelley, Block/Cardano}.hs",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_dispatch_status_describes_arc_outcome() {
        let s = analysis_dispatch_status();
        assert_eq!(s.status, "9-of-13-shipped");
        assert_eq!(s.deferred_round, "R489");
        assert!(s.depends_on.contains("R475-R481"));
        assert!(s.depends_on.contains("CheckNoThunksEvery"));
        assert!(s.depends_on.contains("NotApplicableToRust"));
        assert!(s.depends_on.contains("TraceLedgerProcessing"));
        assert!(s.depends_on.contains("BenchmarkLedgerOps"));
        assert!(s.depends_on.contains("LedgerState::apply_block"));
        assert!(s.upstream_reference.contains("HasAnalysis"));
        assert!(s.upstream_reference.contains("Analysis"));
    }

    #[test]
    fn status_is_clone_eq_hash_round_trip() {
        let s1 = analysis_dispatch_status();
        let s2 = s1.clone();
        assert_eq!(s1, s2);
        let mut set = std::collections::HashSet::new();
        set.insert(s1);
        assert!(set.contains(&s2));
    }
}
