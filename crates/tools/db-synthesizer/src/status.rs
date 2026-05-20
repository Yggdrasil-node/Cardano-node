//! Programmatic-introspection helpers for the db-synthesizer forge surface.
//!
//! The forge-loop control path, `preOpenChainDB` supervisor, genesis
//! loading, consensus-protocol construction, leader credentials, evolving
//! ledger / nonce state, and Praos leader-check + KES-signed block forge
//! are now wired through the production path. The leader-check stake
//! fraction is derived from the rotating ledger-view stake snapshots
//! before calling `checkShouldForge`.
//!
//! Mirrors the precedent set by cardano-tracer's R424-R429
//! carve-out inventory + snapshot-converter + kes-agent-control.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation
//! infrastructure for the db-synthesizer parity status surface.

/// Status descriptor for the forge surface.
///
/// Praos leader checking, stake-based sigma, and KES-signed block forging
/// are live. This descriptor tracks the remaining ChainDB byte-equivalence
/// soak.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ForgeLoopStatus {
    /// One-line summary of what is implemented vs. deferred.
    pub status: &'static str,
    /// What the remaining deferred axis depends on.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
    /// Pointer to the upstream Haskell entry points this surface mirrors.
    pub upstream_reference: &'static str,
}

/// Get the status descriptor for the db-synthesizer forge surface.
///
/// R3c-4 wires the production path to the shared block-producer
/// `checkShouldForge` / `forgeBlock` equivalents. R3c-5 derives the
/// epoch stake distribution from the ledger-view snapshots.
pub fn forge_loop_status() -> ForgeLoopStatus {
    ForgeLoopStatus {
        status: "functional - forge control loop + ChainDB supervisor + genesis loading \
                 + consensus protocol + leader credentials + stake-based Praos \
                 leader-check/KES forge live; ChainDB byte-equivalence soak remains",
        depends_on: "integration byte-equivalence soak against upstream db-synthesizer \
                     ChainDB output.",
        deferred_round: "Phase 4 db-synthesizer closeout soak",
        upstream_reference: ".reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/{Forging,Run}.hs",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_loop_status_describes_functional_state() {
        let s = forge_loop_status();
        assert!(s.status.contains("functional"));
        assert!(s.status.contains("Praos leader-check"));
        assert!(s.status.contains("KES forge"));
        assert!(s.depends_on.contains("byte-equivalence"));
        assert!(s.deferred_round.contains("closeout"));
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
