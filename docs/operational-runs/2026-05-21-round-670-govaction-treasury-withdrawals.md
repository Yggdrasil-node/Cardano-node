---
title: "Round 670 Typed GovAction TreasuryWithdrawals (A5 Phase-2.5)"
parent: Reference
---

# Round 670 Typed GovAction TreasuryWithdrawals (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `GovAction::TreasuryWithdrawals` variant (tag 2).
After R670, 4 of 7 `GovAction` variants are fully typed.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:825-829,883`
  (`TreasuryWithdrawals (Map AccountAddress Coin) (StrictMaybe
  ScriptHash)`; decoder `2 -> SumD TreasuryWithdrawals <! From
  <! D (decodeNullStrictMaybe decCBOR)`).

## Changes

- Refactored `GovAction::TreasuryWithdrawals(Vec<u8>)` → struct
  variant `{ withdrawals: Vec<(RewardAccount, u64)>, guardrail:
  Option<[u8; 28]> }`.
- `GovAction::from_decoder` special-cases tag 2: decodes the
  3-element envelope `[2, Map AccountAddress Coin,
  decodeNullStrictMaybe ScriptHash]` — the withdrawal CBOR map
  and the null-encoded guardrail script hash.
- Display: `TreasuryWithdrawals (fromList [(<AccountAddress>,
  Coin <n>), ...]) (<StrictMaybe ScriptHash>)`.

2 tests (1 new, 1 updated):
- `_malformed_proposal_treasury_withdrawals` — new, one
  withdrawal entry + a `SJust` guardrail.
- `_zero_treasury_withdrawals_tag15` — updated to assert the
  typed empty-map / SNothing-guardrail render.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (338 lib + 4
  doctests + 1 main, +1 net new test vs R669 baseline of 337)

## Remaining (A5 Phase-2.5+)

- `GovAction` raw variants: tag 0 ParameterChange
  (PParamsUpdate), tag 4 UpdateCommittee, tag 5 NewConstitution
  (Constitution).
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
