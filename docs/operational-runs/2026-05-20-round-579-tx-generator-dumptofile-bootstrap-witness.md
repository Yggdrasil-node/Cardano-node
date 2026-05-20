---
title: "Round 579 tx-generator DumpToFile bootstrap witnesses"
parent: Reference
---

# Round 579 tx-generator DumpToFile bootstrap witnesses

Date: 2026-05-20

## Scope

This round lifts the last `TxGenError` boundary inside the witness
set: bootstrap witnesses. Previously, any tx with non-empty
`bootstrap_witnesses` failed `SubmitMode::DumpToFile` with `does not
yet support bootstrap witnesses`. After this round
`show_alonzo_witness_set` renders bootstrap witnesses through
`atwrBootAddrTxWits = fromList [BootstrapWitness {bwKey, bwSignature,
bwChainCode, bwAttributes}, ...]` matching upstream stock-derived
`Show BootstrapWitness`.

With R579 the witness set is boundary-free across all 5 carrier
fields: vkey witnesses, native scripts, Plutus V1/V2/V3 scripts,
plutus data, redeemers, and now bootstrap witnesses.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Keys/Bootstrap.hs:67-110`
  (`ChainCode`, `BootstrapWitness`, `Ord BootstrapWitness`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxWits.hs:247`
  (`atwrBootAddrTxWits :: !(Set BootstrapWitness)`).

## Changes

- Replaced the bootstrap-witnesses rejection in
  `show_alonzo_witness_set` with positive rendering via two new
  helpers:
  - `show_alonzo_bootstrap_witnesses(&[BootstrapWitness]) -> String`
    emits `fromList [...]` with sorted entries.
  - `show_bootstrap_witness(&BootstrapWitness) -> String` emits the
    upstream record form with VKey / SignedDSIGN / ChainCode /
    ByteArray inner Shows.
- Re-used `show_haskell_bytestring` (R572) for the `chain_code` and
  `attributes` byte fields, producing Latin1-escaped quoted strings
  matching upstream `Show ByteString`.

## Byte-parity caveat

Upstream `Ord BootstrapWitness = comparing bootstrapWitKeyHash` where
`bootstrapWitKeyHash` is the Blake2b-224 of a Byron AddressInfo built
from `public_key + chain_code + attributes`. Yggdrasil does not yet
implement the Byron AddressInfo packing required for that hash, so
multi-witness sets sort here by the canonical `(public_key,
signature, chain_code, attributes)` tuple lex — deterministic within
a session and stable across reruns, but not byte-equivalent to
upstream for multi-witness sets. Single-witness cases are byte-
equivalent because the empty-set case is unaffected by ordering.

A future round can close upstream-`Ord` parity once a Byron
AddressInfo port lands.

## Changes

- `show_alonzo_witness_set` no longer rejects any
  `ShelleyWitnessSet` shape; the only validation it performs is the
  positive renderers' own per-field structural checks. Removed the
  former bootstrap-witnesses early return.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (37 tests, +3
  from R578)
- `cargo test -p yggdrasil-tx-generator` (220 lib tests + 5
  CLI/golden, +3 from R578 baseline)

## Remaining

- Render Conway `ProposalProcedures` OSet entries — needs `GovAction`
  Show (7 variants: ParameterChange, HardForkInitiation,
  TreasuryWithdrawals, NoConfidence, UpdateCommittee, NewConstitution,
  InfoAction) plus `AccountAddress` decoding for `pProcReturnAddr`.
- Close upstream `bootstrapWitKeyHash` byte-parity for multi-witness
  sets (Byron AddressInfo Blake2b-224).
- Full Haskell `Show (ByteString)` mnemonic-escape coverage for
  `\NUL`/`\SOH`/.../`\DEL` byte parity.
- Capture upstream-binary comparison evidence once a runnable upstream
  binary environment is available.
