---
title: Round 540 Tx-Generator Script Core NtC Query
layout: default
parent: Operational Runs
nav_order: 540
---

# Round 540 - Tx-Generator Script/Core NtC Query

## Scope

R540 continues the strict mirror of upstream
`.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`.

The slice moves the Core query boundary beyond a placeholder:

- `queryEra` now builds and drives the upstream-shaped
  `BlockQuery (QueryHardFork GetCurrentEra)` LocalStateQuery payload.
- `queryRemoteProtocolParameters` now queries the active era first, then
  sends `QueryIfCurrent GetCurrentPParams` for that era.
- `getLocalConnectInfo` now carries the node-to-client network magic
  derived from the script `NetworkId`.
- Queried protocol parameters are preserved as era-native CBOR hex in
  `protocol-parameters-queried.json`, matching the upstream side effect
  that leaves a protocol-parameter evidence file on disk.
- Non-Unix builds keep an explicit Unix-domain socket error boundary,
  because Cardano node-to-client sockets are Unix-domain sockets in the
  current network stack.

## Upstream References

- `Cardano.Benchmarking.Script.Core.queryEra`
- `Cardano.Benchmarking.Script.Core.queryRemoteProtocolParameters`
- `Cardano.Benchmarking.Script.Core.getLocalConnectInfo`
- `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query`
- `Ouroboros.Consensus.Ledger.Query.queryEncodeNodeToClient`

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator`

Focused result: 74 tx-generator library tests and 5 CLI/golden tests
passed after the NtC query slice.

## Remaining Gate

R540 does not implement transaction construction or submission. The next
tx-generator rounds should port `GeneratorTx` stream construction and the
`LocalSocket` / `Benchmark` submission modes before any replacement claim
against upstream `tx-generator`.
