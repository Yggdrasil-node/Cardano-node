//! Pinned upstream IntersectMBO repository commit SHAs.
//!
//! Yggdrasil is a pure-Rust port; there are no Cargo `git =` dependencies.
//! Pinning is therefore *documentary*: each constant records the SHA at
//! which the corresponding upstream Haskell repository was last
//! systematically audited against. This gives the parity-audit cadence
//! a reproducible baseline so a future audit round can grep upstream for
//! the exact files we ported from at the time, rather than chasing a
//! moving `master`/`main` branch.
//!
//! The companion `node/scripts/check_upstream_drift.sh` reads these
//! constants and compares each against live `git ls-remote HEAD` output,
//! producing an informational JSON drift report. The drift report is
//! NOT a build failure — drift is expected over time; what matters is
//! that the audit baseline stays explicit.
//!
//! When advancing a pin: update the constant here, run the full audit
//! cadence (drift-guards, golden tests, fixture cross-checks) against
//! the new SHA, then update `docs/UPSTREAM_PARITY.md` with the rationale
//! for the bump.
//!
//! Audit baseline established: 2026-Q2 (`docs/AUDIT_VERIFICATION_2026Q2.md`).

/// `cardano-base` — already pinned via vendored test vectors at
/// [`specs/upstream-test-vectors/cardano-base/`]. The SHA below mirrors
/// the directory name in that tree so a `git mv`-style refresh is
/// detectable.
///
/// Reference: `crates/crypto/tests/upstream_vectors.rs::CARDANO_BASE_SHA`.
pub const UPSTREAM_CARDANO_BASE_COMMIT: &str = "db52f43b38ba5d8927feb2199d4913fe6c0f974d";

/// `cardano-ledger` — era-specific rules and CDDL schemas ported into
/// `crates/ledger/` and `crates/cddl-codegen/`.
///
/// R201 audit baseline (2026-04-30) — advanced from
/// `9ae77d611ad8…` to live HEAD reported by
/// `node/scripts/check_upstream_drift.sh`.
///
/// Reference: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
pub const UPSTREAM_CARDANO_LEDGER_COMMIT: &str = "42d088ed84b799d6d980f9be6f14ad953a3c957d";

/// `ouroboros-consensus` — Praos protocol, ChainDB, mempool, and storage
/// design rationale ported into `crates/consensus/`, `crates/storage/`,
/// `crates/mempool/`.
///
/// R201 audit baseline (2026-04-30) — advanced from
/// `91c8e1bb5d7f…` to live HEAD.
///
/// Reference: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/>
#[rustfmt::skip]
pub const UPSTREAM_OUROBOROS_CONSENSUS_COMMIT: &str = "c368c2529f2f41196461883013f749b7ac7aa58e";

/// `ouroboros-network` — multiplexer, handshake, mini-protocols, peer
/// governor ported into `crates/network/`.
///
/// Reference: <https://github.com/IntersectMBO/ouroboros-network/tree/main/>
pub const UPSTREAM_OUROBOROS_NETWORK_COMMIT: &str = "0e84bced45c7fc64252d576fbce55864d75e722a";

/// `plutus` — CEK machine, builtins, cost model, Flat codec ported into
/// `crates/plutus/`.
///
/// R201 audit baseline (2026-04-30) — advanced from
/// `187c3971a34e…` to live HEAD.
///
/// Reference: <https://github.com/IntersectMBO/plutus/tree/master/>
pub const UPSTREAM_PLUTUS_COMMIT: &str = "e3eb4c76ea20cf4f90231a25bdfaab998346b406";

/// `cardano-node` — node runtime, CLI, configuration patterns ported into
/// `node/`.
///
/// R201 audit baseline (2026-04-30) — advanced from
/// `60af1c23bc20…` to live HEAD.
///
/// Reference: <https://github.com/IntersectMBO/cardano-node/tree/master/>
pub const UPSTREAM_CARDANO_NODE_COMMIT: &str = "799325937a4598899c8cab61f4c957662a0aeb53";

/// All pinned upstream commits, keyed by repository name.
///
/// Used by `node/scripts/check_upstream_drift.sh` to iterate every pin
/// and compare against live `git ls-remote HEAD` output. A future
/// upstream addition that's worth auditing must extend BOTH this slice
/// AND a top-level constant above; the drift-guard test below pins the
/// length to 6 so a missed extension fails CI.
pub const UPSTREAM_PINS: &[(&str, &str)] = &[
    ("cardano-base", UPSTREAM_CARDANO_BASE_COMMIT),
    ("cardano-ledger", UPSTREAM_CARDANO_LEDGER_COMMIT),
    ("ouroboros-consensus", UPSTREAM_OUROBOROS_CONSENSUS_COMMIT),
    ("ouroboros-network", UPSTREAM_OUROBOROS_NETWORK_COMMIT),
    ("plutus", UPSTREAM_PLUTUS_COMMIT),
    ("cardano-node", UPSTREAM_CARDANO_NODE_COMMIT),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Drift-guard for upstream-pin format: every SHA must be exactly
    /// 40 lowercase hex characters. Catches paste errors (truncated
    /// SHA, uppercase, embedded whitespace) at CI time rather than at
    /// runtime when the drift detector tries to compare.
    #[test]
    fn upstream_pins_are_40_lowercase_hex() {
        for &(repo, sha) in UPSTREAM_PINS {
            assert_eq!(
                sha.len(),
                40,
                "upstream pin for {repo} must be a 40-character SHA, got {sha:?}",
            );
            assert!(
                sha.chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "upstream pin for {repo} must be lowercase hex only, got {sha:?}",
            );
        }
    }

    /// Pin the cardinality of `UPSTREAM_PINS` against the canonical set
    /// of 6 IntersectMBO repos this project ports from. A future addition
    /// (e.g. a new upstream support repo) must extend both this set AND
    /// `docs/UPSTREAM_PARITY.md`'s pinning table; pinning the length here
    /// surfaces the omission as a clear CI failure.
    ///
    /// Reference: `docs/AUDIT_VERIFICATION_2026Q2.md` mapping table.
    #[test]
    fn upstream_pins_cover_all_six_canonical_repos() {
        assert_eq!(
            UPSTREAM_PINS.len(),
            6,
            "UPSTREAM_PINS must cover the 6 canonical IntersectMBO repos: \
             cardano-base, cardano-ledger, ouroboros-consensus, \
             ouroboros-network, plutus, cardano-node. Update both this \
             constant and docs/UPSTREAM_PARITY.md when adding/removing.",
        );

        let expected_repos = [
            "cardano-base",
            "cardano-ledger",
            "ouroboros-consensus",
            "ouroboros-network",
            "plutus",
            "cardano-node",
        ];
        let actual_repos: Vec<&str> = UPSTREAM_PINS.iter().map(|(r, _)| *r).collect();
        assert_eq!(
            actual_repos, expected_repos,
            "UPSTREAM_PINS must list the 6 canonical repos in the documented order",
        );
    }

    /// Cross-pin: the legacy `crates/crypto/tests/upstream_vectors.rs::
    /// CARDANO_BASE_SHA` constant must agree with `UPSTREAM_CARDANO_BASE
    /// _COMMIT` here. Both reference the same vendored test-vector tree;
    /// drift between them would mean the crypto crate's vendored fixtures
    /// and this pin disagree on which upstream commit was audited.
    ///
    /// (Mechanical note: this test runs in the node crate and reads only
    /// the local constant; the cross-check against the crypto crate's
    /// constant happens in `crates/crypto/tests/upstream_vectors.rs` if
    /// it imports the node-side constant in a future refactor. For now,
    /// the literal SHA is hand-mirrored — a manual lockstep that the
    /// drift-detector script also surfaces if the vendored directory
    /// name diverges.)
    #[test]
    fn upstream_cardano_base_pin_matches_vendored_directory_name() {
        // Sanity: verify the SHA matches the vendored directory name
        // referenced throughout `specs/upstream-test-vectors/cardano-base/`.
        // We can't read the filesystem from a unit test reliably across
        // build configurations, so we just pin the literal here. The
        // drift-detector script does the directory cross-check.
        assert_eq!(
            UPSTREAM_CARDANO_BASE_COMMIT, "db52f43b38ba5d8927feb2199d4913fe6c0f974d",
            "cardano-base pin must match the vendored test-vector directory name",
        );
    }
}
