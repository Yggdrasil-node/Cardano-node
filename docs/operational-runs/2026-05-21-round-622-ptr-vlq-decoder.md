---
title: "Round 622 Typed Ptr decoder for Shelley pointer addresses (A5 Phase-2.5)"
parent: Reference
---

# Round 622 Typed Ptr decoder for Shelley pointer addresses (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Closes the last raw-hex tail in Shelley addresses by adding a VLQ
(variable-length quantity) decoder for the 3 pointer fields
(slot, tx_ix, cert_ix) and wiring it into Addr Display.

After R622, **every Shelley address type renders with full
upstream stock-derived Show shape**: base (key/key, script/key,
key/script, script/script), pointer (key, script — with typed
Ptr), enterprise (key, script). Only Byron bootstrap addresses
still render with a hex marker pending the full Byron typed
parse.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Address.hs:347-373`
  (`putPtr`, `putVariableLengthWord64`, `word64ToWord7s` — the
  variable-length encoding writes 7 data bits per byte MSB-first
  with the high bit set on continuation bytes).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Credential.hs:230-258`
  (`SlotNo32`, `Ptr`, stock-derived `Show Ptr`).

## Changes

- Added `decode_addr_vlq_word64` helper that reads bytes
  MSB-first, accumulating 7-bit groups into a `u64` while the
  continuation bit is set. Caps at 10 bytes (≥ 70 bits, enough
  for any Word64) and rejects overflow.
- Added `decode_addr_ptr` helper that reads three consecutive
  VLQ Word64s (slot, tx_ix, cert_ix) and returns them as a tuple.
  Returns `None` on truncated or malformed input.
- Wired the pointer-address branch of `Addr::Display` to use
  `decode_addr_ptr`. Successful decode renders:
  `Addr <Net> (<payment>) (StakeRefPtr (Ptr (SlotNo32 N) (TxIx
  {unTxIx = N}) (CertIx {unCertIx = N})))` matching upstream
  stock-derived Show. Malformed tails route to
  `StakeRefPtr <malformed-ptr hex N bytes: ...>`.
- Extended R621's
  `addr_typed_display_covers_all_shelley_types` test:
  - Single-byte VLQ case (slot=5, tx_ix=3, cert_ix=7).
  - Multi-byte VLQ case (slot=300 encoded as 0x82 0x2C).
  - Truncated-tail rejection (only 2 VLQ ints provided —
    routes to malformed marker).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (229 lib + 4
  doctests + 1 main, net 0 vs R621 baseline of 229 — same test
  extended with 2 new assertions plus a malformed-tail check).

## Remaining (A5 Phase-2.5+)

- Typed Byron bootstrap parse (recovers network from address
  attributes; upstream `BootstrapAddress`).
- Alonzo 3-array TxOut + Babbage map-form TxOut (era-specific
  output shapes).
- Per-era predicate-failure tree mirrors for Allegra / Mary /
  Alonzo / Babbage / Conway eras.
