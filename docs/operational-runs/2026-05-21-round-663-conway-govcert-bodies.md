---
title: "Round 663 Typed ConwayTxCertGov certificate bodies (A5 Phase-2.5)"
parent: Reference
---

# Round 663 Typed ConwayTxCertGov certificate bodies (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `StrictMaybeAnchor` type and fully types the
governance-certificate family — `TxCert::ConwayTxCertGov` now
carries typed `credential` / `hot_credential` / `deposit` /
`anchor` fields instead of an opaque raw body.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/TxCert.hs:671-692`
  (`conwayTxCertDelegDecoder` tags 14-18 — 14
  AuthCommitteeHotKey `[cred, key]`, 15 ResignCommitteeCold
  `[cred, decodeNullStrictMaybe Anchor]`, 16 RegDRep `[cred,
  deposit, decodeNullStrictMaybe Anchor]`, 17 UnRegDRep `[cred,
  deposit]`, 18 UpdateDRep `[cred, decodeNullStrictMaybe
  Anchor]`).

## Changes

- Added `StrictMaybeAnchor(Option<Anchor>)` — decodes a
  `null`-encoded `StrictMaybe Anchor` (upstream
  `decodeNullStrictMaybe`: CBOR `null` → `SNothing`, otherwise
  the encoded `Anchor` → `SJust`). Display: `SNothing` / `SJust
  (<Anchor>)`.
- Refactored `TxCert::ConwayTxCertGov` from `{ cert_tag, raw }`
  → `{ cert_tag, credential: Credential, hot_credential:
  Option<Credential>, deposit: Option<u64>, anchor:
  Option<StrictMaybeAnchor> }`. `TxCert::from_decoder` decodes
  the leading credential and the positional per-tag tail.
- Display: `ConwayTxCertGov (<CertConstructor> (<Credential>)
  [(<hotCred>)] [(Coin <n>)] [(<StrictMaybeAnchor>)])`.

2 new focused unit tests:
- `tx_cert_decodes_gov_auth_committee_hot_key` — tag 14, cold +
  hot credentials.
- `tx_cert_decodes_gov_reg_drep_with_anchor` — tag 16, DRep
  credential + deposit + `SNothing` anchor.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (331 lib + 4
  doctests + 1 main, +2 new tests vs R662 baseline of 329)

## Remaining (A5 Phase-2.5+)

- `TxCert::ConwayTxCertPool` body (`PoolCert` — `RegPool`
  PoolParams / `RetirePool`, tags 3-4).
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
