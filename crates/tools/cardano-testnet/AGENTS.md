# Guidance for the pure-Rust port of upstream `cardano-testnet`.

**Status:** `partial`. The CLI skeleton, era-free portable type
surface, Parsers/Cardano option-composition layer, typed `Command`
payload wiring, `version` subcommand dispatch,
`Testnet/Types.hs` process-handle runtime record carriers, the
pure `Testnet/Process/Cli/Keys.hs` command builders plus the
`Testnet/Process/Cli/Transaction.hs` pure sign/submit/txid and
spend-output txbody builders, `Testnet/Process/Cli/DRep.hs` pure key/cert/vote builders plus the pure
`Testnet/Process/Cli/SPO.hs` certificate/vote builders, the
`Testnet/Process/Run.hs` flexible process wrappers, and the
`Testnet/Process/RunIO.hs` plan-json binary-resolution and execution helpers
plus the pure `Testnet/Property/{Util,Assert}.hs` helpers, the
`assertExpectedSposInLedgerState` CLI-backed stake-pool assertion wrapper, and
the pure `Testnet/Property/Run.hs` harness-control/planning helpers are complete
(R772-R839 - see *Current functional surface*); what remains is node/KES
spawning and supervision, era-genesis, the runtime-heavy SPO registration/check
workflows, DRep runtime workflows, transaction runtime/query orchestration,
and the remaining Process/Property harness execution for `cardano` and
`create-env`.
Scope band: **LARGE**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/cardano-testnet/` (82 `.hs` files).

## Mini-arc scope

Local multi-node testnet harness. The old C-arc CLI-MVS prerequisite is
closed. The next implementation slices should start from the vendored
`Testnet/Start/*`, `Testnet/Types.hs`, `Testnet/Components/*`, and
`Testnet/Process/Cli/*` surfaces, while preserving the approved
Hedgehog Process/Property carve-out (`tokio::process` + `proptest`).

## Current functional surface (post-R839)

### CLI surface

- ✅ `<binary> --help` / `--version` byte-equivalent to upstream
  (golden tests in `tests/`).
- ✅ Typed `parser::Command` dispatch — 3 subcommands (`cardano`,
  `create-env`, `version`) now carry upstream-shaped payloads.
- ✅ `version` subcommand dispatch emits the same captured version
  banner as `--version`.
- ✅ Parsers/Cardano option parser helpers through top-level
  `opts_testnet` / `opts_create_testnet` composition (R818-R823).

### Era-free portable type surface — complete (R772-R785)

- ✅ `types.rs` — the full `Testnet/Start/Types.hs` operator surface:
  the numeric newtypes (`NodeId`, `NumPools`, `NumRelays`,
  `NumDReps`), the option enums (`UpdateTimestamps`,
  `TestnetOnChainParams`, `RpcSupport`, `NodeLoggingFormat`,
  `GenesisHashesPolicy`, `PraosCredentialsSource`, `UserProvidedData`),
  the era tags (`CardanoEra`, `ShelleyBasedEra` with `era_to_string`),
  and every option record (`GenesisOptions`, `NodeOption`,
  `TestnetRuntimeOptions`, `TestnetEnvOptions`, `TestnetCreationOptions`,
  `NoUserProvidedEnvOptions`, `StartFromEnvOptions`,
  `CardanoTestnetCliOptions`, `CardanoTestnetCreateEnvOptions`). R772
  fixed an inverted `UpdateTimestamps` `Default` (parity bug).
- ✅ `runtime_types.rs` — `Testnet/Types.hs` portable + runtime records:
  `KeyPair<K>` + the six key-kind markers, `SpoNodeKeys`,
  `PaymentKeyInfo`, `Delegator`, `LeadershipSlot`,
  `TESTNET_DEFAULT_IPV4_ADDRESS`, `TestnetRuntime`, `TestnetNode`,
  `TestnetKesAgent`, `testnet_sprockets`, `spo_nodes`,
  `relay_nodes`, `node_socket_path`, `node_rpc_socket_path`, and
  `node_connection_info`.
- ✅ `paths.rs` — the `Cardano.Node.Testnet.Paths` directory
  conventions.
- ✅ `filepath.rs` — `Testnet/Filepath.hs`: `TmpAbsolutePath`,
  `Sprocket`, the temp-path helpers. The string-returning FilePath
  helpers intentionally preserve `/` separators instead of
  platform-native `Path::join` output so Windows Cargo gates still
  match the upstream-shaped path fixtures.
- ✅ `defaults.rs` — `Testnet/Defaults.hs` era-free scripts
  (`simple_script`, the Plutus test scripts).
- ✅ `components/` — `TestnetWaitPeriod` (`Query.hs`) and the
  `Configuration.hs` constants.
- ✅ `process/cli/keys.rs` — pure command builders for
  `Testnet/Process/Cli/Keys.hs`: Shelley payment/stake/VRF/KES
  keygen argv, node cold-keygen argv, and legacy Byron key/address
  path+argv plans.
- ✅ `process/cli/transaction.rs` — pure command builders for
  `Testnet/Process/Cli/Transaction.hs`: `signTx`, `submitTx`,
  `failToSubmitTx` result classification, `retrieveTransactionId`,
  `mkSpendOutputsOnlyTx`, and `mkSimpleSpendOutputsOnlyTx` argv/path
  plans. Runtime UTxO selection and script-address execution remain
  deferred.
- ✅ `process/cli/drep.rs` — pure command builders for
  `Testnet/Process/Cli/DRep.hs`: DRep keygen, registration
  certificate, vote files, certificate-publication txbody, and voting
  txbody plans. Runtime workflows (`registerDRep`, `delegateToDRep`,
  `getLastPParamUpdateActionId`, `makeActivityChangeProposal`) remain
  deferred.
- ✅ `process/cli/spo.rs` — pure command builders for
  `Testnet/Process/Cli/SPO.hs`: stake-key/script stake registration,
  delegation, deregistration certificates, and SPO vote-file plans.
  Runtime/query-heavy workflows (`registerSingleSpo`,
  `checkStakeKeyRegistered`, `checkStakePoolRegistered`) remain
  deferred.
- ✅ `process/run.rs` — flexible process execution helpers for
  `Testnet/Process/Run.hs`: `ExecConfig` env/cwd carriers,
  `mkExecConfig` / `mkExecConfigOffline`, environment prepending,
  procFlex-style environment override resolution, CLI/node/KES-agent/
  submit-api/chairman process plans, create-script-context and
  kes-agent-control execution wrappers, JSON stdout parsing, and
  `initiateProcess`-style child startup.
- ✅ `process/run_io.rs` — binary resolution, process-plan, and execution
  helpers for `Testnet/Process/RunIO.hs`: default `ExecConfig`, `mkExecConfig`,
  `planJsonFile` discovery, `binDist` recursive component lookup,
  executable suffix handling, `binFlex`, `procFlex`, `procNode`, and
  `procKesAgent` plan construction, `execFlexAny'`, `execFlex'`, `execCli'`,
  `execCli_`, `execKesAgentControl_`, and `liftIOAnnotated` error wrapping.
- ✅ `property/assert.rs` — `Testnet/Property/Assert.hs` JSON-lines,
  TraceNode leader/not-leader slot extraction, deadline, stake-pool count,
  era-equality helpers, and the CLI-backed `assertExpectedSposInLedgerState`
  stake-pool query wrapper with injectable `cardano-cli` execution.
- ✅ `property/run.rs` — pure `Testnet/Property/Run.hs` harness-control
  projection: `UserProvidedEnv`, `testnetProperty` workspace/action planning,
  OS-ignore disposition helpers, `runTestnet`'s failed-start branch, and its
  operator-facing running-testnet message rendering. The actual Hedgehog/Tasty
  resource runner and indefinite keepalive remain deferred.

### Deferred — runtime / era-genesis / harness

- ❌ `cardano` / `create-env` runtime dispatch — returns
  `RunError::SubcommandEraDispatchDeferred`. See **Carve-out
  inventory** below. The parser payloads and `version` subcommand are
  no longer part of this deferral.
- ❌ The concrete node/KES-agent spawning and supervision bodies, the
  per-era genesis records (`Defaults.hs`), the `Components/` node-query
  / genesis-creation bodies, the `Start/*` era startup, the remaining
  runtime/query-heavy SPO registration/check workflows, the runtime/query-heavy DRep
  workflows, transaction runtime UTxO/script-address orchestration, and the
  `Process/Property/Run.hs` Hedgehog-to-Rust execution harness carve-out.
- ❌ End-to-end behavioral tests against the upstream binary —
  pending that runtime layer.

## Carve-out inventory (post-R839 property-run planning boundary)

`crates/tools/cardano-testnet/src/status.rs` ships a typed
`Subcommand` enum (3 verbs: `cardano`, `create-env`, `version`) +
`era_dispatch_status()` helper.

| Carve-out                            | Status helper                       | Deferral rationale (one-liner)                                            |
|--------------------------------------|-------------------------------------|---------------------------------------------------------------------------|
| Runtime / era-genesis dispatch       | `status::era_dispatch_status()`     | R772-R823 shipped the era-free type records and Parsers/Cardano option composition; R825 threaded typed records into `Command::Cardano` / `Command::CreateEnv` and wired `version`; R826 added `Testnet/Types.hs` runtime record carriers and socket/connect-info helpers; R827 added pure `Testnet/Process/Cli/Keys.hs` cardano-cli command builders; R828 added `Testnet/Process/Cli/Transaction.hs` sign/submit/txid builders; R829 added `Testnet/Process/Cli/DRep.hs` pure key/cert/vote builders; R830 added `Testnet/Process/Cli/SPO.hs` pure certificate/vote builders; R831 added `Testnet/Process/Cli/Transaction.hs` pure spend-output txbody builders with preselected tx inputs and pre-resolved script addresses; R832 added `Testnet/Process/Run.hs` flexible process config, executable resolution, process plans, execution, JSON stdout, and child-start helpers; R833 added `Testnet/Process/RunIO.hs` plan-json discovery, `binDist`/`binFlex`, and procFlex process-plan helpers; R834 added RunIO `mkExecConfig`, execFlex/execCli/KES-agent-control execution wrappers, and `liftIOAnnotated` error wrapping; R835 added the pure `Testnet/Property/Util.hs` retry/workspace naming, `DISABLE_RETRIES`, Linux predicate, and JSON object lookup helpers; R836 added pure `Testnet/Property/Assert.hs` JSON-lines, relevant-slot extraction, deadline, stake-pool count, and era-equality assertion helpers; R837 added the CLI-backed `assertExpectedSposInLedgerState` stake-pool query wrapper; R838 added the pure `Testnet/Property/Run.hs` UserProvidedEnv, OS-ignore disposition helpers, and running-testnet operator message rendering; R839 added the pure `testnetProperty` workspace/action plan, keepalive delay, intentional-failure fact, and failed-start rendering. Pending: build node/KES spawning and supervision, era-genesis, SPO runtime registration/check workflows, DRep runtime workflows, transaction runtime UTxO/script-address orchestration, and the remaining Process/Property harness execution for `cardano` and `create-env`. Hedgehog Process/Property modules remain an approved Rust-idiomatic carve-out using `tokio::process` + `proptest`. |

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-cardano-testnet

# Run via the universal launcher (recommended).
scripts/run-tools.sh cardano-testnet --help
scripts/run-tools.sh cardano-testnet --version

# Or invoke the binary directly:
target/release/cardano-testnet --help
```

The binary is named `cardano-testnet` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
once concrete dispatch and upstream comparison evidence land.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `cardano-testnet` is the
  acceptance gate for any concrete implementation.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new release with different help output, refresh the
  fixtures + bump the relevant SHA pin in
  `crates/node/config/src/upstream_pins.rs` as a coordinated round.

## Round roadmap

This crate's full implementation remains an A4 sister-tool build-out:

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at R335-R336).
- ✅ Era-free option/runtime/path/default/component records and
  Parsers/Cardano option composition shipped through R823.
- ✅ Typed parser records thread through `Command::Cardano` /
  `Command::CreateEnv`, and `version` dispatch is operational (R825).
- ✅ `Testnet/Types.hs` runtime record carriers and socket/connect-info
  helpers are present (R826).
- ✅ `Testnet/Process/Cli/Keys.hs` pure key command builders are present
  (R827).
- ✅ `Testnet/Process/Cli/Transaction.hs` sign/submit/txid builders are
  present (R828), and pure spend-output txbody builders are present
  (R831).
- ✅ `Testnet/Process/Cli/DRep.hs` pure key/cert/vote builders are
  present (R829).
- ✅ `Testnet/Process/Cli/SPO.hs` pure certificate/vote builders are
  present (R830).
- ✅ `Testnet/Process/Run.hs` flexible process wrappers are present
  (R832).
- ✅ `Testnet/Process/RunIO.hs` plan-json binary-resolution helpers
  are present (R833).
- ✅ `Testnet/Process/RunIO.hs` execution/liftIO helpers are present
  (R834).
- ✅ `Testnet/Property/Util.hs` pure retry/workspace naming, OS predicate,
  and JSON object lookup helpers are present (R835).
- ✅ `Testnet/Property/Assert.hs` pure JSON-lines, slot extraction,
  deadline, SPO-count, and era-equality assertion helpers are present
  (R836).
- ✅ `Testnet/Property/Assert.hs` CLI-backed `assertExpectedSposInLedgerState`
  stake-pool query wrapper is present (R837).
- ✅ `Testnet/Property/Run.hs` pure `UserProvidedEnv`, OS-ignore helpers,
  `testnetProperty` planning, failed-start rendering, and running-testnet
  operator message rendering are present (R838-R839).
- 🟡 Next: port DRep/SPO runtime workflows, transaction runtime
  execution, node spawning, era-genesis, and the remaining Process/Property
  harness execution in strict-mirror-sized slices.
- 🟡 Closeout — when all subcommands are functional, parity-matrix
  entry advances `partial → verified_11_0_1`. Operators can then
  swap upstream binary for the yggdrasil binary without script
  changes.

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-cardano-testnet

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/cardano-testnet --help) \
     <(target/debug/cardano-testnet --help)
diff <(.reference-haskell-cardano-node/install/bin/cardano-testnet --version) \
     <(target/debug/cardano-testnet --version)
# (empty diffs expected — byte-equivalent)
```

## Maintenance Guidance

- Update this AGENTS.md when concrete subcommand implementations
  land (replace `❌ not yet implemented` rows with `✅ shipped` +
  round number).
- Keep the per-tool migration status in sync with
  `docs/COMPLETION_ROADMAP.md` and `docs/parity-matrix.json`.
- If upstream ships a new release: refresh the help/version
  fixtures, advance the relevant SHA pin in `upstream_pins.rs`,
  re-run the full cargo gate.
