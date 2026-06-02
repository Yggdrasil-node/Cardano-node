# Round 835 - cardano-testnet Property/Util pure helpers

## Scope

Port the pure, projectable surface from upstream
`Testnet/Property/Util.hs`: one-test integration projections, retry workspace
naming including the `DISABLE_RETRIES=1` branch, Linux OS detection, and
`aesonObjectLookUp` JSON object lookup/error shape.

This round deliberately stops before the Hedgehog-to-proptest execution
harness in `Testnet/Property/{Assert,Run}.hs` and before `decodeEraUTxO`,
which depends on Cardano API era-typed UTxO values that are not yet present in
the Rust `cardano-testnet` crate.

## Upstream facts

- `integration` wraps one Hedgehog integration property with `withTests 1`.
- `integrationRetryWorkspace n workspaceName` uses
  `<workspaceName>-no-retries` when `DISABLE_RETRIES=1`; otherwise it retries
  with `<workspaceName>-<i>`.
- `integrationWorkspace` uses the provided workspace name directly.
- `isLinux` is `System.Info.os == "linux"`.
- `aesonObjectLookUp` returns `Maybe Aeson.Value` for JSON objects and fails
  with `Expected an Aeson Object but got: ...` for non-object values.

## Changes

- Added `property.rs` and `property/util.rs` under
  `crates/tools/cardano-testnet/src/`.
- Exposed `pub mod property;` from the crate root.
- Added pure `IntegrationPlan` projections plus
  `disable_retries_from_env`, `integration_retry_workspace_names`,
  `is_linux_os`, `is_linux`, and `aeson_object_lookup`.
- Added two focused tests covering retry/no-retry workspace names,
  `DISABLE_RETRIES`, OS detection, JSON object lookup, missing-key behavior,
  and the non-object error prefix.
- Updated cardano-testnet status docs, parity matrix evidence, stale-status
  guards, and the living test baseline to R835 / 7,243 passing tests / 7,246
  listed tests.

## Validation

- Red first: `cargo test -p yggdrasil-cardano-testnet property_util --lib`
  failed with `could not find property in crate`.
- Green focused implementation check:
  `cargo test -p yggdrasil-cardano-testnet property_util --lib` passed with 2
  tests.
- Green package check: `cargo test -p yggdrasil-cardano-testnet` passed 122
  lib tests plus 3 CLI golden tests.
- Green: `cargo fmt --all -- --check`.
- Green: `python dev/test/check-stale-placement.py --self-test`.
- Green: `python dev/test/check-stale-placement.py`.
- Green: `python dev/test/check-doc-status-headers.py --self-test`.
- Green: `python dev/test/check-doc-status-headers.py`.
- Green: `python dev/test/check-parity-matrix.py`.
- Green: `python dev/test/check-strict-mirror.py --fail-on-violation`.
- Green: `python -m py_compile dev/test/check-stale-placement.py
  dev/test/check-doc-status-headers.py dev/test/check-parity-matrix.py
  dev/test/filetree.py`.
- Green: `python dev/test/filetree.py accept-current` followed by
  `python dev/test/filetree.py check`.
- Green: `cargo check-all`.
- Green: `cargo lint`.
- Green inventory: `cargo test-all -- --list | Select-String -Pattern
  ': test$' | Measure-Object` returned `7246`.
- Green: `cargo test-all` passed the full workspace suite. The living status
  docs now record 7,243 passing, 0 failing, and 3 ignored tests (7,246 listed
  tests total).

## Remaining risk

The `cardano` and `create-env` subcommands still return the structured
deferral until node/KES spawning and supervision, era-genesis builders,
DRep/SPO runtime workflows, transaction runtime/query orchestration, and the
remaining Process/Property harnesses are ported.
