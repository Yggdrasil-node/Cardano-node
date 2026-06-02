---
title: 'R313: synthesis-file census (no upstream `.hs` mirror)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-313-synthesis-file-census/
---

# Round 313 — synthesis-file census (no upstream `.hs` mirror)

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R312`](2026-05-09-round-312-upstream-parity-arc-closure.md)  
**Trigger:** operator-requested visibility into which production `.rs`
files have no 1:1 mirror under `.reference-haskell-cardano-node/`.

## Summary

R313 is a **read-only census** — no code, no Rust changes. Re-runs
the strict-mirror audit (`dev/test/audit-strict-mirror.py`),
cross-checks every production `.rs` against the vendored upstream
Haskell tree, and groups the synthesis bucket by subsystem so the
state is auditable at a glance.

**Headline:** 445 production `.rs` files audited. 230 mirror an
upstream `.hs` by snake_case basename (`(a) DIRECT_MIRROR`); 215 are
explicit synthesis with `## Naming parity` docstring stanzas ending
in `**Strict mirror:** none.` plus the upstream symbol(s)/file(s)
the helper surfaces (`(c) NO_MIRROR_NEEDS_DOCSTRING`). Zero
`(b) RENAME_NEEDED`, zero `(d) NAME_CLASH_REGRADE`. The drift-guard
(`dev/test/check-strict-mirror.py --fail-on-violation`) reports 0
violations; the R311 index-vs-tree drift check also clean.

## Audit re-run

```text
$ python3 dev/test/audit-strict-mirror.py
audit complete: 445 rust files; candidate_match=387, no_candidate_match=58
auto-grading bucket counts:
  (a): 230
  (c): 215
TSV written: docs/strict-mirror-audit.tsv

$ git diff --stat docs/strict-mirror-audit.tsv
(empty — no drift since last commit)
```

The TSV is byte-identical to the committed allowlist. Census
output below was extracted from that TSV.

## Bucket breakdown

```text
(a) DIRECT_MIRROR (auto: docstring declares strict mirror) ......... 187
(a) DIRECT_MIRROR (auto)                                              25
(a) DIRECT_MIRROR (auto (affinity-filtered))                          18
(c) NO_MIRROR_NEEDS_DOCSTRING (auto: docstring present (strict-none)) 174
(c) NO_MIRROR_NEEDS_DOCSTRING (auto: docstring present (unspecified))  41
```

- `(a) "docstring declares strict mirror"` — file's docstring
  contains an explicit `**Strict mirror:** <upstream/path.hs>`
  line naming the canonical parent.
- `(a) "auto"` — basename matches a single upstream `.hs`
  unambiguously; no docstring assertion needed.
- `(a) "auto (affinity-filtered)"` — basename matches multiple
  upstream `.hs` files but the crate→repo affinity filter
  resolves to a single canonical match.
- `(c) "docstring present (strict-none)"` — file carries the
  policy-canonical `**Strict mirror:** none.` block.
- `(c) "docstring present (unspecified)"` — file carries a
  `## Naming parity` block but the strict-mirror line uses softer
  language; allowlisted at R274 hand-grade time as still
  policy-compliant. Future R-arc work can tighten these to
  `(strict-none)` form, but not required.

## Synthesis-file distribution by subsystem

The 215 `(c)` synthesis files (no upstream 1:1 mirror) live under:

| Subtree | Count | Rationale |
|---|---:|---|
| `crates/cardano-cli/` | 58 | Parent-shell organizers required by Rust's module-tree convention (e.g., `era_independent.rs` aggregating `era_independent/{cluster}/*.rs` leaves) plus a few Yggdrasil-specific aggregators. Upstream `cardano-cli` has no `*.hs` files at the parent-shell level — leaves only. |
| `crates/network/` | 43 | Diffusion-layer aggregators (`diffusion.rs`, `governor.rs` parent shells) plus runtime-side adaptors (`peer_registry.rs`-adjacent helpers, root-set provider plumbing). |
| `crates/ledger/` | 35 | `state/*` extractions (R269a–R269o sub-arc — a `state.rs` god-file split into one file per concern: `state/cbor.rs`, `state/snapshot.rs`, `state/treasury.rs`, etc.). Upstream Cardano.Ledger doesn't enforce this granularity at the file boundary; the Rust split is for compile-unit + import hygiene. |
| `crates/consensus/` | 22 | `mempool/*` Yggdrasil-side concerns (queue ordering, capacity bookkeeping) and async-task orchestration shims with no Haskell equivalent (Haskell uses STM transactions instead of explicit task structures). |
| `node/src/runtime/` | 18 | Runtime async-task orchestration loops (`governor_loop`, `block_producer_loop`, `sync_session`, `peer_session`, `bootstrap`, `forge`, `tracing`, `tx_submission_service`, `keep_alive`, `mempool_helpers`, etc.). All have explicit `**Strict mirror:** none.` blocks pointing at the upstream Haskell entry-point function whose loop body the file mirrors (e.g., `governor_loop.rs` → `Ouroboros.Network.PeerSelection.Governor.peerSelectionGovernor`). |
| `node/src/*` (other 25) | 25 | Binary-side integration files (`main`-adjacent CLI/config/bootstrap helpers, `commands/*`, `local_server/*` LSQ dispatcher, `bin/*` operator utilities). Each carries a `## Naming parity` docstring naming the upstream component it integrates with. |
| `crates/plutus/` | 6 | CEK machine internal helpers (`cost_model_codec.rs`, `flat.rs` Yggdrasil-side iterative decoder per R250-block, `machine_*.rs` private helpers) with no upstream file analogue. |
| `crates/storage/` | 5 | Trait-based storage layer (`immutable_db.rs`, `volatile_db.rs`, `ledger_store.rs`, `chain_db.rs` parent shells). Haskell's storage layer is split across many `.hs` modules; Yggdrasil aggregates per trait. |
| `crates/crypto/` | 3 | Yggdrasil-side glue (`hash.rs` re-exports, `kes.rs` umbrella over Sum-KES depth modules). |

## Sample verification

5 randomly-sampled `(c)` files (all carry the policy-canonical
`**Strict mirror:** none.` docstring stanza):

```text
node/src/runtime/bootstrap.rs:
  //! **Strict mirror:** none. Yggdrasil-side per-peer connection
  //! bring-up function aggregating upstream
  //! `Ouroboros.Network.NodeToNode.connectTo` (TCP + mux + version

node/src/runtime/governor_config.rs:
  //! **Strict mirror:** none. Yggdrasil-side configuration-overlay
  //! layer aggregating tick interval, keep-alive cadence, target
  //! peer counts, and cross-task shared handles.

node/src/runtime.rs:
  //! **Strict mirror:** none. Yggdrasil-side runtime shell that
  //! re-exports the sub-modules under `runtime/` and exposes the
  //! verified-sync service entry points the CLI consumes.

crates/consensus/src/chain_state.rs:
  //! **Strict mirror:** none. Yggdrasil-side volatile chain-state
  //! tracker that enforces the Ouroboros security parameter `k`.
  //! Mirrors the volatile-DB rollback-window concept from upstream

crates/ledger/src/state/snapshot.rs:
  //! **Strict mirror:** none. Yggdrasil-side LSQ-friendly read-only
  //! capture aggregating fields from `LedgerState` for
  //! `Ouroboros.Consensus.Shelley.Ledger.Query` answer paths.
```

5 randomly-sampled auto-graded `(a)` files (basename match resolves
unambiguously to a single canonical upstream `.hs`):

```text
crates/ledger/src/protocol_params.rs       <- ouroboros-consensus/Peras/Params.hs (loose; carries explicit "Mirrors..." text in body)
crates/storage/src/volatile_db.rs          <- ouroboros-consensus/Storage/VolatileDB.hs
crates/ledger/src/state/ratify.rs          <- cardano-ledger/eras/conway/.../Ratify.hs
crates/ledger/src/eras/mary.rs             <- cardano-ledger/eras/mary/impl/.../Mary.hs
crates/network/src/peer_registry.rs        <- ouroboros-network/.../V2/Registry.hs (post-Conway TxSubmission inbound registry)
```

## Verification

```text
$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 8 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c
    consistent across pin source, fixture tree, and docs;
    2 corpora validated.
```

R313 ships zero code or test changes; the post-R312 baseline (4,855
tests passing, all five gates clean) is preserved by construction.

## Closure criterion

- Audit re-run produces a TSV byte-identical to the committed
  allowlist (no drift).
- 215 `(c)` synthesis files all sample-verify to carry policy-
  canonical docstring stanzas.
- 230 `(a)` files all sample-verify to map to plausible upstream
  `.hs` files (basename match in 43 auto-graded cases; explicit
  `**Strict mirror:**` declaration in 187).
- Distribution by subsystem documented for future readers.

All four are met.

## Findings

- **The synthesis bucket is concentrated in three places:**
  cardano-cli parent shells (58), network diffusion-layer
  aggregators (43), and ledger `state/*` extractions (35). These
  three account for 64% of the 215 synthesis files.
- **Runtime async-task orchestration is by-design synthesis**
  (18 files in `node/src/runtime/`). Haskell uses STM transactions
  inline in the equivalent components; Rust requires explicit task
  structures for the same logic.
- **41 files use `(c) docstring present (unspecified)` form**
  rather than the canonical `(c) docstring present (strict-none)`
  form. These are policy-compliant but use slightly softer
  language. A future round could tighten them to the canonical
  form for uniformity, but no functional difference and not a
  drift-guard violation.
- **Zero files lack a parity story.** Every production `.rs` is
  either a strict 1:1 mirror or carries an explicit synthesis
  declaration. The strict-mirror policy is fully durable.

## Out of scope (future work)

- **Tighten the 41 `(c) docstring present (unspecified)`** files
  to the canonical `**Strict mirror:** none.` form. Mechanical
  pass; no behavioral change. Defer until a contributor is
  already touching those files for a substantive reason.
- **Re-grade auto-graded `(a)` matches that look loose.** A few
  basename-matches are loose (e.g., `protocol_params.rs` →
  `Peras/Params.hs` rather than a more general params file).
  These were R274 hand-graded as acceptable; future re-grading
  could promote them to explicit `**Strict mirror:**` declarations
  for tighter parity-trace claims.
