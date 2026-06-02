---
title: 'R316: reclassify 3 more partial-mirrors to synthesis after content-vs-name audit'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-316-reclassify-three-more-synthesis/
---

# Round 316 — reclassify 3 more partial-mirrors to synthesis

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R315`](2026-05-09-round-315-reclassify-synthesis-files.md)

## Trigger

Operator asked whether the 9 remaining `(c) strict-partial` files
could be promoted to direct mirrors. Content-vs-name audit revealed
3 files where the basename matches an upstream `.hs` but the
**content diverges** — the files are genuinely synthesis with
basename collision against unrelated upstream concepts.

## Files reclassified (3 total)

| Rust path | Yggdrasil content | Upstream basename match | Verdict |
|---|---|---|---|
| `crates/network/src/governor/churn.rs` | `ChurnPhase`, `ChurnConfig`, `FetchMode`/`ChurnMode`/`ChurnRegime` enums + regime-aware decrease helpers | `Ouroboros.Network.PeerSelection.Churn` (PeerChurnArgs + churnLoop driver) | Content diverges. Yggdrasil's churn loop driver lives in `governor.rs`; this file is the policy-config half it consumes. **Reclassify to synthesis.** |
| `crates/network/src/governor/peer_metric.rs` | `PickPolicy` (PRNG + uniform/scored peer-pick), `PeerFailureRecord`, `RequestBackoffState`, Yggdrasil's `PeerMetrics` (upstreamyness/fetchyness scoring) | `Ouroboros.Network.PeerSelection.PeerMetric` (PSQ-based slot metrics, AverageMetrics, witnessedPeer) | Both files have a `PeerMetrics` type but they're different concepts (Yggdrasil scores by combined upstreamyness+fetchyness; upstream tracks per-slot averages in a slot-PSQ). **Reclassify to synthesis.** |
| `crates/consensus/src/praos/common.rs` | `ActiveSlotCoeff` data + `leadership_threshold` math + `compute_neg_ln_one_minus` Taylor series + `gcd_u64` | `Ouroboros.Consensus.Protocol.Praos.Common` (`MaxMajorProtVer`, `PraosTiebreakerView`, `VRFTiebreakerFlavor`, `PraosCanBeLeader`, `PraosCredentialsSource`, `PraosNonces`, `PraosProtocolSupportsNode`) | Content completely diverges. Yggdrasil extracts `ActiveSlotCoeff` from `Cardano.Ledger.BaseTypes` + math helpers from `Praos.VRF`. The upstream `Praos/Common.hs` carries protocol-class type machinery (different concern). **Reclassify to synthesis.** |

Each new docstring preserves the upstream symbol cross-references
(so a parity researcher can still trace where each piece comes from)
but declares synthesis honestly:
`**Strict mirror:** none. Yggdrasil-side <description>. Surfaces ...`

## Bucket-count delta

| Bucket | R315 | R316 | Δ |
|---|---:|---:|---:|
| `(a) DIRECT_MIRROR` (any auto-grade) | 254 | 254 | 0 |
| `(c) docstring present (strict-none)` | 182 | 185 | **+3** |
| `(c) docstring present (strict-partial)` | 9 | 6 | **−3** |
| **Grand total** | 445 | 445 | 0 |

## Why these aren't promotions to direct mirror

The strict-mirror policy says: a file claims `**Strict mirror:** <upstream-path>`
when it 1:1 implements the concepts of that upstream `.hs`.
Basename match alone is **not** sufficient — content must align too.

For these 3 files:
- `churn.rs` content is the policy/config side of churn (mode classification, decrease helpers); upstream `Churn.hs` content is the loop driver itself.
- `peer_metric.rs` content is upstreamyness/fetchyness scoring + PRNG-based peer-pick; upstream `PeerMetric.hs` content is slot-PSQ metric tracking.
- `praos/common.rs` content is ASC + leader-threshold math; upstream `Praos/Common.hs` content is protocol-class type machinery.

In each case, declaring strict-mirror would be a misleading parity
claim. Synthesis with explicit upstream cross-references is the
honest classification.

## Verification

```text
$ python3 dev/test/audit-strict-mirror.py
audit complete: 445 rust files; candidate_match=387, no_candidate_match=58
auto-grading bucket counts:
  (a): 254
  (c): 191

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo fmt --all -- --check          # clean
$ cargo check --workspace --all-targets   # clean
$ cargo clippy ... -D warnings         # clean
$ cargo test --workspace --all-features
passed: 4855  failed: 0
```

## Closure criterion

- 3 misclassified `(strict-partial)` files reclassified to canonical
  synthesis form.
- New docstrings preserve upstream symbol cross-references.
- All five workspace gates green at 4,855-test baseline.
- All four CI parity validators clean.

All four are met.

## Out of scope (R317+)

The remaining 6 `(c) strict-partial` files split into:
- **3 refactor candidates** (mux+multiplexer merge → R317; handshake split → R318; inbound_governor split → R319). After refactor, become direct mirrors.
- **3 intentional partials** (plutus/builtins.rs, plutus/machine.rs, and the 4th remaining file pending R317–R319). Yggdrasil-specific splits along axes upstream doesn't use; refactor would force types/runtime/cost back together (opposite direction from current Yggdrasil idiom).
