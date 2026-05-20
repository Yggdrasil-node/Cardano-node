# Round R541 - tx-generator GeneratorTx/SizedMetadata mirror

## Scope

Ported the upstream
`.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx/SizedMetadata.hs`
surface into `crates/tools/tx-generator/src/generator_tx/sized_metadata.rs`.

This round is a transaction-construction dependency for
`Benchmarking.Script.Core.toMetadata` and the `NtoM` generator path. It
does not claim full `GeneratorTx` or submission completion.

## Upstream References

- `Cardano.Benchmarking.GeneratorTx.SizedMetadata`:
  `maxMapSize`, `maxBSSize`, `assume_cbor_properties`,
  `assumeMapCosts`, `measureMapCosts`, `assumeBSCosts`,
  `measureBSCosts`, `listMetadata`, `mkMetadata`.
- `Cardano.Benchmarking.Script.Core.toMetadata`: consumes
  `mkMetadata` for optional `NtoM` additional-size payloads.

## Implementation

- Added `generator_tx.rs` as the strict mirror parent for
  `Cardano.Benchmarking.GeneratorTx`.
- Added `generator_tx/sized_metadata.rs` with upstream-shaped metadata
  cost steps, bytes/map assumptions, deterministic CBOR metadata
  encoding, and the `mkMetadata` minimum-size/full-chunk algorithm.
- Wired `Script/Core.toMetadata` to use the new helper.
- Added `NtoM` metadata-size preflight inside the still-pending
  `submitInEra` boundary so invalid metadata sizes fail before the
  remaining transaction-construction sentinel.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator`

Focused result: 85 library tests and 5 CLI/golden tests passed.

## Remaining Work

- Port the rest of `GeneratorTx` transaction construction, including
  wallet/fund streaming and real transaction body/witness assembly.
- Port `LocalSocket` submission and `Benchmark` submission client
  behavior.
- Run end-to-end comparison against the upstream
  `.reference-haskell-cardano-node/install/bin/tx-generator` binary.
