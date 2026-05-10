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

/// Get the deferral-status descriptor for the upstream
/// per-era HasAnalysis surface + 13-variant Analysis.hs dispatch.
pub fn analysis_dispatch_status() -> AnalysisDispatchStatus {
    AnalysisDispatchStatus {
        status: "deferred",
        depends_on: "yggdrasil's per-era ImmutableStore block-iteration surface (Block/{Byron, Shelley, Cardano} traits + the 13-variant Analysis name dispatch) — the analysis logic is large (1057 upstream lines) and depends on era-specific block deserialization which spans crates/ledger/src/eras/*. Tracked under Phase B.2 (R391-R400) per the playful-tickling-plum.md plan.",
        deferred_round: "R365+",
        upstream_reference: ".reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/{HasAnalysis, Analysis, Run, Block/Byron, Block/Shelley, Block/Cardano}.hs",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_dispatch_status_describes_deferral() {
        let s = analysis_dispatch_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("ImmutableStore"));
        assert!(s.depends_on.contains("Phase B.2"));
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
