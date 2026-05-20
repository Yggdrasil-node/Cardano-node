---
title: "Round 559 tx-generator DumpToFile"
parent: Reference
---

# Round 559 tx-generator DumpToFile

Date: 2026-05-20

## Scope

Advanced the upstream `submitInEra` `DumpToFile` branch for the
deterministic Allegra selftest path. This round mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Selftest.hs`

## Changes

- `SubmitMode::DumpToFile` now evaluates finite transaction streams and
  writes newline-prefixed Haskell `Show (Tx)` records instead of
  returning the old `DumpToFile` sentinel.
- The first supported renderer covers the upstream selftest shape:
  `ShelleyTx ShelleyBasedEraAllegra` with key-witnessed Shelley-family
  outputs, empty certs/withdrawals/update/auxiliary data, and
  `ShelleyTxWitsRaw` vkey witnesses.
- Unsupported era, witness, address, and auxiliary-data shapes remain
  explicit `TxGenError` boundaries so the tool does not emit a local
  non-parity format.
- `selftest FILEPATH` now succeeds and writes 4,000 rendered
  transactions; command-dispatch and selftest unit coverage were updated
  from "expected boundary" to "expected output file".

## Upstream Evidence

The vendored upstream binary needs `tx_generator_datadir` set on this
Windows workspace because its static build otherwise looks for the
fixture under a Nix store path:

```text
tx_generator_datadir=/mnt/v/workspace/Cardano-node/.reference-haskell-cardano-node/bench/tx-generator \
  .reference-haskell-cardano-node/install/bin/tx-generator \
  selftest /mnt/v/workspace/Cardano-node/target/parity-evidence/upstream-txgen-selftest.out
```

Observed upstream output:

```text
bytes: 6,813,330
non-empty records: 4,000
first record prefix:
ShelleyTx ShelleyBasedEraAllegra (ShelleyTx {stBody = MkAllegraTxBody
```

Rust output was generated with:

```text
target/debug/tx-generator selftest target/parity-evidence/rust-txgen-selftest.out
```

Comparison result:

```text
upstream sha256: 7e84fd1e99064f5c6fe6a7cc581163a66f612b3120760c91e6d34e60fa5403af
rust sha256:     4217d7d1f039e4569a18e5bc152c86806dd8b129285ef2cb3fbccd01a180181f
first record length: 1,700 bytes both sides
first differing semantic field:
  upstream input tx id: 3986ae75caaf853a53e6963288c680baf8a7be1239eceec7705d7ef6f045700a
  rust input tx id:     3f1ccb88206b3362b3be0106361ba57c8308a6c35d3592a311b6aaf659a5372f
```

The output formatter is now executable, but full byte-equivalence is not
claimed. The upstream comparison exposed a generated transaction
body/signature drift before the final selftest `NtoM` stream.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-tx-generator selftest
cargo check -p yggdrasil-tx-generator
```

Observed result:

```text
selftest filter: 5 passed
yggdrasil-tx-generator check: passed
```

## Remaining Tx-Generator Gaps

The next tx-generator slice should diff the preceding selftest split
transactions against upstream to close the body-hash/signature drift
before expanding `DumpToFile` beyond the Allegra key-witnessed selftest
shape. Benchmark submission remains open on the `GeneratorTx.Submission`
client/scheduler surface.
