# R534 tx-generator Testnet Discovery

Date: 2026-05-20

Area: sister-tools / `crates/tools/tx-generator`

## Scope

Continued the concrete `tx-generator` A4 implementation arc by porting
the upstream `Cardano.TxGenerator.Setup.TestnetDiscovery` surface. This
closes the `json_highlevel --testnet-config-dir` preparation layer, not
the transaction generation or submission runtime.

## Changes

- Added `src/setup.rs` to preserve the upstream
  `Cardano.TxGenerator.Setup.*` namespace boundary.
- Added `src/setup/testnet_discovery.rs` as the strict mirror of
  upstream `Setup/TestnetDiscovery.hs`.
- Ported cardano-testnet path conventions:
  `node-data/nodeN`, `utxo-keys/utxoN/utxo.skey`,
  `socket/nodeN/sock`, `configuration.yaml`, and node port files.
- Ported `parseNodeIndex`, `discoverNodes`, `readNodeDescription`,
  localhost `NodeDescription` JSON, `mergeValues`, and
  `validateFileExists`.
- Moved `TestnetConfig` out of `command.rs` and into the setup mirror,
  matching upstream ownership.
- Wired `json_highlevel --testnet-config-dir DIR` to read the user JSON
  config and perform testnet discovery / connection-setting merge before
  returning the explicit command-execution sentinel.

## Verification

- `cargo check -p yggdrasil-tx-generator`
- `cargo test -p yggdrasil-tx-generator` (27 lib tests + 4
  CLI/golden tests)

## Remaining Gate

The next tx-generator slices are script compile/run behavior,
generator transaction construction, and submission client runtime
parity. Operator swap-in remains blocked on the eventual upstream
binary comparison soak.
