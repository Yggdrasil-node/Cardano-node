# Round 526 - submit-tx socket driver delegation

Date: 2026-05-20

## Goal

Continue stale-placement cleanup by removing the node-local LocalTxSubmission
socket driver and making the node `submit-tx` command reuse the cardano-cli
transaction submission client.

## Changes

- Updated `crates/node/cardano-node/src/commands/submit_tx.rs` to call
  `yggdrasil_cardano_cli::lsq_tokio::TokioLsqClient.submit_tx`.
- Removed the duplicate node-local `LocalTxSubmissionClient` state-machine
  setup from the `submit-tx` command adapter.
- Updated `crates/node/cardano-node/src/main.rs` so top-level `submit-tx`
  no longer constructs a second runtime around an async node-local wrapper.
- Updated `crates/node/cardano-node/src/commands/cardano_cli.rs` so migrated
  `cardano-cli` LSQ and transaction-submit paths use `TokioLsqClient`
  directly rather than a node-specific `NodeCardanoCliClient`.
- Extended `scripts/check-stale-placement.py` to require that the node
  `submit-tx` adapter delegates to the shared cardano-cli client.
- Updated AGENTS guidance and cardano-cli LSQ docs to pin the ownership
  boundary: LocalTxSubmission socket driving belongs to `TokioLsqClient`.

## Verification

- `cargo fmt --all -- --check` passed.
- `cargo check -p yggdrasil-cardano-cli` passed.
- `cargo check -p yggdrasil-node` passed.
- `cargo test -p yggdrasil-cardano-cli submit_tx_against_missing_socket_returns_wrapped_error` passed.
- `cargo test -p yggdrasil-node submit_tx` passed (no matching tests; command completed cleanly).
- `cargo check-all` passed.
- `cargo lint` passed.
- Initial `cargo test-all` hit a transient Windows socket bind failure
  (`WSAENOBUFS`) in `yggdrasil-network::keepalive_client_single_ping`.
- `cargo test -p yggdrasil-network --test integration keepalive_client_single_ping -- --exact` passed.
- Rerun `cargo test-all` passed.
- `python scripts/check-stale-placement.py` passed.
- `python scripts/check-stale-placement.py --self-test` passed.
- `python scripts/check-strict-mirror.py --fail-on-violation` passed.
- `python scripts/check-parity-matrix.py` passed.
- `python scripts/check-fixture-manifest.py` passed.
- `python .claude/scripts/filetree.py accept-current` refreshed the manifest
  for this round's changed files.
- `python .claude/scripts/filetree.py check` passed after manifest refresh.
- `git diff --check` passed with only existing Windows CRLF warnings.
