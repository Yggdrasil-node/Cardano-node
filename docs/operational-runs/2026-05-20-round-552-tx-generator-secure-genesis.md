---
title: "Round 552 tx-generator SecureGenesis"
parent: Reference
---

# Round 552 tx-generator SecureGenesis

Date: 2026-05-20

## Scope

Closed the `SecureGenesis` placeholder in the pure-Rust `tx-generator`
port. This round mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Genesis.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Action.hs`

## Changes

- Added `tx_generator/genesis.rs` as the strict mirror for
  `Cardano.TxGenerator.Genesis`.
- `startProtocol` now resolves, hash-verifies, loads, and prepares
  Shelley genesis `initialFunds` through `yggdrasil-node-genesis`.
- `Generator::SecureGenesis` now finds the matching genesis initial
  fund by key-derived address, spends the genesis pseudo-input, applies
  `txParamFee` and `txParamTTL`, signs with the GenesisUTxO key, and
  inserts the resulting payment fund into the target wallet.
- Tx-generator TextEnvelope seed decoding now accepts both
  `PaymentSigningKeyShelley_ed25519` and
  `GenesisUTxOSigningKey_ed25519`, matching upstream
  `Setup.SigningKey` casting behavior.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-tx-generator --lib tx_generator::genesis
cargo test -p yggdrasil-tx-generator --lib script::action
cargo test -p yggdrasil-tx-generator --lib
```

Observed result:

```text
tx_generator::genesis: 2 passed
script::action: 5 passed
yggdrasil-tx-generator --lib: 141 passed
```

## Remaining Tx-Generator Gaps

SecureGenesis is no longer a parity blocker. Remaining concrete gaps are
Plutus pre-execution / auto-budget fitting, script-spend integrity
hashing, exact `DumpToFile` rendering, Benchmark submission,
`RoundRobin` / `OneOf`, selftest execution, and upstream binary
comparison evidence.
