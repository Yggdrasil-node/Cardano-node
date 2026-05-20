# R537 tx-generator Script/Aeson

Date: 2026-05-20

Area: sister-tools / `crates/tools/tx-generator`

## Scope

Continued the concrete `tx-generator` A4 implementation arc by porting
the upstream `Cardano.Benchmarking.Script.Aeson` script JSON boundary.
This makes `json FILEPATH` read and validate low-level transaction
generator scripts before reaching the explicit runtime-execution
sentinel. It does not yet execute actions or submit transactions.

## Changes

- Added `src/script/aeson.rs` as the strict mirror of upstream
  `Benchmarking/Script/Aeson.hs`.
- Ported `testJSONRoundTrip`, deterministic pretty printing,
  `scanScriptFile`, `parseJSONFile`, and `parseScriptFileAeson`.
- Extended `src/script/types.rs` with upstream-shaped
  ObjectWithSingleField decoding for `Action`, `Generator`,
  `ProtocolParametersSource`, `SubmitMode`, `PayMode`, and
  `ScriptBudget`.
- Added script-level `NetworkId` JSON handling for upstream
  `"Mainnet"` / `{ "Testnet": n }` values.
- Added `Dijkstra` to the tx-generator era surface so low-level
  script parsing accepts the latest upstream era constructor.
- Wired `json FILEPATH` to parse low-level script JSON and report the
  parsed action count before the runtime-execution sentinel.

## Verification

- `cargo test -p yggdrasil-tx-generator` (52 lib tests + 5
  CLI/golden tests)

## Remaining Gate

The next tx-generator slice is upstream script execution: `Script/Core`
and `Script/Action` state transitions, then `GeneratorTx` construction
and submission-client runtime parity. Operator swap-in remains blocked
on the eventual upstream binary comparison soak.
