---
title: "Round 623 Conway LEDGER predicate-failure scaffold (A5 Phase-2.5)"
parent: Reference
---

# Round 623 Conway LEDGER predicate-failure scaffold (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Opens **Conway-era LEDGER coverage** with the
`ConwayLedgerPredFailure` 9-variant scaffold. After R623, the
typed predicate-failure tree covers **all 6 supported eras** at
the LEDGER root: Shelley/Allegra/Mary/Alonzo/Babbage reuse
`ShelleyLedgerPredFailure` directly (per upstream's
`type instance EraRuleFailure "LEDGER" <Era> =
ShelleyLedgerPredFailure <Era>`); Conway has its own
`ConwayLedgerPredFailure` (replaces DELEGS with CERTS, adds the
new GOV sub-rule for governance actions).

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Ledger.hs:117-127,225-260`
  (variant ADT + CBOR encoder/decoder; tags start at 1 — upstream
  deliberately skips tag 0).
- Per-era LEDGER coverage scan (R623):
  - `eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ledger.hs:148`
    Shelley → ShelleyLedgerPredFailure.
  - `eras/allegra/impl/src/Cardano/Ledger/Allegra/Rules/Ledger.hs:14`
    Allegra → ShelleyLedgerPredFailure.
  - `eras/mary/impl/src/Cardano/Ledger/Mary/Rules/Ledger.hs:14`
    Mary → ShelleyLedgerPredFailure.
  - `eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Ledger.hs:49`
    Alonzo → ShelleyLedgerPredFailure.
  - `eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/Ledger.hs:38`
    Babbage → ShelleyLedgerPredFailure.
  - `eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Ledger.hs:129`
    Conway → ConwayLedgerPredFailure (the only era with its own
    LEDGER predicate-failure type).

## Changes

- Added `NonEmptyKeyHash` carrier (`Vec<KeyHash>` with non-empty
  invariant; CBOR wire format is a regular array of 28-byte
  bytestrings). Display matches upstream `Show (NonEmpty a)`:
  `<head> :| [<tail>...]`.
- Added `ConwayLedgerPredFailure` 9-variant enum:
  - Tag 1 `ConwayUtxowFailure(Vec<u8>)` — UTXOW sub-rule
    (Conway's variant set; raw pending Conway-specific decoder).
  - Tag 2 `ConwayCertsFailure(Vec<u8>)` — CERTS sub-rule (raw
    pending). Replaces Shelley's DELEGS.
  - Tag 3 `ConwayGovFailure(Vec<u8>)` — new governance sub-rule
    (raw pending).
  - Tag 4 `ConwayWdrlNotDelegatedToDRep(NonEmptyKeyHash)` —
    typed (R623).
  - Tag 5 `ConwayTreasuryValueMismatch(Mismatch<u64>)` — typed
    (R623), `ToGroup` flattened with expected-first encoding.
  - Tag 6 `ConwayTxRefScriptsSizeTooBig(Mismatch<u64>)` — typed
    (R623), `ToGroup` flattened.
  - Tag 7 `ConwayMempoolFailure(String)` — typed (R623) via
    CBOR text-string (major type 3).
  - Tag 8 `ConwayWithdrawalsMissingAccounts(Withdrawals)` —
    reuses R596 typed Withdrawals.
  - Tag 9 `ConwayIncompleteWithdrawals(IncompleteWithdrawals)` —
    reuses R597 typed IncompleteWithdrawals.
- Added `show_haskell_bytestring_like` helper rendering Text
  payloads with Haskell `Show String` escapes (subset covering
  common cases: backslash, quote, newline, tab, control-char
  decimal escapes).
- `from_cbor` dispatcher walks the outer CBOR array (length
  2-4), reads the Word8 tag, dispatches per-variant. Each tag
  enforces exact envelope length. Tags 5/6 use the upstream
  `ToGroup` flattened encoding (3-element envelope with the
  Mismatch fields inlined into the outer list; tag 5 uses
  expected-first ordering per `swapMismatch`). Unknown tags
  (including tag 0) reject explicitly.
- Display routes typed payloads through their typed Display;
  raw sub-rule variants emit `<Constructor> <raw-cbor N bytes>`.

8 new focused unit tests:
- `_utxow_raw_routing_tag1` — sub-rule raw fallback.
- `_wdrl_not_delegated_tag4` — NonEmptyKeyHash typed decode.
- `_treasury_mismatch_tag5` — Mismatch Coin with expected-first
  encoding.
- `_ref_scripts_too_big_tag6` — Mismatch Word with supplied-first
  encoding.
- `_mempool_failure_tag7` — CBOR text-string decode.
- `_withdrawals_missing_accounts_tag8` — reuses Withdrawals.
- `_unknown_tag_rejects` — tag 99 rejection.
- `_tag0_rejects` — Conway-specific upstream skip enforcement.

Lint cleanup: collapsed `1 | 2 | 3` to `1..=3` for
`clippy::manual_range_patterns`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (237 lib + 4
  doctests + 1 main, +8 new tests vs R622 baseline of 229)

## Remaining (A5 Phase-2.5+)

- Conway UTXOW / CERTS / GOV sub-rule decoders (tag 1/2/3
  payloads). UTXOW especially has its own Conway-era variant
  additions for Plutus / reference scripts / governance actions
  on top of Babbage's variant set.
- Typed Byron bootstrap parse (recovers network from address
  attributes — uncommon legacy era).
- Alonzo 3-array TxOut + Babbage map-form TxOut typed shapes
  (era-specific output forms).
- Era-aware top-level wiring: `TxValidationErrorInCardanoMode`
  currently carries `EraApplyTxError` (raw + rendered text) at
  every era variant. Wiring the typed decoder through requires
  an `ApplyTxError = NonEmpty PredicateFailure` walker (single-
  decode → multi-failure list).
