---
title: "Round 660 Conway TxCert scaffold (A5 Phase-2.5)"
parent: Reference
---

# Round 660 Conway TxCert scaffold (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `TxCert` scaffold — the Conway-era transaction
certificate type — and wires it into
`ConwayPlutusPurposeItem::ConwayCertifying` (R655 left that
purpose item raw).

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/TxCert.hs:643-714`
  (`data ConwayTxCert era = ConwayTxCertDeleg ConwayDelegCert |
  ConwayTxCertPool PoolCert | ConwayTxCertGov ConwayGovCert`;
  `decCBOR = decodeRecordSum "ConwayTxCert"` — a flat `Sum`,
  tags 0-2 + 7-13 = deleg, 3-4 = pool, 14-18 = govcert; tags
  5/6 (genesis/MIR) rejected).

## Changes

- Added `TxCert` — a 3-variant enum mirroring upstream's
  `ConwayTxCert` family split (`ConwayTxCertDeleg` /
  `ConwayTxCertPool` / `ConwayTxCertGov`), each carrying the
  upstream `decodeRecordSum` tag and the raw per-certificate
  payload. `from_decoder` reads the flat `Sum` envelope `[tag,
  ...payload]`, classifies the tag into its family, and rejects
  the removed genesis/MIR tags (5/6). `cert_constructor` names
  the specific certificate (RegTxCert / RegPoolTxCert /
  RegDRepTxCert / …) from the wire tag. Display:
  `<Family> (<CertificateConstructor> <raw-cbor N bytes>)`.
- Refactored `ConwayPlutusPurposeItem::ConwayCertifying(Vec<u8>)`
  → `ConwayCertifying(TxCert)`. The decode path routes through
  `TxCert::from_decoder`.

1 new focused unit test:
- `conway_utxow_pred_failure_missing_redeemers_certifying_txcert`
  — a `MissingRedeemers` carrying a `ConwayCertifying` purpose
  whose item is a Conway `TxCert` (Sum tag 1 = UnRegTxCert,
  delegation family), asserting the typed family + constructor
  render.

Lint fix: `11 | 12 | 13` → `11..=13` (clippy
`manual_range_patterns`).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (327 lib + 4
  doctests + 1 main, +1 new test vs R659 baseline of 326)

## Remaining (A5 Phase-2.5+)

- `TxCert` per-certificate bodies (`ConwayDelegCert`,
  `PoolCert`, `ConwayGovCert`).
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
