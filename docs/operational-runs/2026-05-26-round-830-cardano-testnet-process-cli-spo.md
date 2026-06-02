# Round 830 - cardano-testnet Process/Cli SPO pure builders

## Scope

Advance the `cardano-testnet` process-harness mirror by adding the pure
`Testnet/Process/Cli/SPO.hs` command-construction surface that can be
made deterministic without spawning `cardano-cli`, querying epoch state,
or submitting transactions. This covers stake-key and script-stake
registration/delegation/deregistration certificate builders, plus SPO
vote-file builders.

This round deliberately stops before `registerSingleSpo`,
`checkStakeKeyRegistered`, and `checkStakePoolRegistered` because those
workflows depend on key generation orchestration, transaction build/sign
and submit execution, epoch-state folding, stake-address and stake-pool
queries, and JSON result decoding.

## Findings

- Upstream certificate helpers write their output under
  `<tmp-absolute-path>/<output-file>` and pass that full path to
  `--out-file`.
- Conway stake registration/deregistration adds
  `--key-reg-deposit-amt <deposit>` after the `--out-file` argument;
  pre-Conway Shelley-based eras omit that deposit argument.
- Script-stake registration uses `--stake-script-file`; script-stake
  delegation uses the script file plus the pool cold verification key.
- Upstream `generateVoteFiles` numbers output files from one
  (`vote-spo-1`, `vote-spo-2`, ...) and uses each SPO cold
  verification key as the voting credential.

## Changes

- Added `process/cli/spo.rs`, a strict mirror of
  `cardano-testnet/src/Testnet/Process/Cli/SPO.hs` for pure SPO
  builders.
- Added typed plan records for SPO certificate and vote-file argv/path
  plans.
- Added pure builders:
  `create_stake_delegation_certificate_plan`,
  `create_stake_key_registration_certificate_plan`,
  `create_script_stake_registration_certificate_plan`,
  `create_script_stake_delegation_certificate_plan`,
  `create_stake_key_deregistration_certificate_plan`, and
  `generate_vote_file_plans`.
- Updated cardano-testnet status docs, parity-matrix evidence, and stale
  placement guards so the remaining gap is node/KES spawning,
  era-genesis, SPO/DRep runtime workflows, Transaction spend-output
  txbody builders, and Process harness execution.

## Validation

- Red: `cargo test -p yggdrasil-cardano-testnet process::cli::spo::tests::stake_key_certificate_builders_match_upstream_args --lib`
  failed because the new SPO argv/path-builder functions did not exist.
- Green: `cargo test -p yggdrasil-cardano-testnet process::cli::spo::tests --lib`
  passed with 4 SPO command-builder tests.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed with 111 lib
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
- Green after accepting the R830 metadata update:
  `python dev/test/filetree.py check`.
- Green: `cargo check-all`.
- Green: `cargo lint`.
- Test inventory:
  `cargo test-all -- --list --format terse | Select-String -Pattern ': test$' | Measure-Object | Select-Object -ExpandProperty Count`
  returned `7235`.
- Green: `cargo test-all` passed with no failures and the expected 3
  ignored `yggdrasil_node_tracer` doctests.

## Remaining risk

This round adds deterministic SPO command plans only. Runtime SPO
workflows still need query execution, epoch-state folding, key-generation
orchestration, transaction build/sign/submit plumbing, and stake
registration checks. The `cardano` and `create-env` subcommands still
return the structured deferral until the remaining Process/Cli helpers,
node/KES spawning, era-genesis builders, and higher-level process
orchestration are ported and compared against upstream.
