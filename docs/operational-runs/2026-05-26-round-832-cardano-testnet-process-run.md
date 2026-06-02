# Round 832 - cardano-testnet Process/Run wrappers

## Scope

Continue the cardano-testnet strict-mirror build-out by porting the
lightweight `Testnet/Process/Run.hs` process-run surface that can be expressed
without the higher-level testnet orchestration layer.

This round deliberately stops before node/KES-agent supervision, DRep/SPO
runtime workflows, transaction runtime UTxO/query orchestration, era-genesis
construction, and the Process/RunIO + Process/Property harness carve-outs.

## Upstream facts

- `mkExecConfig` prepends `CARDANO_NODE_SOCKET_PATH` and
  `CARDANO_NODE_NETWORK_ID` to the inherited environment and sets the child cwd
  to the testnet temp base.
- `mkExecConfigOffline` preserves the inherited environment and sets the child
  cwd, matching the Windows requirement that children receive the parent env.
- `addEnvVarsToConfig` prepends new env vars ahead of any existing config env.
- `procFlex` resolves a package binary through its environment override first;
  `procChairman` prepends the `run` argument.
- `execFlexAny` captures exit code, stdout, and stderr without treating
  non-zero exit as a test failure; `execFlex` does fail on non-zero exit.

## Changes

- Added `crates/tools/cardano-testnet/src/process/run.rs` as the strict mirror
  of `cardano-testnet/src/Testnet/Process/Run.hs`.
- Added `ExecConfig`, `ProcessPlan`, `ProcessOutput`, process-run error types,
  environment/cwd constructors, procFlex-style process planners, CLI/node/
  KES-agent/submit-api/chairman wrappers, create-script-context and
  kes-agent-control execution wrappers, JSON stdout parsing, process-group
  setup, and `initiateProcess`-style child startup.
- Promoted `serde_json` from dev-only to a production dependency for the
  `execCliStdoutToJson` mirror helper.
- Updated the cardano-testnet deferral status, parity matrix, status headers,
  stale-current-status guard, and living docs to R832 / 7,238 passing tests /
  7,241 listed tests.

## Validation

- Red first: `cargo test -p yggdrasil-cardano-testnet process::run::tests --lib`
  failed before the `process::run` surface existed.
- Green focused implementation check:
  `cargo test -p yggdrasil-cardano-testnet process::run::tests --lib` passed
  with 3 tests.
- Green formatting check during implementation: `cargo fmt --all`.
- Green after accepting the R832 metadata update:
  `cargo fmt --all -- --check`.
- Green: `cargo test -p yggdrasil-cardano-testnet process::run::tests --lib`.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed 117 lib tests plus
  3 CLI golden tests.
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
- Green inventory: `cargo test-all -- --list` returned `7241`.
- Green: `cargo test-all` passed the full workspace suite. The living status
  docs now record 7,238 passing, 0 failing, and 3 ignored tests (7,241 listed
  tests total).

## Remaining risk

The `cardano` and `create-env` subcommands still return the structured
deferral until node/KES spawning and supervision, era-genesis builders,
DRep/SPO runtime workflows, transaction runtime/query orchestration, and the
Process/RunIO + Process/Property harnesses are ported and compared against
upstream.
