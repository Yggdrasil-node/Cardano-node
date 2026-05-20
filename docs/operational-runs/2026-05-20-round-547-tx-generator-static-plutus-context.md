# Round 547 - tx-generator static Plutus context

## Scope

- Ported upstream `Cardano.TxGenerator.Setup.Plutus.readPlutusScript` for
  `.plutus` TextEnvelope loading and bundled `scripts-fallback` resolution.
- Ported upstream `Cardano.TxGenerator.PlutusContext.readScriptData` and
  `scriptDataModifyNumber` for detailed-schema Plutus datum/redeemer JSON.
- Wired the static-budget branch of `Script/Core.hs::makePlutusContext` into
  `interpretPayMode`, so `PayToScript` can now produce `mkUTxOScript` builders
  with real datum, redeemer, execution-unit, and script-byte witness data.

## Boundaries left explicit

- `preExecutePlutusScript` is still pending the Plutus evaluator integration.
- `plutusAutoScaleBlockfit` / `AutoScript` budget fitting is still pending.
- Full GeneratorTx transaction assembly and LocalSocket / Benchmark submission
  remain in the next strict-mirror slices.

## Focused validation

```text
cargo test -p yggdrasil-tx-generator --lib
120 passed; 0 failed
```
