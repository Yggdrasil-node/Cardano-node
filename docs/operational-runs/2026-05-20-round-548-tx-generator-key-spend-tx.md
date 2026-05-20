# Round 548 - tx-generator key-spend transaction construction

## Scope

- Ported upstream `Cardano.TxGenerator.Tx` into
  `crates/tools/tx-generator/src/tx_generator/tx.rs`.
- Added `sourceToStoreTransaction`, `sourceToStoreTransactionNew`,
  `sourceTransactionPreview`, signed Shelley-family `genTx`, and
  `txSizeInBytes` for key-witnessed inputs.
- Added upstream `Benchmarking.Wallet` helper coverage for
  `createAndStore`, `mangle`, and `mangleWithChange`, so generated
  tx ids now feed directly into stored wallet funds.

## Boundaries left explicit

- Plutus script spends still fail at an explicit script-integrity /
  pre-execution boundary. Creating script outputs and carrying witness
  data remains wired from R546-R547.
- `Script/Core.submitInEra` still needs the runtime stream/submission
  slice before command execution stops returning its sentinel.

## Focused validation

```text
cargo test -p yggdrasil-tx-generator --lib tx_generator::tx
5 passed; 0 failed

cargo test -p yggdrasil-tx-generator --lib wallet
8 passed; 0 failed

cargo clippy -p yggdrasil-tx-generator --all-targets
clean
```
