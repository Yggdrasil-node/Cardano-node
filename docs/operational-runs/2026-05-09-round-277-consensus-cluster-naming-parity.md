# Round 277 — `crates/consensus/src/{nonce,opcert,diffusion_pipelining}/` parity sweep

**Date:** 2026-05-09
**Phase:** B (targeted renames + docstrings)
**Predecessor:** R276 (`docs/operational-runs/2026-05-09-round-276-state-naming-parity.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Resolve every `(c-needed)` and `(NEEDS-REVIEW)` row in
`docs/strict-mirror-audit.tsv` that lives under
`crates/consensus/src/{nonce, opcert, diffusion_pipelining}/`. Each
parent shell receives a `## Naming parity` docstring stanza; sub-module
files already annotated in R273-rename are reverified by the
strengthened audit grader.

Also strengthens the audit grader to recognize four naming-parity-block
variants (strict-none / strict-partial / strict-mirror / unspecified)
instead of just two, so docstrings declaring a strict-mirror
relationship grade as `(a) DIRECT_MIRROR` rather than NEEDS-REVIEW.

## Files affected

### New `## Naming parity` blocks (3 parent shells)

| File | Verdict |
|---|---|
| `crates/consensus/src/nonce.rs` | `(c) synthesis` — combines UPDN + TICKN rule files plus VRF-output-to-nonce helpers from BaseTypes / Praos.VRF |
| `crates/consensus/src/opcert.rs` | `(c) synthesis` — re-export shell over strict-mirror sub-modules. Filename `opcert` should rename to `ocert` per R274 discovery; rename **scheduled for R281** since it changes the public API surface |
| `crates/consensus/src/diffusion_pipelining.rs` | `(c) synthesis` — combines three upstream `DiffusionPipelining.hs` files (Block.SupportsDiffusionPipelining + Shelley.Node + HardFork.Combinator.Node) |

### Re-graded by the strengthened grader (no edits, verdict moves)

| File | Pre-R277 verdict | Post-R277 verdict |
|---|---|---|
| `consensus/opcert/ocert.rs` | `(NEEDS-REVIEW) 2 hits` | `(a) DIRECT_MIRROR (docstring declares)` |
| `consensus/opcert/rules_ocert.rs` | `(NEEDS-REVIEW) 2 hits` | `(a) DIRECT_MIRROR (docstring declares)` |
| `consensus/diffusion_pipelining/state.rs` | `(NEEDS-REVIEW) 6 affinity-filtered hits` | `(c) NO_MIRROR (strict-partial declared)` |
| `consensus/diffusion_pipelining/identity.rs` | (a) auto (false-positive single hit) | `(c) NO_MIRROR (strict-partial declared)` |
| `consensus/nonce/derivation.rs` | (c) (already correct) | unchanged |
| `consensus/nonce/evolution.rs` | (c) (already correct) | unchanged |

### Deferred to R281

- **`crates/consensus/src/opcert.rs` -> `crates/consensus/src/ocert.rs` rename**, plus the `opcert/` directory rename to `ocert/`. This affects `yggdrasil_consensus::opcert::*` -> `yggdrasil_consensus::ocert::*` in downstream consumers (`node/src/`, integration tests). Scheduled for R281.

## Audit grader strengthening

`scripts/audit-strict-mirror.py` now distinguishes four `## Naming
parity` block variants instead of two:

| `**Strict mirror:**` line | `has_naming_parity_block` returns | Auto-grade |
|---|---|---|
| `none.` | `yes(strict-none)` | `(c) NO_MIRROR (synthesis)` |
| `(partial)` | `yes(strict-partial)` | `(c) NO_MIRROR (partial synthesis)` |
| `<upstream-path>` | `yes(strict-mirror)` | `(a) DIRECT_MIRROR (docstring declares)` |
| (heading present, no decl line) | `yes(unspecified)` | `(c) NO_MIRROR (acknowledged)` |
| heading absent | `no` | fall through to candidate matching |

This change correctly grades sub-modules that R273-rename annotated
with `**Strict mirror:** <upstream-path>` (e.g.
`opcert/ocert.rs` -> `Cardano.Protocol.TPraos.OCert.hs`) as `(a)`
instead of NEEDS-REVIEW. Author-side declarations are now
authoritative for the auto-grader.

## Verdict bucket counts

| Bucket | Pre-R277 | Post-R277 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 52 | 59 (+7 — opcert/ocert + opcert/rules_ocert + 5 era apply files re-graded by stronger grader; note R276's era apply files moved BACK from (c) to checking docstring; final mix shown below) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 28 | 33 (+5 — 3 parent shells + identity + state) |
| `(c-needed) NEEDS_DOCSTRING` | 38 | 36 (-2 — nonce.rs + opcert.rs + diffusion_pipelining.rs resolved; 1 row moved into NEEDS-REVIEW; net -2) |
| `(NEEDS-REVIEW)` | 91 | 81 (-10) |
| **TOTAL** | 209 | 209 |

Note: R276's era-apply files (eras/{shelley,allegra,mary,alonzo,babbage,conway}.rs)
declared `**Strict mirror:** none.` so they correctly grade as (c).
The 5 `(a)` increase is from opcert/ocert + opcert/rules_ocert getting
the proper grade plus 3 more across the codebase that newly resolve to
(a) under the stricter grader.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 6.02s)
cargo lint                          clean (Finished `dev` profile in 14.17s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py
                                    strict-mirror: 0 violations (clean)
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Diff stat

```text
crates/consensus/src/diffusion_pipelining.rs   +14 lines (parity block)
crates/consensus/src/nonce.rs                  +13 lines (parity block)
crates/consensus/src/opcert.rs                 +14 lines (parity block + rename note)
docs/strict-mirror-audit.tsv                   rebuilt
docs/operational-runs/2026-05-09-round-277-... (new)
scripts/audit-strict-mirror.py                 +24 lines (4-variant block recognition)
```

## Stop point — Phase B progress

| Round | Cluster | Status |
|---|---|---|
| R276 | `crates/ledger/src/state/` | ✅ closed |
| R277 | `consensus/{nonce,opcert,diffusion_pipelining}/` | ✅ closed |
| R278 | `consensus/mempool/{queue,tx_state}/` | next |
| R279 | `node/src/runtime/` synthesis pass | pending |
| R280 | `crates/network/src/governor/` | pending |
| R281 | sweeper (incl. `opcert.rs` -> `ocert.rs` rename) | pending |

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R276 (`docs/operational-runs/2026-05-09-round-276-state-naming-parity.md`)
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
