---
title: "Round 688 Stale-doc-comment audit (A5 Phase-2.5)"
parent: Reference
---

# Round 688 Stale-doc-comment audit (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Documentation-accuracy pass over `crates/tools/cardano-submit-api/
src/types.rs` — corrects the enum-level doc comments that still
described decoders as "scaffold ... pending" or gave a now-stale
typed-variant count, after the R611-R687 rounds completed every
predicate-failure decoder.

## Rationale

Stale doc comments are a correctness defect: they misdescribe
the code to future readers. Several enum headers still claimed
"R6XX ships the scaffold ... the N remaining variants keep raw
inner CBOR pending …" even though every variant is now typed.

## Changes (comment-only)

- `EraApplyTxError` — replaced "per-variant CBOR decoders
  layered in follow-on rounds" with the typed-decode accessor
  reference.
- `ShelleyLedgerPredFailure` — replaced the "raw CBOR bytes;
  per-rule typed payloads land in follow-on rounds" header with
  the fully-typed statement; corrected the `DelegsFailure`
  variant comment.
- `ShelleyDelegsPredFailure` / `ShelleyDelplPredFailure` —
  replaced the "ships the scaffold ... raw inner CBOR ... lands
  in a follow-on round" headers and the stale DELPL Display
  doc comment.
- `ConwayLedgerPredFailure` (9/9), `ConwayUtxowPredFailure`
  (19/19), `ConwayUtxoPredFailure` (23/23) — replaced the
  "ships the scaffold with typed payloads for N of M ... the
  remaining variants keep raw inner CBOR" headers with the
  fully-typed statement.

No behavior change — comment edits only.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (355 lib + 4
  doctests + 1 main — unchanged; comment-only round)

## Remaining (A5 Phase-2.5+)

- `PParamValue::Raw` remains a tolerant fallback by design.
