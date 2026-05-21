---
title: "Round 621 Typed Addr Display — Shelley header decoding (A5 Phase-2.5)"
parent: Reference
---

# Round 621 Typed Addr Display — Shelley header decoding (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Replaces R607's hex-marker Display on the `Addr` wrapper with a
typed Display that decodes the header byte and renders the
upstream stock-derived `Show Addr` shape for all 8 Shelley address
types. Byron bootstrap addresses still render as a hex marker
(typed Byron parse pending). Pointer addresses render the typed
payment credential with the pointer tail as a hex marker (typed
`Ptr` decoder pending).

After R621, the typed Shelley LEDGER predicate-failure tree
renders **full upstream-shape addresses** through all UTxO-bearing
variants (UTXO tag 8 WrongNetwork, tag 9 WrongNetworkWithdrawal,
tags 6/10 NonEmpty TxOut).

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Address.hs:167-170`
  (`data Addr = Addr Network (Credential Payment)
  StakeReference | AddrBootstrap BootstrapAddress`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Address.hs:263-280`
  (header-bit constants: `byron=7`, `notBaseAddr=6`,
  `isEnterpriseAddr=5`, `stakeCredIsScript=5`,
  `payCredIsScript=4`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Credential.hs:217-221`
  (`data StakeReference = StakeRefBase | StakeRefPtr |
  StakeRefNull`).

## Changes

- Extended `Addr::Display` to decode the header byte and render
  the typed Shelley shape:
  - Header bit 7 (`0x80`) → Byron bootstrap branch. Renders as
    `AddrBootstrap <hex N bytes: ...>`.
  - Header bits 6-7 == `0b01_1x` (high nibble 0x60/0x70) →
    enterprise address. Renders as
    `Addr <Network> (<KeyHashObj|ScriptHashObj> (<hash>))
    StakeRefNull`.
  - Header bits 6-7 == `0b01_0x` (high nibble 0x40/0x50) →
    pointer address. Renders as
    `Addr <Network> (<payment>) (StakeRefPtr <hex N bytes>)` —
    the variable-length pointer tail keeps a hex marker pending
    a typed `Ptr` decoder.
  - Header bits 6-7 == `0b00_xx` (high nibble 0x00-0x30) →
    base address. Renders as
    `Addr <Network> (<payment>) (StakeRefBase (<stake>))` where
    both payment and stake credentials use the typed KeyHashObj
    vs ScriptHashObj rendering.
- Added `Addr::network_from_header` helper extracting the
  network from the header byte's low nibble. Byron bootstrap
  defaults to Mainnet pending the full Byron attribute decode.
- Updated R609's `_output_too_small_decodes_tag6` test
  assertion: header `0x61` (enterprise/key/Mainnet) now renders
  the typed `Addr Mainnet (KeyHashObj (KeyHash {unKeyHash =
  "aaaa..."})) (StakeRefNull)` shape.
- Updated R607's `_wrong_network_decodes_tag8` test
  similarly.
- Updated R620's `_shelley_tx_out_typed_round_trip` test —
  header `0xE1` has bit 7 set so the typed Display routes to
  the Bootstrap branch (`AddrBootstrap <hex 29 bytes: ...>`).
- Added focused `addr_typed_display_covers_all_shelley_types`
  test exercising all 5 typed shapes (base key/key, base
  script/script, enterprise script, pointer key, byron
  bootstrap).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (229 lib + 4
  doctests + 1 main, +1 net new test vs R620 baseline of 228)

## Remaining (A5 Phase-2.5+)

- Typed `Ptr` decoder (variable-length `slot tx_ix cert_ix`
  triple per upstream `Ptr`).
- Typed Byron bootstrap parse (recovers network from address
  attributes; upstream `BootstrapAddress`).
- Alonzo 3-array TxOut + Babbage map-form TxOut (era-specific
  output shapes).
- Per-era predicate-failure tree mirrors for Allegra / Mary /
  Alonzo / Babbage / Conway (separate enum trees with their own
  per-era variant additions).
