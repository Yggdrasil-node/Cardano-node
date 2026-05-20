# R535 tx-generator NixService

Date: 2026-05-20

Area: sister-tools / `crates/tools/tx-generator`

## Scope

Continued the concrete `tx-generator` A4 implementation arc by porting
the upstream `Cardano.TxGenerator.Setup.NixService` high-level config
surface. This closes typed option parsing/projection for
`json_highlevel` and `compile`; it does not yet execute generated
scripts or submit transactions.

## Changes

- Added `src/types.rs` as the strict mirror subset of upstream
  `Cardano.TxGenerator.Types` needed by high-level config parsing:
  era names, tx-generator counters/rates, `TxGenTxParams`,
  `TxGenConfig`, and Plutus config discriminants/helpers.
- Added `src/setup/nix_service.rs` as the strict mirror of upstream
  `Setup/NixService.hs`.
- Moved `NodeDescription` ownership to the NixService mirror, matching
  upstream, and kept `Setup/TestnetDiscovery.hs` using that type.
- Ported `NixServiceOptions` JSON field names, non-empty
  `targetNodes`, `defaultKeepaliveTimeout`, `getKeepaliveTimeout`,
  `getNodeAlias`, `getNodeConfigFile`, `setNodeConfigFile`,
  `txGenTxParams`, `txGenConfig`, and `txGenPlutusParams`.
- Ported the command-layer `mangleNodeConfig` and `mangleTracerConfig`
  rules used by upstream `json_highlevel`.
- Changed `discover_testnet_config` to return typed `NixServiceOptions`
  instead of only a merged JSON value.
- Wired `json_highlevel` and `compile` to read and validate
  high-level config JSON before reaching the explicit
  command-execution sentinel.

## Verification

- `cargo check -p yggdrasil-tx-generator`
- `cargo test -p yggdrasil-tx-generator` (39 lib tests + 4
  CLI/golden tests)

## Remaining Gate

The next tx-generator slice is upstream `Compiler.hs` script generation,
followed by script run behavior, generator transaction construction, and
submission client runtime parity. Operator swap-in remains blocked on
the eventual upstream binary comparison soak.
