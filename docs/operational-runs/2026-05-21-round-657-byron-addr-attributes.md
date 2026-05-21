---
title: "Round 657 Typed Byron AddrAttributes decode (A5 Phase-2.5)"
parent: Reference
---

# Round 657 Typed Byron AddrAttributes decode (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Closes the typed Byron `Attributes AddrAttributes` decode — the
attribute map that R656 left as a byte-count marker. The Byron
bootstrap address now renders fully typed end-to-end.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/byron/ledger/impl/src/Cardano/Chain/Common/AddrAttributes.hs:47-119`
  (`data AddrAttributes = AddrAttributes { aaVKDerivationPath ::
  Maybe HDAddressPayload, aaNetworkMagic :: NetworkMagic }`;
  `encCBORAttributes` — attribute key 1 = derivation path, key 2
  = network magic, absent key 2 means `NetworkMainOrStage`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/byron/ledger/impl/src/Cardano/Chain/Common/Attributes.hs:60-202`
  (`Attributes` is a `Map Word8 ByteString`; known keys parsed,
  unknown keys land in `attrRemain :: UnparsedFields`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/byron/ledger/impl/src/Cardano/Chain/Common/NetworkMagic.hs:42-77`
  (`data NetworkMagic = NetworkMainOrStage | NetworkTestnet
  Word32`).

## Changes

- Added `decode_byron_addr_attributes` — decodes the attribute
  CBOR map: key 1's value (a CBOR-wrapped HDAddressPayload
  bytestring) → `Just (HDAddressPayload "<hex>")`; key 2's value
  (a CBOR-wrapped Word32) → `NetworkTestnet <n>`; absent key 2 →
  `NetworkMainOrStage`. Unknown keys are counted into the
  `attrRemain` (`UnparsedFields`) render. Returns the typed
  `Attributes {attrData = AddrAttributes {aaVKDerivationPath,
  aaNetworkMagic}, attrRemain = UnparsedFields (...)}` shape.
- `render_byron_bootstrap` now calls `decode_byron_addr_attributes`
  for the `addrAttributes` field instead of emitting a
  byte-count marker.

1 new test + 1 corrected:
- New `addr_typed_display_byron_bootstrap_with_attributes` —
  Byron Address carrying both a derivation path (key 1) and a
  network magic (key 2), asserting the typed renders.
- `addr_typed_display_byron_bootstrap` updated for the typed
  empty-attributes render (`aaVKDerivationPath = Nothing,
  aaNetworkMagic = NetworkMainOrStage`).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (323 lib + 4
  doctests + 1 main, +1 new test vs R656 baseline of 322)

## Remaining (A5 Phase-2.5+)

- Alonzo 3-array TxOut + Babbage map-form TxOut typed Display.
- Deepest leaf payloads: `TxCert`, `PParamsUpdate`,
  `Constitution`, `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
