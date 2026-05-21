---
title: "Round 656 Typed Byron bootstrap address parse (A5 Phase-2.5)"
parent: Reference
---

# Round 656 Typed Byron bootstrap address parse (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the Byron bootstrap address branch of the `Addr` Display —
the `header & 0x80` (CBOR array `0x82...`) path that previously
rendered only a raw hex marker.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/byron/ledger/impl/src/Cardano/Chain/Common/Address.hs:141-159`
  (`data Address = Address { addrRoot :: AddressHash Address',
  addrAttributes :: Attributes AddrAttributes, addrType ::
  AddrType }`; `toCBOR = encodeCrcProtected (addrRoot,
  addrAttributes, addrType)`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/byron/ledger/impl/src/Cardano/Chain/Common/CBOR.hs:106-108`
  (`encodeCrcProtected x = encodeListLen 2 <>
  encodeUnknownCborDataItem body <> toCBOR (crc32 body)` — a
  2-array `[#6.24(body), crc32]`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/byron/ledger/impl/src/Cardano/Chain/Common/AddrSpendingData.hs:85-112`
  (`data AddrType = ATVerKey | ATRedeem`; CBOR 0 = ATVerKey,
  2 = ATRedeem).

## Changes

- Added `render_byron_bootstrap` — parses the CRC-protected
  Byron `Address` CBOR structure:
  1. outer 2-array `[#6.24(inner), crc32]`,
  2. tag-24-wrapped inner bytestring,
  3. inner 3-tuple `[addrRoot (28-byte AddressHash),
     addrAttributes (Attributes map), addrType]`.
  Renders the typed `AddrBootstrap (BootstrapAddress
  {unBootstrapAddress = Address {addrRoot = AbstractHash
  "<hex>", addrAttributes = <Attributes N bytes>, addrType =
  ATVerKey}}) <crc32 N>` shape. The `Attributes AddrAttributes`
  map is surfaced as a byte-count marker rather than fully
  decoded.
- Updated `Addr::Display` — the Byron-bootstrap branch now
  routes through `render_byron_bootstrap`, falling back to an
  `AddrBootstrap <malformed hex N bytes: ...>` marker on
  structurally invalid input.

1 new test + 1 corrected:
- New `addr_typed_display_byron_bootstrap` — well-formed Byron
  Address round-trip asserting the typed render + crc32.
- `shelley_tx_out_typed_round_trip` corrected to use a realistic
  enterprise Shelley address (`0x61`) rather than a
  reward-account header (`0xE1`, which is never a valid TxOut
  payment address); the malformed-Byron assertion in
  `addr_typed_display_covers_all_shelley_types` updated for the
  new `<malformed hex ...>` marker wording.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (322 lib + 4
  doctests + 1 main, +1 net new test vs R655 baseline of 321)

## Remaining (A5 Phase-2.5+)

- Byron `AddrAttributes` typed decode (derivation path +
  network-magic — currently a byte-count marker).
- Alonzo 3-array TxOut + Babbage map-form TxOut typed Display.
- Deepest leaf payloads: `TxCert`, `PParamsUpdate`,
  `Constitution`, `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
