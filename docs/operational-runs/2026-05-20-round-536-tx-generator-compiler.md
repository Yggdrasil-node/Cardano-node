# R536 tx-generator Compiler

Date: 2026-05-20

Area: sister-tools / `crates/tools/tx-generator`

## Scope

Continued the concrete `tx-generator` A4 implementation arc by porting
the upstream `Cardano.Benchmarking.Compiler` script-generation surface
and the `Cardano.Benchmarking.Script.Types` IR needed to represent its
output. This makes `compile FILEPATH` functional; it does not yet run
the generated script or submit transactions.

## Changes

- Added `src/script.rs` to preserve the upstream
  `Cardano.Benchmarking.Script.*` namespace boundary.
- Added `src/script/types.rs` as the strict mirror of upstream
  `Benchmarking/Script/Types.hs` for the generated `Action`,
  `Generator`, `SubmitMode`, `PayMode`, `ScriptBudget`, and
  `ScriptSpec` surfaces used by compiler output.
- Added `src/compiler.rs` as the strict mirror of upstream
  `Benchmarking/Compiler.hs`.
- Ported `compileOptions` / `compileToScript`, fixed signing-key names,
  payment-signing-key text-envelope output, `initConstants`, genesis
  fund import, optional collateral setup, split planning,
  `unfoldSplitSequence`, `evilFeeMagic`, benchmarking-phase
  submit-mode selection, and Plutus script-budget checks.
- Wired `compile FILEPATH` to write generated action JSON to stdout.
- Wired `json_highlevel` to compile its final `NixServiceOptions`
  before reaching the explicit runtime-execution sentinel.

## Verification

- `cargo test -p yggdrasil-tx-generator` (49 lib tests + 5
  CLI/golden tests)

## Remaining Gate

The next tx-generator slice is upstream script run behavior, followed by
generator transaction construction and submission client runtime parity.
Operator swap-in remains blocked on the eventual upstream binary
comparison soak.
