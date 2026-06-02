# Round 831 - cardano-testnet Process/Cli transaction txbody builders

## Scope

Advance the `cardano-testnet` process-harness mirror by adding the pure
`Testnet/Process/Cli/Transaction.hs` spend-output transaction-body
builder surface that can be made deterministic without querying epoch
state or executing `cardano-cli`. This covers `mkSpendOutputsOnlyTx`
and `mkSimpleSpendOutputsOnlyTx` after the caller has already selected
the funding `tx-in`; script outputs also carry the script address
returned by the runtime `cardano-cli address build` step.

This round deliberately stops before runtime UTxO selection and
script-address command execution because those workflows depend on
`EpochStateView`, `findLargestUtxoForPaymentKey`, and concrete
`cardano-cli` process execution.

## Findings

- Upstream writes the unsigned transaction body to
  `<work>/<prefix>.txbody`.
- Upstream starts `transaction build` with the era, source wallet change
  address, and selected transaction input.
- Pubkey outputs render as `--tx-out <payment-address>+<lovelace>`.
  The optional reference-script tuple field is accepted by the Haskell
  type but not emitted for pubkey outputs.
- Script outputs first run `cardano-cli <era> address build
  --payment-script-file <script>`, then render the returned address as
  `--tx-out <script-address>+<lovelace>` and append
  `--tx-out-reference-script-file <script>` when provided.

## Changes

- Extended `process/cli/transaction.rs` with typed spend-output plan
  inputs and output records:
  `ResolvedTxOutAddress`, `SpendOutput`, `ScriptAddressPlan`, and
  `SpendOutputsOnlyTxPlan`.
- Added pure builders:
  `mk_spend_outputs_only_tx_plan` and
  `mk_simple_spend_outputs_only_tx_plan`.
- Updated cardano-testnet status docs, parity-matrix evidence, and
  living verification baselines so the remaining cardano-testnet gap is
  node/KES spawning, era-genesis, DRep/SPO runtime workflows,
  transaction runtime/query execution, and Process harness execution.

## Validation

- Red: `cargo test -p yggdrasil-cardano-testnet process::cli::transaction::tests::spend_outputs_only_tx_plan_matches_upstream_pubkey_outputs --lib`
  failed because the new `SpendOutput`, `ScriptAddressPlan`, and txbody
  builder functions did not exist.
- Green: `cargo test -p yggdrasil-cardano-testnet process::cli::transaction::tests --lib`
  passed with 6 transaction command-builder tests.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed with 114 lib
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
- Green after accepting the R831 metadata update:
  `python dev/test/filetree.py check`.
- Green: `cargo check-all`.
- Green: `cargo lint`.
- Test inventory:
  `cargo test-all -- --list --format terse | Select-String -Pattern ': test$' | Measure-Object | Select-Object -ExpandProperty Count`
  returned `7238`.
- Green: `cargo test-all` passed with no failures and the expected 3
  ignored `yggdrasil_node_tracer` doctests.

## Remaining risk

This round adds deterministic transaction txbody command plans only.
Runtime transaction workflows still need UTxO selection,
script-address command execution, process orchestration, and comparison
against the upstream binary. The `cardano` and `create-env` subcommands
still return the structured deferral until node/KES spawning,
era-genesis builders, higher-level process orchestration, and
Process/Property harnesses are ported.
