# Round R546 - tx-generator script UTxO output builders

## Scope

Ported the script-output half of:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/UTxO.hs`

This round does not claim full `PayToScript` execution. The upstream
`Script/Core.makePlutusContext` path still needs to read scripts,
datum/redeemer files, budgets, protocol parameters, and witnesses
before `interpretPayMode` can call `mkUTxOScript` end to end.

## Implementation

- Extended `ToUtxo` into a single upstream-shaped key-or-script
  builder instead of keeping script outputs as a separate future shape.
- Added `ScriptLanguage`, `ScriptInAnyLang`, `mk_utxo_script`,
  `script_address`, `script_hash`, and `script_data_hash`.
- Built Plutus script enterprise addresses from
  `Blake2b-224(language_tag || script_bytes)`.
- Built Alonzo outputs with `datum_hash = Some(hashScriptDataBytes
  datum)` and Babbage-family outputs with `DatumOption::Hash`.
- Preserved upstream era/language failure strings:
  `scriptDataSupportedInEra==Nothing` and
  `scriptLanguageSupportedInEra==Nothing`.
- Updated `Fund` so script-created funds retain their script witness and
  carry no signing key, matching upstream `mkUTxOScript`.

## Validation

- `cargo fmt --all`
- `cargo test -p yggdrasil-tx-generator tx_generator::utxo --lib`
- `cargo test -p yggdrasil-tx-generator tx_generator::fund --lib`
- `cargo test -p yggdrasil-tx-generator script::core --lib`
- `cargo test -p yggdrasil-tx-generator`
- `cargo clippy -p yggdrasil-tx-generator --all-targets`
- `python dev/test/check-strict-mirror.py --fail-on-violation`
- `python dev/test/check-parity-matrix.py`
- `python dev/test/check-stale-placement.py`
- `python dev/test/filetree.py check`
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`

Focused result: 9 `TxGenerator.UTxO` tests, 5 fund/fund-queue tests, and
20 `Script/Core` tests passed. Crate result: 113 library tests and 5
CLI/golden tests passed. Workspace result: `cargo test-all` passed with
the existing 3 ignored tracer doctests.

## Remaining Work

- Port Plutus `makePlutusContext` and wire `PayToScript` end to end.
- Port real transaction assembly and witness construction.
- Port `LocalSocket` and `Benchmark` submission behavior.
