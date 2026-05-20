# Round 549 - tx-generator finite submitInEra runtime

Date: 2026-05-20
Area: sister-tools / `crates/tools/tx-generator`

## Summary

- Wired upstream `Benchmarking.Script.Core.submitInEra` through finite
  key-spend transaction generation for `Split`, `SplitN`, `NtoM`,
  `Sequence`, and `Take (Cycle ...)`.
- `DiscardTX` now forces real generated `MultiEraSubmittedTx` values
  and updates source/destination wallets through upstream-shaped
  source/store semantics.
- `LocalSocket` now submits finite generated streams through the
  existing NtC LocalTxSubmission client.
- `NtoM` now performs the upstream-style preview pass and records
  projected transaction-size traces before consuming source funds.

## Remaining Boundaries

- `DumpToFile` remains blocked on byte-equivalent upstream `Show (Tx)`
  rendering rather than emitting a non-parity local format.
- `Benchmark` remains blocked on the `GeneratorTx.Submission`
  scheduler/client slice.
- `SecureGenesis`, `RoundRobin`, `OneOf`, and Plutus script-spend
  integrity/pre-execution remain explicit parity boundaries.

## Validation

```text
cargo test -p yggdrasil-tx-generator --lib script::core
cargo test -p yggdrasil-tx-generator --lib
cargo clippy -p yggdrasil-tx-generator --all-targets
```
