---
title: "Round 560 tx-generator StrictSeq selftest parity"
parent: Reference
---

# Round 560 tx-generator StrictSeq selftest parity

Date: 2026-05-20

## Scope

Closed the R559 Allegra selftest body-hash drift. This round mirrors:

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-binary/src/Cardano/Ledger/Binary/Encoding/Encoder.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/allegra/impl/src/Cardano/Ledger/Allegra/TxBody.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Selftest.hs`

## Changes

- Added a shared CBOR helper for upstream `StrictSeq` variable-length
  encoding: definite arrays through 23 elements and indefinite arrays
  for larger sequences.
- Applied that helper to Shelley-family transaction body outputs and
  certificates across Shelley, Allegra, Mary, Alonzo, Babbage, and
  Conway body encoders.
- Added matching decode support for definite and indefinite transaction
  output/certificate sequences where the local decoders only accepted
  definite arrays.
- Added an Allegra round-trip guard for a 24-output body and pinned the
  tx-generator selftest first final transaction to the upstream body
  hashes.

## Upstream Evidence

The vendored upstream static binary was run under WSL with its Cabal
data directory supplied explicitly:

```text
tx_generator_datadir=/mnt/v/workspace/Cardano-node/.reference-haskell-cardano-node/bench/tx-generator \
  .reference-haskell-cardano-node/install/bin/tx-generator \
  selftest /tmp/txgen-selftest.fifo
```

A FIFO reader captured all four `DumpToFile` overwrites: the three setup
split stages plus the final stream. After the fix, Rust output matched
the captured upstream bytes:

```text
stage1: 2,732 bytes, sha256 154c0b2c9cabbd492f9a8c62a088b64c5a15dd5dcfe497e975bec7805a9ff83b
stage2: 55,620 bytes, sha256 894cdcd3f92db862acbeb9addc3cd7a012e82efa47257f54706633252a31c22d
stage3: 1,650,800 bytes, sha256 fc765d9c087a1a7cedca3d7b19f0035ce99868e50d358596d3e960353edcb648
stage4: 6,809,330 bytes, sha256 3dd3232454186a3660fa5a112ba52825dc8c3d8a2f22d5b768b8962c50f591b9
```

The drift root cause was the stage-2 30-output split. Upstream
`cardano-ledger-binary` uses indefinite sequence encoding once
`StrictSeq` length exceeds 23; the Rust ledger had emitted definite
arrays for every output count.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-ledger tx_body_outputs_above_strict_seq_threshold_use_indefinite_array
cargo test -p yggdrasil-tx-generator run_selftest_with_output_file_writes_haskell_show_transactions
cargo run -p yggdrasil-tx-generator -- selftest target/parity-evidence/rust-txgen-selftest-fixed.out
```

Observed result:

```text
Allegra StrictSeq threshold test: passed
tx-generator selftest output test: passed
upstream stage4 sha256: 3dd3232454186a3660fa5a112ba52825dc8c3d8a2f22d5b768b8962c50f591b9
rust stage4 sha256:     3dd3232454186a3660fa5a112ba52825dc8c3d8a2f22d5b768b8962c50f591b9
```

## Remaining Tx-Generator Gaps

The Allegra key-witnessed selftest `DumpToFile` path is now
byte-equivalent for the deterministic upstream fixture. Remaining work
is broader `DumpToFile` `Show (Tx)` coverage and the
`GeneratorTx.Submission` Benchmark client/scheduler surface.
