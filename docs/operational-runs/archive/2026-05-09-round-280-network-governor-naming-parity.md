# Round 280 — `crates/network/src/governor/` parity sweep

**Date:** 2026-05-09
**Phase:** B (targeted renames + docstrings)
**Predecessor:** R279 (`docs/operational-runs/2026-05-09-round-279-runtime-naming-parity.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Add `## Naming parity` docstring stanzas to all 6 files in the
`crates/network/src/governor/` cluster (parent `governor.rs` + 5
sub-modules). The cluster is the post-R270 split of upstream
`Ouroboros.Network.PeerSelection.Governor.hs` (a single ~3000-line
module) into 5 sub-files along functional seams.

## Files affected

| File | Verdict | Mirror story |
|---|---|---|
| `governor.rs` | `(c) synthesis` | parent shell over 5 sub-modules; upstream is monolithic `Governor.hs` |
| `governor/types.rs` | `(c) synthesis` | combines `Governor/Types.hs` + `PeerSharing.hs` + `Bootstrap.hs` |
| `governor/state.rs` | `(c) synthesis` | combines `Governor/PeerSelectionState.hs` + `Monitor.hs` + `PeerSelectionActions.hs` |
| `governor/churn.rs` | `(c) partial-mirror` | mirrors `Churn.hs` plus folds in `BlockFetch.ConsensusInterface::FetchMode` + `Cardano.Node.Diffusion.mkReadFetchMode` |
| `governor/peer_metric.rs` | `(c) partial-mirror` | mirrors `PeerMetric.hs` plus folds in `LedgerPeers.Utils` + `Governor.RootPeers` |
| `governor/counters.rs` | `(c) synthesis` | combines `Governor/Types` view-layer + `ConnectionManager/Types::ConnectionManagerCounters` |

## Pattern: post-R270 governor split is a synthesis

The R270 arc (a–e) split upstream's monolithic `Governor.hs` into 5
sub-files for cohesion. Strict 1:1 file mirroring would require a
single `governor.rs` matching `Governor.hs` (a 3000-line monolith) —
but Yggdrasil's split improves readability without semantic loss.
Each sub-file declares its synthesis story explicitly:

- `types.rs` — names the 3 upstream files unified (Types + PeerSharing + Bootstrap).
- `state.rs` — names the 3 upstream files unified (PeerSelectionState + Monitor + PeerSelectionActions).
- `churn.rs` — names the canonical mirror (Churn.hs) + 2 fold-ins.
- `peer_metric.rs` — names the canonical mirror (PeerMetric.hs) + 2 fold-ins.
- `counters.rs` — names the 2 upstream view-layer files unified.

## Verdict bucket counts

| Bucket | Pre-R280 | Post-R280 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 53 | 52 (-1: peer_metric.rs re-graded from (a) to (c partial)) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 54 | 60 (+6 governor cluster resolved) |
| `(c-needed)` | 25 | 24 (-1: governor/counters.rs resolved) |
| `(NEEDS-REVIEW)` | 77 | 73 (-4: 4 governor files re-graded) |
| **TOTAL** | 209 | 209 |

network/governor cluster (6 files) is fully resolved with 0 (a) and 6
(c) verdicts.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 4.52s)
cargo lint                          clean (Finished `dev` profile in 10.68s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 dev/test/check-strict-mirror.py
                                    strict-mirror: 0 violations (clean)
```

## Diff stat

```text
crates/network/src/governor.rs            +9 lines (parity block)
crates/network/src/governor/churn.rs      +9 lines (partial-mirror block)
crates/network/src/governor/counters.rs   +10 lines (synthesis block)
crates/network/src/governor/peer_metric.rs +11 lines (partial-mirror block)
crates/network/src/governor/state.rs      +12 lines (synthesis block)
crates/network/src/governor/types.rs      +10 lines (synthesis block)
docs/strict-mirror-audit.tsv              rebuilt
docs/operational-runs/2026-05-09-round-280-... (new)
```

## Stop point — Phase B 5 of 6 closed

| Round | Cluster | Status |
|---|---|---|
| R276 | `crates/ledger/src/state/` (24 files) | ✅ closed |
| R277 | `consensus/{nonce,opcert,diffusion_pipelining}/` (9 files) | ✅ closed |
| R278 | `consensus/mempool/` (7 files) | ✅ closed |
| R279 | `node/src/runtime/` (18 files) | ✅ closed |
| R280 | `crates/network/src/governor/` (6 files) | ✅ closed |
| R281 | sweeper (residuals: storage, crypto, network non-governor, plutus, node top-level) | next |

R281 closes Phase B by handling the residual ~24 `(c-needed)` files
+ 73 `(NEEDS-REVIEW)` rows across the remaining clusters, plus the
`opcert.rs` -> `ocert.rs` rename surfaced by R274 discovery.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R279 (`docs/operational-runs/2026-05-09-round-279-runtime-naming-parity.md`)
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
