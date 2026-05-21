---
title: "Round 645 Typed Conway GOV InvalidGuardrailsScriptHash variant (A5 Phase-2.5)"
parent: Reference
---

# Round 645 Typed Conway GOV InvalidGuardrailsScriptHash variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway GOV tag 11 (`InvalidGuardrailsScriptHash`) by
adding the `StrictMaybeScriptHash` type. After R645, **5 of 19
Conway GOV variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:200-204,283-284`
  (`InvalidGuardrailsScriptHash (StrictMaybe ScriptHash)
  (StrictMaybe ScriptHash)`; encoder `Sum
  InvalidGuardrailsScriptHash 11 !> To got !> To expected`).

## Changes

- Added `StrictMaybeScriptHash(Option<[u8; 28]>)` — decodes a
  `StrictMaybe ScriptHash` from a CBOR list (0-element =
  SNothing, 1-element = SJust 28-byte hash). Display: `SNothing`
  / `SJust (ScriptHash "<hex>")`.
- Refactored `ConwayGovPredFailure::InvalidGuardrailsScriptHash(Vec<u8>)`
  → struct variant `{ got: StrictMaybeScriptHash, expected:
  StrictMaybeScriptHash }`. `from_cbor` decodes the 3-element
  envelope `[11, got SMaybe, expected SMaybe]`. Display:
  `InvalidGuardrailsScriptHash (<got>) (<expected>)`.

1 new focused unit test:
- `_invalid_guardrails_script_hash_tag11` — got SJust +
  expected SNothing, asserting the full Display.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (304 lib + 4
  doctests + 1 main, +1 new test vs R644 baseline of 303)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants (14) — typed decoders for `GovAction
  era`, `Voter`, `ProposalProcedure era`, `ProtVer`,
  `Credential` cold/hot committee roles, `NonEmptyMap`.
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
