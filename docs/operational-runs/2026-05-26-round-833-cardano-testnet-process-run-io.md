# Round 833 - cardano-testnet Process/RunIO plan-json helpers

## Scope

Continue the cardano-testnet process harness by porting the deterministic
binary-resolution and process-planning portion of upstream
`Testnet/Process/RunIO.hs`.

This round deliberately stops before node/KES-agent supervision, remaining
RunIO execution/liftIO helpers, DRep/SPO runtime workflows, transaction
runtime/query orchestration, era-genesis construction, and the Process/Property
harness carve-out.

## Upstream facts

- `planJsonFile` uses `../$CABAL_BUILDDIR/cache/plan.json` when
  `CABAL_BUILDDIR` is set; otherwise it searches upward for
  `dist-newstyle/cache/plan.json`.
- `binFlex` prefers the environment variable override and falls back to
  `binDist`.
- `binDist` decodes Cabal `plan.json`, recursively searches nested components
  for `component-name = "exe:<pkg>"`, reads `bin-file`, and applies the Windows
  executable suffix rule.
- `procFlex'` preserves the supplied environment/cwd and requests a separate
  process group for signal isolation.

## Changes

- Added `crates/tools/cardano-testnet/src/process/run_io.rs` as the strict
  mirror for the RunIO plan-json/process-planning helpers.
- Added `RunIoError`, `default_exec_config`, `plan_json_file_from_env`,
  `find_default_plan_json_file_from`, `add_exe_suffix`,
  `bin_dist_from_plan_json`, `bin_flex_with_plan`, `bin_flex`,
  `proc_flex_with_plan`, `proc_flex`, `proc_node`, and `proc_kes_agent`.
- Updated cardano-testnet status docs, parity matrix evidence, stale-status
  guards, and the living test baseline to R833 / 7,240 passing tests / 7,243
  listed tests.

## Validation

- Red first: `cargo test -p yggdrasil-cardano-testnet process::run_io_tests --lib`
  failed with `could not find run_io in super`.
- Green focused implementation check:
  `cargo test -p yggdrasil-cardano-testnet process::run_io_tests --lib` passed
  with 2 tests.
- Green after accepting the R833 metadata update:
  `cargo fmt --all -- --check`.
- Green: `cargo test -p yggdrasil-cardano-testnet process::run_io_tests --lib`.
- Green regression check:
  `cargo test -p yggdrasil-cardano-testnet process::run::tests --lib`.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed 119 lib tests plus
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
- Green inventory: `cargo test-all -- --list` returned `7243`.
- Green: `cargo test-all` passed the full workspace suite. The living status
  docs now record 7,240 passing, 0 failing, and 3 ignored tests (7,243 listed
  tests total).

## Remaining risk

The `cardano` and `create-env` subcommands still return the structured
deferral until node/KES spawning and supervision, era-genesis builders,
DRep/SPO runtime workflows, transaction runtime/query orchestration, remaining
RunIO execution/liftIO helpers, and Process/Property harnesses are ported.
