//! Programmatic-introspection helpers for the kes-agent deferred
//! surfaces.
//!
//! R443 surfaces the upstream kes-agent daemon surfaces (socket
//! server protocol, KES key lifecycle, daemonization) as a
//! `*_status()` helper returning a structured descriptor.
//!
//! Mirrors the precedent set by R424-R429 cardano-tracer carve-outs
//! + R439-R442 sister-tool deferral sweeps.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation
//! infrastructure for the kes-agent deferred carve-outs.

/// Status descriptor for the deferred kes-agent daemon surface.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DaemonStatus {
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

/// Get the deferral-status descriptor for the upstream kes-agent
/// daemon surface — socket server, KES key lifecycle, and
/// start/stop/run/restart/status subcommand wiring.
pub fn daemon_status() -> DaemonStatus {
    DaemonStatus {
        status: "deferred",
        depends_on: "the kes-agent mini-arc per the playful-tickling-plum.md plan (R344-R354 — \
             highest-stakes parity since the socket protocol must be byte-equivalent or live \
             SPO setups break). The daemon depends on yggdrasil's KES key lifecycle in \
             crates/crypto/src/kes/ + sum_kes/ (which is already shipped) plus a \
             byte-equivalent server-side socket protocol that lands in the named mini-arc.",
        deferred_round: "R335+",
        upstream_reference: ".reference-haskell-cardano-node (post-R326b kes-agent vendor) — Cardano.KESAgent.Processes.{ServiceMain, ServiceClient, RunCommands} + daemonization wiring",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_status_describes_deferral() {
        let s = daemon_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("kes-agent mini-arc"));
        assert!(s.depends_on.contains("crates/crypto/src/kes"));
        assert!(s.upstream_reference.contains("ServiceMain"));
    }

    #[test]
    fn status_is_clone_eq_hash_round_trip() {
        let s1 = daemon_status();
        let s2 = s1.clone();
        assert_eq!(s1, s2);
        let mut set = std::collections::HashSet::new();
        set.insert(s1);
        assert!(set.contains(&s2));
    }
}
