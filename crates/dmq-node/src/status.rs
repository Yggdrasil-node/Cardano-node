//! Programmatic-introspection helpers for the dmq-node deferred
//! surfaces.
//!
//! R444 surfaces the Diffusion / NodeKernel / PeerSelection wiring carve-outs as a `*_status()` helper.
//!
//! Mirrors the precedent set by R424-R429 cardano-tracer +
//! R439-R443 sister-tool deferral sweeps.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation
//! infrastructure for the dmq-node deferred carve-outs.

/// Status descriptor for the deferred dmq-node Diffusion +
/// NodeKernel + PeerSelection surface.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DiffusionWiringStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// What this deferral depends on.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
    /// Pointer to the upstream Haskell entry points this surface
    /// would mirror.
    pub upstream_reference: &'static str,
}

/// Get the deferral-status descriptor for the upstream dmq-node
/// Diffusion/NodeKernel/PeerSelection wiring.
pub fn diffusion_wiring_status() -> DiffusionWiringStatus {
    DiffusionWiringStatus {
        status: "deferred",
        depends_on: "the dmq-node mini-arc per the playful-tickling-plum.md plan (R450-R459 — Tier 4 sister project). The wiring leverages crates/network/'s existing Diffusion / NodeKernel / PeerSelection surfaces (already shipped) but needs the dmq-specific wire protocol + local-socket server (per R455+ of the plan).",
        deferred_round: "R361+",
        upstream_reference: ".reference-haskell-cardano-node (post-R326b dmq-node vendor) — DMQ.Node.{Diffusion, Run, NodeKernel} + the dmq mempool-queue wire protocol",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffusion_wiring_status_describes_deferral() {
        let s = diffusion_wiring_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("dmq-node mini-arc"));
        assert!(s.depends_on.contains("R450-R459"));
        assert!(s.upstream_reference.contains("DMQ.Node"));
    }

    #[test]
    fn status_is_clone_eq_hash_round_trip() {
        let s1 = diffusion_wiring_status();
        let s2 = s1.clone();
        assert_eq!(s1, s2);
        let mut set = std::collections::HashSet::new();
        set.insert(s1);
        assert!(set.contains(&s2));
    }
}
