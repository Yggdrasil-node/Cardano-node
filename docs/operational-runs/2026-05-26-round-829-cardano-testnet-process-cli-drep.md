# Round 829 - cardano-testnet Process/Cli DRep pure builders

## Scope

Advance the `cardano-testnet` process-harness mirror by adding the pure
`Testnet/Process/Cli/DRep.hs` command-construction surface that can be
made deterministic without spawning `cardano-cli` or querying an epoch
state view. This covers DRep key generation, registration-certificate
creation, vote-file creation, and the two DRep txbody builders when the
caller has already selected the input UTxO.

This round deliberately stops before `registerDRep`, `delegateToDRep`,
`getLastPParamUpdateActionId`, and `makeActivityChangeProposal` because
those workflows depend on query execution, epoch waits, JSON gov-state
reads, local HTTP serving for proposal anchors, transaction signing, and
submission.

## Findings

- Upstream `generateDRepKeyPair` writes `verification.vkey` and
  `signature.skey` under `<work>/<prefix>/` and invokes
  `cardano-cli conway governance drep key-gen`.
- Upstream `generateRegistrationCertificate` writes
  `<work>/<prefix>.regcert` with `governance drep
  registration-certificate`, the DRep verification key, and the deposit
  amount.
- Upstream `generateVoteFiles` numbers output files from one
  (`vote-drep-1`, `vote-drep-2`, ...) and emits the vote choice as a
  flag such as `--yes` or `--abstain`.
- Upstream DRep txbody builders are mostly deterministic argv assembly
  after `findLargestUtxoForPaymentKey`; the Rust surface accepts that
  preselected `tx-in` explicitly so query/runtime execution stays out of
  this slice.

## Changes

- Added `process/cli/drep.rs`, a strict mirror of
  `cardano-testnet/src/Testnet/Process/Cli/DRep.hs` for pure DRep
  builders.
- Added the `Certificate` marker plus typed plan records for keygen,
  registration certificates, vote files, and DRep txbody plans.
- Added pure builders:
  `generate_drep_key_pair_plan`,
  `generate_registration_certificate_plan`, `generate_vote_file_plans`,
  `create_certificate_publication_tx_body_plan`, and
  `create_voting_tx_body_plan`.
- Updated cardano-testnet status docs, parity-matrix evidence, and stale
  placement guards so the remaining gap is SPO helpers, DRep runtime
  workflows, Transaction spend-output txbody builders, node spawning,
  era genesis, and Process harness execution.

## Validation

- Red: `cargo test -p yggdrasil-cardano-testnet process::cli::drep::tests::drep_keygen_and_registration_certificate_match_upstream_args --lib`
  failed because the new DRep argv/path-builder functions did not exist.
- Green: `cargo test -p yggdrasil-cardano-testnet process::cli::drep::tests --lib`
  passed with 3 DRep command-builder tests.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed with 107 lib
  tests and 3 CLI golden tests.
- Green: `cargo fmt --all -- --check`.
- Green: `python scripts/check-stale-placement.py --self-test`.
- Green: `python scripts/check-stale-placement.py`.
- Green: `python scripts/check-doc-status-headers.py --self-test`.
- Green: `python scripts/check-doc-status-headers.py`.
- Green: `python scripts/check-parity-matrix.py` validated 22 entries
  against the 11.0.1 reference tag.
- Green: `python scripts/check-strict-mirror.py --fail-on-violation`.
- Green: `python -m py_compile scripts/check-stale-placement.py scripts/check-doc-status-headers.py scripts/check-parity-matrix.py .claude/scripts/filetree.py`.
- Green after accepting the R829 metadata update:
  `python .claude/scripts/filetree.py check`.
- Green: `cargo check-all`.
- Green: `cargo lint`.
- Test inventory:
  `cargo test-all -- --list --format terse | Select-String -Pattern ': test$' | Measure-Object | Select-Object -ExpandProperty Count`
  returned `7231`.
- Green: `cargo test-all` passed with no failures and the expected 3
  ignored `yggdrasil_node_tracer` doctests.

## Remaining risk

This round adds deterministic DRep command plans only. Runtime DRep
workflows still need the query layer, epoch waits, proposal-anchor
serving, signing/submission orchestration, and governance-state parsing.
The `cardano` and `create-env` subcommands still return the structured
deferral until the remaining Process/Cli helpers, process execution
wrappers, node/KES spawning, and era-genesis builders are ported and
compared against upstream.
