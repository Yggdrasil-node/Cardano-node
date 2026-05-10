//! Programmatic-introspection helpers for the snapshot-converter
//! deferred surfaces.
//!
//! R439 surfaces the upstream `convertSnapshot` + filesystem-watcher
//! daemon carve-outs as `*_status()` helpers returning structured
//! descriptors, mirroring the precedent set by the cardano-tracer
//! carve-out inventory (R424's `run_ekg_acceptor_status` /
//! `run_data_points_acceptor_status`, R427's `run_logs_rotator_status`,
//! R429's `tls_bind_plan_status`, etc.).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation
//! infrastructure for the snapshot-converter deferred carve-outs.

/// Status descriptor for the deferred mem↔lsm `convertSnapshot`
/// logic.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ConvertSnapshotStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// What this deferral depends on — the missing yggdrasil-side
    /// surface that needs to land before the conversion can run.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
    /// Pointer to the upstream Haskell entry point this surface
    /// would mirror.
    pub upstream_reference: &'static str,
}

/// Get the deferral-status descriptor for the
/// `convertSnapshot` mem↔lsm logic. Mirror of upstream's
/// `Ouroboros.Consensus.Cardano.SnapshotConversion.convertSnapshot`.
pub fn convert_snapshot_status() -> ConvertSnapshotStatus {
    ConvertSnapshotStatus {
        status: "deferred",
        depends_on: "yggdrasil-format LedgerStore reader/writer (a separate parity arc — \
             upstream's ledger-DB on-disk format differs from yggdrasil's storage layout, \
             so the conversion logic must be reimplemented over the yggdrasil-native \
             format rather than ported byte-for-byte)",
        deferred_round: "R363+",
        upstream_reference: ".reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/.../SnapshotConversion.hs::convertSnapshot",
    }
}

/// Status descriptor for the deferred filesystem-watcher daemon.
/// Mirror of upstream's `withManager` / `watchTree` from
/// `System.FSNotify`.
pub fn daemon_watcher_status() -> ConvertSnapshotStatus {
    ConvertSnapshotStatus {
        status: "deferred",
        depends_on: "the convert_snapshot logic itself (the daemon is a thin filesystem-watcher \
             loop around it — porting the watcher without a working converter would be \
             a no-op shell). Yggdrasil-side equivalent uses the `notify` crate which is \
             pure-Rust + license-compatible.",
        deferred_round: "R363+",
        upstream_reference: ".reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/app/snapshot-converter.hs (withManager/watchTree section)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_snapshot_status_describes_deferral() {
        let s = convert_snapshot_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("LedgerStore"));
        assert!(s.upstream_reference.contains("convertSnapshot"));
    }

    #[test]
    fn daemon_watcher_status_describes_deferral() {
        let s = daemon_watcher_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("convert_snapshot logic"));
        assert!(s.depends_on.contains("notify"));
        assert!(s.upstream_reference.contains("withManager"));
    }

    #[test]
    fn statuses_are_clone_eq_hash_round_trip() {
        let s1 = convert_snapshot_status();
        let s2 = s1.clone();
        assert_eq!(s1, s2);
        // Hash impl exists so callers can stash these in a HashSet
        // for de-dup if introspecting multiple carve-outs.
        let mut set = std::collections::HashSet::new();
        set.insert(s1);
        assert!(set.contains(&s2));
    }
}
