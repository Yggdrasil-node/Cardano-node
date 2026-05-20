# Round R544 - tx-generator UTxO output-builder mirror

## Scope

Ported the key-output builder surface from:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/UTxO.hs`

This round does not claim full transaction construction or submission
completion. The upstream `mkUTxOScript` path stays with the later
Plutus witness-builder slice because it depends on script context and
budget plumbing outside this file.

## Implementation

- Added `tx_generator/utxo.rs` with `ToUtxo`, `ToUtxoList`,
  `make_to_utxo_list`, `mk_utxo_variant`, and `key_address`.
- Derived Shelley-family enterprise payment addresses from
  `PaymentSigningKeyShelley_ed25519` TextEnvelope `cborHex` payloads
  using the existing pure-Rust `yggdrasil-crypto` Ed25519 surface and
  ledger `vkey_hash`.
- Built era-native outputs for Shelley/Allegra, Mary, Alonzo, and
  Babbage-family eras using `MultiEraTxOut`.
- Preserved upstream `zip3 fkts values [TxIx 0 ..]` behavior by
  truncating to the shorter builder/value list and assigning indexes
  from zero.

## Validation

- `cargo fmt --all`
- `cargo test -p yggdrasil-tx-generator`

Focused result: 106 library tests and 5 CLI/golden tests passed.

## Remaining Work

- Port script-output UTxO construction with Plutus witness context.
- Port real transaction assembly and witness construction.
- Port `LocalSocket` and `Benchmark` submission behavior.
