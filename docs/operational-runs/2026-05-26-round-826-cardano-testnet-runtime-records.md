# Round 826 - cardano-testnet runtime record carriers

## Scope

Advance the `cardano-testnet` `Testnet/Types.hs` mirror beyond
portable key records by adding the process-handle runtime record
carriers used by the remaining node-spawning and era-startup work.

## Findings

- Upstream `Testnet.Types` exposes `TestnetRuntime`, `TestnetNode`,
  `TestnetKesAgent`, `testnetSprockets`, `spoNodes`, `relayNodes`,
  `nodeSocketPath`, `nodeRpcSocketPath`, and `nodeConnectionInfo`.
- Yggdrasil's `runtime_types.rs` still documented those process-handle
  records as a future harness-round item, leaving downstream runtime
  slices without a typed place to carry spawned node / KES-agent state.
- The smallest safe R826 slice is the data-carrier and pure helper
  surface only. Concrete process spawning, era genesis, and
  Process/Property harness execution remain deferred.

## Changes

- Added `TestnetRuntime`, `TestnetNode`, and `TestnetKesAgent` to
  `runtime_types.rs`.
- Added `TestnetStdinHandle` and `TestnetProcessHandle` wrappers with
  placeholder variants for deterministic unit tests and child variants
  for future spawned subprocesses.
- Added upstream-shaped local connection carriers:
  `NetworkMagic`, `NetworkId`, `CardanoModeParams`, and
  `LocalNodeConnectInfo`.
- Added `testnet_sprockets`, `spo_nodes`, `relay_nodes`,
  `is_testnet_node_spo`, `node_socket_path`, `node_rpc_socket_path`,
  and `node_connection_info`.
- Updated cardano-testnet status docs and stale-current-status guards
  so the remaining gap is node spawning / era genesis / Process harness
  execution, not `Testnet/Types.hs` runtime records.

## Validation

- Red: `cargo test -p yggdrasil-cardano-testnet runtime_types::tests::testnet_node_spo_predicate_and_runtime_filters_match_upstream --lib`
  failed because `TestnetRuntime`, `TestnetNode`, `TestnetKesAgent`,
  process-handle wrappers, socket helpers, and `LocalNodeConnectInfo`
  did not exist.
- Red: `cargo test -p yggdrasil-cardano-testnet runtime_types::tests::node_connection_info_reports_socket_magic_and_cardano_epoch_slots --lib`
  failed for the same missing `Testnet/Types.hs` runtime surface after
  tightening the expected `LocalNodeConnectInfo` shape.
- Green: `cargo test -p yggdrasil-cardano-testnet runtime_types::tests::testnet_node_spo_predicate_and_runtime_filters_match_upstream --lib`
  passed after the runtime record carriers and helper functions landed.
- Green: `cargo test -p yggdrasil-cardano-testnet runtime_types::tests --lib`
  passed with 12 runtime-types tests.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed with 98 lib
  tests and 3 CLI golden tests.
- Green: `cargo fmt --all -- --check`.
- Green: `cargo check-all`.
- Green: `cargo lint`.
- Test inventory:
  `cargo test-all -- --list --format terse | Select-String -Pattern ': test$' | Measure-Object | Select-Object -ExpandProperty Count`
  returned `7222`.
- Green: `cargo test-all` passed with no failures and the expected 3
  ignored `yggdrasil_node_tracer` doctests.
- Green: `python scripts/check-stale-placement.py --self-test`.
- Green: `python scripts/check-stale-placement.py`.
- Green: `python scripts/check-doc-status-headers.py --self-test`.
- Green: `python scripts/check-doc-status-headers.py`.
- Green: `python scripts/check-parity-matrix.py` validated 22 entries
  against the 11.0.1 reference tag.
- Green: `python -m py_compile scripts/check-stale-placement.py scripts/check-doc-status-headers.py scripts/check-parity-matrix.py .claude/scripts/filetree.py`.
- Green: `git diff --check`; output was limited to the expected
  LF-to-CRLF working-copy warnings.

## Remaining risk

This round adds the runtime state carriers only. The `cardano` and
`create-env` subcommands still return the structured deferral until the
node/KES-agent process spawning, era-genesis builders, and
Process/Property harness bodies are ported and compared against
upstream.
