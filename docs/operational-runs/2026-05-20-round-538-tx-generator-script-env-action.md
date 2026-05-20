# R538 tx-generator Script/Env + Script/Action

Date: 2026-05-20

Area: sister-tools / `crates/tools/tx-generator`

## Scope

Continued the concrete `tx-generator` A4 implementation arc by porting
the upstream `Cardano.Benchmarking.Script` run boundary and the
state/action surfaces needed to execute deterministic script prefixes.
This makes `json FILEPATH` run supported state-only actions before
failing at the first unimplemented protocol/query/submission boundary.

## Changes

- Updated `src/script.rs` to mirror upstream
  `Cardano.Benchmarking.Script.hs` and expose `run_script`.
- Added `src/script/env.rs` as the strict mirror of upstream
  `Benchmarking/Script/Env.hs`.
- Ported the `Env` shape, `ProtocolParameterMode`, `Error`
  constructors, wallet/key/protocol placeholders, async-control
  placeholder, and accessor semantics.
- Added `src/script/action.rs` as the strict mirror of upstream
  `Benchmarking/Script/Action.hs`.
- Ported deterministic state-only action execution for
  `SetNetworkId`, `SetSocketPath`, `InitWallet`,
  `SetProtocolParameters`, `ReadSigningKey`, `DefineSigningKey`,
  `AddFund`, `Delay`, `LogMsg`, `Reserved`, `WaitBenchmark`, and
  `CancelBenchmark`.
- Left `StartProtocol`, `WaitForEra`, and `Submit` as explicit
  `TxGenError` runtime boundaries for the later `Script/Core` and
  `GeneratorTx` slices.
- Wired `json FILEPATH` to call `run_script` instead of stopping after
  JSON parsing.

## Verification

- `cargo test -p yggdrasil-tx-generator` (61 lib tests + 5
  CLI/golden tests)

## Remaining Gate

The next tx-generator slice is upstream `Script/Core.hs` protocol/query
behavior. Generator transaction construction and submission-client
runtime parity remain pending before operator swap-in can be tested
against the upstream binary.
