# Round 839 - cardano-testnet Property/Run testnetProperty planning

## Scope

Continue the strict mirror of upstream `Testnet/Property/Run.hs` by porting
the remaining deterministic planning facts around `testnetProperty` and the
failed-start branch of `runTestnet`.

This round still deliberately stops before executing a live Hedgehog/Tasty
resource runner. The concrete `cardano` and `create-env` bodies remain
deferred until node/KES spawning, era-genesis, runtime/query workflows, and the
remaining Process/Property execution harness land.

## Upstream facts

- `NoUserProvidedEnv` uses `integrationWorkspace "testnet"`.
- `UserProvidedEnv` first makes the user path absolute, checks whether the
  directory exists, notes either `Reusing <abs path>` or `Created <abs path>`,
  and creates the directory only for the missing case.
- `forkAndRunTestnet` starts a resource keepalive loop with a
  `10_000_000` microsecond delay, runs `runTn conf`, and then intentionally
  fails the property to force a report.
- The `runTestnet` post-check branch prints `Failed to start testnet.` when no
  runtime was captured.

## Changes

- Added `TESTNET_WORKSPACE_NAME` and `KEEPALIVE_DELAY_MICROS` constants.
- Added `TestnetPropertyPlan`, `TestnetPropertyWorkspace`, and
  `UserProvidedEnvAction` as pure projections of the upstream
  `testnetProperty` branches.
- Added `no_user_provided_env_testnet_property_plan` and
  `user_provided_env_testnet_property_plan`, preserving workspace selection,
  create/reuse action, note text, keepalive delay, and intentional failure.
- Added `render_run_testnet_result`, which dispatches to the R838 running
  message renderer for captured runtimes and renders the upstream failed-start
  branch for `None`.
- Added two focused tests for the workspace/action plan and failed-start branch.
- Updated cardano-testnet status docs, parity matrix evidence, stale-status
  guards, and the living test baseline to R839 / 7,251 passing tests / 7,254
  listed tests.

## Validation

- Red first: `cargo test -p yggdrasil-cardano-testnet
  property_run_testnet_property_plan --lib` failed with unresolved imports for
  the new planning constants, enums, and helper functions.
- Green focused planning check:
  `cargo test -p yggdrasil-cardano-testnet
  property_run_testnet_property_plan --lib` passed with 1 test.
- Green full Property/Run focused set:
  `cargo test -p yggdrasil-cardano-testnet property_run --lib` passed with 5
  tests.

## Remaining risk

The helpers added here are a deterministic projection of upstream harness
behavior, not the live harness. The remaining work is still the actual
Process/Property execution layer, plus node/KES supervision, era-genesis,
DRep/SPO runtime workflows, transaction runtime/query orchestration, and
end-to-end comparison against upstream behavior.
