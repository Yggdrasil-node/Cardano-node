# R539 tx-generator Script/Core

Date: 2026-05-20

Area: sister-tools / `crates/tools/tx-generator`

## Scope

Continued the concrete `tx-generator` A4 implementation arc by adding
the strict `Cardano.Benchmarking.Script.Core` mirror and moving the
Core-owned state helpers out of the `Script/Action` dispatcher. This
round improves file/name parity and narrows the remaining Core work to
real node-to-client protocol/query behavior, transaction stream
evaluation, Plutus context construction, and submission.

## Changes

- Added `src/script/core.rs` as the strict mirror of upstream
  `Benchmarking/Script/Core.hs`.
- Moved Core-owned deterministic helpers into `script/core.rs`:
  `withEra`, `setProtocolParameters`, `readSigningKey`,
  `defineSigningKey`, `addFund`, `addFundToWallet`, `delay`,
  `waitBenchmarkCore`, `waitBenchmark`, `cancelBenchmark`,
  `getLocalConnectInfo`, `getProtocolParameters`, `initWallet`,
  `traceTxGeneratorVersion`, and `reserved`.
- Added explicit Core runtime boundaries for `queryEra`,
  `queryRemoteProtocolParameters`, `waitForEra`, `submitAction`, and
  `submitInEra`.
- Reduced `src/script/action.rs` to the upstream-style dispatcher plus
  local `startProtocol` bridge.
- Added `traceBenchTxSubmit`, `traceError`, and `traceDebug` helpers to
  `src/script/env.rs`, matching the upstream Env responsibility.

## Verification

- `cargo test -p yggdrasil-tx-generator` (67 lib tests + 5
  CLI/golden tests)

## Remaining Gate

The next tx-generator slice is the node-to-client and protocol half of
`Script/Core.hs`: real protocol initialisation, current-era query,
remote protocol-parameter query, and local connect info backed by the
yggdrasil runtime. Generator transaction construction and submission
client runtime parity remain pending.
