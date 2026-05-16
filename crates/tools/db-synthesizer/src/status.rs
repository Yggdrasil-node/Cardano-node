//! Programmatic-introspection helpers for the db-synthesizer
//! partially-deferred forge surface.
//!
//! The forge-loop *control path* and the `preOpenChainDB` supervisor
//! are implemented as of the Phase 4 R1 slice ([`crate::forging`] +
//! [`crate::run`]). What remains deferred is the **Praos-forging**
//! axis — the per-slot VRF/KES/OpCert leader check and genesis
//! loading. This helper surfaces that surviving carve-out as a
//! structured descriptor.
//!
//! Mirrors the precedent set by cardano-tracer's R424-R429
//! carve-out inventory + snapshot-converter + kes-agent-control.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation
//! infrastructure for the db-synthesizer deferred carve-outs.

/// Status descriptor for the partially-deferred forge surface.
///
/// The deterministic non-Praos structural forge loop is live; this
/// descriptor tracks the *remaining* Praos-forging + genesis-loading
/// work that upstream's `Cardano.Tools.DBSynthesizer.{Forging, Run}`
/// performs (the VRF/KES/OpCert leader check fed by a parsed
/// `ShelleyGenesis`).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ForgeLoopStatus {
    /// One-line summary of what is implemented vs. deferred.
    pub status: &'static str,
    /// What the remaining deferred axis depends on — the missing
    /// yggdrasil-side surface that needs to land first.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
    /// Pointer to the upstream Haskell entry points this surface
    /// would mirror.
    pub upstream_reference: &'static str,
}

/// Get the status descriptor for the db-synthesizer forge surface.
///
/// As of the Phase 4 R1 slice the forge *control loop* + ChainDB
/// supervisor are live (`forging.rs` / `run.rs`); the Praos-forging
/// path (VRF/KES/OpCert leader check + genesis loading) is the
/// surviving carve-out reported here.
pub fn forge_loop_status() -> ForgeLoopStatus {
    ForgeLoopStatus {
        status: "partial — forge control loop + preOpenChainDB supervisor live (Phase 4 R1); \
                 Praos-forging path deferred",
        depends_on: "Genesis loading (db-synthesizer R2: wire orphans::parse_node_config_stub \
                     into Run.initialize + parse ShelleyGenesis for the real epoch length) \
                     and the Praos-forging path (db-synthesizer R3: leverage \
                     crates/node/block-producer for the per-slot VRF/KES/OpCert leader check \
                     + KES-signed header). The R1 slice forges deterministic non-Praos \
                     structural blocks against a stubbed epoch size.",
        deferred_round: "R2/R3 of the Phase 4 db-synthesizer sister-tool arc",
        upstream_reference: ".reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/{Forging,Run}.hs",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_loop_status_describes_partial_state() {
        let s = forge_loop_status();
        assert!(s.status.contains("partial"));
        assert!(s.status.contains("Praos-forging"));
        assert!(s.depends_on.contains("Genesis loading"));
        assert!(s.depends_on.contains("block-producer"));
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
