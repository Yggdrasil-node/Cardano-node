# Guidance for the pure-Rust port of upstream `tx-generator`.

**Status:** `partial` (post-R540 Script/Core NtC query slice). The old
cardano-cli CLI-MVS prerequisite is closed; concrete work here is now
the tx-generator Script / GeneratorTx / Submission
implementation arc plus upstream comparison evidence. Scope band:
**LARGE**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/bench/tx-generator/`
(46 `.hs` files).

## Mini-arc scope

Transaction-stream load generator for benchmarking. The active arc
starts from the vendored `Command.hs`, `Setup/*`, and
`GeneratorTx/Submission.hs` surfaces, then finishes with an end-to-end
soak against a yggdrasil node on preview. The Calibrate sub-tree
carve-out (Compiler.hs, Benchmarking/Script/*, PureExample) remains an
approved synthesis area from the sister-tools plan.

## Current Functional Surface

- Shipped: `<binary> --help` byte-equivalent to upstream (golden test
  pinned in `tests/cli_help_golden.rs`).
- Shipped: `<binary> --version` byte-equivalent to upstream.
- Shipped R533: `Command.hs` parser surface. `command.rs` mirrors the
  upstream `Command` sum type and `commandParser` grammar for `json`,
  `json_highlevel`, `compile`, `selftest`, and `version`.
- Shipped R533: `parser::Args` now carries typed `command::Command`
  instead of raw passthrough.
- Shipped R534: `Setup/TestnetDiscovery.hs` surface. `setup/testnet_discovery.rs`
  discovers `cardano-testnet` output directories, reads node port files,
  builds localhost `targetNodes`, and deep-merges discovered connection
  settings over user JSON config.
- Shipped R534: `json_highlevel --testnet-config-dir DIR` now reads the
  high-level config JSON and performs testnet discovery before reaching
  the command-execution sentinel.
- Shipped R535: `Setup/NixService.hs` high-level config surface.
  `setup/nix_service.rs` parses `NixServiceOptions`, owns upstream
  `NodeDescription`, projects `txGenTxParams` / `txGenConfig` /
  `txGenPlutusParams`, and applies `nodeConfig` / cardano-tracer CLI
  override rules.
- Shipped R535: `json_highlevel` and `compile` now read and validate
  high-level config JSON before reaching their command-execution
  sentinel; `discover_testnet_config` now returns typed
  `NixServiceOptions` like upstream.
- Shipped R536: `Compiler.hs` high-level script generation surface.
  `compiler.rs` emits typed `Action` scripts from `NixServiceOptions`,
  including fixed signing-key envelopes, genesis import, collateral
  setup, split planning, benchmark submission mode selection, and the
  upstream split/fee helper arithmetic.
- Shipped R536: `Benchmarking/Script/Types.hs` action/generator IR
  surface. `script/types.rs` serializes the generated script with
  upstream ObjectWithSingleField-style action, generator, submit-mode,
  pay-mode, and script-budget wrappers.
- Shipped R536: `compile FILEPATH` is functional and writes the
  generated script JSON to stdout; `json_highlevel` compiles its final
  options before reaching the runtime-execution sentinel.
- Shipped R537: `Benchmarking/Script/Aeson.hs` script JSON surface.
  `script/aeson.rs` parses low-level script files and
  `script/types.rs` now decodes upstream ObjectWithSingleField-style
  `Action`, `Generator`, submit-mode, pay-mode, protocol-parameter,
  and script-budget wrappers.
- Shipped R537: `json FILEPATH` now reads and validates low-level
  script JSON before reaching the runtime-execution sentinel.
- Shipped R538: `Benchmarking/Script/Env.hs` state surface.
  `script/env.rs` owns the upstream `Env`, `ProtocolParameterMode`,
  `Error`, wallet/key/protocol placeholders, and accessor semantics
  used by action execution.
- Shipped R538: `Benchmarking/Script/Action.hs` dispatch surface.
  `script/action.rs` executes deterministic state-only actions
  (`SetNetworkId`, `SetSocketPath`, `InitWallet`,
  `SetProtocolParameters`, `ReadSigningKey`, `DefineSigningKey`,
  `AddFund`, `Delay`, `LogMsg`, `Reserved`, and benchmark-control
  checks) and returns explicit runtime-pending errors for protocol,
  query, transaction-generation, and submission actions.
- Shipped R538: `json FILEPATH` now calls `run_script`, so low-level
  scripts execute their supported state prefix before failing at the
  first missing async/runtime boundary.
- Shipped R539: `Benchmarking/Script/Core.hs` state-helper surface.
  `script/core.rs` now owns upstream-shaped `withEra`,
  `setProtocolParameters`, signing-key loading/definition,
  fund insertion, delay, benchmark-control checks, local-connect-info
  carrier, protocol-parameter mode resolution, `submitAction`
  boundary, `initWallet`, version tracing, and `reserved`.
- Shipped R539: `Benchmarking/Script/Action.hs` now mirrors the
  upstream split more closely: dispatch remains in `script/action.rs`,
  while Core-owned action bodies live in `script/core.rs`.
- Shipped R540: `Benchmarking/Script/Core.hs` node-to-client query
  surface. `queryEra` and `queryRemoteProtocolParameters` now build the
  upstream LocalStateQuery envelopes (`QueryHardFork GetCurrentEra` and
  `QueryIfCurrent GetCurrentPParams`), drive the NtC socket on Unix,
  preserve era-native protocol-parameter CBOR in
  `protocol-parameters-queried.json`, and keep non-Unix builds on an
  explicit Unix-socket boundary.
- Pending: concrete command execution. Dispatch returns a
  command-specific "not yet implemented" sentinel until the GeneratorTx
  construction and submission slices land.
- Pending: end-to-end behavioral tests against the upstream binary.

## Build + Run

```bash
# Build (release).
cargo build --release -p yggdrasil-tx-generator

# Run via the universal launcher (recommended).
scripts/run-tools.sh tx-generator --help
scripts/run-tools.sh tx-generator --version

# Or invoke the binary directly:
target/release/tx-generator --help
```

The binary is named `tx-generator` (matching upstream exactly).
Operators can swap upstream's binary for the yggdrasil one in their
automation once concrete dispatch and upstream comparison evidence land.

## Rules

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `tx-generator` is the
  acceptance gate for any concrete implementation.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies from
  crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream ships a
  new release with different help output, refresh the fixtures + bump
  the relevant SHA pin in `crates/node/config/src/upstream_pins.rs` as
  a coordinated round.

## Round Roadmap

This crate's full implementation remains an A4 sister-tool build-out:

- Shipped: skeleton (R327 + R335-pattern bulk skeleton at R335-R336).
- Shipped: Command parser (R533): `Command.hs` `Command`,
  `TestnetConfig`, and command-parser grammar.
- Shipped: Testnet discovery (R534): `Setup/TestnetDiscovery.hs`
  path conventions, node discovery, JSON deep-merge, and runtime
  `json_highlevel --testnet-config-dir` preparation.
- Shipped: Nix-service options (R535): `Setup/NixService.hs`
  high-level JSON shape, target-node parsing, config/tracer override
  helpers, and tx-generator parameter projections.
- Shipped: Compiler/script generation (R536): `Compiler.hs`
  `compileOptions` plus the `Script/Types.hs` IR needed for generated
  scripts; `compile` now emits generated action JSON.
- Shipped: Script JSON parsing (R537): `Script/Aeson.hs`
  `parseScriptFileAeson`, `scanScriptFile`, JSON round-trip checking,
  and low-level `json FILEPATH` script validation.
- Shipped: Script state/action execution (R538): `Script.hs`
  `runScript` boundary plus `Script/Env.hs` state/accessors and
  `Script/Action.hs` deterministic state-only action dispatch.
- Shipped: Script/Core state helpers (R539): `Script/Core.hs`
  non-network state helpers and explicit runtime boundaries moved into
  a strict mirror file.
- Shipped: Script/Core NtC query behavior (R540): `queryEra` /
  `queryRemoteProtocolParameters` use upstream LocalStateQuery wire
  shapes and write queried protocol-parameter evidence.
- Next: port GeneratorTx transaction construction and LocalSocket /
  Benchmark submission in strict-mirror-sized slices.
- Closeout: when all subcommands are functional, parity-matrix entry
  advances `partial -> verified_11_0_1`. Operators can then swap
  upstream binary for the yggdrasil binary without script changes.

## Comparison With Upstream

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-tx-generator

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/tx-generator --help) \
     <(target/debug/tx-generator --help)
diff <(.reference-haskell-cardano-node/install/bin/tx-generator --version) \
     <(target/debug/tx-generator --version)
# (empty diffs expected; byte-equivalent)
```

## Maintenance Guidance

- Update this AGENTS.md when concrete command implementations land.
- Keep the per-tool migration status in sync with
  `docs/COMPLETION_ROADMAP.md` and `docs/parity-matrix.json`.
- If upstream ships a new release: refresh the help/version fixtures,
  advance the relevant SHA pin in `upstream_pins.rs`, and re-run the
  full cargo gate.
