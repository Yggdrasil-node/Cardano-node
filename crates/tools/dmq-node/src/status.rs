//! Programmatic-introspection helpers for the dmq-node deferred
//! surfaces.
//!
//! R444 surfaced the initial Diffusion / NodeKernel / PeerSelection wiring
//! carve-out as a `*_status()` helper. R717-R816 filled in the protocol,
//! inbound governor, NodeKernel helper records, and NtN/NtC mux bundles; the
//! remaining explicit deferral is the final `run()` event-loop assembly.
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
        depends_on: "the R717-R816 dmq-node implementation arc has shipped the DMQ protocol surface, inbound-V2 governor, NodeKernel records, peer-sharing/KeepAlive/DeltaQ helpers, and NtN/NtC mux bundles. Remaining work is the run() event loop: socket accept loops, handshakes, mux driver startup, and protocol-task wiring through crates/network.",
        deferred_round: "R817+",
        upstream_reference: ".reference-haskell-cardano-node (post-R326b dmq-node vendor) — DMQ.Node.{Run,Diffusion,NodeKernel,NodeToNode,NodeToClient,Tracer} plus the DMQ protocol and inbound-governor modules",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffusion_wiring_status_describes_deferral() {
        let s = diffusion_wiring_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("R717-R816"));
        assert!(s.depends_on.contains("run() event loop"));
        assert_eq!(s.deferred_round, "R817+");
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
