# Round R545 - tx-generator Script/Core pay-mode preflight

## Scope

Ported the transaction-stream preflight boundary from:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`

This round does not claim full transaction construction or submission.
The upstream `PayToScript` branch still depends on the later
`makePlutusContext` and `mkUTxOScript` slices.

## Implementation

- Added `SelectedCollateral` plus `select_collateral_funds`, preserving
  upstream empty-wallet and unsupported-era error shapes.
- Added `InterpretedPayMode` plus `interpret_pay_mode` for the
  key-output `PayToAddr` path, reusing the R544 `mk_utxo_variant` and
  `key_address` builders.
- Wired `Split`, `SplitN`, and `NtoM` preflight to verify wallets,
  network id, destination payment key, collateral funds, and upstream
  output-address trace points before value splitting.
- Left `PayToScript` on an explicit
  `interpretPayMode: PayToScript is pending makePlutusContext/mkUTxOScript`
  boundary.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator script::core --lib`
- `cargo test -p yggdrasil-tx-generator`
- `python scripts/check-strict-mirror.py --fail-on-violation`
- `python scripts/check-parity-matrix.py`
- `python .claude/scripts/filetree.py check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`

Focused result: 20 `Script/Core` tests passed. Crate result: 109
library tests and 5 CLI/golden tests passed. Workspace result:
`cargo test-all` passed with the existing 3 ignored tracer doctests.

## Remaining Work

- Port Plutus `makePlutusContext` and script-output UTxO construction.
- Port real transaction assembly and witness construction.
- Port `LocalSocket` and `Benchmark` submission behavior.
