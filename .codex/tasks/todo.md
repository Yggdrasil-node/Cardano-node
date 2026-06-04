# Core-First Completion TODO

## Plan

- [x] Full-completion quality push (2026-06-03).
  - [x] Review current lessons, root guidance, roadmap, parity summary, and technical debt before implementation.
  - [x] Inspect dirty worktree and preserve existing cardano-cli/filetree changes as active local work.
  - [x] Validate the in-progress `cardano-cli key verification-key` slice against upstream `Cardano.CLI.EraIndependent.Key.*`.
  - [x] Fix any correctness, parity, or test issues found in that slice.
  - [x] Run focused `yggdrasil-cardano-cli` tests plus required formatting/parity guards for touched areas.
  - [x] Run the broad local quality gates that are feasible in this environment.
  - [x] Record exact verification results and remaining external blockers in this file's Review section.

- [ ] Full-completion continuation plan (2026-06-02).
  - [x] Establish the current baseline without overwriting existing worktree changes.
    - [x] Review `tasks/lessons.md` for WSL/Linux, reference-binary, and commit-identity constraints.
    - [x] Review `docs/COMPLETION_ROADMAP.md` and `docs/PARITY_SUMMARY.md` for current blockers.
    - [x] Inspect `docs/parity-matrix.json` status counts and identify non-verified entries.
    - [x] Run lightweight parity/status preflight guards before any code changes.
    - [x] Reconcile the existing dirty worktree with the active plan and preserve user changes.
  - [x] Close executable local quality gaps first.
    - [x] Run and fix failures from `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, and focused tests for touched crates.
    - [x] Run and fix failures from `dev/test/check-strict-mirror.py`, `dev/test/check-stale-placement.py`, `dev/test/check-doc-status-headers.py`, `dev/test/check-parity-matrix.py`, `dev/test/check-fixture-manifest.py`, and `dev/test/filetree.py check`.
    - [x] Restore root operator script executable modes if still required by stale-placement.
    - [x] Update stale live helper-placement guidance for the `dev/{scripts,evidence,reference,test}` split.
    - [x] Update the relevant local `AGENTS.md` files only when the folder guidance is stale or incomplete.
  - [x] Remove retired AI-harness workspace artifacts for Codex-only operation.
    - [x] Move filetree state into neutral `dev/filetree/` metadata.
    - [x] Remove retired AI-harness files from the live workspace.
    - [x] Update living docs, guards, and filetree metadata to stop referencing retired AI-harness live paths.
    - [x] Preserve historical operational-run references as audit history unless they break a live guard.
  - [x] Advance full naming parity for one bounded partial sister-tool slice.
    - [x] Pick one bounded sister-tool arc from the partial parity-matrix entries (`kes-agent`, `kes-agent-control`, `cardano-tracer`, `db-analyser`, `snapshot-converter`, `cardano-testnet`, `tx-generator`, or `dmq-node`).
    - [x] Read that tool's `AGENTS.md` and the mirrored upstream Haskell files before changing terminology or behavior.
    - [x] Implement one strict naming-parity slice with focused Rustdocs/tests and update `docs/strict-mirror-audit.tsv` only when the verdict actually changes.
  - [x] Advance `cardano-cli hash genesis-file` naming parity.
    - [x] Read `crates/tools/cardano-cli/AGENTS.md` and upstream `Cardano.CLI.EraIndependent.Hash.{Command,Option,Run}` before changing terminology.
    - [x] Replace the hash command/run placeholder surface with the upstream-shaped `HashCmds` / `GenesisFile` slice.
    - [x] Wire the nested `hash genesis-file --genesis FILE` parser and runner through the top-level dispatcher.
    - [x] Add focused parser/hash tests and run the relevant Rust/parity guards.
  - [ ] Preserve evidence honesty for non-local blockers.
    - [ ] Do not mark Gap BO, Gap BP, R178, or BlockFetch complete without strict Haskell/operator artifacts under `target/core-closeout/`.
    - [ ] Use `dev/evidence/stage-core-closeout-artifacts.py` and `dev/test/check-core-closeout-artifacts.py` when live evidence becomes available.
    - [ ] Keep `docs/parity-matrix.json` as the first source updated for any status change, then synchronize prose docs.
  - [x] Document verification and review.
    - [x] Add a review section entry with exact commands, pass/fail status, and any blockers.
    - [x] Record user corrections in `tasks/lessons.md` before continuing after correction.

- [x] Create task tracking files before code changes.
- [x] Restore parity infrastructure.
  - [x] Make `.reference-haskell-cardano-node` ignored at the repository root.
  - [x] Verify `dev/test/check-stale-placement.py --self-test`.
  - [x] Verify `dev/test/check-stale-placement.py`.
  - [x] Provision `.reference-haskell-cardano-node` sources via `dev/reference/setup-reference.sh --sources-only` under WSL/Linux.
  - [x] Verify `dev/test/check-parity-matrix.py`.
  - [x] Pin LF checkout bytes for shell scripts, vendored network JSON, and upstream CLI help/version fixtures.
- [x] Remove supply-chain drift.
  - [x] Remove the `aws-lc-rs` / `aws-lc-sys` dependency path.
  - [x] Add explicit cargo-deny bans for AWS-LC native crypto crates.
  - [x] Update dependency policy docs.
- [x] Clean stale status documentation.
  - [x] Align Plutus next steps with current wiring.
  - [x] Align network/tracer status wording around evidence vs implementation.
  - [x] Clarify parity dashboard/core evidence status.
- [ ] Core closure follow-up arcs.
  - [x] Stabilize missing local/WSL tooling.
    - [x] Install or otherwise provision `cargo-deny`.
    - [x] Install or otherwise provision `check-jsonschema` / Python `jsonschema`.
    - [x] Provision full `.reference-haskell-cardano-node/install/` under WSL/Linux.
    - [x] Verify `dev/test/check-reference-artifacts.py`.
  - [x] Keep core evidence helpers self-tested as one local preflight.
    - [x] Add `dev/test/check-core-evidence-harnesses.py` to run the Gap BO, Gap BP, R178, and BlockFetch helper self-tests together.
    - [x] Validate durable preflight artifacts, including the BlockFetch self-test `summary.json` strict-mode invariants.
    - [x] Validate fresh Gap BO, Gap BP, and R178 self-test fixtures in the core preflight.
    - [x] Reject native Windows execution for the parity/shell preflight so it cannot use Windows-hosted Bash.
  - [x] Add a strict final live closeout artifact gate.
    - [x] Validate final Gap BO, Gap BP, R178, and BlockFetch Section 6.5 artifact locations under `target/core-closeout/`.
    - [x] Make the gate reject missing artifacts, self-test fixtures, weak equality modes, wrong slots/trace IDs, and weak BlockFetch soaks.
    - [x] Stamp and require `generated_at_utc` plus strict closeout-mode metadata on final Gap BO, Gap BP, and R178 fixtures.
    - [x] Add a WSL-only staging helper that copies strict live artifacts into the canonical closeout layout and runs the final gate.
    - [x] Require final BlockFetch closeout summaries to reference existing log, metrics, summary, node-log, tip-snapshot, and Haskell tip-comparison artifacts.
    - [x] Make BlockFetch closeout staging self-contained by copying referenced logs/metrics/tip artifacts under the canonical closeout tree and rewriting staged summary paths.
  - [ ] Gap BO TPraos VRF replay and regression fixture.
    - [x] Add `YGG_DUMP_TPRAOS_VRF` / `YGG_DUMP_TPRAOS_VRF_FILE` evidence logging for overlay classification, delegate/key hashes, nonce state, TPraos seeds, VRF outputs, and proof hashes.
    - [x] Add preprod Gap BO `mkSeed` golden coverage for slots 429460 and 432000.
    - [x] Add `dev/evidence/compare-gap-bo-tpraos-vrf.py` to compare Rust and future Haskell/operator TPraos VRF evidence by slot.
    - [x] Require complete Gap BO evidence schema before writing captured/pass comparison output.
    - [x] Add canonical nonce hex fields, a nonce-state phase marker, and a Rust evidence-line contract test for Gap BO captures.
    - [x] Add deterministic TPraos proof plumbing coverage for seedL/seedEta verification and cross-usage rejection.
    - [x] Make `dev/evidence/compare-gap-bo-tpraos-vrf.py --require-equal` fail unless Haskell evidence is supplied.
    - [x] Make Gap BO strict comparison require the configured target slot (`429460` by default) so wrong-slot evidence cannot close the blocker.
    - [x] Make Gap BO closeout mode require both `--require-haskell` and `--require-equal`.
    - [x] Add a guarded Gap BO `--write-fixture` path so passing Rust/Haskell evidence can be normalized into a replayable regression fixture.
    - [ ] Capture upstream Haskell replay output for the failing preprod slot and add the final fixture.
  - [ ] Gap BP Plutus V2 cost replay and regression fixture.
    - [x] Add `YGG_DUMP_CEK_FLUSHES` / `YGG_DUMP_CEK_FLUSHES_FILE` accumulated-step CEK flush logging with per-kind counters and budget deltas.
    - [x] Add `YGG_DUMP_SCRIPT_CONTEXT_FILE` append support while preserving the existing `YGG_DUMP_SCRIPT_CONTEXT` stderr capture.
    - [x] Deepen the captured Gap BP V2 ScriptContext regression for field counts, V2 TxOutRef wrapping, Babbage TxOut shape, fee/mint maps, validity range, redeemer keys, and active spending purpose.
    - [x] Add `dev/evidence/compare-gap-bp-script-context.py` to compare Rust and future Haskell ScriptContext CBOR dumps and report first divergent byte windows.
    - [x] Add `dev/evidence/compare-gap-bp-script-context.py --self-test` to validate parser, declared-length checks, byte comparison, diff windows, and artifact writing without the captured fixture.
    - [x] Add `dev/evidence/compare-gap-bp-cek-flushes.py` to compare Rust and future Haskell accumulated-step CEK flush traces by ordinal index.
    - [x] Add a Rust CEK flush trace contract test proving `YGG_DUMP_CEK_FLUSHES` emits the fields consumed by the comparator.
    - [x] Add `dev/evidence/compare-gap-bp-builtin-costs.py` to compare Rust and future Haskell per-builtin cost traces by ordinal index.
    - [x] Add a Rust builtin-cost trace contract test proving `YGG_DUMP_BUILTIN_COSTS` emits the fields consumed by the comparator.
    - [x] Add `dev/evidence/compare-gap-bp-traces.py` to run ScriptContext, CEK flush, and builtin-cost comparisons as one Gap BP evidence gate.
    - [x] Make `dev/evidence/compare-gap-bp-traces.py --require-equal` fail unless all three Haskell evidence logs are supplied.
    - [x] Make each individual Gap BP comparator equality mode fail unless its Haskell evidence log is supplied.
    - [x] Add `trace_id=<tx_hash>:<script_hash>:<version>` to ScriptContext, CEK flush, and builtin-cost evidence so noisy captures cannot compare the wrong evaluation.
    - [x] Make the aggregate Gap BP trace gate fail when ScriptContext, CEK flush, and builtin-cost evidence do not share the same trace identity.
    - [x] Make Gap BP strict comparison require `--expected-trace-id` so wrong preview V2 transaction/script evidence cannot close the blocker.
    - [x] Make Gap BP aggregate closeout mode require both `--require-haskell` and `--require-equal`.
    - [x] Make standalone Gap BP ScriptContext, CEK flush, and builtin-cost closeout modes require explicit `--require-haskell` plus equality.
    - [x] Add a guarded Gap BP aggregate `--write-fixture` path so passing Rust/Haskell traces can be normalized into a replayable regression fixture.
    - [ ] Capture the preview V2 failing transaction/script and compare the Rust flush trace against upstream Haskell.
  - [ ] R178 Conway HFC LSQ envelope comparison and fix.
    - [x] Add a Conway `QueryIfCurrent` regression proving HFC `Right` match and `Left` mismatch response envelopes.
    - [x] Extend the R178 regression across `gov-state`, `constitution`, and `committee-state`, including full `MsgResult` frame bytes.
    - [x] Add `dev/evidence/compare-conway-lsq.py` to drive upstream `cardano-cli conway query` against Yggdrasil and optional Haskell sockets, recording raw stdout hashes and optional byte-equality checks.
    - [x] Add `dev/evidence/compare-conway-lsq.py --self-test` to validate network-argument selection, JSON normalization, and byte/normalized comparison assertions without sockets.
    - [x] Harden `dev/evidence/compare-conway-lsq.py` to write raw binary artifacts, include raw-byte diff windows, and record the upstream `cardano-cli --version` used for evidence.
    - [x] Require `--haskell-socket` when `dev/evidence/compare-conway-lsq.py --require-haskell` is used for R178 closeout evidence.
    - [x] Make `--require-byte-equal` / `--require-normalized-equal` fail unless `--haskell-socket` is supplied.
    - [x] Add live socket preflight and bounded `cardano-cli` query timeouts so stale R178 closeout sockets fail loudly.
    - [x] Make R178 closeout mode require both `--require-haskell` and an explicit byte or normalized equality flag.
    - [x] Add a guarded R178 `--write-fixture` path so passing Yggdrasil/Haskell LSQ comparisons can be normalized into a replayable regression fixture.
    - [ ] Run byte-for-byte `cardano-cli` Conway LSQ comparison against the installed upstream 11.0.1 reference binary.
  - [ ] Section 6.5 BlockFetch worker activation and Haskell tip-comparison soak.
    - [x] Migrate direct bootstrap BlockFetch handles into the shared worker pool when `max_concurrent_block_fetch_peers > 1`.
    - [x] Add a runtime regression proving the shared worker pool and `yggdrasil_blockfetch_workers_registered` gauge become nonzero for the direct bootstrap path.
    - [x] Align `--max-concurrent-block-fetch-peers` CLI help with the shipped default `2`.
    - [x] Add `dev/evidence/parallel_blockfetch_soak.sh --self-test` to validate metrics parsing and assertion helpers without starting a live node.
    - [x] Harden `dev/evidence/parallel_blockfetch_soak.sh` so `REQUIRE_TIP_COMPARISON=1` also requires multi-worker expectations, worker assertions, and progress assertions.
    - [x] Harden `dev/evidence/compare_tip_to_haskell.sh` so missing/invalid JSON tip fields cannot compare as empty-string matches.
    - [x] Harden `dev/evidence/parallel_blockfetch_soak.sh` strict mode to require expected workers through final sample, no post-activation worker shortfalls, and a minimum tip-comparison count.
    - [x] Bound `dev/evidence/compare_tip_to_haskell.sh` Yggdrasil/Haskell tip queries so stale sockets cannot hang Section 6.5 sign-off.
    - [x] Validate and record `TIP_QUERY_TIMEOUT_SECONDS` in `dev/evidence/parallel_blockfetch_soak.sh` so sign-off cannot inherit invalid timeout config.
    - [x] Add a machine-readable BlockFetch §6.5 `summary.json` so passing soaks preserve strict-mode assertions and artifact paths.
    - [x] Record and require BlockFetch Haskell tip-comparison log paths in `summary.json`.
    - [ ] Run preprod Section 6.5 two-peer and knob=4 Haskell tip-comparison soaks.
    - [ ] Run mainnet Section 6.5 knob=2 24h Haskell tip-comparison soak.
- [ ] Sister-tool strict naming parity follow-up arcs.
  - [x] Port the next pure `cardano-testnet` `Testnet/Defaults.hs`
    topology defaults slice.
    - [x] Add typed `defaultMainnetTopology` /
      `defaultP2PTopology` builders using the existing network
      topology model.
    - [x] Pin the upstream local/public root, ledger-peer, bootstrap,
      valency, trust, advertise, and peer-snapshot defaults with
      focused tests.
    - [x] Update `cardano-testnet` status guidance and filetree
      metadata.
    - [x] Run focused crate tests and the required workspace/parity
      guards before review.
  - [x] Align generated topology JSON with upstream optional-field
    omission semantics.
    - [x] Omit disabled `bootstrapPeers` and absent `peerSnapshotFile`
      during `TopologyConfig` serialization.
    - [x] Pin the serialization shape at the network model layer.
    - [x] Pin the `cardano-testnet` default topology builders against
      the same emitted JSON shape.
    - [x] Run focused network/cardano-testnet tests and required
      workspace/parity guards.
  - [x] Port the next pure `cardano-testnet`
    `Testnet/Components/Configuration.hs` helper slice.
    - [x] Read `crates/tools/cardano-testnet/AGENTS.md` and upstream
      `Testnet/Components/Configuration.hs`.
    - [x] Add pure `createConfigJsonNoHash`, `eraToString`, and
      `anyEraToString` mirrors using existing local defaults/era
      helpers.
    - [x] Run focused cardano-testnet tests and required workspace/parity
      guards.
  - [x] Port the `cardano-testnet` hash-bearing
    `Testnet/Components/Configuration.hs` config slice.
    - [x] Re-read upstream `createConfigJson`, `getByronGenesisHash`,
      and `getShelleyGenesisHash`.
    - [x] Add `createConfigJson`, `getByronGenesisHash`, and
      `getShelleyGenesisHash` mirrors using shared
      `yggdrasil-node-genesis` hash helpers.
    - [x] Run focused cardano-testnet tests and required workspace/parity
      guards.
  - [x] Port the `cardano-testnet` Dijkstra era-tag/config slice.
    - [x] Read upstream `Testnet/Defaults.hs` Dijkstra direct-hardfork
      config and `eraToProtocolVersion` handling.
    - [x] Confirm touched filenames keep their strict mirror status and
      touched public types/functions preserve upstream terminology.
    - [x] Add Dijkstra to the portable `CardanoEra` /
      `ShelleyBasedEra` tags and default YAML hardfork config.
    - [x] Treat Dijkstra as Conway-onwards for SPO stake-key
      deregistration deposit args.
    - [x] Run focused cardano-testnet tests and required workspace/parity
      guards.
  - [x] Add type/name parity guard coverage for completed public surfaces.
    - [x] Add `docs/type-parity-audit.tsv` rows for the completed
      `cardano-cli key verification-key` and `cardano-testnet`
      Dijkstra/configuration/defaults surfaces.
    - [x] Add `dev/test/check-type-parity.py` with a self-test and
      exact Rust/upstream text checks.
    - [x] Expand coverage to the completed `cardano-cli hash
      genesis-file` command/run surface and the completed
      `cardano-testnet` default key/path/topology helpers.
    - [x] Expand coverage to the completed `cardano-testnet`
      `Process/Cli/{Keys,Transaction,DRep,SPO}.hs` pure builder
      surface.
    - [x] Expand coverage to the completed `cardano-testnet`
      `Testnet/Types.hs` portable/runtime record surface carried in
      `runtime_types.rs`.
    - [x] Expand coverage to the completed `cardano-testnet`
      `Testnet/Process/{Run,RunIO}.hs` and
      `Testnet/Property/{Util,Assert,Run}.hs` helper surfaces.
    - [x] Expand coverage to the completed `cardano-testnet`
      `Testnet/Start/Types.hs` operator option/type surface.
    - [x] Expand coverage to the completed `cardano-testnet`
      shared `Cardano.Node.Testnet.Paths` and `Testnet/Filepath.hs`
      path-helper surfaces.
    - [x] Expand coverage to the completed `cardano-testnet`
      `Parsers/{Cardano,Run,Version}.hs` option/command parser
      surfaces.
    - [x] Expand coverage to the completed `cardano-testnet`
      `Testnet/Components/Query.hs` `TestnetWaitPeriod` surface.
    - [x] Expand coverage to the completed `snapshot-converter`
      `snapshot-converter.hs` parser, config/type, run-mode, and
      structured-deferral status surface.
    - [x] Document the guard in root verification expectations and
      `docs/SPECS.md`.
    - [x] Run the type-parity guard plus required workspace/parity
      checks.
  - [x] Wire the type/name parity guard into standard verification.
    - [x] Add `check-type-parity.py` to `just parity-all`.
    - [x] Add a CI type/name parity validator after the source-only
      upstream reference fetch.
    - [x] Update README and docs guidance so operators run the guard
      after changing manifest-listed public parity surfaces.
    - [x] Run the integrated parity workflow and affected guards.
  - [x] Confirm accepted operator/reference shell executable modes.
    - [x] Verify every tracked `dev/{scripts,evidence,reference}/*.sh`
      helper is `100755` in the Git index.
    - [x] Verify `dev/test/check-stale-placement.py` passes with the
      accepted `dev/` helper placement.
    - [x] Retire the obsolete root `scripts/*.sh` follow-up wording;
      root operator helpers no longer live there.
  - [x] Tighten `kes-agent-control` parser/type parity.
    - [x] Read `crates/tools/kes-agent-control/AGENTS.md` and upstream
      `deps/kes-agent/kes-agent/cli/ControlMain.hs`.
    - [x] Replace local shorthand parser flags with upstream
      `--kes-verification-key-file` and `--opcert-file` spellings.
    - [x] Add exact type/name audit rows for the shipped
      `CommonOptions`, per-subcommand option records,
      `ProgramOptions`, parser entry points, and common-option fields.
    - [x] Run focused crate tests and required type-parity checks.
  - [x] Port the `kes-agent` typed `AgentMain.hs` parser surface.
    - [x] Read `crates/tools/kes-agent/AGENTS.md` and upstream
      `deps/kes-agent/kes-agent/cli/AgentMain.hs`.
    - [x] Replace the raw passthrough parser with typed
      `ProgramOptions`, `ProgramModeOptions`, `NormalModeOptions`,
      `ServiceModeOptions`, `LogTarget`, and log-level parsing.
    - [x] Preserve the daemon/socket runtime deferral while parsing
      upstream `start|stop|restart|status|run` commands and
      `run`-mode options.
    - [x] Remove the non-upstream `-v` version shortcut so only
      `--version` short-circuits, matching upstream help.
    - [x] Add exact type/name audit rows for the shipped
      `kes-agent` parser and option surface.
    - [x] Run focused crate tests and required type-parity checks.
  - [x] Port the `kes-agent` `AgentMain.hs` environment option surface.
    - [x] Add `nmo_from_env`, `smo_from_env`, and testable lookup
      variants for the upstream env-var overlays.
    - [x] Add `split_by` matching upstream `splitBy ':'` behavior for
      bootstrap-path lists.
    - [x] Preserve upstream invalid log-level/log-target failure
      behavior as typed parser errors.
    - [x] Add exact type/name audit rows for the new env helpers.
    - [x] Run focused crate tests and required type-parity checks.
  - [x] Port the `kes-agent` common protocol type vocabulary.
    - [x] Read upstream `Protocols/AgentInfo.hs`,
      `Protocols/RecvResult.hs`, and `Protocols/Types.hs`.
    - [x] Add strict filename mirrors under
      `crates/tools/kes-agent/src/protocols/`.
    - [x] Mirror common protocol enums/records, enum ordinals, and
      selected upstream pretty-rendering contracts without wiring socket
      runtime behavior.
    - [x] Add exact type/name audit rows for the shipped common
      protocol vocabulary.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
  - [x] Port the `kes-agent` version-handshake protocol vocabulary.
    - [x] Read upstream `Protocols/VersionHandshake/Protocol.hs`,
      `Peers.hs`, and `Driver.hs`.
    - [x] Strengthen `VersionIdentifier` from a numeric alias to a
      typed version tag matching upstream `mkVersionIdentifier`.
    - [x] Add strict filename mirrors for version-handshake states,
      messages, state tokens, pure peer negotiation, and driver trace
      vocabulary.
    - [x] Add exact type/name audit rows for the shipped
      version-handshake identifiers and helpers.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
  - [x] Port the `kes-agent` Control V0 protocol vocabulary.
    - [x] Read upstream `Protocols/Control/V0/Protocol.hs`,
      `Peers.hs`, and `Driver.hs`.
    - [x] Add strict filename mirrors for Control V0 states,
      messages, the crypto-parameterized `AgentInfo` record shape, pure
      command peer choices, command discriminators, and read-error trace
      vocabulary.
    - [x] Preserve the upstream `Control:<crypto>:0.5` version
      identifier formula and the current `StandardCrypto` registration
      text used by the V0 control driver path.
    - [x] Preserve the upstream V0 behavior where `controlDropKey`
      sends `DropStagedKeyMessage` and `DropKeyCmd` decodes to
      `ProtocolErrorMessage`.
    - [x] Add exact type/name audit rows for the shipped Control V0
      identifiers and helpers.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
  - [x] Port the `kes-agent` Control V1 protocol vocabulary.
    - [x] Read upstream `Protocols/Control/V1/Protocol.hs`,
      `Peers.hs`, and `Driver.hs`.
    - [x] Add strict filename mirrors for Control V1 states,
      messages, the direct `BundleInfo` `AgentInfo` record shape, pure
      command peer choices, command discriminators, and read-error
      trace vocabulary.
    - [x] Pin the typed `Control:1.0` version identifier and preserve
      upstream `ControlProtocol`, `Message`, `SControlProtocol`,
      `ControlDriverTrace`, and `ReadResult` terminology in Rust form.
    - [x] Preserve the upstream V1 behavior where `controlDropKey`
      sends `DropStagedKeyMessage` and `DropKeyCmd` decodes to
      `ProtocolErrorMessage`.
    - [x] Add exact type/name audit rows for the shipped Control V1
      identifiers and helpers.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
  - [x] Port the `kes-agent` Control V2 protocol vocabulary.
    - [x] Read upstream `Protocols/Control/V2/Protocol.hs`,
      `Peers.hs`, and `Driver.hs`.
    - [x] Add strict filename mirrors for Control V2 states,
      messages, the older `AgentInfo` record shape, pure command peer
      choices, command discriminators, and read-error trace vocabulary.
    - [x] Pin the typed `Control:2.0` version identifier and preserve
      upstream `ControlProtocol`, `Message`, `SControlProtocol`,
      `ControlDriverTrace`, and `ReadResult` terminology in Rust form.
    - [x] Add exact type/name audit rows for the shipped Control V2
      identifiers and helpers.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
  - [x] Port the `kes-agent` Control V3 protocol vocabulary.
    - [x] Read upstream `Protocols/Control/V3/Protocol.hs`,
      `Peers.hs`, and `Driver.hs`.
    - [x] Add strict filename mirrors for Control V3 states,
      messages, `AgentInfo` records, pure command peer choices,
      command discriminators, and read-error trace vocabulary.
    - [x] Pin the typed `Control:3.0` version identifier and preserve
      upstream `ControlProtocol`, `Message`, `SControlProtocol`,
      `ControlDriverTrace`, and `ReadResult` terminology in Rust form.
    - [x] Add exact type/name audit rows for the shipped Control V3
      identifiers and helpers.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
  - [x] Port the `kes-agent` Service V1 protocol vocabulary.
    - [x] Read upstream `Protocols/Service/V1/Protocol.hs`,
      `Peers.hs`, and `Driver.hs`.
    - [x] Add strict filename mirrors for Service V1 states,
      messages, state tokens, pure receiver/pusher choices, key-message
      payload handling, and read-error trace vocabulary.
    - [x] Pin the typed `Service:1.0` version identifier and preserve
      upstream `ServiceProtocol`, `ServiceMessage`, and `ReadResult`
      terminology in Rust form.
    - [x] Preserve the upstream V1 no-next-key pusher path as
      `RecvErrorUnsupportedOperation` without adding V2 timestamp/drop
      behavior.
    - [x] Add exact type/name audit rows for the shipped Service V1
      identifiers and helpers.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
  - [x] Port the `kes-agent` Service V2 protocol vocabulary.
    - [x] Read upstream `Protocols/Service/V2/Protocol.hs`,
      `Peers.hs`, and `Driver.hs`.
    - [x] Add strict filename mirrors for Service V2 states,
      messages, state tokens, pure receiver/pusher choices, key/drop
      discriminants, and read-error trace vocabulary.
    - [x] Pin the typed `Service:2.0` version identifier and preserve
      upstream `ServiceProtocol`, `ServiceMessage`, `KeyMessageTypeID`,
      and `ReadResult` terminology in Rust form.
    - [x] Add exact type/name audit rows for the shipped Service V2
      identifiers and helpers.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
    - [x] Run post-slice broad workspace/parity gates.
  - [x] Port the `kes-agent` Service V0 protocol vocabulary.
    - [x] Read upstream `Protocols/Service/V0/Protocol.hs`,
      `Peers.hs`, and `Driver.hs`.
    - [x] Add strict filename mirrors for Service V0 states,
      messages, state tokens, pure receiver/pusher choices,
      crypto-version identifiers, and read-error trace vocabulary.
    - [x] Pin the typed Service V0 version formula and preserve
      upstream `ServiceProtocol`, `ServiceMessage`, and `ReadResult`
      terminology in Rust form.
    - [x] Add exact type/name audit rows for the shipped Service V0
      identifiers and helpers.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
    - [x] Run post-slice broad workspace/parity gates.
  - [x] Port the `kes-agent` BearerUtil protocol helper vocabulary.
    - [x] Read upstream `Protocols/BearerUtil.hs`.
    - [x] Add a strict filename mirror for `BearerConnectionClosed`,
      `withDuplexBearer`, the one-byte-at-a-time receive buffering
      semantics, and the 1024-byte receiver buffer constant.
    - [x] Preserve daemon/socket dispatch deferral while making the
      helper testable without raw socket I/O.
    - [x] Add exact type/name audit rows for the shipped BearerUtil
      identifiers and helpers.
    - [x] Run focused crate tests, focused Clippy, strict mirror, and
      type-parity checks.
    - [x] Run post-slice broad workspace/parity gates.

## Review

- 2026-06-05 `snapshot-converter` type/name parity audit slice:
  expanded `docs/type-parity-audit.tsv` from 721 to 749 exact
  Rust/upstream checks for the completed `snapshot-converter.hs`
  parser, config/type, run-mode, and structured-deferral status
  surfaces. Covered names include `Config`, `DaemonConfig`,
  `NoDaemonConfig`, `Snapshot'`, `StandaloneSnapshot'`,
  `LSMSnapshot'`, `SnapshotsDirectoryWithFormat`, `parseConfig`,
  `main`, `convertSnapshot`, `watchTree`, and every shipped
  operator-facing snapshot-converter flag spelling.
- 2026-06-05 `snapshot-converter` focused verification passed:
  `python3 dev/test/check-type-parity.py` (`749 rows clean`);
  `cargo test -p yggdrasil-snapshot-converter --lib` (`33 passed`);
  `cargo test -p yggdrasil-snapshot-converter --test cli_help_golden`
  (`2 passed`); `cargo fmt --all -- --check`; `python3
  dev/test/check-strict-mirror.py --fail-on-violation`; `python3
  dev/test/check-type-parity.py --self-test`.
- 2026-06-05 post-`snapshot-converter` broad verification passed:
  `python3 dev/test/check-stale-placement.py --self-test`; `python3
  dev/test/check-stale-placement.py`; `python3
  dev/test/check-parity-matrix.py`; `python3
  dev/test/check-doc-status-headers.py`; `python3
  dev/test/check-fixture-manifest.py`; `cargo check-all`; `cargo
  lint`; `cargo lint-no-default`; `cargo test-all` (passed, with the
  existing 3 ignored node-tracer doctests).

- 2026-06-04 `kes-agent` BearerUtil slice: added a strict filename
  mirror for upstream `Protocols/BearerUtil.hs`. The slice ships
  `BearerConnectionClosed`, the 1024-byte `bufferSize` mirror, a
  pure `TChan Word8`-style receive buffer model, one-byte-at-a-time
  forwarding, EOF-to-connection-closed detection, and a testable
  `withDuplexBearer` model while keeping concrete raw socket I/O in
  the daemon/socket follow-on. `docs/type-parity-audit.tsv` now
  carries 721 exact Rust/upstream rows.
- 2026-06-04 `kes-agent` BearerUtil focused verification passed:
  `cargo fmt --all`; `cargo test -p yggdrasil-kes-agent --lib`
  (`119 passed`); `cargo clippy -p yggdrasil-kes-agent --all-targets
  --all-features -- -D warnings`; `python3
  dev/test/check-type-parity.py` (`721 rows clean`); `python3
  dev/test/check-strict-mirror.py --fail-on-violation`.
- 2026-06-04 post-`kes-agent` BearerUtil broad verification passed:
  `cargo fmt --all -- --check`; `python3
  dev/test/check-type-parity.py`; `python3
  dev/test/check-strict-mirror.py --fail-on-violation`; `python3
  dev/test/check-stale-placement.py --self-test`; `python3
  dev/test/check-stale-placement.py`; `python3
  dev/test/check-parity-matrix.py`; `python3
  dev/test/check-doc-status-headers.py`; `python3
  dev/test/check-fixture-manifest.py`; `cargo check-all`; `cargo
  lint`; `cargo lint-no-default`; `cargo test-all` (passed, with the
  existing 3 ignored node-tracer doctests). The first filetree check
  correctly flagged the task-log edit after metadata refresh; filetree
  was refreshed and rechecked before final staging.

- 2026-06-04 `kes-agent` Service V0 slice: added strict filename
  mirrors for upstream `Protocols/Service/V0/{Protocol,Peers,Driver}.hs`.
  The slice pins the legacy `Service:<crypto>:0.4` version formula,
  mirrors the V0 key-only message surface, pure receiver/pusher choices,
  no-next-key `RecvErrorUnsupportedOperation` path, state tokens,
  key-message payload selection, and Service read-error trace mapping.
  `docs/type-parity-audit.tsv` now carries 712 exact Rust/upstream rows.
- 2026-06-04 `kes-agent` Service V0 focused verification passed:
  `cargo fmt --all`; `python3 dev/test/check-type-parity.py`
  (`712 rows clean`); `git add` for new production mirrors before
  the index-based guard; `cargo test -p yggdrasil-kes-agent --lib`
  (`112 passed`); `cargo clippy -p yggdrasil-kes-agent --all-targets
  --all-features -- -D warnings`; `python3
  dev/test/check-strict-mirror.py --fail-on-violation`.
- 2026-06-04 post-`kes-agent` Service V0 broad verification passed:
  `cargo fmt --all -- --check`; `python3
  dev/test/check-type-parity.py`; `python3
  dev/test/check-strict-mirror.py --fail-on-violation`; `python3
  dev/test/check-stale-placement.py --self-test`; `python3
  dev/test/check-stale-placement.py`; `python3
  dev/test/check-parity-matrix.py`; `python3
  dev/test/check-doc-status-headers.py`; `python3
  dev/test/check-fixture-manifest.py`; `python3 dev/test/filetree.py
  check`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`;
  `cargo test-all` (passed, with the existing 3 ignored node-tracer
  doctests).

- 2026-06-04 `kes-agent` Service V1 slice: added strict filename
  mirrors for upstream `Protocols/Service/V1/{Protocol,Peers,Driver}.hs`.
  The slice pins the typed `Service:1.0` version identifier, mirrors the
  V1 key-only message surface without V2's timestamp/drop discriminator,
  preserves the no-next-key `RecvErrorUnsupportedOperation` pusher path,
  carries the Service read-error trace mapping, and expands
  `docs/type-parity-audit.tsv` to 680 exact Rust/upstream rows while
  preserving the daemon/socket runtime deferral.
- 2026-06-04 `kes-agent` Service V1 focused verification passed:
  `cargo fmt --all`; `cargo test -p yggdrasil-kes-agent --lib`
  (`104 passed`); `cargo clippy -p yggdrasil-kes-agent --all-targets
  --all-features -- -D warnings`; `python3
  dev/test/check-type-parity.py` (`680 rows clean`); `python3
  dev/test/check-strict-mirror.py --fail-on-violation`.
- 2026-06-04 post-`kes-agent` Service V1 broad verification passed:
  `cargo fmt --all -- --check`; `cargo check-all`; `cargo lint`;
  `cargo lint-no-default`; `cargo test-all`; `python3
  dev/test/check-type-parity.py`; `python3
  dev/test/check-strict-mirror.py --fail-on-violation`; `python3
  dev/test/check-stale-placement.py --self-test`; `python3
  dev/test/check-stale-placement.py`; `python3
  dev/test/check-parity-matrix.py`; `python3
  dev/test/check-doc-status-headers.py`; `python3
  dev/test/check-fixture-manifest.py`; `python3
  dev/test/filetree.py accept-current`; `python3
  dev/test/filetree.py render`; `python3 dev/test/filetree.py check`.

- 2026-06-04 kes-agent Control V0 protocol slice: added strict
  filename mirrors for upstream
  `Protocols/Control/V0/{Protocol,Peers,Driver}.hs`. The Rust surface
  now carries the `Control:<crypto>:0.5` version formula, the current
  `Control:StandardCrypto:0.5` registration helper, the V0
  crypto-parameterized `AgentInfo` shape, V0 state/message vocabulary,
  pure peer helpers, driver command discriminators,
  `DropKeyCmd -> ProtocolErrorMessage`, and the upstream V0
  `controlDropKey` behavior that sends `DropStagedKeyMessage`.
- 2026-06-04 kes-agent Control V0 verification passed: `cargo fmt
  --all`; `python3 dev/test/check-type-parity.py` (`650 rows clean`);
  `python3 dev/test/check-strict-mirror.py --fail-on-violation`;
  `cargo test -p yggdrasil-kes-agent --lib` (`96 passed`); `cargo
  clippy -p yggdrasil-kes-agent --all-targets --all-features -- -D
  warnings`; `cargo fmt --all -- --check`; `python3
  dev/test/check-stale-placement.py --self-test`; `python3
  dev/test/check-stale-placement.py`; `python3
  dev/test/check-parity-matrix.py`; `python3
  dev/test/check-doc-status-headers.py`; `python3
  dev/test/check-fixture-manifest.py`; `cargo check-all`; `cargo
  lint`; `cargo lint-no-default`; `cargo test-all`; `python3
  dev/test/filetree.py accept-current`; `python3
  dev/test/filetree.py render`; `python3 dev/test/filetree.py check`.
- 2026-06-04 kes-agent Control V1 protocol slice: added strict
  filename mirrors for upstream
  `Protocols/Control/V1/{Protocol,Peers,Driver}.hs`. The Rust surface
  now carries `Control:1.0`, the V1 direct `AgentInfo` `BundleInfo`
  shape, V1 state/message vocabulary, pure peer helpers, driver command
  discriminators, `DropKeyCmd -> ProtocolErrorMessage`, and the
  upstream V1 `controlDropKey` behavior that sends
  `DropStagedKeyMessage`.
- 2026-06-04 kes-agent Control V1 verification passed: `cargo fmt
  --all`; `python3 dev/test/check-type-parity.py` (`592 rows clean`);
  `python3 dev/test/check-strict-mirror.py --fail-on-violation`;
  `cargo test -p yggdrasil-kes-agent --lib` (`84 passed`); `cargo
  clippy -p yggdrasil-kes-agent --all-targets --all-features -- -D
  warnings`; `cargo fmt --all -- --check`; `python3
  dev/test/check-stale-placement.py --self-test`; `python3
  dev/test/check-stale-placement.py`; `python3
  dev/test/check-parity-matrix.py`; `python3
  dev/test/check-doc-status-headers.py`; `python3
  dev/test/check-fixture-manifest.py`; `cargo check-all`; `cargo
  lint`; `cargo lint-no-default`; `cargo test-all`; `python3
  dev/test/filetree.py accept-current`; `python3
  dev/test/filetree.py render`; `python3 dev/test/filetree.py check`.
- 2026-06-03 full-completion quality push: validated the dirty `cardano-cli key verification-key` slice against upstream `Cardano.CLI.EraIndependent.Key.{Command,Option,Run}`. The Rust implementation now exposes the upstream-shaped `key verification-key` group, derives verification-key TextEnvelopes from supported normal Ed25519 payment, stake, DRep, constitutional committee, genesis, genesis delegate, genesis UTxO, and stake-pool signing-key TextEnvelopes, dispatches through the standalone CLI runner, and has parser plus exact output-shape tests. The current standalone CLI docs now report 41 operational commands: the completed 40-command C-arc plus this post-C-arc key slice.
- 2026-06-03 verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-cardano-cli key --lib`; `cargo test -p yggdrasil-cardano-cli --lib`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-strict-mirror.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- Remaining full-completion blockers are not locally closable by code edits alone: Gap BO upstream Haskell TPraos VRF replay fixture, Gap BP upstream Plutus V2 trace comparison, R178 Conway LSQ socket byte/normalized comparison, and BlockFetch §6.5 multi-peer/mainnet Haskell tip-comparison soaks remain open per the roadmap and parity matrix.
- 2026-06-03 continuation: strengthened `key verification-key` from the initial payment/stake subset to every normal Ed25519 signing-key TextEnvelope family that shares the 32-byte DSIGN payload shape: payment, stake, DRep, constitutional committee cold/hot, genesis, genesis delegate, genesis UTxO, and stake-pool. Extended/BIP32 and KES/VRF key shapes remain intentionally unsupported until their codecs exist locally.
- 2026-06-03 continuation verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-cardano-cli verification_key_cmd --lib`; `cargo test -p yggdrasil-cardano-cli --lib`; `cargo check-all`; `cargo lint`; `cargo test-all`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-strict-mirror.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-fixture-manifest.py`.
- 2026-06-03 cardano-testnet Configuration helper slice: ported the pure upstream `Testnet/Components/Configuration.hs` exports `createConfigJsonNoHash`, `eraToString`, and `anyEraToString` as `create_config_json_no_hash`, `era_to_string`, and `any_era_to_string`, delegating to the existing `default_yaml_hardfork_via_config` and canonical era helpers. Updated `crates/tools/cardano-testnet/AGENTS.md`, `crates/tools/cardano-testnet/src/status.rs`, and `docs/COMPLETION_ROADMAP.md` so the completed pure `Configuration.hs` surface is discoverable while the IO/genesis/hash work remains honestly deferred.
- 2026-06-03 cardano-testnet Configuration verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-cardano-testnet configuration --lib`; `cargo test -p yggdrasil-cardano-testnet --lib`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-strict-mirror.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- 2026-06-03 cardano-testnet hash-bearing Configuration slice: ported upstream `createConfigJson`, `getByronGenesisHash`, and `getShelleyGenesisHash` as `create_config_json`, `get_byron_genesis_hash`, and `get_shelley_genesis_hash`. The implementation delegates Byron canonical-JSON hashing and Shelley-family raw-file hashing to the shared `yggdrasil-node-genesis` helpers, preserving upstream's split hash semantics and merging the resulting hash fields with `default_yaml_hardfork_via_config`.
- 2026-06-03 cardano-testnet hash-bearing Configuration verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-cardano-testnet configuration --lib`; `cargo test -p yggdrasil-cardano-testnet --lib`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-strict-mirror.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-fixture-manifest.py`.
- 2026-06-03 cardano-testnet Dijkstra era-tag/config slice: added Dijkstra to the portable `CardanoEra` / `ShelleyBasedEra` tags, default genesis filepath derivation, `default_yaml_hardfork_via_config` protocol-version major 10 and cumulative `TestDijkstraHardForkAtEpoch`, `create_config_json` Dijkstra genesis hash path, and SPO stake-key deregistration deposit handling for Conway-onwards eras. The default creation era remains Conway, matching upstream `defaultEra`.
- 2026-06-03 Dijkstra parity audit: confirmed the touched files keep their current strict mirror status (`types.rs` -> `Testnet/Start/Types.hs`, `defaults.rs` -> `Testnet/Defaults.hs`, `process/cli/spo.rs` -> `Testnet/Process/Cli/SPO.hs`; `components/configuration.rs` remains an explicit strict-none component placement for `Testnet/Components/Configuration.hs`). Public names preserve upstream terminology through idiomatic Rust casing: `CardanoEra::Dijkstra`, `ShelleyBasedEra::Dijkstra`, `DijkstraGenesisHash`, `DijkstraGenesisFile`, and `TestDijkstraHardForkAtEpoch`.
- 2026-06-03 cardano-testnet Dijkstra verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-cardano-testnet --lib`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-strict-mirror.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- 2026-06-03 type/name parity guard slice: added `docs/type-parity-audit.tsv` and `dev/test/check-type-parity.py` so completed public parity surfaces now have exact Rust/upstream identifier checks in addition to strict filename mirror checks. The initial manifest covers the completed `cardano-cli key verification-key` command/run surface and the completed `cardano-testnet` era/defaults/configuration/SPO Dijkstra surface.
- 2026-06-03 type/name parity guard verification passed: `python3 -m py_compile dev/test/check-type-parity.py`; `python3 dev/test/check-type-parity.py --self-test`; `python3 dev/test/check-type-parity.py`; `cargo fmt --all -- --check`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-strict-mirror.py`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-fixture-manifest.py`.
- 2026-06-03 type/name parity CI wiring: added the guard to `just parity-all` and `.github/workflows/ci.yml` after the upstream source-only reference fetch, then documented it in `README.md` and `docs/AGENTS.md`.
- 2026-06-03 type/name parity CI wiring verification passed: `just parity-all` could not run because `just` is not installed in this environment, so the exact constituent commands were run directly and passed: `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-type-parity.py --self-test`; `python3 -m py_compile dev/test/check-type-parity.py`; `cargo fmt --all -- --check`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- 2026-06-04 type/name parity audit expansion: expanded `docs/type-parity-audit.tsv` from the initial key/config rows to 238 exact Rust/upstream checks by adding the completed `cardano-cli hash genesis-file` command/run names; the completed `cardano-testnet` default committee, DRep, SPO, delegator, UTxO, and topology helper names; the completed `cardano-testnet` `Process/Cli/{Keys,Transaction,DRep,SPO}.hs` pure builder names; the completed `Testnet/Types.hs` portable/runtime record names carried in `runtime_types.rs`; the completed `Testnet/Process/{Run,RunIO}.hs` plus `Testnet/Property/{Util,Assert,Run}.hs` helper names; the completed `Testnet/Start/Types.hs` operator option/type surface; the completed shared `Cardano.Node.Testnet.Paths` plus `Testnet/Filepath.hs` path-helper surfaces; the completed `Parsers/{Cardano,Run,Version}.hs` option/command parser surfaces; and the completed `Testnet/Components/Query.hs` `TestnetWaitPeriod` surface.
- 2026-06-03 type/name parity audit expansion verification passed: `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-type-parity.py --self-test`; `python3 -m py_compile dev/test/check-type-parity.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- 2026-06-04 type/name parity guard hardening: `dev/test/check-type-parity.py` now rejects duplicate manifest mappings plus absolute or root-escaping manifest paths, and its self-test exercises those failure modes through the same `check_manifest` path as the live guard.
- 2026-06-04 type/name parity guard hardening verification passed: `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-type-parity.py --self-test`; `python3 -m py_compile dev/test/check-type-parity.py`.
- 2026-06-04 type/name parity self-test robustness: `dev/test/check-type-parity.py --self-test` now creates its `target/` parent explicitly before writing temporary root-relative files, so clean checkouts and CI jobs do not depend on pre-existing Cargo output directories.
- 2026-06-04 type/name parity self-test robustness verification passed: `python3 dev/test/check-type-parity.py --self-test`; `python3 dev/test/check-type-parity.py`; `python3 -m py_compile dev/test/check-type-parity.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- 2026-06-04 type/name parity side-root hardening: `dev/test/check-type-parity.py` now requires manifest Rust paths to live under `crates/` and upstream paths to live under `.reference-haskell-cardano-node/`, with self-test coverage for both rejection paths. Root `AGENTS.md`, `docs/AGENTS.md`, and `docs/SPECS.md` now document the enforced invariant.
- 2026-06-04 type/name parity side-root hardening verification passed: `python3 dev/test/check-type-parity.py --self-test`; `python3 dev/test/check-type-parity.py`; `python3 -m py_compile dev/test/check-type-parity.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `git diff --check`.
- 2026-06-04 type/name parity ordering hardening: `dev/test/check-type-parity.py` now rejects unsorted manifest rows, its self-test proves that failure mode, and `docs/type-parity-audit.tsv` is normalized by `(scope, rust_path, rust_text, upstream_path, upstream_text)` so future public parity additions are deterministic and reviewable.
- 2026-06-04 type/name plus filename parity ordering verification passed: `python3 dev/test/check-type-parity.py --self-test`; `python3 dev/test/check-type-parity.py`; `python3 -m py_compile dev/test/check-type-parity.py`; direct manifest sort assertion (`238` rows, sorted); `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `git diff --check`.
- 2026-06-04 type/name parity token-boundary hardening: `dev/test/check-type-parity.py` now checks manifest text with identifier-boundary token matching instead of raw substring matching, and its self-test rejects partial matches. The broad `CardanoEra` / `ShelleyBasedEra` rows now point at upstream `Testnet/Defaults.hs`, where those names appear as real imported tokens rather than substrings of `AnyCardanoEra` / `AnyShelleyBasedEra`.
- 2026-06-04 type/name plus filename parity token-boundary verification passed: `python3 dev/test/check-type-parity.py --self-test`; `python3 dev/test/check-type-parity.py`; `python3 -m py_compile dev/test/check-type-parity.py`; direct manifest assertion (`238` rows, sorted, `0` boundary misses); `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `git diff --check`.
- 2026-06-04 type/name parity source-file hardening: `dev/test/check-type-parity.py` now requires manifest Rust evidence paths to end in `.rs` and upstream evidence paths to end in `.hs`, with self-test coverage for both rejection paths. Root/docs/CI guidance now describes the manifest as Rust source ↔ upstream Haskell source evidence, not generic path evidence.
- 2026-06-04 type/name plus filename parity source-file verification passed: `python3 dev/test/check-type-parity.py --self-test`; `python3 dev/test/check-type-parity.py`; `python3 -m py_compile dev/test/check-type-parity.py`; direct manifest suffix assertion (`238` rows, Rust suffixes `[.rs]`, upstream suffixes `[.hs]`); `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `git diff --check`.
- 2026-06-04 type/name parity scope-taxonomy hardening: `dev/test/check-type-parity.py` now requires lower-kebab-case manifest scope names, with self-test coverage for invalid scope text. Root/docs/CI guidance now documents the scope naming rule so future public parity rows stay stable and grep-friendly.
- 2026-06-04 type/name plus filename parity scope-taxonomy verification passed: `python3 dev/test/check-type-parity.py --self-test`; `python3 dev/test/check-type-parity.py`; `python3 -m py_compile dev/test/check-type-parity.py`; direct manifest assertion (`238` rows, `24` scopes, all lower-kebab-case); `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `git diff --check`.
- 2026-06-04 type/name parity note-quality hardening: `dev/test/check-type-parity.py` now requires every manifest note to be an explanatory sentence with no leading/trailing whitespace, with self-test coverage for placeholder notes. Root/docs/CI guidance now documents that `docs/type-parity-audit.tsv` notes must explain the mapped surface rather than carry terse placeholders.
- 2026-06-04 type/name plus filename parity note-quality verification passed: `python3 dev/test/check-type-parity.py --self-test`; `python3 dev/test/check-type-parity.py`; `python3 -m py_compile dev/test/check-type-parity.py`; direct manifest assertion (`238` rows, `0` note issues); `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `git diff --check`.
- 2026-06-04 type/name parity text-token hardening: `dev/test/check-type-parity.py` now requires `rust_text` and `upstream_text` to be single tokens with no whitespace or hidden padding, with self-test coverage for invalid token text. Root/docs/CI guidance now documents that the manifest maps exact identifiers/config keys rather than phrases.
- 2026-06-04 type/name plus filename parity text-token verification passed: `python3 dev/test/check-type-parity.py --self-test`; `python3 dev/test/check-type-parity.py`; `python3 -m py_compile dev/test/check-type-parity.py`; direct manifest assertion (`238` rows, `0` token-text issues); `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `git diff --check`.
- 2026-06-04 operator helper executable-mode audit: retired the stale open root `scripts/*.sh` task and replaced it with the current accepted `dev/{scripts,evidence,reference}/*.sh` evidence. `git ls-files -s '*.sh'` shows all tracked dev shell helpers as `100755`, and the filesystem mode scan shows the same helpers as executable (`755`).
- 2026-06-04 operator helper executable-mode verification passed: `git ls-files -s '*.sh'`; filesystem mode scan for `dev/{scripts,evidence,reference}/*.sh`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`.
- 2026-06-04 checklist hygiene: closed stale parent TODO boxes where all child evidence was already recorded: current-baseline establishment, relevant AGENTS guidance updates, one bounded partial sister-tool naming-parity slice, `cardano-cli hash genesis-file` naming parity, and verification/review documentation. Left the broad cargo rerun item, live closeout evidence policy, Gap BO, Gap BP, R178, and BlockFetch soak blockers open because they still require either fresh broad verification or upstream/operator artifacts.
- 2026-06-04 checklist hygiene verification passed: `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- 2026-06-04 broad local quality-gate rerun passed: `cargo fmt --all -- --check`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/filetree.py check`.
- 2026-06-04 `kes-agent-control` parser/type parity slice: aligned the shipped parser with upstream `ControlMain.hs` flag spellings (`--kes-verification-key-file` and `--opcert-file`) instead of the earlier local shorthand, renamed the helper to `extract_ver_key_file`, and expanded `docs/type-parity-audit.tsv` from 238 to 267 exact Rust/upstream rows with `kes-agent-control` parser and option-type coverage. Runtime ControlClient socket I/O remains deliberately deferred on the R444+ daemon/socket follow-on.
- 2026-06-04 `kes-agent-control` focused verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-kes-agent-control --lib`; `python3 dev/test/check-type-parity.py`.
- 2026-06-04 post-`kes-agent-control` broad verification passed: `cargo fmt --all -- --check`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- 2026-06-04 `kes-agent` typed parser slice: ported the locally closable `AgentMain.hs` parser/options surface while keeping daemon/socket dispatch deferred. The parser now returns typed `ProgramOptions` for `start|stop|restart|status|run`, config-file aliases, normal-mode socket/bootstrap/log/genesis/cold-key flags, upstream log-level/log-target spellings, and service/normal default records. It also rejects the previous local `-v` version shortcut because upstream exposes only `--version`. `docs/type-parity-audit.tsv` expanded from 267 to 295 rows with exact `kes-agent` parser/type coverage.
- 2026-06-04 `kes-agent` focused verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-kes-agent --lib`; `cargo test -p yggdrasil-kes-agent --test cli_help_golden`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`.
- 2026-06-04 post-`kes-agent` broad verification passed: `cargo fmt --all -- --check`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/filetree.py accept-current`.
- 2026-06-04 `kes-agent` env-option slice: added deterministic mirrors of upstream `nmoFromEnv`, `smoFromEnv`, and `splitBy`, with closure-injected test variants. The helpers read the upstream `KES_AGENT_*` env names, preserve `splitBy ':'` empty/trailing segment behavior for bootstrap paths, convert invalid log-level/log-target env values into typed parser errors, and expanded `docs/type-parity-audit.tsv` from 295 to 300 exact rows.
- 2026-06-04 `kes-agent` env-option focused verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-kes-agent --lib`; `python3 dev/test/check-type-parity.py`.
- 2026-06-04 post-`kes-agent` env-option broad verification passed: `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `git diff --check`.
- 2026-06-04 `kes-agent` common protocol type slice: added strict filename mirrors for upstream `Cardano.KESAgent.Protocols.AgentInfo`, `RecvResult`, and `Types`, exported through `protocols::*`. The slice ships common protocol records/enums, enum ordinal helpers, receive/command pretty rendering, selected driver trace pretty contracts, and expanded `docs/type-parity-audit.tsv` from 300 to 350 exact Rust/upstream rows while preserving the daemon/socket runtime deferral.
- 2026-06-04 `kes-agent` common protocol focused verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-kes-agent --lib`; `cargo clippy -p yggdrasil-kes-agent --all-targets --all-features -- -D warnings`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`.
- 2026-06-04 post-`kes-agent` common protocol broad verification passed: `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- 2026-06-04 `kes-agent` version-handshake slice: added strict filename mirrors for upstream `Cardano.KESAgent.Protocols.VersionHandshake.{Protocol,Peers,Driver}`. The slice strengthens `VersionIdentifier` to a typed version tag, pins `vpVersionIdentifier` as `VersionHandshake:0.1`, mirrors handshake states/messages/tokens, implements the upstream client/server common-version selection as pure helpers, adds driver trace pretty contracts, and expands `docs/type-parity-audit.tsv` from 350 to 376 exact Rust/upstream rows.
- 2026-06-04 `kes-agent` version-handshake focused verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-kes-agent --lib`; `cargo clippy -p yggdrasil-kes-agent --all-targets --all-features -- -D warnings`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`.
- 2026-06-04 post-`kes-agent` version-handshake broad verification passed: `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`; `git diff --check`; `git diff --cached --check`.
- 2026-06-04 `kes-agent` Control V2 slice: added strict filename mirrors for upstream `Cardano.KESAgent.Protocols.Control.V2.{Protocol,Peers,Driver}`. The slice pins the typed `Control:2.0` version identifier, mirrors Control V2 states/messages/tokens, the older `AgentInfo`/bundle/bootstrap record vocabulary, pure command peer choices, command discriminators, and `readErrorToControlDriverTrace`, and expands `docs/type-parity-audit.tsv` from 473 to 536 exact Rust/upstream rows while preserving the daemon/socket runtime deferral.
- 2026-06-04 `kes-agent` Control V2 focused verification passed: `cargo fmt --all`; `cargo test -p yggdrasil-kes-agent --lib` (72 tests); `cargo clippy -p yggdrasil-kes-agent --all-targets --all-features -- -D warnings`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`.
- 2026-06-04 post-`kes-agent` Control V2 broad verification passed: `cargo fmt --all -- --check`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`.
- 2026-06-04 `kes-agent` Control V3 slice: added strict filename mirrors for upstream `Cardano.KESAgent.Protocols.Control.V3.{Protocol,Peers,Driver}`. The slice pins the typed `Control:3.0` version identifier, mirrors Control V3 states/messages/tokens, `AgentInfo`/bundle/bootstrap record vocabulary, pure command peer choices, command discriminators, and `readErrorToControlDriverTrace`, and expands `docs/type-parity-audit.tsv` from 410 to 473 exact Rust/upstream rows while preserving the daemon/socket runtime deferral.
- 2026-06-04 `kes-agent` Control V3 focused verification passed: `cargo fmt --all`; `cargo test -p yggdrasil-kes-agent --lib` (60 tests); `cargo clippy -p yggdrasil-kes-agent --all-targets --all-features -- -D warnings`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`.
- 2026-06-04 post-`kes-agent` Control V3 broad verification passed: `cargo fmt --all -- --check`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`.
- 2026-06-04 `kes-agent` Service V2 slice: added strict filename mirrors for upstream `Cardano.KESAgent.Protocols.Service.V2.{Protocol,Peers,Driver}`. The slice pins the typed `Service:2.0` version identifier, mirrors Service V2 states/messages/tokens, pure receiver/pusher choices, key/drop discriminants, and `readErrorToServiceDriverTrace`, and expands `docs/type-parity-audit.tsv` from 376 to 410 exact Rust/upstream rows while preserving the daemon/socket runtime deferral.
- 2026-06-04 `kes-agent` Service V2 focused verification passed: `cargo fmt --all -- --check`; `cargo test -p yggdrasil-kes-agent --lib`; `cargo clippy -p yggdrasil-kes-agent --all-targets --all-features -- -D warnings`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `git diff --check`; `git diff --cached --check`.
- 2026-06-04 post-`kes-agent` Service V2 broad verification passed: `cargo fmt --all -- --check`; `cargo check-all`; `cargo lint`; `cargo lint-no-default`; `cargo test-all`; `python3 dev/test/check-type-parity.py`; `python3 dev/test/check-strict-mirror.py --fail-on-violation`; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-fixture-manifest.py`.

- `cardano-cli hash genesis-file` parity slice: replaced the hash command/run placeholders with upstream-shaped `HashCmds`, `GenesisFile`, `render_hash_cmds`, and `run_hash_cmds`; wired nested `hash genesis-file --genesis FILE` parsing/dispatch; added deterministic Blake2b-256 genesis-file hashing tests.
- Smoke-test placement fix: full `cargo test-all` exposed stale root `scripts/` lookups in `crates/node/cardano-node/tests/smoke.rs`; tests now resolve accepted `dev/scripts/` and `dev/evidence/` locations, including Windows Git Bash relative path handling.
- Verification passed after installing the missing local linker toolchain (`build-essential`): `cargo fmt --all -- --check`, `cargo check -p yggdrasil-cardano-cli --all-targets`, `cargo test -p yggdrasil-cardano-cli hash --lib`, `cargo test -p yggdrasil-cardano-cli --lib`, `cargo test -p yggdrasil-node --test smoke`, `cargo check-all`, `cargo lint`, `cargo test-all`, `python3 dev/test/check-strict-mirror.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-parity-matrix.py`, `python3 dev/test/filetree.py check`, and `git diff --check`.

- Hygiene placement slice: updated live guidance and ownership to reflect the accepted `dev/{scripts,evidence,reference,test}` helper split instead of stale root `scripts/` wording. Touched `dev/AGENTS.md`, `crates/node/cardano-node/AGENTS.md`, `README.md`, `.github/CODEOWNERS`, `justfile`, `docs/PARITY_SUMMARY.md`, `docs/UPSTREAM_PARITY.md`, and the current R517/R824 operational-run notes; historical/archive run records were left as audit history.
- Hygiene verification passed: targeted stale-placement phrase scan over live docs/tooling returned only valid `dev/scripts/...` paths or historical evidence; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/check-doc-status-headers.py`; `python3 dev/test/check-parity-matrix.py`; `python3 dev/test/check-strict-mirror.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`; `git diff --check`.
- Codex-only workspace cleanup removed retired AI-harness tree/guidance, moved filetree to `dev/filetree`, updated live docs/tooling to use root `AGENTS.md` + `dev/test/*`, and hardened stale-placement so retired AI-harness paths cannot reappear.
- Additional Codex-only cleanup normalized non-archived operational-run notes away from removed assistant-specific guidance/plan paths; archived historical run notes are left as audit history.
- Latest Codex-only verification passed: retired-path live scan excluding archive/guard returned no matches; `python3 dev/test/check-stale-placement.py --self-test`; `python3 dev/test/check-stale-placement.py`; `python3 dev/test/filetree.py accept-current`; `python3 dev/test/filetree.py render`; `python3 dev/test/filetree.py check`.
- Initial local audit before implementation: `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, and `cargo lint-no-default` passed.
- Initial blocked gates: `dev/test/check-parity-matrix.py` failed because `.reference-haskell-cardano-node` was absent; `dev/test/check-stale-placement.py` failed because Git did not ignore the bare root reference path.
- Implemented local parity-infrastructure fix: root reference path is now ignored, and stale-placement self-test/live checks pass with the bundled Python runtime.
- Provisioned the source-only Haskell reference tree at upstream tag `11.0.1`; `dev/test/check-parity-matrix.py` passes against the local reference tree.
- Implemented supply-chain fix: `cargo tree -i aws-lc-sys` and `cargo tree -i aws-lc-rs` no longer find packages after switching `axum-server` to the no-provider Rustls feature.
- Hardened line-ending policy and normalized the current checkout for byte-sensitive shell, JSON genesis, and CLI help/version fixtures; this fixed Windows-only raw-byte parity failures in `cargo test-all`.
- Verification passed: `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all` (full test alias run outside the sandbox because Git Bash is blocked inside the sandbox with Win32 access-denied before scripts can start).
- Parity/documentation guards passed after the final edits: doc-status headers, fixture manifest, strict mirror, parity matrix, stale-placement self-test/live checks, and filetree check.
- Security dependency checks passed by absence for `aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys`; feature tree shows `axum-server` using `tls-rustls-no-provider` and `rustls` using `ring`.
- New execution plan accepted: stabilize missing tooling and full reference install first, then close Gap BO, Gap BP, R178, and BlockFetch in that order.
- Installed `cargo-deny` 0.19.8 and ran `cargo deny check advisories bans licenses sources`: passed with warnings only for pre-existing duplicate/unused-license allowances.
- Installed `check-jsonschema` / `jsonschema` and validated `docs/parity-matrix.json` against `docs/parity-matrix.schema.json`.
- Provisioned the full IntersectMBO `cardano-node` 11.0.1 Linux reference install tree under WSL/Linux with `dev/reference/setup-reference.sh`.
- Verified the full reference install with `python3 dev/test/check-reference-artifacts.py`: 9 binaries and 3 network share dirs passed.
- Gap BO evidence slice: added opt-in TPraos VRF evidence logging and preprod `mkSeed` golden coverage; focused `yggdrasil-node-sync` TPraos overlay tests and `yggdrasil-consensus` preprod seed test pass.
- Gap BP evidence slice: added opt-in CEK accumulated-step flush logging; focused `yggdrasil-plutus` machine tests pass.
- R178 evidence slice: added Conway `QueryIfCurrent` match/mismatch envelope regression; focused `yggdrasil-node-ntc-server` test passes.
- BlockFetch Section 6.5 code slice: direct bootstrap sessions now migrate their BlockFetch handle into the shared worker pool when `max_concurrent_block_fetch_peers > 1`, unregister on reconnect/handoff/shutdown, and update the worker gauge; focused `yggdrasil-node-runtime` regression passes.
- Current post-slice Rust gates pass: `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Current post-slice parity/status guards pass: stale-placement self-test/live check, doc-status headers, fixture manifest, strict mirror, parity matrix, filetree check, JSON schema validation, and `git diff --check`.
- Current post-slice security gates pass: `cargo deny check advisories bans licenses sources` exits clean with only pre-existing warnings, and `cargo tree -i aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys` report no matching package IDs.
- R178 comparison harness slice: added `dev/evidence/compare-conway-lsq.py`, verified it with `python -m py_compile` and `--help`, and strengthened the local Conway envelope test to compare full HFC match/mismatch bytes. Live socket comparison still remains open.
- R178 harness verification passed: `cargo fmt --all -- --check`, focused `cargo test -p yggdrasil-node-ntc-server conway_query_if_current_uses_hfc_match_and_mismatch_envelopes --lib`, `cargo check-all`, `python dev/test/check-stale-placement.py`, filetree check, and `git diff --check`.
- Gap BP ScriptContext evidence slice: added `YGG_DUMP_SCRIPT_CONTEXT_FILE` file append support, preserved stderr fallback, added a replayable evidence-line formatter test, and pinned deeper captured V2 ScriptContext field shapes. Focused `yggdrasil-node-plutus-eval` tests pass, including the full crate `--lib` suite.
- Gap BP slice guards passed: `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, `python dev/test/check-stale-placement.py`, `python dev/test/check-strict-mirror.py`, and `git diff --check`.
- Gap BP offline comparison harness slice: added `dev/evidence/compare-gap-bp-script-context.py`, documented it in `scripts/AGENTS.md`, and self-tested it against the captured Rust log both without Haskell input and with self-comparison plus `--require-byte-equal`.
- Gap BP harness guards passed: `python -m py_compile dev/evidence/compare-gap-bp-script-context.py`, `python dev/evidence/compare-gap-bp-script-context.py --help`, `python dev/test/check-stale-placement.py`, filetree accept/check, and `git diff --check`.
- Full post-Gap-BP-harness Rust gate passed: `cargo test-all` completed successfully with the new Plutus evaluator tests included.
- Gap BP status-doc cleanup: `docs/UPSTREAM_PARITY.md` now records the R266/R266b/R266c narrowing and points operators at the ScriptContext/CEK flush captures instead of treating step-cost drift as the only active suspect.
- Post-doc cleanup guards passed: `python dev/test/check-doc-status-headers.py`, `python dev/test/check-stale-placement.py`, and `git diff --check`.
- BlockFetch soak harness hardening: added `dev/evidence/parallel_blockfetch_soak.sh --self-test` so the Section 6.5 helper validates worker metric lookup, missing-metric fallback, numeric comparisons, and average-duration formatting without requiring a live node.
- BlockFetch harness guards passed: `bash dev/evidence/parallel_blockfetch_soak.sh --self-test`, `bash dev/evidence/parallel_blockfetch_soak.sh --help`, `bash -n dev/evidence/parallel_blockfetch_soak.sh`, `python dev/test/check-stale-placement.py`, executable-mode check (`100755`), filetree accept/check, and `git diff --check`.
- R178 LSQ harness hardening: added `dev/evidence/compare-conway-lsq.py --self-test` and documented the helper in `scripts/AGENTS.md`.
- R178 LSQ harness guards passed: `python -m py_compile dev/evidence/compare-conway-lsq.py`, `python dev/evidence/compare-conway-lsq.py --self-test`, `python dev/evidence/compare-conway-lsq.py --help`, `python dev/test/check-stale-placement.py`, filetree accept/check, and `git diff --check`.
- Gap BO TPraos evidence harness slice: added `dev/evidence/compare-gap-bo-tpraos-vrf.py` to parse `TPRAOS_VRF_EVIDENCE` lines, compare stable parity keys by slot, emit `target/gap-bo-tpraos-vrf-comparison/summary.json`, and self-test parsing of nonce values containing spaces.
- Gap BO harness guards passed: `python -m py_compile dev/evidence/compare-gap-bo-tpraos-vrf.py`, `python dev/evidence/compare-gap-bo-tpraos-vrf.py --self-test`, `python dev/evidence/compare-gap-bo-tpraos-vrf.py --help`, `python dev/test/check-stale-placement.py`, filetree accept/check, and `git diff --check`.
- Gap BP comparator hardening: added `dev/evidence/compare-gap-bp-script-context.py --self-test` to validate raw-hex and `cbor_hex=` parsing, declared-length mismatches, equal/mismatched CBOR comparison, diff-window generation, and artifact writes without depending on the captured preview fixture.
- Gap BP comparator guards passed: `python -m py_compile dev/evidence/compare-gap-bp-script-context.py`, `python dev/evidence/compare-gap-bp-script-context.py --self-test`, `python dev/evidence/compare-gap-bp-script-context.py --help`, `python dev/test/check-stale-placement.py`, filetree accept/check, and `git diff --check`.
- Core evidence preflight slice: added `dev/test/check-core-evidence-harnesses.py` so local preflight runs the Gap BO, Gap BP, R178, and BlockFetch evidence helper self-tests together and writes `target/core-evidence-harnesses/summary.json`.
- Core evidence preflight guards passed: `python -m py_compile dev/test/check-core-evidence-harnesses.py`, `python dev/test/check-core-evidence-harnesses.py --help`, `python dev/test/check-stale-placement.py`, `python dev/test/check-core-evidence-harnesses.py`, and `git diff --check`.
- Filetree manifest refreshed after the new preflight script and tracker updates; `python dev/test/filetree.py check` passes.
- WSL reference artifact recheck passed: `python3 dev/test/check-reference-artifacts.py` validates the full IntersectMBO 11.0.1 install tree; `cardano-cli --version` reports `cardano-cli 11.0.0.0` with git rev `97036a66bcf8c89f687ae57a048eecc0389977ef`.
- Gap BO comparator schema hardening: `dev/evidence/compare-gap-bo-tpraos-vrf.py` now requires `slot` plus every compared parity key before accepting Rust or Haskell evidence, so truncated operator logs fail loudly instead of producing weak captured/pass output.
- Gap BO schema hardening guards passed: `python -m py_compile dev/evidence/compare-gap-bo-tpraos-vrf.py dev/test/check-core-evidence-harnesses.py`, `python dev/evidence/compare-gap-bo-tpraos-vrf.py --self-test`, `python dev/test/check-stale-placement.py`, and `python dev/test/check-core-evidence-harnesses.py`.
- R178 comparator hardening: `dev/evidence/compare-conway-lsq.py` now records `cardano-cli --version`, writes raw binary stdout/stderr artifacts beside UTF-8 convenience logs, includes raw stdout/stderr diff windows when a Haskell socket is supplied, and self-tests the HFC `QueryIfCurrent` match/mismatch envelope byte facts.
- R178 comparator guards passed: `python -m py_compile dev/evidence/compare-conway-lsq.py dev/test/check-core-evidence-harnesses.py`, `python dev/evidence/compare-conway-lsq.py --self-test`, and `python dev/test/check-core-evidence-harnesses.py`.
- R178 Rust regression hardening: `conway_query_if_current_uses_hfc_match_and_mismatch_envelopes` now covers the three default Conway operator queries (`gov-state`, `constitution`, `committee-state`), verifies the HFC match envelope payloads, corrects mismatch wording to `[requested, ledger]`, and asserts full `MsgResult` frames inline the match/mismatch envelopes.
- R178 Rust regression guards passed: `cargo fmt --all -- --check`, `cargo test -p yggdrasil-node-ntc-server conway_query_if_current_uses_hfc_match_and_mismatch_envelopes --lib`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `python dev/test/check-core-evidence-harnesses.py`.
- Gap BO Rust evidence contract hardening: `format_tpraos_overlay_vrf_evidence` now emits `nonce_state_phase=ticked_for_verification` plus canonical `{epoch,evolving,candidate,prev_hash,lab}_nonce_hex` fields, and `tpraos_overlay_vrf_evidence_line_carries_required_comparison_keys` pins the Rust-emitted key set against the comparator schema.
- Gap BO comparator metadata hardening: `dev/evidence/compare-gap-bo-tpraos-vrf.py` now requires `era` and `verification` metadata in addition to `slot` and all default comparison keys, including the new nonce hex/phase fields.
- Gap BO evidence contract guards passed: `cargo fmt --all -- --check`, `cargo test -p yggdrasil-node-sync tpraos_overlay_vrf_evidence_line_carries_required_comparison_keys --lib`, `python -m py_compile dev/evidence/compare-gap-bo-tpraos-vrf.py dev/test/check-core-evidence-harnesses.py`, `python dev/evidence/compare-gap-bo-tpraos-vrf.py --self-test`, `python dev/test/check-core-evidence-harnesses.py`, `cargo check-all`, `cargo lint`, and `cargo lint-no-default`.
- Gap BO TPraos proof plumbing hardening: added `tpraos_leader_and_nonce_proofs_are_usage_separated`, using deterministic VRF key material to prove over `seedL` and `seedEta`, verify each intended path, reject seedL as seedEta, reject seedEta as seedL, and reject TPraos seedL under Praos `mkInputVRF`.
- Gap BO TPraos proof plumbing guards passed: `cargo fmt --all -- --check`, `cargo test -p yggdrasil-consensus tpraos_leader_and_nonce_proofs_are_usage_separated --lib`, `python dev/test/check-core-evidence-harnesses.py`, and `cargo check-all`.
- Gap BP CEK flush comparison harness: added `dev/evidence/compare-gap-bp-cek-flushes.py` to parse `YGG_DUMP_CEK_FLUSHES` lines, require accumulated-step flush keys, compare Rust/Haskell flushes by ordinal index, report mismatched budget/count fields, and write `target/gap-bp-cek-flush-comparison/summary.json`.
- Gap BP CEK flush harness guards passed: `python -m py_compile dev/evidence/compare-gap-bp-cek-flushes.py dev/test/check-core-evidence-harnesses.py`, `python dev/evidence/compare-gap-bp-cek-flushes.py --self-test`, and `python dev/test/check-core-evidence-harnesses.py`.
- Gap BP CEK flush emitter contract: added `cek_flush_trace_line_carries_required_comparison_keys`, which runs a deterministic four-step CEK term with `YGG_DUMP_CEK_FLUSHES=1` and pins the exact accumulated-step flush line consumed by `dev/evidence/compare-gap-bp-cek-flushes.py`.
- Gap BP CEK flush emitter guards passed: `cargo fmt --all -- --check`, `cargo test -p yggdrasil-plutus cek_flush_trace_line_carries_required_comparison_keys --lib`, `python dev/evidence/compare-gap-bp-cek-flushes.py --self-test`, `python dev/test/check-core-evidence-harnesses.py`, `cargo check-all`, `cargo lint`, and `cargo lint-no-default`.
- Gap BP builtin-cost comparison harness: added `dev/evidence/compare-gap-bp-builtin-costs.py` to parse `YGG_DUMP_BUILTIN_COSTS` lines, require builtin name, arg-size, charge, and remaining-budget keys, compare Rust/Haskell builtin charges by ordinal index, and write `target/gap-bp-builtin-cost-comparison/summary.json`.
- Gap BP builtin-cost harness guards passed: `python -m py_compile dev/evidence/compare-gap-bp-builtin-costs.py dev/test/check-core-evidence-harnesses.py`, `python dev/evidence/compare-gap-bp-builtin-costs.py --self-test`, `python dev/test/check-core-evidence-harnesses.py`, `python dev/test/check-stale-placement.py`, filetree accept/check, and `git diff --check`.
- Gap BP builtin-cost emitter contract: added `builtin_cost_trace_line_carries_required_comparison_keys`, which runs a deterministic `AddInteger` builtin with `YGG_DUMP_BUILTIN_COSTS=1` and pins the exact per-builtin trace line consumed by `dev/evidence/compare-gap-bp-builtin-costs.py`.
- Gap BP builtin-cost emitter guards passed: `cargo fmt --all -- --check`, `cargo test -p yggdrasil-plutus builtin_cost_trace_line_carries_required_comparison_keys --lib`, `cargo check-all`, `cargo lint`, and `cargo lint-no-default`.
- Post-builtin-cost final guards passed: `python dev/test/check-core-evidence-harnesses.py`, `python dev/test/check-stale-placement.py`, filetree accept/check, and `git diff --check`.
- Gap BP aggregate evidence harness: added `dev/evidence/compare-gap-bp-traces.py` so the preview V2 closeout can run ScriptContext CBOR, CEK flush, and builtin-cost comparisons together; capture mode allows Rust-only artifacts, while parity closeout requires `--require-haskell --require-equal`.
- Gap BP aggregate harness guards passed: `python -m py_compile dev/evidence/compare-gap-bp-traces.py dev/test/check-core-evidence-harnesses.py`, `python dev/evidence/compare-gap-bp-traces.py --self-test`, `python dev/test/check-core-evidence-harnesses.py`, `python dev/test/check-doc-status-headers.py`, `python dev/test/check-stale-placement.py`, filetree accept/check, and `git diff --check`.
- R178 closeout hardening: `dev/evidence/compare-conway-lsq.py` now has `--require-haskell` so byte/normalized equality closeout cannot silently run with only a Yggdrasil socket, and living docs no longer claim Conway LSQ wire parity is fully closed before the upstream socket comparison lands.
- R178 closeout/documentation guards passed: `python -m py_compile dev/evidence/compare-conway-lsq.py dev/test/check-stale-placement.py dev/test/check-core-evidence-harnesses.py`, `python dev/evidence/compare-conway-lsq.py --self-test`, `python dev/test/check-stale-placement.py --self-test`, `python dev/test/check-core-evidence-harnesses.py`, `python dev/test/check-doc-status-headers.py`, `python dev/test/check-stale-placement.py`, stale-phrase scan, and `git diff --check`.
- Closeout equality guard hardening: `dev/evidence/compare-conway-lsq.py` now rejects equality flags without `--haskell-socket`, and `dev/evidence/compare-gap-bp-traces.py --require-equal` rejects missing Haskell ScriptContext, CEK flush, or builtin-cost logs.
- Direct comparator equality hardening: `dev/evidence/compare-gap-bo-tpraos-vrf.py`, `dev/evidence/compare-gap-bp-script-context.py`, `dev/evidence/compare-gap-bp-cek-flushes.py`, and `dev/evidence/compare-gap-bp-builtin-costs.py` now reject strict equality flags unless their Haskell evidence log is supplied.
- Direct comparator equality guards passed: focused `--self-test` runs for Gap BO, Gap BP ScriptContext, Gap BP CEK flushes, and Gap BP builtin costs; `python dev/test/check-core-evidence-harnesses.py`; doc-status and stale-placement scans; `git diff --check`; and manual negative CLI checks proving each direct comparator rejects strict equality mode without `--haskell-log`.
- BlockFetch sign-off hardening: `dev/evidence/parallel_blockfetch_soak.sh` now treats `REQUIRE_TIP_COMPARISON=1` as strict sign-off mode and rejects missing `HASKELL_SOCK`, `EXPECT_WORKERS < 2`, `REQUIRE_WORKERS=0`, `REQUIRE_PROGRESS=0`, or a comparison interval longer than the run window before startup.
- BlockFetch sign-off hardening guards passed: `bash -n dev/evidence/parallel_blockfetch_soak.sh`, `bash dev/evidence/parallel_blockfetch_soak.sh --self-test`, and `python -m py_compile dev/test/check-core-evidence-harnesses.py`.
- BlockFetch review follow-up: `dev/evidence/compare_tip_to_haskell.sh` now has a self-test and fails closed on command failure, invalid JSON, or missing required `slot`/`hash`; `dev/test/check-core-evidence-harnesses.py` runs that self-test.
- BlockFetch strict soak follow-up: `dev/evidence/parallel_blockfetch_soak.sh` now requires `EXPECT_WORKERS >= MAX_CONCURRENT_BLOCK_FETCH_PEERS`, `MIN_TIP_COMPARE_PASSES >= 2`, enough run window for the required comparisons, final worker count at expectation, and zero post-activation worker shortfall samples in `REQUIRE_TIP_COMPARISON=1` mode.
- BlockFetch strict follow-up guards passed: `bash -n dev/evidence/compare_tip_to_haskell.sh`, `bash dev/evidence/compare_tip_to_haskell.sh --self-test`, `bash -n dev/evidence/parallel_blockfetch_soak.sh`, `bash dev/evidence/parallel_blockfetch_soak.sh --self-test`, `python -m py_compile dev/test/check-core-evidence-harnesses.py`, `python dev/test/check-core-evidence-harnesses.py`, `python dev/test/check-doc-status-headers.py`, `python dev/test/check-stale-placement.py`, `python dev/test/filetree.py check`, `git diff --check`, and executable-mode check for the two shell helpers.
- User correction captured: Linux/WSL must be used for Haskell reference binaries, socket/operator evidence, and parity-run shell scripts; native Windows is reserved for Windows Rust gates or simple repository inspection.
- Gap BP correlation hardening: `CekMachine` now accepts an explicit trace ID, node Plutus evaluation sets it to `<tx_hash>:<script_hash>:<version>`, ScriptContext evidence emits the same ID, and CEK flush/builtin-cost trace lines include it.
- Gap BP aggregate guard hardening: CEK flush and builtin-cost comparators require `trace_id`, and `dev/evidence/compare-gap-bp-traces.py` fails when the ScriptContext, CEK flush, and builtin-cost evidence streams cannot be proven to refer to the same evaluation.
- Gap BP correlation guards passed under WSL: `python3 -m py_compile dev/evidence/compare-gap-bp-cek-flushes.py dev/evidence/compare-gap-bp-builtin-costs.py dev/evidence/compare-gap-bp-traces.py`, each focused comparator `--self-test`, and focused Rust tests for CEK flush trace, builtin-cost trace, and ScriptContext evidence line propagation.
- Gap BP diagnostic isolation fix: CEK flush and builtin-cost dumps now require an explicit trace ID before writing, preventing unrelated local CEK tests or ad-hoc evaluations from appending anonymous `trace_id=unknown` evidence.
- Final WSL Rust gates for the correlation slice passed: `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Final WSL focused suites passed: `cargo test -p yggdrasil-plutus --lib` (448 tests) and `cargo test -p yggdrasil-node-plutus-eval --lib` (188 tests).
- Final WSL parity/status/security gates passed: `dev/test/check-reference-artifacts.py`, stale-placement self-test/live check, doc-status self-test/live check, fixture manifest, parity matrix, strict mirror, `dev/test/check-core-evidence-harnesses.py`, `cargo deny check advisories bans licenses sources`, and absence checks for `aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys`.
- R178 live-closeout hardening: `dev/evidence/compare-conway-lsq.py` now rejects missing/non-socket Yggdrasil or Haskell socket paths before invoking `cardano-cli`, bounds each query with `--timeout-seconds`, records timeout metadata in `summary.json`, and self-tests stale socket and timeout rejection.
- R178 live-closeout hardening guards passed under WSL: `python3 -m py_compile dev/evidence/compare-conway-lsq.py dev/test/check-core-evidence-harnesses.py`, `python3 dev/evidence/compare-conway-lsq.py --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, `python3 dev/test/check-stale-placement.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Gap BO closeout hardening: `dev/evidence/compare-gap-bo-tpraos-vrf.py --require-equal` now fails unless the compared evidence includes `--target-slot` (default preprod slot `429460`), preventing accidental sign-off against nearby TPraos evidence.
- Gap BO target-slot guards passed under WSL: `python3 -m py_compile dev/evidence/compare-gap-bo-tpraos-vrf.py dev/test/check-core-evidence-harnesses.py`, `python3 dev/evidence/compare-gap-bo-tpraos-vrf.py --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-strict-mirror.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Gap BP closeout hardening: `dev/evidence/compare-gap-bp-traces.py --require-equal` now requires `--expected-trace-id <tx_hash>:<script_hash>:<version>` and fails when Rust or Haskell ScriptContext, CEK flush, or builtin-cost evidence belongs to a different evaluation.
- Gap BP expected-trace guards passed under WSL: `python3 -m py_compile dev/evidence/compare-gap-bp-traces.py dev/test/check-core-evidence-harnesses.py`, `python3 dev/evidence/compare-gap-bp-traces.py --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-strict-mirror.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- R178 closeout-mode hardening: `dev/evidence/compare-conway-lsq.py` now rejects `--require-haskell` without `--require-byte-equal`/`--require-normalized-equal`, and rejects equality flags without `--require-haskell`, so closeout mode cannot be weakened accidentally.
- User WSL correction captured in `tasks/lessons.md`: Linux-style parity/reference shell work must run as explicit `wsl bash -lc ...`, with native Windows exceptions called out before use.
- R178 closeout-mode guards passed under WSL: `python3 -m py_compile dev/evidence/compare-conway-lsq.py dev/test/check-core-evidence-harnesses.py`, `python3 dev/evidence/compare-conway-lsq.py --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-strict-mirror.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Gap BP aggregate closeout-mode hardening: `dev/evidence/compare-gap-bp-traces.py` now rejects `--require-haskell` without `--require-equal`, and rejects `--require-equal` without `--require-haskell`, so preview V2 trace closeout cannot skip Haskell identity comparison.
- Gap BP aggregate closeout-mode guards passed under WSL: `python3 -m py_compile dev/evidence/compare-gap-bp-traces.py dev/test/check-core-evidence-harnesses.py`, `python3 dev/evidence/compare-gap-bp-traces.py --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-strict-mirror.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Gap BO closeout-mode hardening: `dev/evidence/compare-gap-bo-tpraos-vrf.py` now rejects `--require-haskell` without `--require-equal`, rejects `--require-equal` without `--require-haskell`, records closeout flags in `summary.json`, and `docs/UPSTREAM_PARITY.md` now cites the explicit Gap BO/Gap BP closeout commands.
- Gap BO closeout-mode guards passed under WSL: `python3 -m py_compile dev/evidence/compare-gap-bo-tpraos-vrf.py dev/test/check-core-evidence-harnesses.py`, `python3 dev/evidence/compare-gap-bo-tpraos-vrf.py --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-strict-mirror.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Standalone Gap BP closeout-mode hardening: `dev/evidence/compare-gap-bp-script-context.py`, `dev/evidence/compare-gap-bp-cek-flushes.py`, and `dev/evidence/compare-gap-bp-builtin-costs.py` now require explicit `--require-haskell` plus equality for closeout mode, while `dev/evidence/compare-gap-bp-traces.py` passes that marker through to child comparators.
- Standalone Gap BP closeout-mode guards passed under WSL: `python3 -m py_compile dev/evidence/compare-gap-bp-script-context.py dev/evidence/compare-gap-bp-cek-flushes.py dev/evidence/compare-gap-bp-builtin-costs.py dev/evidence/compare-gap-bp-traces.py dev/test/check-core-evidence-harnesses.py`, focused self-tests for the three direct comparators and aggregate trace gate, `python3 dev/test/check-core-evidence-harnesses.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-strict-mirror.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- BlockFetch tip-comparison timeout hardening: `dev/evidence/compare_tip_to_haskell.sh` now bounds both Yggdrasil and Haskell tip queries with `TIP_QUERY_TIMEOUT_SECONDS` (default 60s), fails stale sockets as exit 2, and self-tests invalid timeout, successful stdout preservation, and timeout reporting.
- BlockFetch timeout guards passed under WSL: `bash -n dev/evidence/compare_tip_to_haskell.sh dev/evidence/parallel_blockfetch_soak.sh`, focused helper self-tests, `python3 dev/test/check-core-evidence-harnesses.py`, doc-status/stale-placement/strict-mirror scans, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- BlockFetch timeout security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with the known duplicate/unused-license warnings, and `cargo tree -i aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys` each reported no matching package IDs.
- BlockFetch soak timeout-contract hardening: `dev/evidence/parallel_blockfetch_soak.sh` now validates `TIP_QUERY_TIMEOUT_SECONDS`, passes it explicitly to `compare_tip_to_haskell.sh`, rejects strict sign-off configs where the timeout is zero or consumes the whole comparison cadence, and records the timeout in the operator summary.
- BlockFetch timeout-contract guards passed under WSL: `bash -n dev/evidence/parallel_blockfetch_soak.sh dev/evidence/compare_tip_to_haskell.sh`, focused helper self-tests, `python3 dev/test/check-core-evidence-harnesses.py`, doc-status/stale-placement/strict-mirror scans, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- BlockFetch timeout-contract security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with only the known duplicate/unused-license warnings.
- Gap BO fixture-writer hardening: `dev/evidence/compare-gap-bo-tpraos-vrf.py` now supports `--write-fixture <path>` only in strict `--require-haskell --require-equal` closeout mode, writes a normalized target-slot JSON fixture only after Rust/Haskell evidence passes, and self-tests both artifact writing and refusal on failed evidence.
- Gap BO fixture-writer guards passed under WSL: `python3 -m py_compile dev/evidence/compare-gap-bo-tpraos-vrf.py dev/test/check-core-evidence-harnesses.py`, `python3 dev/evidence/compare-gap-bo-tpraos-vrf.py --self-test`, validation of the self-test fixture JSON, `python3 dev/test/check-core-evidence-harnesses.py`, doc-status/stale-placement/strict-mirror scans, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Gap BO fixture-writer security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with only the known duplicate/unused-license warnings.
- Gap BP aggregate fixture-writer hardening: `dev/evidence/compare-gap-bp-traces.py` now supports `--write-fixture <path>` only in strict `--require-haskell --require-equal` closeout mode, writes a normalized aggregate JSON fixture after ScriptContext, CEK flush, and builtin-cost comparisons all pass for the expected trace identity, and refuses fixture output for failed or weak captures.
- Gap BP aggregate fixture-writer guards passed under WSL: `python3 -m py_compile dev/evidence/compare-gap-bp-traces.py dev/test/check-core-evidence-harnesses.py`, `python3 dev/evidence/compare-gap-bp-traces.py --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, doc-status/stale-placement/strict-mirror scans, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Gap BP aggregate fixture-writer security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with only the known duplicate/unused-license warnings.
- R178 LSQ fixture-writer hardening: `dev/evidence/compare-conway-lsq.py` now supports `--write-fixture <path>` only for strict `--require-haskell` plus byte/normalized equality closeout mode, writes a normalized fixture with CLI version/query hashes/normalized JSON/raw comparison facts, and keeps socket-specific command paths out of the fixture.
- R178 LSQ fixture-writer guards passed under WSL: `python3 -m py_compile dev/evidence/compare-conway-lsq.py dev/test/check-core-evidence-harnesses.py`, `python3 dev/evidence/compare-conway-lsq.py --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, doc-status/stale-placement/strict-mirror scans, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- R178 LSQ fixture-writer security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with only the known duplicate/unused-license warnings, and `cargo tree -i aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys` each reported no matching package IDs.
- BlockFetch §6.5 JSON summary hardening: `dev/evidence/parallel_blockfetch_soak.sh` now writes machine-readable `$LOG_DIR/summary.json` beside `summary.txt`, carrying strict sign-off assertions, worker/progress metrics, tip comparison counts, timeout contract, and artifact paths.
- BlockFetch §6.5 JSON summary guards passed under WSL: `bash -n dev/evidence/parallel_blockfetch_soak.sh dev/evidence/compare_tip_to_haskell.sh`, `bash dev/evidence/parallel_blockfetch_soak.sh --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, doc-status/stale-placement/strict-mirror scans, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- BlockFetch §6.5 JSON summary security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with only the known duplicate/unused-license warnings, and `cargo tree -i aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys` each reported no matching package IDs.
- Core evidence artifact-validation hardening: `dev/test/check-core-evidence-harnesses.py` now fails if the BlockFetch soak self-test does not leave a strict, passing `target/blockfetch-soak-self-test/summary.json` artifact, and records the artifact check in `target/core-evidence-harnesses/summary.json`.
- Core evidence artifact-validation guards passed under WSL: `python3 -m py_compile dev/test/check-core-evidence-harnesses.py`, `bash dev/evidence/parallel_blockfetch_soak.sh --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, direct JSON assertion for `target/core-evidence-harnesses/summary.json::artifact_checks`, doc-status/stale-placement/strict-mirror scans, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Core evidence artifact-validation security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with only the known duplicate/unused-license warnings, and `cargo tree -i aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys` each reported no matching package IDs.
- Core fixture artifact-validation hardening: `dev/test/check-core-evidence-harnesses.py` now deletes known self-test artifacts before running and fails unless fresh Gap BO, Gap BP, R178, and BlockFetch artifacts pass strict schema/content checks.
- Core fixture artifact-validation guards passed under WSL: `python3 -m py_compile dev/test/check-core-evidence-harnesses.py dev/evidence/compare-gap-bp-traces.py dev/evidence/compare-conway-lsq.py`, `python3 dev/test/check-core-evidence-harnesses.py`, and direct JSON assertions for all four artifact checks in `target/core-evidence-harnesses/summary.json`.
- Core fixture artifact-validation full gates passed under WSL: `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-strict-mirror.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- cardano-testnet Defaults topology slice: added typed `defaultMainnetTopology`
  / `defaultP2PTopology` builders over `yggdrasil_network::TopologyConfig`,
  pinned upstream local/public root groups, advertise/trust flags, hot/warm
  valencies, ledger-peer/bootstrap policy, and peer-snapshot defaults, and
  updated cardano-testnet status guidance.
- cardano-testnet topology slice guards passed: `cargo fmt --all -- --check`,
  `cargo test -p yggdrasil-cardano-testnet`, `cargo check-all`, `cargo lint`,
  `cargo lint-no-default`, `cargo test-all`, `python3 dev/test/check-strict-mirror.py
  --fail-on-violation`, `python3 dev/test/check-stale-placement.py`, `python3
  dev/test/check-doc-status-headers.py`, `python3 dev/test/check-parity-matrix.py`,
  `python3 dev/test/check-fixture-manifest.py`, `python3
  dev/test/check-reference-artifacts.py`, `python3 dev/test/filetree.py
  check`, and `git diff --check`.
- Core fixture artifact-validation security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with only known duplicate/unused-license warnings, and `cargo tree -i aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys` each reported no matching package IDs.
- Core preflight environment hardening: `dev/test/check-core-evidence-harnesses.py` now rejects native Windows execution and points operators to `wsl -e bash -lc "python3 dev/test/check-core-evidence-harnesses.py"` so local parity helpers cannot accidentally run through Windows-hosted Bash.
- Core preflight environment guards passed: WSL `python3 -m py_compile dev/test/check-core-evidence-harnesses.py` and `python3 dev/test/check-core-evidence-harnesses.py` pass, while native Windows `python scripts\check-core-evidence-harnesses.py` exits before running shell helpers with the WSL/Linux requirement.
- Core preflight environment full gates passed under WSL: `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-strict-mirror.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Core preflight environment security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with only known duplicate/unused-license warnings, and `cargo tree -i aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys` each reported no matching package IDs.
- Network topology serialization parity slice: `TopologyConfig` now omits
  disabled `bootstrapPeers` and absent `peerSnapshotFile` during JSON
  serialization, matching upstream `networkTopologyToJSON` /
  `UseBootstrapPeers` omission behavior; `cardano-testnet` default topology
  builders are pinned against the same emitted JSON shape.
- Network topology serialization guards passed: `cargo fmt --all -- --check`,
  focused `cargo test -p yggdrasil-network topology_config_serializes`,
  focused `cargo test -p yggdrasil-cardano-testnet
  default_topologies_serialize_with_upstream_optional_field_omissions`,
  `cargo check-all`, `cargo lint`, `cargo lint-no-default`, `cargo test-all`,
  `python3 dev/test/check-strict-mirror.py --fail-on-violation`, `python3
  dev/test/check-stale-placement.py`, `python3 dev/test/check-doc-status-headers.py`,
  `python3 dev/test/check-parity-matrix.py`, `python3
  dev/test/check-fixture-manifest.py`, `python3 dev/test/check-reference-artifacts.py`,
  `python3 dev/test/filetree.py accept-current && python3
  dev/test/filetree.py check`, and `git diff --check`.
- Optional dependency-policy recheck could not run in this shell because
  `cargo deny` is not installed on the active Cargo toolchain.
- Core closeout artifact gate: added `dev/test/check-core-closeout-artifacts.py` to validate the final live Gap BO/BP/R178 fixtures and BlockFetch preprod/mainnet soak summaries under `target/core-closeout/`; the normal gate currently fails as expected because those live artifacts have not been collected yet.
- Core closeout artifact gate guards passed under WSL: `python3 -m py_compile dev/test/check-core-closeout-artifacts.py`, `python3 dev/test/check-core-closeout-artifacts.py --self-test`, and a controlled normal-mode run proving missing live artifacts are reported as failures.
- Core closeout artifact gate full checks passed under WSL: `python3 dev/test/check-core-evidence-harnesses.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-strict-mirror.py`, `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo lint-no-default`, and `cargo test-all`.
- Core closeout artifact gate security recheck passed under WSL: `cargo deny check advisories bans licenses sources` exited clean with only known duplicate/unused-license warnings, and `cargo tree -i aws-lc-sys`, `aws-lc-rs`, `native-tls`, and `openssl-sys` each reported no matching package IDs.
- Core closeout fixture metadata hardening: Gap BO, Gap BP, and R178 fixture writers now stamp `generated_at_utc` plus strict closeout-mode flags, and both `dev/test/check-core-evidence-harnesses.py` and `dev/test/check-core-closeout-artifacts.py` require that metadata before accepting fixtures.
- Core closeout fixture metadata guards passed under WSL: `python3 -m py_compile` for the touched scripts, focused Gap BO/Gap BP/R178 self-tests, `python3 dev/test/check-core-evidence-harnesses.py`, and `python3 dev/test/check-core-closeout-artifacts.py --self-test`.
- BlockFetch tip-log evidence hardening: `dev/evidence/parallel_blockfetch_soak.sh` now records `tip_compare_logs`, `tip_compare_log_count`, and `tip_snapshots_dir` in `summary.json`, and both core evidence gates require those fields so Section 6.5 sign-off remains auditable after the soak.
- BlockFetch tip-log evidence guards passed under WSL: `bash -n dev/evidence/parallel_blockfetch_soak.sh`, `python3 -m py_compile` for the touched validators, `bash dev/evidence/parallel_blockfetch_soak.sh --self-test`, `python3 dev/test/check-core-evidence-harnesses.py`, and `python3 dev/test/check-core-closeout-artifacts.py --self-test`.
- Core closeout staging helper: added `dev/evidence/stage-core-closeout-artifacts.py` so operators can stage the six strict live artifacts into `target/core-closeout/` without manual placement drift; the helper refuses accidental overwrite and returns the final validator result.
- Core closeout staging helper guards passed under WSL: `python3 -m py_compile dev/evidence/stage-core-closeout-artifacts.py`, `python3 dev/evidence/stage-core-closeout-artifacts.py --self-test`, and `python3 dev/test/check-core-evidence-harnesses.py` including the new staging helper self-test.
- Self-contained BlockFetch staging: `dev/evidence/stage-core-closeout-artifacts.py` now copies BlockFetch log, metrics, tip-snapshot, node-log, summary-text, and Haskell tip-comparison artifacts under the canonical closeout tree, rewrites staged summary paths to those durable copies, and self-tests that deleting the original source artifact directory does not break the final validator.
- Full-completion continuation baseline (2026-06-02): added a bounded full-completion plan at the top of this file, confirmed `docs/parity-matrix.json` still has 2 `verified_11_0_1`, 12 `implemented_needs_11_0_1_evidence`, and 8 `partial` entries, and kept Gap BO/Gap BP/R178/BlockFetch as evidence-blocked rather than claiming completion.
- Local stale-placement repair (2026-06-02): release and reproducible-build workflows now stage `dev/` alongside `configuration/`; `dev/scripts/install_from_release.sh` now requires and installs the bundled `dev/` tooling; `dev/test/check-strict-mirror.py` now loads `dev/test/audit-strict-mirror.py`; tracked `dev/{evidence,reference,scripts}/*.sh` helpers are executable in the Git index.
- Verification for the local repair passed: `python3 dev/test/check-stale-placement.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-parity-matrix.py`, `python3 dev/test/check-fixture-manifest.py`, `python3 dev/test/check-strict-mirror.py`, `python3 dev/test/filetree.py accept-current`, `python3 dev/test/filetree.py check`, `python3 -m py_compile dev/test/check-strict-mirror.py`, `bash -n dev/scripts/install_from_release.sh`, and `git diff --check`.
- Full Rust gates were not rerun for this slice because the changed files were workflow YAML, shell installer text, Python guard pathing, file metadata, filetree metadata, and task tracking; no Rust source was edited by this slice. The existing worktree remains broadly dirty from the larger migration and must be reviewed before commit.
- Codex-only workspace cleanup (2026-06-02): removed the retired AI-harness tree and its guidance file, moved reusable filetree state to `dev/filetree/{manifest.json,FILETREE.md}`, updated live docs/tooling to use root `AGENTS.md` plus `dev/test/*` validators, and hardened `check-stale-placement.py` so retired AI-harness paths cannot reappear in current surfaces.
- Codex-only cleanup verification passed: `python3 dev/test/check-stale-placement.py --self-test`, `python3 dev/test/check-stale-placement.py`, `python3 dev/test/filetree.py accept-current && python3 dev/test/filetree.py render && python3 dev/test/filetree.py check`, `python3 dev/test/check-strict-mirror.py`, `python3 dev/test/check-doc-status-headers.py`, `python3 dev/test/check-parity-matrix.py`, `python3 dev/test/check-fixture-manifest.py`, `python3 -m py_compile dev/test/filetree.py dev/test/check-stale-placement.py dev/evidence/stage-core-closeout-artifacts.py dev/test/check-parity-matrix.py`, `cargo fmt --all -- --check`, and `git diff --check`. Full Rust workspace gates were not rerun because this slice changed docs, workflow text, Python guards, metadata, and one Rustdoc-only comment; no Rust behavior changed.
