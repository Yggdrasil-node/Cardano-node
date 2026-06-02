# Round 827 - cardano-testnet Process/Cli key command builders

## Scope

Advance the `cardano-testnet` process-harness mirror by adding the
pure `Testnet/Process/Cli/Keys.hs` command-construction surface. This
round deliberately stops at deterministic argv/path builders; concrete
`cardano-cli` execution and node/KES process supervision remain in the
runtime harness backlog.

## Findings

- Upstream `Testnet.Process.Cli.Keys` wraps `cardano-cli` calls for
  Shelley payment/stake key generation, node VRF/KES key generation,
  node cold-key generation with an operator counter, and legacy Byron
  key/address helpers.
- Yggdrasil already had the typed key-pair carriers from
  `Testnet/Types.hs`, so the smallest safe next slice was to make the
  command vectors explicit and testable without spawning child
  processes.
- Windows Cargo gates still need upstream-shaped `/` path rendering in
  these builders, matching the earlier `Testnet/Filepath.hs` decision.

## Changes

- Added the `process` / `process::cli` module parents with strict-mirror
  docstrings.
- Added `process/cli/keys.rs`, a strict mirror of
  `cardano-testnet/src/Testnet/Process/Cli/Keys.hs`.
- Added marker types for upstream `OperatorCounter`,
  `ByronDelegationKey`, `ByronDelegationCert`, `ByronKeyLegacy`, and
  `ByronAddr`.
- Added pure argv builders:
  `cli_address_key_gen_args`, `cli_stake_address_key_gen_args`,
  `cli_node_key_gen_vrf_args`, `cli_node_key_gen_kes_args`, and
  `cli_node_key_gen_args`.
- Added legacy Byron planning helpers:
  `cli_key_gen_plan` and `cli_byron_signing_key_address_plan`.
- Updated cardano-testnet status docs, parity-matrix evidence, and stale
  placement guards so the remaining gap is the remaining Process/Cli
  helpers plus node spawning / era genesis / Process harness execution.

## Validation

- Red: `cargo test -p yggdrasil-cardano-testnet process::cli::keys::tests::shelley_keygen_builders_match_upstream_cli_argv --lib`
  failed because the new `Process/Cli/Keys.hs` argv/path-builder
  functions did not exist.
- Green: `cargo test -p yggdrasil-cardano-testnet process::cli::keys::tests --lib`
  passed with 3 key-command-builder tests.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed with 101 lib
  tests and 3 CLI golden tests.
- Green: `cargo fmt --all -- --check`.
- Green: `cargo check-all`.
- Green: `cargo lint`.
- Test inventory:
  `cargo test-all -- --list --format terse | Select-String -Pattern ': test$' | Measure-Object | Select-Object -ExpandProperty Count`
  returned `7225`.
- Green: `cargo test-all` passed with no failures and the expected 3
  ignored `yggdrasil_node_tracer` doctests.
- Green: `python dev/test/check-stale-placement.py --self-test`.
- Green: `python dev/test/check-stale-placement.py`.
- Green: `python dev/test/check-doc-status-headers.py --self-test`.
- Green: `python dev/test/check-doc-status-headers.py`.
- Green: `python dev/test/check-parity-matrix.py` validated 22 entries
  against the 11.0.1 reference tag.
- Green: `python dev/test/check-strict-mirror.py --fail-on-violation`.
- Green: `python -m py_compile dev/test/check-stale-placement.py dev/test/check-doc-status-headers.py dev/test/check-parity-matrix.py dev/test/filetree.py`.
- Green: `python dev/test/filetree.py check`.

## Remaining risk

This round adds deterministic key command plans only. The `cardano` and
`create-env` subcommands still return the structured deferral until the
remaining Process/Cli helpers, node/KES spawning, era-genesis builders,
and higher-level process orchestration are ported and compared against
upstream.
