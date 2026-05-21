---
title: "Round 627 ConwayCertPredFailure scaffold + wire CERTS tag 1 (A5 Phase-2.5)"
parent: Reference
---

# Round 627 ConwayCertPredFailure scaffold + wire CERTS tag 1 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ConwayCertPredFailure` 3-variant scaffold (the CERT
sub-rule that Conway CERTS tag 1 dispatches into) and wires the
parent variant to the typed enum. Tag 2 reuses the existing
typed `ShelleyPoolPredFailure` directly since upstream's Conway
CERT continues to use Shelley's POOL type unchanged.

After R627, the Conway LEDGER → CERTS → CERT → POOL chain
renders typed end-to-end through the POOL leaf — every variant
of the Shelley POOL enum (5/6 fully typed at R616 + the
flattened-Mismatch tag 1 at R619) now reachable from Conway.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Cert.hs:102-106,266-285`
  (`data ConwayCertPredFailure era` 3-variant ADT — DelegFailure,
  PoolFailure (Shelley.ShelleyPoolPredFailure reuse), GovCertFailure
  — CBOR encoder with tags 1/2/3; upstream skips tag 0).
- `Conway.Rules.Cert.hs:117-118` confirms
  `InjectRuleFailure "CERT" Shelley.ShelleyPoolPredFailure
  ConwayEra` — i.e. Conway POOL = Shelley POOL.

## Changes

- Added `ConwayCertPredFailure` 3-variant enum:
  - Tag 1 `DelegFailure(Vec<u8>)` — raw pending
    `ConwayDelegPredFailure` decoder.
  - Tag 2 `PoolFailure(ShelleyPoolPredFailure)` — typed via
    Shelley POOL reuse.
  - Tag 3 `GovCertFailure(Vec<u8>)` — raw pending
    `ConwayGovCertPredFailure` decoder.
- `from_cbor` enforces 2-element envelope; unknown tags
  (including tag 0) reject.
- Display routes tag 2 through typed `ShelleyPoolPredFailure`;
  tags 1/3 emit `<raw-cbor N bytes>`.
- Refactored
  `ConwayCertsPredFailure::CertFailure(Vec<u8>)` →
  `CertFailure(ConwayCertPredFailure)`. CERTS `from_cbor`
  decodes through the typed sub-rule; Display routes the typed
  nested payload.
- Updated R625's `_cert_failure_tag1` test to construct a typed
  inner CERT payload (DelegFailure raw) and assert the typed
  nested Display chain.

3 new focused unit tests:
- `_pool_failure_decodes_tag2` end-to-end CERT → POOL with
  Shelley POOL tag-0 inner.
- `_deleg_failure_raw_routing_tag1` raw fallback confirmation.
- `_unknown_tag_rejects` (tag 0 — upstream skip enforcement).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (254 lib + 4
  doctests + 1 main, +3 net new tests vs R626 baseline of 251)

## Remaining (A5 Phase-2.5+)

- `ConwayDelegPredFailure` (CERT tag 1) — DELEG sub-rule for
  Conway-era certificate predicates (stake/DRep delegation).
- `ConwayGovCertPredFailure` (CERT tag 3) — GOVCERT sub-rule
  for governance certificates (DRep registration, committee
  hot/cold authorization).
- Conway UTXOW raw variants (tag 0 nested UTXO, 10/11/12/13/15/18).
- Conway UTXO sub-rule (referenced by UTXOW tag 0).
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
