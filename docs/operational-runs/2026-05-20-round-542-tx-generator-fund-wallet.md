# Round R542 - tx-generator Fund/FundQueue/Wallet mirror

## Scope

Ported the upstream wallet/fund queue support used by
`Cardano.Benchmarking.Script.Core.evalGenerator` before transaction
assembly:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Internal/Fifo.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Fund.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/FundQueue.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Wallet.hs`

This round does not claim full transaction construction or submission
completion.

## Implementation

- Added `tx_generator/internal/fifo.rs` with the paired-list FIFO
  behavior used by upstream `FundQueue`.
- Added `tx_generator/fund.rs` with `Fund`, `FundInEra`, accessors, and
  TxIn-keyed equality/order.
- Added `tx_generator/fund_queue.rs` with upstream thin wrappers over
  the FIFO queue.
- Added `wallet.rs` with `WalletRef`, insertion, source, and preview
  semantics over `FundQueue`.
- Updated `script/env.rs` to use the upstream-shaped `Fund` and
  `WalletRef` carriers instead of an ad hoc Vec-backed wallet.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator`

Focused result: 94 library tests and 5 CLI/golden tests passed.

## Remaining Work

- Port `Cardano.TxGenerator.UTxO` and the value-splitting helpers used
  by `Split`, `SplitN`, and `NtoM`.
- Port real transaction assembly and witness construction.
- Port `LocalSocket` and `Benchmark` submission behavior.
