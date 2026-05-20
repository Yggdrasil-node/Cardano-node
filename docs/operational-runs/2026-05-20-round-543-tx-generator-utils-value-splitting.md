# Round R543 - tx-generator Utils value-splitting mirror

## Scope

Ported the pure value-splitting helpers used by
`Cardano.Benchmarking.Script.Core.evalGenerator` before transaction
assembly:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Utils.hs`

This round does not claim full transaction construction or submission
completion.

## Implementation

- Added `tx_generator/utils.rs` with `inputs_to_outputs_with_fee`,
  `include_change`, and `mk_tx_in`.
- Added `PayWithChange` to the local `TxGenerator.Types` mirror.
- Updated `Script/Core.submitInEra` to preflight `Split`, `SplitN`,
  and `NtoM` wallet value splitting before the remaining
  transaction-build sentinel.
- Kept `NtoM` metadata sizing preflight from R541 in the same
  generator preflight path.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator`
- `git diff --check`

Focused result: 100 library tests and 5 CLI/golden tests passed.

## Remaining Work

- Port `Cardano.TxGenerator.UTxO` output builders.
- Port real transaction assembly and witness construction.
- Port `LocalSocket` and `Benchmark` submission behavior.
