//! Programmatic-introspection helpers for the db-synthesizer
//! deferred surfaces.
//!
//! R441 surfaces the upstream `Cardano.Tools.DBSynthesizer.Forging`
//! + `Run.hs` carve-outs as a `*_status()` helper returning a structured descriptor.
//!
//! Mirrors the precedent set by cardano-tracer's R424-R429
//! carve-out inventory + R439's snapshot-converter + R440's
//! kes-agent-control.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation
//! infrastructure for the db-synthesizer deferred carve-outs.

/// Status descriptor for the deferred forge-loop + Run.hs
/// supervisor. Mirror of upstream's
/// `Cardano.Tools.DBSynthesizer.{Forging, Run}` — the per-block
/// forging loop that consumes a chain config + writes synthetic
/// blocks to a ChainDB.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ForgeLoopStatus {
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

/// Get the deferral-status descriptor for the upstream forge-loop
/// + Run.hs supervisor.
pub fn forge_loop_status() -> ForgeLoopStatus {
    ForgeLoopStatus {
        status: "deferred",
        depends_on: "Phase C authorization checkpoint per the playful-tickling-plum.md plan (Phase C \
             entry — db-synthesizer at R408-R415 — is hard-gated on the cardano-cli MVS \
             completing in the parallel C-arc; once unlocked, the forge loop can leverage \
             node/src/block_producer.rs for the actual block-construction logic).",
        deferred_round: "R364+",
        upstream_reference: ".reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/{Forging,Run}.hs",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_loop_status_describes_deferral() {
        let s = forge_loop_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("Phase C"));
        assert!(s.depends_on.contains("block_producer"));
        assert!(s.upstream_reference.contains("Forging"));
        // The reference uses brace-expansion for the Forging+Run pair.
        assert!(s.upstream_reference.contains("Run"));
    }

    #[test]
    fn status_is_clone_eq_hash_round_trip() {
        let s1 = forge_loop_status();
        let s2 = s1.clone();
        assert_eq!(s1, s2);
        let mut set = std::collections::HashSet::new();
        set.insert(s1);
        assert!(set.contains(&s2));
    }
}
