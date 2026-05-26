# Round 834 - cardano-testnet Process/RunIO execution helpers

## Scope

Finish the remaining small helper surface in upstream
`Testnet/Process/RunIO.hs` by wiring `mkExecConfig`, execFlex/execCli
execution wrappers, KES-agent-control unit execution, and `liftIOAnnotated`
error wrapping on top of the R832 `ProcessPlan` executor and the R833
plan-json binary resolver.

This round deliberately stops before node/KES-agent supervision, DRep/SPO
runtime workflows, transaction runtime/query orchestration, era-genesis
construction, and the Process/Property harness carve-out.

## Upstream facts

- RunIO `mkExecConfig` has the same socket/network-id environment shape as
  Process/Run `mkExecConfig`, but reads the inherited environment directly.
- `execFlexAny'` builds a RunIO `procFlex'` plan and captures exit code,
  stdout, and stderr without treating non-zero exit as an immediate failure.
- `execFlex'` returns stdout on success and reports stdout/stderr on non-zero
  exit.
- `execCli'`, `execCli_`, and `execKesAgentControl_` are thin executable-name
  specializations.
- `liftIOAnnotated` carries IO failures through the RunIO error path.

## Changes

- Exposed `exec_process_plan` from `process/run.rs` for precomputed
  `ProcessPlan` execution.
- Extended `process/run_io.rs` with `mk_exec_config`,
  `exec_flex_any_with_plan`, `exec_flex_with_plan`, `exec_cli_with_plan`,
  `exec_cli_unit_with_plan`, `exec_kes_agent_control_unit_with_plan`, and
  `lift_io_annotated`.
- Added a focused test covering environment override execution, stdout capture,
  unit-discard semantics, IO error wrapping, and `mkExecConfig` env/cwd shape.
- Updated cardano-testnet status docs, parity matrix evidence, stale-status
  guards, and the living test baseline to R834 / 7,241 passing tests / 7,244
  listed tests.

## Validation

- Red first: `cargo test -p yggdrasil-cardano-testnet
  process::run_io_tests::run_io_execution_helpers_use_env_override_and_exec_config_shape --lib`
  failed on missing `exec_cli_with_plan`, `exec_flex_with_plan`,
  `exec_cli_unit_with_plan`, `lift_io_annotated`, and `mk_exec_config`.
- Green focused implementation check:
  `cargo test -p yggdrasil-cardano-testnet
  process::run_io_tests::run_io_execution_helpers_use_env_override_and_exec_config_shape --lib`
  passed with 1 test.
- Repair loop: `cargo lint` initially rejected a needless `Ok(...?)` wrapper
  in `process/run_io.rs`; replacing it with the direct `lift_io_annotated`
  result fixed the root cause.
- Green after the lint repair:
  `cargo fmt --all -- --check`.
- Green focused check:
  `cargo test -p yggdrasil-cardano-testnet process::run_io_tests --lib`.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed 120 lib tests plus
  3 CLI golden tests.
- Green: `python scripts/check-stale-placement.py --self-test`.
- Green: `python scripts/check-stale-placement.py`.
- Green: `python scripts/check-doc-status-headers.py --self-test`.
- Green: `python scripts/check-doc-status-headers.py`.
- Green: `python scripts/check-parity-matrix.py`.
- Green: `python scripts/check-strict-mirror.py --fail-on-violation`.
- Green: `python -m py_compile scripts/check-stale-placement.py
  scripts/check-doc-status-headers.py scripts/check-parity-matrix.py
  .claude/scripts/filetree.py`.
- Green: `python .claude/scripts/filetree.py accept-current` followed by
  `python .claude/scripts/filetree.py check`.
- Green: `cargo check-all`.
- Green: `cargo lint`.
- Green inventory: `cargo test-all -- --list` returned `7244`.
- Green: `cargo test-all` passed the full workspace suite. The living status
  docs now record 7,241 passing, 0 failing, and 3 ignored tests (7,244 listed
  tests total).

## Remaining risk

The `cardano` and `create-env` subcommands still return the structured
deferral until node/KES spawning and supervision, era-genesis builders,
DRep/SPO runtime workflows, transaction runtime/query orchestration, and
Process/Property harnesses are ported.
