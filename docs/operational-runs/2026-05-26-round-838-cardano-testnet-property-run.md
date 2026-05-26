# Round 838 - cardano-testnet Property/Run pure harness-control helpers

## Scope

Port the pure, Rust-observable surface of upstream
`Testnet/Property/Run.hs` without starting the actual Hedgehog/Tasty
resource runner or indefinite `runTestnet` keepalive. The slice covers
`UserProvidedEnv`, the OS-ignore helper dispositions, and the operator-facing
message printed once `runTestnet` has captured a `TestnetRuntime`.

The remaining `cardano` and `create-env` execution bodies stay deferred until
node/KES spawning, era-genesis, runtime query workflows, and the concrete
Process/Property execution harness land.

## Upstream facts

- `UserProvidedEnv` has two constructors: `NoUserProvidedEnv` and
  `UserProvidedEnv FilePath`.
- `ignoreOn` reports `IGNORED on <os>` through both the Tasty success reason
  and `resultShortDescription`.
- `ignoreOnWindows`, `ignoreOnMac`, `ignoreOnMacAndWindows`, and `disabled`
  wrap a property into either a normal test tree or an ignored test tree.
- After a runtime is captured, `runTestnet` prints the config path, then either
  the first SPO node log/socket/network-id guidance or a "Failed to find any
  SPO node" warning, and finally `Type CTRL-C to exit.`

## Changes

- Added `property/run.rs` as the strict mirror of upstream
  `cardano-testnet/src/Testnet/Property/Run.hs`.
- Added `UserProvidedEnv` plus `workspace_hint` for the stable env-mode
  projection.
- Added `IgnoredProperty`, `PropertyDisposition`, `ignore_on`, `disabled`,
  Windows/Mac ignore helpers, and current-platform wrappers.
- Added `render_running_testnet_message`, using `TestnetRuntime` and
  `spo_nodes` to preserve the upstream operator guidance without spawning a
  live testnet.
- Wired `pub mod run` under `property.rs`.
- Added three focused tests for env mode, ignore helper behavior, SPO runtime
  message rendering, and the missing-SPO branch.
- Updated cardano-testnet status docs, parity matrix evidence, stale-status
  guards, and the living test baseline to R838 / 7,249 passing tests / 7,252
  listed tests.

## Validation

- Red first: `cargo test -p yggdrasil-cardano-testnet property_run --lib`
  failed with unresolved import `crate::property::run`.
- Green focused check: `cargo test -p yggdrasil-cardano-testnet property_run
  --lib` passed with 3 tests.
- Package check: `cargo test -p yggdrasil-cardano-testnet` passed 128 lib tests
  plus 3 CLI golden tests.
- Formatting: `cargo fmt --all -- --check` exited 0 after applying rustfmt to
  the new module and tests.
- Focused validators:
  `python scripts/check-stale-placement.py --self-test`,
  `python scripts/check-stale-placement.py`,
  `python scripts/check-doc-status-headers.py --self-test`,
  `python scripts/check-doc-status-headers.py`,
  `python scripts/check-parity-matrix.py`,
  `python scripts/check-strict-mirror.py --fail-on-violation`, and
  `python -m py_compile scripts/check-stale-placement.py
  scripts/check-doc-status-headers.py scripts/check-parity-matrix.py
  .claude/scripts/filetree.py` exited 0.
- Broad Rust gates: `cargo check-all`, `cargo lint`, and `cargo test-all`
  exited 0.
- Green inventory: `cargo test-all -- --list | Select-String -Pattern
  ': test$' | Measure-Object | Select-Object -ExpandProperty Count` returned
  `7252`.

## Remaining risk

This round deliberately stops before actual `runTestnet` execution. The
operator-facing subcommands still return the structured deferral until the
node/KES supervision, era-genesis, DRep/SPO runtime workflows, transaction
runtime/query orchestration, and remaining Process/Property execution harness
are implemented and compared against upstream behavior.
