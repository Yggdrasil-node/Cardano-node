---
title: "Round 662 Typed DRep + delegation-cert tail (A5 Phase-2.5)"
parent: Reference
---

# Round 662 Typed DRep + delegation-cert tail (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `DRep` enum and fully types the delegation-certificate
tail — `TxCert::ConwayTxCertDeleg` now carries typed `pool` /
`drep` / `deposit` fields instead of an opaque raw tail. The
entire `ConwayTxCertDeleg` family is now fully typed.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/DRep.hs:64-93`
  (`data DRep = DRepKeyHash (KeyHash DRepRole) | DRepScriptHash
  ScriptHash | DRepAlwaysAbstain | DRepAlwaysNoConfidence`;
  CBOR `Sum` tags 0-3).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/TxCert.hs:656-705`
  (`conwayTxCertDelegDecoder` — the positional per-tag tail:
  tag 2 = pool, 7/8 = deposit, 9 = DRep, 10 = pool+DRep,
  11 = pool+deposit, 12 = DRep+deposit, 13 = pool+DRep+deposit).

## Changes

- Added `DRep` — a 4-variant enum decoding the CBOR `Sum`
  (`[0, keyhash]` / `[1, scripthash]` / `[2]` / `[3]`). Display
  matches the stock-derived Show.
- Refactored `TxCert::ConwayTxCertDeleg` from `{ cert_tag,
  credential, rest }` → `{ cert_tag, credential, pool:
  Option<KeyHash>, drep: Option<DRep>, deposit: Option<u64> }`.
  `TxCert::from_decoder` decodes the tail positionally per the
  upstream `conwayTxCertDelegDecoder` tag layout.
- Display: `ConwayTxCertDeleg (<CertConstructor> (<Credential>)
  [(<KeyHash>)] [(<DRep>)] [(Coin <n>)])` — each present tail
  field rendered typed.

3 tests (1 new, 2 updated):
- New `_missing_redeemers_certifying_reg_deposit_deleg` — tag 13
  (RegDepositDelegTxCert / DelegStakeVote) with all three tail
  fields present.
- `_missing_redeemers_certifying_txcert` /
  `_missing_redeemers_certifying_reg_deposit` updated to the new
  typed-field pattern and assertions.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (329 lib + 4
  doctests + 1 main, +1 net new test vs R661 baseline of 328)

## Remaining (A5 Phase-2.5+)

- `TxCert` `ConwayTxCertPool` / `ConwayTxCertGov` bodies
  (`PoolCert` / `ConwayGovCert`).
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
