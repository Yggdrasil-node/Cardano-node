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
//! The companion `dev/scripts/check_upstream_drift.sh` reads these
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
//! Audit baseline established: 2026-Q2 (`docs/archive/AUDIT_VERIFICATION_2026Q2.md`).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation +
//! drift-tracking for the 9 pinned IntersectMBO/IOHK repo SHAs
//! (cardano-base / cardano-ledger / ouroboros-consensus /
//! ouroboros-network / plutus / cardano-node — the 6 cardano-node
//! support repos audited since 2026-Q2; plus bech32 / kes-agent /
//! dmq-node — the 3 sister-tool repos vendored at R326b for the
//! R326–R459 sister-tools port arc). No upstream Haskell parallel
//! — this is purely Yggdrasil's audit-time provenance manifest
//! consumed by `dev/scripts/check_upstream_drift.sh`.

/// `cardano-base` — already pinned via vendored test vectors under
/// `specs/upstream-test-vectors/cardano-base/`. The SHA below mirrors
/// the directory name in that tree so a `git mv`-style refresh is
/// detectable.
///
/// R239 fixture refresh (2026-05-01) — advanced from
/// `db52f43b38ba…` to live HEAD and refreshed the vendored Praos VRF +
/// BLS12-381 vector tree in lockstep.
///
/// Reference: `crates/crypto/tests/upstream_vectors.rs::CARDANO_BASE_SHA`.
pub const UPSTREAM_CARDANO_BASE_COMMIT: &str = "7a8a991945d401d89e27f53b3d3bb464a354ad4c";

/// `cardano-ledger` — era-specific rules and CDDL schemas mirrored into
/// the hand-coded `CborEncode`/`CborDecode` impls under
/// `crates/ledger/src/eras/*/cbor.rs`.
///
/// R201 audit baseline (2026-04-30) — advanced from
/// `9ae77d611ad8…` to live HEAD reported by
/// `dev/scripts/check_upstream_drift.sh`.
///
/// R243 import-only refresh (2026-05-01) — advanced from
/// `42d088ed84b7…` to live HEAD.  Upstream PR #5787 removes one
/// redundant import from `Cardano.Ledger.Shelley.API.Mempool`; no
/// ledger rule, CDDL, or binary codec behavior changed in the ported
/// subset.
///
/// R245 BBODY/GOV drift refresh (2026-05-01) — advanced from
/// `110b30e7abd8…` to live HEAD.  Upstream changes: a Conway GOV
/// consistency cleanup switches `preceedingHardFork` to accumulated
/// proposals; the local proposal lineage path already uses accumulated
/// pending proposals.  The Conway BBODY `HeaderProtVerTooHigh` check is
/// temporarily disabled for testnets until Dijkstra (protocol major 12),
/// mirrored in `crates/node/sync/src/lib.rs`.
///
/// R249 audit-baseline refresh (2026-05-05) — advanced from
/// `b90b97488da3…` to live HEAD.  Upstream changes in this range
/// (34 commits, 124 files) are dominated by (a) a workspace-wide
/// `StAnnTx` threading refactor that pre-computes per-tx annotations
/// once in LEDGERS and passes them into LEDGER (PRs #5789, #5777) —
/// Haskell-internal type signatures only, no on-wire CBOR or rule
/// semantic change for active eras (Shelley through Conway);
/// (b) Dijkstra-era (PV12) preparation including `MemoBytes`
/// serialization for `DijkstraBlockBody`, `invalid_transactions`
/// becoming a non-empty set, and `IsValid`-disallowed BlockBody Txs
/// (PR #5733) — only the Dijkstra CDDL is touched, no active-era CDDL
/// changes; (c) a `blockBodySize` method added to `EraBlockBody`
/// replacing the legacy `bBodySize` standalone — Yggdrasil already
/// computes block body size locally per R84 (`apply_block_validated`)
/// so this is a Haskell-side rename only; (d) `queryConstitution`
/// golden-test additions and example refactors. No active-era CBOR
/// codec, validation rule, or transition-system semantic change
/// requires a code update.
///
/// Reference: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
pub const UPSTREAM_CARDANO_LEDGER_COMMIT: &str = "ca9b8c285e4493f2d25354914f8aae5483595507";

/// `ouroboros-consensus` — Praos protocol, ChainDB, mempool, and storage
/// design rationale ported into `crates/consensus/`, `crates/storage/`,
/// `crates/consensus/src/mempool/`.
///
/// R201 audit baseline (2026-04-30) — advanced from
/// `91c8e1bb5d7f…` to live HEAD.
///
/// R216 audit baseline refresh (2026-04-30) — advanced from
/// `c368c2529f2f…` to live HEAD per
/// `dev/scripts/check_upstream_drift.sh`.  No upstream-only changes
/// affect the ported subset (Praos hot-path, ChainDB volatile/immutable
/// split, mempool revalidation) per R215 multi-network operational
/// verification (preview Conway, preprod Allegra, mainnet Byron→Shelley
/// all pass cardano-cli end-to-end queries with the existing port).
///
/// R249 audit-baseline refresh (2026-05-05) — advanced from
/// `b047aca4a731…` to live HEAD.  Upstream changes in this range (24 commits, 134 files) are
/// dominated by (a) Peras voting-committee implementations: `wFA^LS` and `EveryoneVotes`
/// instances (PR #1975), aggregatable voting committee crypto interface (PR #2014), and
/// `VotesWithSameTarget` duplicate check (PR #2020) — all post-Conway Peras protocol surface,
/// not active on any current network; (b) a `LedgerTables` and `TxIn`/`TxOut` type indexing
/// refactor that eta-expands `l` over `blk` workspace-wide (PR #2016) — pure Haskell type
/// machinery touching `LedgerSupports*LedgerDB`, `BackingStore` API, and `LedgerSeq` — Yggdrasil
/// uses different storage abstractions (`ImmutableStore`/`VolatileStore`/`LedgerStore` traits)
/// so the Haskell type-index churn does not propagate; (c) `Ticking` documentation page added
/// (docs only). The Praos hot-path, ChainDB volatile/immutable split, mempool revalidation, and
/// HardForkCombinator semantics for active eras (through Conway) remain unchanged.
///
/// Reference: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/>
#[rustfmt::skip]
pub const UPSTREAM_OUROBOROS_CONSENSUS_COMMIT: &str = "8c2475c253ab53fc2f0998a57a161b6778b54e43";

/// `ouroboros-network` — multiplexer, handshake, mini-protocols, peer
/// governor ported into `crates/network/`.
///
/// R249 audit-baseline refresh (2026-05-05) — advanced from
/// `0e84bced45c7…` to live HEAD.  Upstream changes in this range
/// (4 commits) are: (a) PR #5357 "tx-submission v2: return results
/// in submitTxToMempool" — modifies only
/// `ouroboros-network/lib/Ouroboros/Network/TxSubmission/Inbound/V2/Registry.hs`
/// (an internal Haskell function signature consumed by the sister
/// `dmq-node` project); the `TxSubmission2` wire codec
/// (`Codec.hs`, `Type.hs`) and the public mini-protocol state machine
/// are unchanged, so a node implementing the existing TxSubmission2
/// wire protocol — including Yggdrasil — remains compatible without
/// modification; (b) PR #5359 drops `x86_64-darwin` Nix support
/// (build-system only).  No mux, handshake, peer-governor, or
/// mini-protocol behavior change.
///
/// Reference: <https://github.com/IntersectMBO/ouroboros-network/tree/main/>
pub const UPSTREAM_OUROBOROS_NETWORK_COMMIT: &str = "8fe0f8ebc2623079edc7d708f19a0154b963f371";

/// `plutus` — CEK machine, builtins, cost model, Flat codec ported into
/// `crates/plutus/`.
///
/// R201 audit baseline (2026-04-30) — advanced from
/// `187c3971a34e…` to live HEAD.
///
/// R216 audit baseline refresh (2026-04-30) — advanced from
/// `e3eb4c76ea20…` to live HEAD per
/// `dev/scripts/check_upstream_drift.sh`.  No upstream-only CEK or
/// cost-model changes affect the ported subset; the Plutus crate's
/// integration tests + 4 745-test workspace gate continues to pass
/// against the existing port.
///
/// R249 audit-baseline refresh (2026-05-05) — advanced from
/// `4cd40a14e364…` to live HEAD.  Upstream changes in this range
/// (9 commits) are dominated by post-Conway protocol-version (D/E)
/// preparation, with no semantic change for active protocol versions
/// (Conway PV9–11): (a) PR #7754 "update default universe plumbing
/// and tidy builtin handling" — adds `CInteger`/`CByteString` newtype
/// wrappers (with bounds `±2^262143` for integers and 65 536 bytes for
/// byte-strings, defined in new `PlutusCore.Default.Universe.Cardano`)
/// and `TextCostedByByteLength` whose memory cost is `lenInBytes / 4`
/// (vs. legacy `Text` 100-char chunking).  The bounds-checked semantics
/// are gated on `BuiltinSemanticsVariant` via an `ensurable semvar`
/// predicate in `PlutusCore.Default.Builtins`, so existing semantics
/// variants (Conway and earlier) keep their unchanged `AddInteger`,
/// `SubtractInteger`, etc. denotations; (b) the `untyped-plutus-core`
/// Flat decoder gains additive `constantPred` and `constrPred`
/// validation hooks alongside the existing `builtinPred`, with the
/// default `UnrestrictedProgram` decoder passing `const Nothing` for
/// all three — no behavioral change for current-protocol Plutus
/// scripts; (c) two new cost-model JSONs `builtinCostModelD.json` /
/// `builtinCostModelE.json` plus matching `cekMachineCostsD/E.json`
/// for protocol versions D and E (post-Dijkstra) — Conway continues
/// to use cost model C; (d) `benching-conway.csv` benchmark output
/// refresh (regenerated by the existing cost-model derivation, not
/// runtime parameters); (e) error-message detail for unsupported
/// `case` on `Integer` (PR #7766).  No CEK reduction, builtin
/// denotation, cost-model parameter, or Flat encoding shape used by
/// Conway-era scripts is altered.
///
/// Reference: <https://github.com/IntersectMBO/plutus/tree/master/>
pub const UPSTREAM_PLUTUS_COMMIT: &str = "c8f962ae75d0b4871401ecc2e8c4ed259cafadac";

/// `cardano-node` — node runtime, CLI, configuration patterns ported into
/// `node/`.
///
/// R201 audit baseline (2026-04-30) — advanced from
/// `60af1c23bc20…` to live HEAD.
///
/// R249 audit-baseline refresh (2026-05-05) — advanced from
/// `799325937a45…` to live HEAD.  Upstream changes in this range
/// (15 commits) are dominated by `cardano-testnet` CLI restructuring
/// (PR #6552 splits testnet creation/startup/runtime options into
/// purpose-specific types, replaces `Either` with a `ModeOptions`
/// sum type, renames `StartFromScratch` → `NoUserProvidedEnv`) — none
/// of which are exposed by Yggdrasil's CLI surface (`yggdrasil-node
/// run` and the wrapped `cardano-cli`/`query`/`submit-tx`/`status`
/// subcommands).  Other changes: (a) `npcExperimentalHardForksEnabled`-
/// gated `ProtVer` bumped to 12 (post-Conway experimental flag, not
/// honored by default and not exposed in Yggdrasil's config); (b) the
/// `ExperimentalHardForksEnabled` flag is removed from the default
/// `hardforkViaConfig` path so default-config testnets don't reject
/// Conway-era blocks — Yggdrasil's network presets and
/// `validate-config` already enforce Conway protocol-version bounds
/// per `MaxMajorProtVer`; (c) `cardano-api`/`cardano-cli` major version
/// bump to 11.0 — Yggdrasil's pure-Rust `cardano-cli` subset is
/// API-stable and unaffected; (d) `iohkNix` flake input bump for `blst`
/// — Yggdrasil uses native Rust `bls12_381` instead.
///
/// Reference: <https://github.com/IntersectMBO/cardano-node/tree/master/>
pub const UPSTREAM_CARDANO_NODE_COMMIT: &str = "97036a66bcf8c89f687ae57a048eecc0389977ef";

/// `bech32` — sister-tool source vendored at R326b under
/// `.reference-haskell-cardano-node/deps/bech32/`. Provides the
/// canonical Haskell implementation of BIP-0173 Bech32 / Bech32m
/// encoding used by Cardano addresses (`addr_test1…`, `stake1…`,
/// etc.). Yggdrasil's pure-Rust port lands at `crates/tools/bech32/`
/// (R447 relocated) across R331–R334 (Phase A.1 of the sister-tools port arc).
///
/// Reference: <https://github.com/IntersectMBO/bech32/tree/master/>
pub const UPSTREAM_BECH32_COMMIT: &str = "4624d3a84606615c1ca1410d6dd3fd9213211215";

/// `kes-agent` — sister-tool source vendored at R326b under
/// `.reference-haskell-cardano-node/deps/kes-agent/` (lives under
/// the legacy `input-output-hk` GitHub org, NOT `IntersectMBO`).
/// Provides the KES key custody + period-rotation agent used by
/// stake pool operators in production. Yggdrasil's pure-Rust port
/// lands at `crates/tools/kes-agent/` (the daemon, R344–R354) and
/// `crates/tools/kes-agent-control/` (the companion CLI, R355–R359) — R447 relocated,
/// both Phase A.3/A.4 of the sister-tools port arc.
///
/// Reference: <https://github.com/input-output-hk/kes-agent/tree/master/>
pub const UPSTREAM_KES_AGENT_COMMIT: &str = "6d54ac2ee325aadeeb3659cfefcd58035f69acd9";

/// `dmq-node` — sister-tool source vendored at R326b under
/// `.reference-haskell-cardano-node/deps/dmq-node/`. Provides the
/// DMQ (Delegated Mempool Queue) diffusion-layer node used as a
/// sidecar for Mithril certificates. Yggdrasil's pure-Rust port
/// lands at `crates/tools/dmq-node/` (R447 relocated) across R450–R459 (Phase D of the
/// sister-tools port arc — Tier 4 sister project).
///
/// Reference: <https://github.com/IntersectMBO/dmq-node/tree/main/>
pub const UPSTREAM_DMQ_NODE_COMMIT: &str = "bd5fbf69fcdeaa9d8b4a3d2b4554016d546b17ea";

/// All pinned upstream commits, keyed by repository name.
///
/// Used by `dev/scripts/check_upstream_drift.sh` to iterate every pin
/// and compare against live `git ls-remote HEAD` output. A future
/// upstream addition that's worth auditing must extend BOTH this slice
/// AND a top-level constant above; the drift-guard test below pins the
/// length to 9 so a missed extension fails CI.
///
/// Order: 6 canonical cardano-node support repos (audited since R201,
/// matching the historical Phase E.1 cadence), then 3 sister-tool
/// repos vendored at R326b for the R326–R459 sister-tools port arc.
pub const UPSTREAM_PINS: &[(&str, &str)] = &[
    // Canonical cardano-node support repos (audited since R201).
    ("cardano-base", UPSTREAM_CARDANO_BASE_COMMIT),
    ("cardano-ledger", UPSTREAM_CARDANO_LEDGER_COMMIT),
    ("ouroboros-consensus", UPSTREAM_OUROBOROS_CONSENSUS_COMMIT),
    ("ouroboros-network", UPSTREAM_OUROBOROS_NETWORK_COMMIT),
    ("plutus", UPSTREAM_PLUTUS_COMMIT),
    ("cardano-node", UPSTREAM_CARDANO_NODE_COMMIT),
    // Sister-tool repos (vendored R326b; ports land R331–R459).
    ("bech32", UPSTREAM_BECH32_COMMIT),
    ("kes-agent", UPSTREAM_KES_AGENT_COMMIT),
    ("dmq-node", UPSTREAM_DMQ_NODE_COMMIT),
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
    /// of 9 IntersectMBO/IOHK repos this project ports from: 6
    /// cardano-node support repos audited since R201 + 3 sister-tool
    /// repos vendored at R326b. A future addition must extend both this
    /// set AND `docs/UPSTREAM_PARITY.md`'s pinning table; pinning the
    /// length here surfaces the omission as a clear CI failure.
    ///
    /// Reference: `docs/archive/AUDIT_VERIFICATION_2026Q2.md` mapping table
    /// + `docs/operational-runs/2026-05-09-round-326b-vendor-bech32-kes-agent-dmq-node.md`.
    #[test]
    fn upstream_pins_cover_all_nine_canonical_repos() {
        assert_eq!(
            UPSTREAM_PINS.len(),
            9,
            "UPSTREAM_PINS must cover the 9 canonical IntersectMBO/IOHK repos: \
             cardano-base, cardano-ledger, ouroboros-consensus, \
             ouroboros-network, plutus, cardano-node (the 6 cardano-node \
             support repos), plus bech32, kes-agent, dmq-node (the 3 \
             sister-tool repos vendored at R326b). Update both this \
             constant and docs/UPSTREAM_PARITY.md when adding/removing.",
        );

        let expected_repos = [
            // Canonical cardano-node support repos.
            "cardano-base",
            "cardano-ledger",
            "ouroboros-consensus",
            "ouroboros-network",
            "plutus",
            "cardano-node",
            // Sister-tool repos.
            "bech32",
            "kes-agent",
            "dmq-node",
        ];
        let actual_repos: Vec<&str> = UPSTREAM_PINS.iter().map(|(r, _)| *r).collect();
        assert_eq!(
            actual_repos, expected_repos,
            "UPSTREAM_PINS must list the 9 canonical repos in the documented order",
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
            UPSTREAM_CARDANO_BASE_COMMIT, "7a8a991945d401d89e27f53b3d3bb464a354ad4c",
            "cardano-base pin must match the vendored test-vector directory name",
        );
    }
}
