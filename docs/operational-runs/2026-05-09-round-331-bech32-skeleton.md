---
title: 'R331: bech32 file-mirror skeleton (Phase A.1 entry)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-331-bech32-skeleton/
---

# Round 331 — bech32 file-mirror skeleton

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R330`](2026-05-09-round-330-dep-audit-bech32-deferred-http.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.1 entry.

## Summary

R331 opens **Phase A.1 — bech32** of the sister-tools port arc.
This is a SMALL 4-round mini-arc (R331-R334) targeting the upstream
`IntersectMBO/bech32` binary's full CLI surface as a deployable Rust
binary.

R331 deliverable: file-mirror skeleton.

- `crates/bech32/src/lib.rs` — strict mirror of upstream
  `Codec/Binary/Bech32.hs` (the public library API). Declares
  placeholder types matching upstream's exported API surface
  (`DataPart`, `HumanReadablePart`, `EncodingError`, `DecodingError`,
  `HumanReadablePartError`, `CharPosition`, `Word5`) as empty
  enums/structs with `Placeholder` suffix. Concrete fields + methods
  land at R333.
- `crates/bech32/src/internal.rs` — strict mirror of upstream
  `Codec/Binary/Bech32/Internal.hs`. Declares the `EncodingSpec`
  enum + `CHARSET` constant (BIP-0173's 32-character alphabet
  `qpzry9x8gf2tvdw0s3jn54khce6mua7l`). Auto-graded by audit as
  `(a) DIRECT_MIRROR`.
- `crates/bech32/src/main.rs` — strict mirror of upstream
  `bech32/app/Main.hs`. Skeleton wrapper delegating to
  `yggdrasil_bech32::run()`; concrete CLI parser (R332) + dispatch
  (R333) replace the placeholder.
- `crates/bech32/Cargo.toml` — wired `bech32 = { workspace = true }`
  dep (workspace dep added at R330) plus a strict-mirror layout
  comment block.

Upstream's `bech32-th/src/Codec/Binary/Bech32/TH.hs` (Template
Haskell helpers) has no Rust analog — Rust uses `macro_rules!` and
proc-macros directly. No corresponding `crates/bech32/src/th.rs`
is created; the strict-mirror policy supports this absence per the
`Setup.hs` / `Orphans.hs` precedents.

## Diff inventory

| Path | Change |
|---|---|
| `crates/bech32/src/lib.rs` | Replaced R327 placeholder with R331 strict-mirror skeleton declaring 7 placeholder types matching upstream's exported API surface. |
| `crates/bech32/src/internal.rs` | New file. Strict mirror of `Codec/Binary/Bech32/Internal.hs` with `EncodingSpec` enum + `CHARSET` constant placeholders. |
| `crates/bech32/src/main.rs` | Strict-mirror docstring updated (R331 progress note + R332/R333 forward references). Body unchanged (delegates to `yggdrasil_bech32::run()`). |
| `crates/bech32/Cargo.toml` | Added `bech32 = { workspace = true }` dep. Comment block expanded to document the file-mirror layout. |
| `docs/parity-matrix.json` | `sister-tool.bech32` entry advanced: status `absent → partial`; next_milestone `R331 → R332`; new `implemented_evidence` list. |
| `dev/test/check-parity-matrix.py` | `ALLOWED_MILESTONES` extended with all per-mini-arc rounds (R326–R459) via a `_arc_range` helper, allowing parity-matrix entries to advance through their mini-arcs without repeated allowlist edits. |
| `docs/strict-mirror-audit.tsv` | Regenerated; total entries 448 → 449 (+1 for `crates/bech32/src/internal.rs`, graded `(a) DIRECT_MIRROR`). |
| `docs/operational-runs/2026-05-09-round-331-bech32-skeleton.md` | This round-doc. |

## Verification

```text
$ cargo fmt --all -- --check
(silent — clean)

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.44s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.44s

$ cargo test --workspace --all-features
passed: 4856  failed: 0

$ python3 dev/test/audit-strict-mirror.py
audit complete: 449 rust files; candidate_match=393, no_candidate_match=56
auto-grading bucket counts:
  (a): 247
  (c): 202

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 20 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)
```

The R311 index-vs-tree drift check correctly caught the new
`internal.rs` file before staging — exactly the failure mode it
was designed for. After staging, all gates green.

Audit-table total: 246 (a) + 202 (c) = **247 (a) + 202 (c) = 449**
(post-R331 final).

## Closure criterion

- File-mirror skeleton wired: `lib.rs ↔ Bech32.hs`, `internal.rs ↔
  Bech32/Internal.hs`, `main.rs ↔ app/Main.hs`.
- 7 placeholder types declared mirroring upstream's exported API.
- Workspace `bech32` dep wired into the crate.
- Parity-matrix entry: `partial`, `next_milestone: R332`.
- All 5 cargo gates + 3 CI parity validators clean.

All five are met.

## Out of scope (R332-R334 next steps)

- **R332 — CLI parser**: clap-based parser matching upstream's
  optparse-applicative `Parser` shape; --help byte-equivalent
  to upstream's `bech32 --help`; golden test pinned in
  `crates/bech32/tests/cli_help_golden.rs`.
- **R333 — Concrete encode/decode**: replace placeholder types
  with real implementations using the `bech32` crate; round-trip
  test against upstream test vectors (vendored at
  `.reference-haskell-cardano-node/deps/bech32/bech32/test/`);
  wire `node/dev/scripts/run-tools.sh bech32` end-to-end.
- **R334 — Closeout**: CHANGELOG entry; AGENTS.md operational
  guide refresh; parity-matrix transition `partial → verified_11_0_1`.
