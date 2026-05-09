# Round 278 — `crates/consensus/src/mempool/` parity sweep

**Date:** 2026-05-09
**Phase:** B (targeted renames + docstrings)
**Predecessor:** R277 (`docs/operational-runs/2026-05-09-round-277-consensus-cluster-naming-parity.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Resolve every `(c-needed)` / `(NEEDS-REVIEW)` row in the audit TSV
under `crates/consensus/src/mempool/`. Three parent shells receive a
`## Naming parity` docstring stanza; sub-modules already annotated in
R273-rename are reverified by the strengthened audit grader.

Also tightens `scripts/audit-strict-mirror.py` non-production-fragment
filter to catch `plutus-benchmark`, `nofib`, `cardano-recon-framework`,
`cardano-timeseries-io`, `sim-tests`, `sim-bench`, `examples/`, etc.
which were leaking through to false-positive `(a)` auto-grades.

## Files affected

### New `## Naming parity` blocks (3 parent shells)

| File | Verdict |
|---|---|
| `mempool.rs` | `(c) synthesis` — top-level shell aggregating upstream `Ouroboros.Consensus.Mempool.{API, Capacity, Impl.Common, Impl.Update, Init, Query, TxSeq, Update}` |
| `mempool/queue.rs` | `(c) synthesis` — Yggdrasil-side queue+capacity aggregation. Previously had no module docstring; full docstring + parity block prepended |
| `mempool/tx_state.rs` | `(c) synthesis` — splits upstream `TxSubmission.Inbound.V2.State.hs` into `state.rs` + `shared.rs` sub-modules |

### Re-graded by tightened non-production filter

- `mempool/queue.rs` was auto-graded as `(a) DIRECT_MIRROR` against
  `plutus-benchmark/nofib/.../Knights/Queue.hs` (a Plutus benchmark
  example, not the cardano mempool queue). The non-production-fragment
  filter has been extended to drop `/plutus-benchmark/` + `/nofib/`
  paths, eliminating this false-positive.

### Already auto-graded (no edits)

- `mempool/queue/inner.rs` ✓ (c) strict-none
- `mempool/queue/shared.rs` ✓ (c) strict-none
- `mempool/tx_state/shared.rs` ✓ (c) strict-none
- `mempool/tx_state/state.rs` ✓ (c) unspecified (R273e annotation, `**Strict mirror (partial):**`)

## Audit grader: tightened non-production filter

Added 11 new fragments to `NON_PRODUCTION_FRAGMENTS`:

```
"/benchmark/", "-benchmark/", "/nofib/", "/plutus-benchmark/",
"/cardano-benchmarking/", "/cardano-recon-framework/",
"/cardano-timeseries-io/", "/examples/", "/sim-tests/",
"/sim-bench/", "/cardano-tools/"
```

These caught at least one false-positive auto-grade in this round and
will preempt similar drift in R279–R281 clusters.

## Verdict bucket counts

| Bucket | Pre-R278 | Post-R278 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 59 | 57 (-2: false-positive purge) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 33 | 36 (+3 — 3 mempool parents resolved) |
| `(c-needed)` | 36 | 36 |
| `(NEEDS-REVIEW)` | 81 | 80 (-1) |
| **TOTAL** | 209 | 209 |

7 mempool files (the entire mempool/ tree) now graded:
- 0 `(a) DIRECT_MIRROR`
- 7 `(c) NO_MIRROR_NEEDS_DOCSTRING`

The mempool tree is fully resolved.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 4.86s)
cargo lint                          clean (Finished `dev` profile in 10.35s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py
                                    strict-mirror: 0 violations (clean)
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Diff stat

```text
crates/consensus/src/mempool.rs              +9 lines (parity block)
crates/consensus/src/mempool/queue.rs        +13 lines (full docstring + parity block)
crates/consensus/src/mempool/tx_state.rs     +9 lines (parity block)
docs/strict-mirror-audit.tsv                 rebuilt
docs/operational-runs/2026-05-09-round-278-... (new)
scripts/audit-strict-mirror.py               +11 lines (non-production fragment additions)
```

## Stop point — Phase B progress

| Round | Cluster | Status |
|---|---|---|
| R276 | `crates/ledger/src/state/` (24 files) | ✅ closed |
| R277 | `consensus/{nonce,opcert,diffusion_pipelining}/` (9 files) | ✅ closed |
| R278 | `consensus/mempool/` (7 files) | ✅ closed |
| R279 | `node/src/runtime/` synthesis pass (~17 files) | next |
| R280 | `crates/network/src/governor/` regrade | pending |
| R281 | sweeper (incl. `opcert.rs` -> `ocert.rs` rename) | pending |

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R277 (`docs/operational-runs/2026-05-09-round-277-consensus-cluster-naming-parity.md`)
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
