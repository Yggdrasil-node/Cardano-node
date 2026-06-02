---
title: "Round 567 tx-generator DumpToFile Shelley/Mary"
parent: Reference
---

# Round 567 tx-generator DumpToFile Shelley/Mary

Date: 2026-05-20

## Scope

This round extends upstream `Benchmarking.Script.Core.submitInEra`
`SubmitMode::DumpToFile` coverage beyond the byte-equivalent Allegra
selftest fixture. The new slice covers key-witnessed Shelley and Mary
transaction streams, matching the upstream `'\n' : show tx` boundary
with explicit error boundaries for shapes that are not yet rendered.

## Changes

- Added Shelley `Show(Tx)` rendering for key-witnessed transaction
  bodies, including `MkShelleyTxBody`, upstream field names, body hash,
  `ShelleyTxWitsRaw`, and witness-set hash.
- Added Mary `Show(Tx)` rendering for key-witnessed coin-only outputs,
  including `MkMaryTxBody AllegraTxBodyRaw`, `MaryValue (Coin ...)`
  with empty `MultiAsset`, body hash, and witness-set hash.
- Kept unsupported optional fields, non-vkey witnesses, and Mary
  multi-asset values as explicit `TxGenError` boundaries instead of
  approximating Haskell `Show` output.
- Added script-core tests that execute Shelley and Mary `SplitN`
  streams through `SubmitMode::DumpToFile` and check the
  Haskell-shaped records.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator dumptofile_submit_generates`
- `cargo test -p yggdrasil-tx-generator`
- `cargo clippy -p yggdrasil-tx-generator --all-targets`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python dev/test/check-parity-matrix.py`
- `python dev/test/check-strict-mirror.py`
- `python dev/test/check-stale-placement.py`
- `python dev/test/filetree.py check`

## Remaining

- Extend `DumpToFile` rendering into Alonzo-family transaction shapes
  where upstream `Show` includes nested `AlonzoTxWits`,
  `TxDats`, and `Redeemers` memo hashes.
- Capture upstream-binary soak evidence for Benchmark scripts before
  promoting `tx-generator` out of `partial`.
