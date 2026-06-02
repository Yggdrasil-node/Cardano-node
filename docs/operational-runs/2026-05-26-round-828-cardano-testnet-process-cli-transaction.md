# Round 828 - cardano-testnet Process/Cli transaction sign/submit builders

## Scope

Advance the `cardano-testnet` process-harness mirror by adding the pure
`Testnet/Process/Cli/Transaction.hs` command-construction surface that
does not require query/runtime execution: transaction signing, transaction
submission, expected-failure classification, and transaction-id retrieval.
This round deliberately stops before `mkSpendOutputsOnlyTx` /
`mkSimpleSpendOutputsOnlyTx` because those builders need epoch-state
queries, script-address execution, UTxO selection, and concrete
`cardano-cli` process execution.

## Findings

- Upstream `signTx` builds a signed transaction path at
  `<work>/<prefix>.tx` and calls `cardano-cli <era> transaction sign`
  with the tx body, every signing-key file, and an output path.
- Upstream `submitTx` and `failToSubmitTx` share the same
  `cardano-cli <era> transaction submit --tx-file <signed.tx>` argv
  shape; the latter classifies success as unexpected and only accepts a
  failure when stderr contains the expected reason substring.
- Upstream `retrieveTransactionId` invokes `cardano-cli latest
  transaction txid --tx-file <signed.tx>`. This slice exposes the argv
  builder; JSON decoding belongs with the future execution wrapper.

## Changes

- Added `process/cli/transaction.rs`, a strict mirror of
  `cardano-testnet/src/Testnet/Process/Cli/Transaction.hs` for the
  pure sign/submit/txid portion.
- Added marker types for upstream `VoteFile`, `TxBody`, `SignedTx`,
  `ScriptJSON`, and `TxOutAddress`.
- Added `AnySigningKey` so typed `KeyPair<K>` values can be erased to
  the signing-key path list accepted by upstream `signTx`.
- Added `sign_tx_plan`, `submit_tx_args`,
  `retrieve_transaction_id_args`, and `classify_failed_submission`.
- Updated cardano-testnet status docs, parity-matrix evidence, and stale
  placement guards so the remaining gap is SPO/DRep helpers,
  transaction-body builders, node spawning, era genesis, and Process
  harness execution.

## Validation

- Red: `cargo test -p yggdrasil-cardano-testnet process::cli::transaction::tests::sign_tx_plan_matches_upstream_era_txbody_signers_and_output --lib`
  failed because the new `Process/Cli/Transaction.hs` argv/path-builder
  functions did not exist.
- Green: `cargo test -p yggdrasil-cardano-testnet process::cli::transaction::tests --lib`
  passed with 3 transaction-command-builder tests.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed with 104 lib
  tests and 3 CLI golden tests.
- Green: `cargo fmt --all -- --check`.
- Green: `python dev/test/check-stale-placement.py --self-test`.
- Green: `python dev/test/check-stale-placement.py`.
- Green: `python dev/test/check-doc-status-headers.py --self-test`.
- Green: `python dev/test/check-doc-status-headers.py`.
- Green: `python dev/test/check-parity-matrix.py` validated 22 entries
  against the 11.0.1 reference tag.
- Green: `python dev/test/check-strict-mirror.py --fail-on-violation`.
- Green: `python -m py_compile dev/test/check-stale-placement.py dev/test/check-doc-status-headers.py dev/test/check-parity-matrix.py dev/test/filetree.py`.
- Green after accepting the R828 metadata update:
  `python dev/test/filetree.py check`.
- Green: `cargo check-all`.
- Green: `cargo lint`.
- Test inventory:
  `cargo test-all -- --list --format terse | Select-String -Pattern ': test$' | Measure-Object | Select-Object -ExpandProperty Count`
  returned `7228`.
- Green: `cargo test-all` passed with no failures and the expected 3
  ignored `yggdrasil_node_tracer` doctests.

## Remaining risk

This round adds deterministic transaction command plans only. The
transaction-body builders still need `EpochStateView`, UTxO lookup,
script address construction, and concrete `cardano-cli` execution. The
`cardano` and `create-env` subcommands still return the structured
deferral until the remaining Process/Cli helpers, process execution
wrappers, node/KES spawning, and era-genesis builders are ported and
compared against upstream.
