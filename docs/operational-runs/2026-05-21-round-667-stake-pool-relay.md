---
title: "Round 667 Typed StakePoolRelay sequence (A5 Phase-2.5)"
parent: Reference
---

# Round 667 Typed StakePoolRelay sequence (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `PoolParams` relay sequence — `PoolParams` now carries
`relays: Vec<StakePoolRelay>` instead of a raw byte tail. The
entire Conway `TxCert` tree is now typed end-to-end.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/State/StakePool.hs:325-418`
  (`data StakePoolRelay = SingleHostAddr (StrictMaybe Port)
  (StrictMaybe IPv4) (StrictMaybe IPv6) | SingleHostName
  (StrictMaybe Port) DnsName | MultiHostName DnsName`; CBOR
  `Sum` tags 0-2, optional fields `null`-encoded).

## Changes

- Added `decode_null_strict_maybe` — a generic helper decoding
  a `null`-encoded `StrictMaybe` value.
- Added `StakePoolRelay` 3-variant enum:
  - `SingleHostAddr { port: Option<u16>, ipv4: Option<[u8; 4]>,
    ipv6: Option<[u8; 16]> }`.
  - `SingleHostName { port: Option<u16>, dns: String }`.
  - `MultiHostName { dns: String }`.
  Display matches upstream stock-derived `Show` — IPv4 renders
  as a dotted quad, IPv6 as colon-separated hex groups, ports
  as `Port {portToWord16 = N}`, DNS names as `DnsName
  {dnsToText = "..."}`.
- Refactored `PoolParams` — replaced `relays_raw: Vec<u8>` with
  `relays: Vec<StakePoolRelay>`. `PoolParams::from_decoder`
  decodes the relay `StrictSeq`; the now-redundant `source`
  parameter was dropped from both `PoolParams::from_decoder` and
  `TxCert::from_decoder` (no certificate family captures raw
  bytes any more).
- Display renders `ppRelays = StrictSeq {getSeq = fromList
  [...]}`.

1 new focused unit test:
- `pool_params_decodes_typed_relays` — a RegPool whose
  PoolParams carries a `SingleHostAddr` (port + IPv4) and a
  `MultiHostName`, asserting both typed renders.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (335 lib + 4
  doctests + 1 main, +1 new test vs R666 baseline of 334)

## Remaining (A5 Phase-2.5+)

- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`, `PlutusPurpose` NoRedeemer.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
