# Changelog

All notable changes to Yggdrasil are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Strict 1:1 file-mirror arc (R273-rename + R274–R311) plus
docstring-classification cleanup sub-arc (R313–R320) plus closure-
status triad refresh (R321). Refreshes the vendored upstream Haskell
tree to policy tag `11.0.1`, lands a strict-mirror CI drift-guard
(warn-only at R275 → fail-build at R288 → index-vs-tree drift check
added R311), sweeps every production `.rs` to declare exactly one of
two canonical docstring forms (`**Strict mirror:** <upstream/path.hs>`
or `**Strict mirror:** none.`), purges all production
`#[allow(dead_code)]` sites + the lone production TODO, expands a
new `crates/cardano-cli/` workspace member mirroring the full
upstream cardano-cli surface (~237 files), trims `docs/` from 23 to
11 top-level markdown files (5 archived to `docs/archive/`), adds two
new CI parity validators (`check-fixture-manifest.py` +
`check-reference-artifacts.py`), and eliminates the entire
`(c) strict-partial` bucket via 24 docstring promotions, 11
reclassifications to canonical synthesis, 1 file merge (`multiplexer
.rs` → `mux.rs`), 2 file splits (`handshake.rs` → 3 leaves;
`inbound_governor.rs` → 2 leaves), and 2 final docstring tightens.
Workspace tests: **4,856 passing, 0 failing**. Final audit table
(post-R324): **246 `(a) DIRECT_MIRROR`** + **202 `(c) strict-none`**
= **448 graded files**; **zero `(c) strict-partial`** (R320);
**zero `(a) auto`** + **zero `(a) auto (affinity-filtered)`**
(R323/R324 — every (a) row has explicit
`**Strict mirror:** <upstream/path.hs>` declaration). The audit
table has exactly two canonical verdicts; zero ambiguity, zero
basename-heuristic reliance.

### Added

- **R274 — strict-mirror audit infrastructure.** Refreshes vendored
  install to `cardano-node 11.0.1`. Adds `scripts/audit-strict-mirror.py`
  (walks production `.rs` files, derives candidate upstream basenames
  via snake_case<->PascalCase, applies crate-to-repo affinity filter
  for ambiguous matches), `docs/upstream-haskell-files.txt` (4,676-path
  upstream `.hs` index), and `docs/strict-mirror-audit.tsv` (per-file
  verdict table; every Rust file graded `(a) DIRECT_MIRROR` or
  `(c) NO_MIRROR_NEEDS_DOCSTRING`).
- **R275 — strict-mirror drift-guard (warn-only).** Adds
  `scripts/check-strict-mirror.py` as a CI gate counterpart to the
  authoring-time skill at `.claude/skills/round-extraction/SKILL.md`.
  Flags any new file lacking either an upstream `.hs` mirror or a
  `## Naming parity` docstring stanza.
- **R288 — drift-guard promoted to fail-build.** After R281 closed
  the last violation, the drift-guard flips from `continue-on-error:
  true` to `--fail-on-violation`. Promoted to the named baseline gate
  set ("the five verification expectations").
- **R285 — Phase 6 wiring closure.** Phase 6 multi-peer BlockFetch
  was already shipped via alternative paths in R258; R285 cleans up
  5 stale `#[allow(dead_code)]` annotations on the original
  scaffolding helpers (3 marked `#[cfg(test)]` for test-only seam
  use; 1 routed via `unregister_worker`; 1 already wired but
  carrying a stale allow).
- **R289–R295 — `crates/cardano-cli/` workspace crate.** New
  `yggdrasil-cardano-cli` crate strict-mirrors all 180 upstream
  `Cardano.CLI.*` modules (~237 Rust files including parent shells).
  Sub-trees: Byron (10), Compatible (21), EraBased (57),
  EraIndependent (34), Type (33), Legacy (5), plus IO/Json/OS/Read/
  Run/Option (~10) and the 8 top-level Cardano.CLI namespace files.
  Each leaf carries a strict-mirror `## Naming parity` block;
  parent shells carry synthesis declarations.
- **R296–R297 — concrete cardano-cli migration kickoff.** Wires the
  new crate into the `yggdrasil-node cardano-cli` subcommand:
  R296 routes the `Version` banner through
  `yggdrasil_cardano_cli::helper::version_info()`; R297 migrates
  `ShowUpstreamConfig`'s path resolution + JSON output into
  `yggdrasil_cardano_cli::environment::*`. `QueryTip` remains in
  the node binary pending an LSQ-client trait abstraction.

### Changed

- **R273-rename — strict naming parity for R273b–i sub-modules.**
  9 `git mv` renames in `consensus/opcert/`, `plutus/types/`,
  `plutus/cost_model/`, `plutus/flat/` aligning sub-file basenames
  with upstream (`OCert.hs`, `Builtins.hs`, `Internal.hs` etc.). The
  authoring-time skill at `.claude/skills/round-extraction/SKILL.md`
  is hardened with explicit filename-mirror rules.
- **R276–R281 — Phase B sub-module docstring sweep.** Adds
  `## Naming parity` blocks to 161 sub-module files across
  `crates/ledger/src/state/` (24), `consensus/{nonce,opcert,
  diffusion_pipelining}/` (9), `consensus/mempool/` (7),
  `node/src/runtime/` (18), `network/governor/` (6), and the
  R281 sweeper (97 residuals). Includes 3 `git mv` renames
  (`opcert.rs`/`opcert/` -> `ocert.rs`/`ocert/` for upstream
  `Cardano.Protocol.TPraos.OCert`).
- **R283 — era_tag wiring.** Drops the stale `#[allow(dead_code)]`
  on `node/src/sync.rs::mod era_tag`. Introduces a new named-
  constant module `lsq_era_index` in `node/src/local_server.rs`
  (collapses Byron into a single LSQ ordinal vs the wire-tag's two)
  and replaces magic-number era arms in `GetCurrentPParams`
  dispatch with named constants.
- **R287 — closure annotations on planning docs.** `docs/code-audit.md`
  C-1/H-1/H-2/M-1..M-8/L-1..L-9 findings each receive an inline
  `[CLOSED in 2026-Q3]` annotation. `docs/REFACTOR_BLUEPRINT.md`
  Phase C–G headers each receive a `[DONE in RNNN]` annotation.
- **Phase D + R288 — living-doc parity-language sweep + drift-guard
  fail-build.** Adds strict-1:1-policy citations to every per-crate
  AGENTS.md (13 files); registers `check-strict-mirror.py` as the
  fifth named verification gate in CLAUDE.md and root AGENTS.md.

### Removed

- **R282 — unused `TextEnvelope::description` field.** Drops the
  `#[allow(dead_code)]` field in `node/src/block_producer.rs`. Serde's
  default behavior silently ignores unknown JSON keys, so wire-format
  compatibility with upstream-produced text-envelope files is
  preserved.
- **R286 — `_runstate_impl_marker` + unused `mk_txout`.** Deletes
  `node/src/runtime/reconnecting.rs::_runstate_impl_marker` (replaced
  with a comment line carrying the same visual seam) and the unused
  `crates/ledger/src/eras/shelley.rs::mk_txout` test helper.

### Documentation

- **R298–R300 — `docs/` cleanup arc.** Top-level markdown reduced
  23 -> 11 (-52%). Archived `docs/code-audit.md`,
  `docs/REFACTOR_BLUEPRINT.md`, `docs/AUDIT_VERIFICATION_2026Q2.md`,
  `docs/PARITY_PLAN.md`, `docs/UPSTREAM_RESEARCH.md` to
  `docs/archive/` with explicit Jekyll permalinks preserving
  published URLs. Trimmed `docs/PARITY_SUMMARY.md` from 485 -> 392
  lines by retiring six pre-execution planning sections superseded
  by the R269–R299 execution arc. Relocated `docs/poolMetaData.json`
  (operator artifact, not a doc) to `node/configuration/`.
- **R301 — SPECS.md improvements.** Adds CIPs repository link,
  a new "Local parity surfaces (machine-readable)" section pointing
  at `parity-matrix.json` + `strict-mirror-audit.tsv` +
  `upstream-haskell-files.txt`, and a new Usage Rule capturing the
  R274+ strict 1:1 file-mirror policy.
- **R302 — CLAUDE.md gate-count consistency.** Lines 12 + 127
  updated from "four verification gates" to "five verification
  gates" to match the post-R288 fail-build flip and the existing
  "All five are the required verification expectations" wording in
  the Commands section (line 96).
- **R303 — two new CI parity validators.** Adds
  `scripts/check-fixture-manifest.py` (cross-checks the
  `cardano-base` SHA pin across `node/src/upstream_pins.rs::
  UPSTREAM_CARDANO_BASE_COMMIT`, `specs/upstream-test-vectors/
  cardano-base/<SHA>/`, `docs/SPECS.md`, `docs/UPSTREAM_PARITY.md`;
  verifies 2 required corpora present) and
  `scripts/check-reference-artifacts.py` (validates
  `.reference-haskell-cardano-node/install/`: `cardano-node
  --version` matches policy tag, 9 binaries present, 3 networks ×
  8 config files). The fixture-manifest validator wired to CI;
  reference-artifacts kept local-only (CI doesn't carry the 1.3 GB
  vendored install).
- **R304 — refine strict-mirror citation in non-Rust dirs.**
  Adjusts `specs/AGENTS.md` and `node/configuration/AGENTS.md`
  to clarify the strict-mirror policy applies to production `.rs`
  files only (vendored fixtures + operator config files don't
  fall under R274's file-naming policy).
- **R305 — Reference landing + index + docs/AGENTS.md cleanup.**
  Reorganizes `docs/reference.md` to group docs into "Architecture
  & parity", "Specs & dependencies", "Validation & release", and
  "Archived planning docs" sections. Updates `docs/AGENTS.md`
  policy + manual chapter inventory.
- **R306 — `crates/cardano-cli/AGENTS.md`.** Adds operational guide
  for the new workspace crate: directory shape, strict-mirror policy
  recap, current status (R289–R295 bootstrap, R296+R297 migration
  kickoff), R298+ migration roadmap, integration with the `node`
  binary. Registers the new AGENTS.md in `crates/AGENTS.md` and
  `CLAUDE.md`.
- **R307 — PARITY_SUMMARY.md round-count + test-badge refresh.**
  Bumps round count from 251 to 306+; updates README test badge
  4.7K+ → 4.8K+ to reflect post-R273 baseline.
- **R308 — PARITY_PROOF.md header refresh + `scripts/AGENTS.md`.**
  Front-matter title `(Round 248)` → ``; document-round line
  refreshed; test-count line refreshed (4.7K+ → 4,855); five-gate
  snapshot added; R273-rename + R274–R307 arc summary blockquote
  added at the top of the body. New `scripts/AGENTS.md` registered
  in `CLAUDE.md`. Tail repair: `cargo fmt` auto-fix on 3 files
  with pre-R308 rustfmt drift.
- **R309 — AGENTS.md Current Phase R273-R308 arc closure.**
  Appends single arc-closure sentence to `AGENTS.md`'s Current
  Phase paragraph; long round-by-round notes (lines 540+) remain
  unchanged per existing line 159 guidance.
- **R310 — `.gitignore` over-broad `debug` pattern fix (CI
  failure).** Root `.gitignore` line 2 was a bare `debug` pattern
  intended for Cargo's `target/debug/` build output. Without a
  leading slash or `**/` prefix, gitignore rules match files or
  dirs with that basename anywhere in the tree, silently swallowing
  `crates/cardano-cli/src/era_independent/debug/` (12 R294 files
  never tracked despite existing locally and being graded in
  `docs/strict-mirror-audit.tsv`). Fixed: replace bare `debug` and
  redundant bare `target` with anchored `/target/`. Stage the 12
  previously-invisible files. Surveyed all other bare-basename
  ignore patterns; only `debug` had a current source-tree
  collision.
- **R311 — strict-mirror index-vs-tree drift check.** Closes the
  failure-detection gap that R310 exposed.
  `scripts/check-strict-mirror.py` previously walked only the local
  filesystem; an over-broad `.gitignore` pattern that silently
  swallowed production `.rs` files (R310's failure mode) was
  invisible to the gate, manifesting only as an opaque `cargo fmt`
  module-resolution error on a fresh CI clone. Adds a second pass
  cross-checking every production `.rs` against `git ls-files`;
  files present locally but not tracked surface as a distinct
  drift-violation class with actionable wording.
- **R312 — `docs/UPSTREAM_PARITY.md` arc-closure cross-walk.**
  Closes the canonical-status doc triad refresh by adding a top-
  of-document arc-closure blockquote covering R273-R311 and
  splitting Verification Baseline into "Five-gate snapshot
  (post-R311)" + "Historical R244–R249 closure evidence" subheads.
- **R313 — synthesis-file census.** Read-only census of the 215
  production `.rs` files that carry no 1:1 upstream mirror under
  `.reference-haskell-cardano-node/`. Re-runs
  `scripts/audit-strict-mirror.py` (TSV byte-identical to committed
  allowlist), groups the synthesis bucket by subsystem, sample-
  verifies docstring stanzas. Headline: 230 `(a) DIRECT_MIRROR` +
  215 `(c) NO_MIRROR_NEEDS_DOCSTRING` = 445 graded files; zero
  `(b)` rename-needed; zero `(d)` clash-regrade.
- **R314 — promote partial-mirror docstrings to canonical strict-
  mirror.** Closes the docstring-classification gap from R313's
  census: audit regex bug fix
  (`STRICT_PARTIAL_PATTERN` didn't match `**Strict mirror
  (partial):**` with the trailing colon — only without — so 41
  files fell through to "unspecified" instead of being recognized
  as "strict-partial"); 24 files promoted to canonical
  `**Strict mirror:** <upstream/path.hs>.` form.
- **R315 — reclassify Yggdrasil-side aggregator/glue files to
  synthesis.** R314 left 17 files in `(c) strict-partial` form. Of
  those, 8 were misclassified — they have no upstream 1:1 parallel
  and should declare canonical synthesis form
  (`**Strict mirror:** none.`) rather than the misleading
  `**Strict mirror (partial):**` form. Reclassified:
  `consensus/genesis_density.rs`, `consensus/in_future.rs`,
  `crypto/sha3_hash.rs`, `ledger/rewards.rs`, `ledger/utxo.rs`,
  `network/blockfetch_pool.rs`, `network/listener.rs`, `node/runtime/keep_alive.rs`.
- **R316 — reclassify 3 more partial-mirrors after content audit.**
  Operator asked whether the 9 remaining files could be promoted
  to direct mirrors. Content-vs-name audit revealed 3 more files
  where the basename matches an upstream `.hs` but the content
  diverges (`governor/churn.rs`, `governor/peer_metric.rs`,
  `praos/common.rs`). Reclassified to synthesis with explicit
  upstream symbol cross-references preserved.
- **R317 — merge `multiplexer.rs` into `mux.rs` (1:1 with
  `Mux.hs`).** Promotes `mux.rs` from `(c) strict-partial` to
  `(a) DIRECT_MIRROR` of upstream `Ouroboros.Network.Mux.hs` by
  merging the previously-separate `multiplexer.rs` types module
  back in. Upstream's `Mux.hs` carries SDU framing types +
  per-channel state machine + multiplexer/demultiplexer runtime
  in a single file; Yggdrasil's earlier split was a code-
  organization choice with no upstream basis. multiplexer.rs
  deleted; 9 importing files bulk-updated to use `crate::mux`.
- **R318 — split `handshake.rs` into 3 leaves matching upstream
  `Type/Version/Codec`.** Promotes 3 new leaves to
  `(a) DIRECT_MIRROR`. Split-impl pattern: `codec.rs` adds
  `impl HandshakeMessage` methods (to_cbor / from_cbor) for the
  type defined in `type.rs` — both modules in the same crate so
  Rust coherence accepts. Parent shell `handshake.rs` preserves
  the flat `crate::handshake::Foo` API via `pub use` re-exports;
  no caller in the workspace needs an import path update.
- **R319 — split `inbound_governor.rs` to mirror upstream
  `State.hs` separation.** Promotes 2 files to `(a) DIRECT_MIRROR`
  of upstream `InboundGovernor.hs` + `InboundGovernor/State.hs` by
  splitting Yggdrasil's previous monolithic file along the same
  axis upstream uses (data definitions vs runtime decision engine).
  Same split-impl pattern as R318.
- **R320 — promote 2 plutus partial-mirrors; strict-partial bucket
  now 0.** Closes the `(c) strict-partial` bucket entirely. The 2
  remaining files (`plutus/builtins.rs` + `plutus/machine.rs`) are
  promoted via docstring tighten with sibling-file rationale
  documented as implementation detail. Both files carrying
  `**Strict mirror (partial):**` because Yggdrasil's idiomatic
  split places supporting concerns (data types in `types/*.rs`,
  cost-model parameters in `cost_model/*.rs`) in sibling modules —
  but the primary runtime denotation logic each file carries IS a
  1:1 mirror of its upstream `.hs`. The `(partial)` qualifier was
  obscuring this.
- **R452-R459 — cardano-tracer DataPoint sub-protocol port arc
  (8 rounds).** Ports the full upstream trace-forward DataPoint
  sub-protocol from
  `.reference-haskell-cardano-node/trace-forward/src/Trace/Forward/`
  into `yggdrasil_network` and integrates it into cardano-tracer's
  per-connection Acceptors mux. Closes R423
  `prepare_data_point_requestor_status` + R424
  `run_data_points_acceptor_status` deferred-status descriptors.
  - **R452 — Type port.** `crates/network/src/protocols/data_point_forward.rs`
    ports `Trace.Forward.Protocol.DataPoint.Type.hs` —
    `DataPointName`/`DataPointValue`/`DataPointValues` newtypes,
    3-variant `DataPointForwardState` state machine, `Agency` enum,
    3-variant `DataPointForwardMessage`, transition validator
    (+16 tests).
  - **R453 — Codec port.** (Same file) ports
    `Trace.Forward.Protocol.DataPoint.Codec.hs` — wire tags
    1=Request, 2=Done, 3=Reply; `Maybe`-encoding mirrors cborg's
    canonical shape (`Nothing → array(0)`, `Just v → [1, bytes(v)]`);
    decoder accepts both definite- and indefinite-length list
    encodings (+15 tests including wire-byte-stable lock-down).
  - **R454 — Protocol-Acceptor driver.**
    `crates/network/src/data_point_acceptor.rs` ports
    `Trace.Forward.Protocol.DataPoint.Acceptor.hs` — collapses
    upstream's continuation-passing `DataPointAcceptor m a` ADT
    into async-method calls (`request(names)`, `done()`) on a
    state-machine-correct driver struct (+6 tokio-mux tests).
  - **R455 — Acceptor configuration.**
    `crates/network/src/protocols/data_point_forward_configuration.rs`
    ports `Trace.Forward.Configuration.DataPoint.hs` —
    `DataPointAcceptorConfiguration`
    (acceptor_tracer + should_we_stop fields; no
    what_to_request) + `DataPointForwarderConfiguration`
    (single-field newtype). Reuses R420's `TraceForwardTracer`
    alias (+7 tests).
  - **R456 — DataPointRequestor STM coordination primitive.**
    `crates/network/src/protocols/data_point_forward_utils.rs`
    ports `Trace.Forward.Utils.DataPoint.hs` (acceptor-side
    subset). Collapses upstream's TVar/TMVar STM record into a
    `tokio::sync::Mutex<RequestorState>` + two `Notify` channels.
    Public API: `new`, `ask_for_data_points(names) ->
    DataPointValues` (with 10s timeout mirroring upstream's
    `tenSeconds` constant), `wait_for_ask`, `put_reply` (+12
    tests).
  - **R457 — Run/Acceptor aggregator.**
    `crates/network/src/data_point_run_acceptor.rs` ports
    `Trace.Forward.Run.DataPoint.Acceptor.hs` — exposes
    `accept_data_points_init` / `accept_data_points_resp`
    entry points; internal `run_until_stopped` loop races
    wait-for-ask against a 50ms brake poll for robust
    brake-driven shutdown (synthesis improvement over
    upstream); `SHUTDOWN_TIMEOUT = 15_000ms` matches R421
    (+5 tokio-mux tests).
  - **R458 — cardano-tracer Acceptors integration.** Per-
    connection mux in `crates/tools/cardano-tracer/src/
    acceptors/{server, client}.rs` extends from `[HANDSHAKE,
    TRACE_OBJECTS]` to `[HANDSHAKE, TRACE_OBJECTS,
    DATA_POINTS]`; the per-connection acceptor task runs
    `accept_trace_objects_*` + `accept_data_points_*`
    concurrently via `tokio::join!`. Both sub-protocols share
    the connection-level brake flag. `acceptors/utils.rs`
    ships `prepare_data_point_requestor()` (the real
    `prepareDataPointRequestor` mirror). Closes R423 + R424
    deferred status descriptors.
  - **R459 — Arc closeout (this entry).** Updates
    `crates/tools/cardano-tracer/AGENTS.md` Current-Functional-
    Surface to mark DataPoint sub-protocol shipped;
    `docs/parity-matrix.json::sister-tool.cardano-tracer`
    implemented_evidence + remaining_work refreshed; adds
    `docs/operational-runs/2026-05-11-round-459-data-point-arc-
    closure.md`. Workspace tests: 5,962 → 6,024 (+62 across 8
    rounds). All 5 verification gates clean. 24/24
    cardano-tracer acceptors tests pass at HEAD. Carve-outs
    surviving R459: DataPoint forwarder side (cardano-node side,
    not blocking cardano-tracer), EKG ReqResp sub-protocol
    (Hackage-source synthesis), TraceObject CBOR upstream-byte-
    equivalence (cardano-logging Hackage source), RemoteSocket
    TCP path.
- **R466 — cardano-tracer `before_program_stops` +
  `sequence_concurrently_` closure.** Closes two remaining
  closable cardano-tracer status descriptors that didn't need
  cross-crate dependencies.
  - **`before_program_stops`** (R398-era descriptor) — installs
    SIGINT + SIGTERM handlers via `tokio::signal::unix::signal`.
    On either signal, the handler trips the supervisor's
    `Arc<RwLock<bool>>` brake flag, triggering cohesive shutdown
    of acceptors + rotator + metrics servers (all share the
    same brake at the supervisor level). `cfg(unix)`-gated;
    non-Unix platforms get a no-op stub matching upstream's
    Windows behavior. Wired into
    `do_run_cardano_tracer_with_state` so operators get clean
    Ctrl-C + systemd-stop shutdown.
  - **`sequence_concurrently_`** (R399-era descriptor) — thin
    wrapper that spawns each future via `tokio::spawn`
    (parallel execution on the runtime), then awaits the
    JoinHandles in input order. Panics in any task propagate
    via `panic::resume_unwind`. Mirror of upstream's
    `Control.Concurrent.Async.runConcurrently . traverse_
    Concurrently` semantics.
  Workspace tests: 6,041 → 6,045 (+4: concurrent-execution
  timing, order preservation, empty-input, SIGINT handler
  smoke). All 5 verification gates clean.
- **R465 — cardano-tracer per-connection HandleRegistry deregister
  hook.** Closes the R462 partial-closure descriptor
  `deregister_node_id_status`. Wires the HandleRegistry teardown
  into the Acceptors per-connection finalizer so disconnecting
  forwarders' open log file handles are dropped (FDs closed via
  Arc-drop) rather than leaking until the next reconnect.
  - `AcceptorsServerState` gains a `handle_registry:
    HandleRegistry` field plumbing the supervisor's shared
    registry through the per-connection spawn body.
  - `crates/tools/cardano-tracer/src/acceptors/utils.rs` adds
    `remove_disconnected_node_with_registry` which resolves
    `node_name` from `connected_nodes_names` (falling back to
    `NodeId::as_str()`), then scans the registry for keys with
    matching `node_name.0` and removes them. SharedLogFile Arc
    drop closes the underlying FDs.
  - Both `acceptors/server.rs` (responder) and
    `acceptors/client.rs` (initiator) updated to use the
    registry-aware variant in both the `on_error` finalizer
    and the post-protocol cleanup paths.
  - `run.rs::run_cardano_tracer_default` constructs the
    registry before `AcceptorsServerState` so the same handle
    is shared across the lo_handler factory, the rotator, and
    the per-connection teardown hook.
  Mirror of upstream `deregisterNodeId tracerEnv nodeId`
  (cardano-tracer/.../Handlers/Logs/TraceObjects.hs). 2 new
  integration tests: clears 2-LoggingParams alice + leaves
  unrelated bob untouched; node_id fallback when no friendly
  name registered. Workspace tests: 6,039 → 6,041 (+2). All 5
  verification gates clean.
- **R464 — cardano-tracer `runMetricsServers` aggregator wiring.**
  Closes the R408-R414-era partial-wiring status descriptor in
  `run.rs::run_metrics_servers_status`. Adds
  `run_metrics_servers(config, state, stop_flag)` — the
  `Servers.hs`-equivalent aggregator that conditionally spawns:
  - `run_prometheus_server` when `config.has_prometheus.is_some()`
    (Prometheus endpoint + labels + metrics_help + no_suffix
    threaded through).
  - `run_monitoring_server` when `config.has_ekg.is_some()`
    (EKG-equivalent HTML/JSON endpoint).
  Each server is started via the existing R408 / R410 entry
  points; the aggregator awaits the supervisor-level brake then
  aborts the spawned `JoinHandle`s so the bound ports are
  released. Mirror of upstream `runMetricsServers tracerEnv =
  sequenceConcurrently_ [whenJust hasEKG $ ..., whenJust
  hasPrometheus $ ...]`.

  Wired into `do_run_cardano_tracer_with_state` alongside the
  acceptors + rotator — shares the same `rotator_stop` brake
  flag so all three subsystems shut down cohesively on supervisor
  exit. Adds `RunCardanoTracerError::MetricsServer` variant for
  bind failures.

  4 new tokio-async integration tests: both endpoints None
  (no-op), Prometheus only, Monitoring only, both concurrent.
  Each binds to a kernel-allocated ephemeral port (`port: 0`)
  + verifies clean brake-driven shutdown within 2 seconds.
  Workspace tests: 6,035 → 6,039 (+4). All 5 verification gates
  clean.
- **R463 — cardano-tracer Logs Rotator + file-write soak test.**
  Validates the R461 + R462 system under realistic concurrent
  load. Adds
  `handlers::logs::rotator::tests::rotator_and_writer_cooperate_under_load`
  — a focused integration test that:
  1. Spawns `run_logs_rotator` in a task with aggressive rotation
     (size limit 80 bytes, 1s frequency, retain 2 historic).
  2. Drives 10 batches of 2 ForMachine events each, fired every
     250ms (interleaves with the rotator's 1s scan cadence).
  3. After settling, trips the brake and verifies the on-disk
     state: ≥1 log file + ≥1 symlink, log count bounded by
     `keep_files_num + slack` (≤4), and exactly one
     HandleRegistry entry for the (node, params) key (proves
     no FD accumulation across rolls).
  Closes the R462 advisor flag that R461/R462 hadn't been
  validated as a system. Also corrects R462's
  `write_trace_objects_to_file_status` descriptor — said
  `create_or_update_empty_log` shipped at R390, actually shipped
  at R402 (R390 was just the pure log-naming + timestamp parser
  subset). Adds
  `docs/operational-runs/2026-05-11-round-463-logs-rotator-soak.md`
  capturing the closure. Workspace tests: 6,034 → 6,035 (+1).
  All 5 verification gates clean.
- **R462 — cardano-tracer trace-objects file-write IO orchestration
  + HandleRegistry handoff.** Closes R461's advisor flag that the
  rotator was operationally inert: `trace_objects_handler` produced
  `FilePending` outcomes that never wrote to disk + never registered
  handles in the supervisor's `HandleRegistry`. R462:
  - Ports upstream `writeTraceObjectsToFile` to
    `crates/tools/cardano-tracer/src/handlers/logs/file.rs::write_trace_objects_to_file`.
    Looks up an existing handle in the shared registry; if absent,
    mints one via `super::utils::create_or_update_empty_log` (which
    creates the subdirectory, opens the file, registers the handle,
    and swaps the convenience symlink). Appends the prepared bytes
    and flushes.
  - Adds `trace_objects_handler_with_registry` to
    `handlers/logs/trace_objects.rs` — the registry-aware variant
    of the dispatcher. Existing `trace_objects_handler` stays for
    backward compatibility with registry-less call sites (returns
    `FilePending`). New variants of `DispatchOutcome`:
    `FileWritten { written_bytes }` for successful writes,
    `FileError { message }` for transport failures.
  - Adds `default_lo_handler_factory_with_registry` to `run.rs` —
    captures the supervisor's shared `(HandleRegistry,
    current_log_lock)` pair so each invocation of the lo_handler
    routes through `trace_objects_handler_with_registry`.
  - Refactors `do_run_cardano_tracer_with_state` to accept an
    optional shared `(HandleRegistry, current_log_lock)` pair and
    spawn the rotator (R461 logic moved here). `do_run_cardano_tracer`
    delegates to it with `None`. `run_cardano_tracer_default`
    constructs the shared registry/lock first, threads them
    through both the factory and the supervisor — so the rotator
    sees the real open handles written to by the lo_handler under
    load.
  - Closes `write_trace_objects_to_file_status` (R402-era deferral
    descriptor). Partial closure on `deregister_node_id_status` —
    registry-stored handles now exist for `Registry::remove` +
    Arc-drop semantics, but the per-connection deregistration hook
    into Acceptors teardown is a follow-on round (bounded leakage,
    not a correctness gap).
  Workspace tests: 6,029 → 6,034 (+5: 2 new file-write tests, 3
  new registry-aware handler tests, 1 existing status-descriptor
  test now asserts closed state). All 5 verification gates clean.
- **R461 — cardano-tracer Logs Rotator IO orchestration port.**
  Closes the previously-deferred IO orchestration in
  `crates/tools/cardano-tracer/src/handlers/logs/rotator.rs`. The
  pure helpers (`logging_params_for_files`, `log_is_full`,
  `check_if_there_are_old_logs`, `logs_to_remove`,
  `sort_logs_oldest_first`) were already shipped; R461 layers the
  IO orchestration on top:
  - `run_logs_rotator(config, registry, lock, stop_flag, error_tracer)` —
    top-level entry, no-ops when `config.rotation == None`.
  - `launch_rotator` — sleep-loop running every
    `rotation.frequency_secs` seconds; brake-aware via
    `tokio::select!` racing the sleep against a 50ms brake poll.
  - `check_root_dir` — lists subdirectories of the log root,
    uses `tokio::task::JoinSet` for per-subdir concurrency
    (mirror of upstream's `forConcurrently_`).
  - `check_logs` — sorts log files oldest-first, calls
    `check_if_current_log_is_full` on the newest log, then
    `check_if_there_are_old_logs` on the older ones.
  - `check_if_current_log_is_full` — queries the open handle's
    metadata for file size, rolls via
    `super::utils::create_or_update_empty_log` when over threshold.
  Wired into `do_run_cardano_tracer` supervisor alongside
  `run_acceptors` via `tokio::spawn` with a supervisor-level
  `Arc<RwLock<bool>>` brake flag. Acceptors finishing (either by
  brake or error) trips the rotator's brake so its sleep-loop
  unwinds cleanly within ~50ms. Both `run_logs_rotator_status`
  descriptors (rotator.rs struct + run.rs `&str`) repurposed from
  deferred to closed state. Carve-out: `showProblemIfAny` →
  caller-supplied `LogsRotatorErrorTracer` closure
  (`Arc<dyn Fn(&str) + Send + Sync>`) since the tracer-trace
  channel from `MetaTrace.hs` remains unported. 4 new tokio-async
  integration tests landed in `rotator.rs`. Workspace tests:
  6,025 → 6,029 (+4). All 5 verification gates clean.
- **R460 — R459 advisor-flag closure.** (1)
  `scripts/check-parity-matrix.py` `ALLOWED_MILESTONES` extends
  via `_arc_range(460, 479)` to admit post-R459 follow-on arcs.
  (2) `acceptors::server::tests::server_round_trips_both_sub_protocols_concurrently`
  is a focused integration smoke that exercises
  `tokio::join!(accept_trace_objects_resp, accept_data_points_resp)`
  over a real Unix socket with full handshake + brake + MsgDone
  exchange on both sub-protocols. Closes the R459 advisor flags
  that (a) the allowlist cap would block any next-arc parity-
  matrix milestone bump and (b) the "boots with DataPoint
  multiplexed" claim was untested at the integration level.
  Workspace tests: 6,024 → 6,025 (+1).
- **R451 — workspace Cargo.toml comment cleanup (final post-R447
  trailing references).** Updates 2 trailing references in
  `Cargo.toml`'s `[workspace.dependencies]` section that still
  pointed at the pre-R447 `crates/bech32/` path:
  - `bech32 = "0.11"` justification comment: "Foundation for
    `crates/bech32/`" → "Foundation for `crates/tools/bech32/`"
    + "R447 relocated under `crates/tools/`" annotation.
  - `bs58 = "0.5"` justification comment: "Used by `crates/bech32/`"
    → "Used by `crates/tools/bech32/` (R447 relocated)".
  Post-R451 comprehensive audit (grep across `*.rs` / `*.toml` /
  `*.yml` / `*.yaml` / `*.sh` / `*.py` excluding `target/`):
  zero remaining stale `crates/<tool>/` references in production
  code, workspace config, CI workflows, scripts. The post-R447
  cleanup is now genuinely complete — every non-historical
  cross-reference has been retargeted to `crates/tools/<tool>/`.
  Workspace test count unchanged at 5,962. All 5 verification
  gates clean + parity-matrix gate clean.
- **R450 — node-crate doc-comment path cleanup (post-R447
  trailing references).** Updates 4 production rustdoc comments
  in the `node/` crate that still pointed at pre-R447
  `crates/<tool>/` paths:
  - `node/src/commands/cardano_cli.rs` (Strict-mirror docstring
    citing `crates/cardano-cli/`)
  - `node/src/upstream_pins.rs` — 3 sister-tool upstream-pin
    descriptions (`crates/bech32/`, `crates/kes-agent/` +
    `crates/kes-agent-control/`, `crates/dmq-node/`)
  Each rewrite preserves the round-band annotation context
  (R331-R334 / R344-R354 / R355-R359 / R450-R459) and appends a
  "R447 relocated" note for searchability.
  Workspace test count unchanged at 5,962. All 5 verification
  gates clean.
  Post-R450 audit: `grep -rE 'crates/(bech32|cardano-cli|cardano-submit-api|...etc.)/' node/ crates/AGENTS.md crates/*/AGENTS.md scripts/AGENTS.md docs/AGENTS.md docs/ARCHITECTURE.md specs/AGENTS.md` returns zero hits — all production code + living docs reference the post-R447 `crates/tools/<tool>/` layout. Historical operational-runs docs intentionally preserved per CLAUDE.md's historical-evidence rule.
- **R449 — post-R447 living-doc path cleanup (CLAUDE.md +
  DEPENDENCIES.md).** Updates the two non-historical documentation
  surfaces that still referenced pre-R447 `crates/<tool>/` paths:
  - **CLAUDE.md**: AGENTS.md index table row for cardano-cli
    rewrites `crates/cardano-cli/AGENTS.md` →
    `crates/tools/cardano-cli/AGENTS.md` + appends "(R447:
    relocated under `crates/tools/`)" annotation.
  - **docs/DEPENDENCIES.md**: 2 forward-looking references updated
    — the `bech32` workspace-dep justification's "Foundation for
    `crates/bech32/`" reference (now `crates/tools/bech32/`) and
    the deferred-tracing-appender pointer to
    `crates/cardano-tracer/src/handlers/logs/rotator.rs` (now
    under `crates/tools/cardano-tracer/`).
  Historical docs (PARITY_SUMMARY.md, PARITY_PROOF.md,
  UPSTREAM_PARITY.md, top-level AGENTS.md's session-closure
  narratives, dated `docs/operational-runs/*` files) intentionally
  preserve their pre-R447 path references per CLAUDE.md's
  historical-evidence rule ("Treat dated files under
  `docs/operational-runs/` as historical evidence...rather than
  rewriting old run records").
  CI workflow audit (`.github/workflows/{ci, pages, release,
  upstream-cardano-node-tests}.yml`): zero stale crate-path
  references — CI uses cargo-target-name dispatch, not path-based.
  Workspace test count unchanged at 5,962. All 5 verification
  gates clean + parity-matrix gate clean.
- **R448 — sister-tools AGENTS.md refresh sweep (post-R447
  documentation cleanup; closes the loop on R439-R445 + R446).**
  Refreshes 7 sister-tool AGENTS.md files (snapshot-converter,
  kes-agent-control, db-synthesizer, db-analyser, kes-agent,
  dmq-node, cardano-testnet) to reflect the structured deferral
  surfaces shipped at R439-R445 + (for snapshot-converter)
  R446's `LedgerSnapshotVersion` scaffolding.
  Each AGENTS.md update is identical in shape:
  - **"Current functional surface"** section refreshed: the
    previously-bullet about "returns 'not yet implemented' sentinel"
    is replaced with a bullet referencing the typed `RunError`
    variant (e.g. `RunError::ConvertSnapshotDeferred`,
    `RunError::SubcommandSocketIoDeferred`, `RunError::ForgeLoopDeferred`,
    etc.) + a forward pointer to the new "Carve-out inventory"
    section.
  - **New "Carve-out inventory" section** lists each `*_status()`
    helper in a table (status helper name + one-line deferral
    rationale + dependency pointer). Mirrors the cardano-tracer
    R424-R429 pattern.
  - Where applicable: typed `Subcommand` enum cli verbs documented
    inline (kes-agent-control's 6 verbs; cardano-testnet's 3 verbs).
  Functional impact: zero. Workspace test count unchanged at 5,962.
  All 5 verification gates clean.
  R448 completes the R439-R446 documentation loop — every operator-
  facing deferred surface in the sister-tools workspace is now
  cross-referenced from its tool's AGENTS.md to the corresponding
  `status::*` helper. Operators / future contributors can grep
  `fn .*_status()` workspace-wide + cross-link to the AGENTS.md
  "Carve-out inventory" tables for the deferral context.
- **R447 — workspace: sister-tools relocated under `crates/tools/`
  (organizational restructure; zero functional change).** Operator-
  requested cleanup that groups the 13 sister tools (bech32,
  cardano-cli, cardano-submit-api, cardano-testnet, cardano-tracer,
  db-analyser, db-synthesizer, db-truncater, dmq-node, kes-agent,
  kes-agent-control, snapshot-converter, tx-generator) under a
  single `crates/tools/<tool>/` subdirectory. After R447, `crates/`
  has 8 entries instead of 19 (6 core crates + 1 `tools/` grouping
  + `AGENTS.md`).
  Path changes:
  - Each `crates/<tool>/` → `crates/tools/<tool>/` via `git mv` (13
    moves; preserves git history per file).
  - Workspace `Cargo.toml` `[workspace.members]` entries rewritten
    to the new paths + reorganized with a "Sister tools" comment
    block introducing the grouping.
  - Workspace dep entry `yggdrasil-cardano-cli = { path = "..." }`
    rewritten to `crates/tools/cardano-cli`.
  - 5 sister-tool `Cargo.toml` files with `path = "../<core>"`
    workspace-internal dep entries updated to `path = "../../<core>"`
    (cardano-submit-api, db-truncater, db-analyser, db-synthesizer
    — all referencing core crates one directory up).
  - `docs/parity-matrix.json` — 12 `rust_surface` entries rewritten
    to `crates/tools/<tool>/` (validated by `check-parity-matrix.py`).
  - `docs/strict-mirror-audit.tsv` — regenerated via
    `audit-strict-mirror.py`; 321 rows under the new `crates/tools/`
    prefix, 0 stale rows under the old `crates/<tool>/` paths.
  - `crates/AGENTS.md` — Current Layout section gains a "Directory
    grouping (R447 restructure)" block explaining the 6 core +
    `tools/` split; LOC-count helper bash block updated to iterate
    `crates/{core...}/` + `crates/tools/*/`.
  - `scripts/check-strict-mirror.py` — single comment-only path
    reference updated.
  Note: the strict-mirror gate's `git ls-files -- "crates/*.rs"`
  pathspec recurses correctly through `crates/tools/<tool>/src/`
  without modification — pathspec `*` in git ls-files is recursive,
  unlike shell glob. `scripts/audit-strict-mirror.py`'s `RUST_ROOTS`
  already used `ROOT / "crates"` which walks all subdirs.
  Functional impact: zero. Workspace tests unchanged at 5,962
  passing (every test that ran before R447 still runs + passes
  post-R447). All 5 verification gates clean:
  - `cargo fmt --all -- --check`
  - `cargo check-all`
  - `cargo test-all` (5,962 passing, 0 failing)
  - `cargo lint`
  - `python3 scripts/check-strict-mirror.py --fail-on-violation`
  - Plus `python3 scripts/check-parity-matrix.py` (20 entries
    validated against reference tag 11.0.1).
  Organizational benefit: `crates/` is now visually compact —
  scanning the directory listing surfaces the 6 core runtime
  crates immediately, with the sister-tools grouped under a single
  `tools/` entry rather than interleaved among the runtime crates.
- **R446 — storage: snapshot-converter format-version design
  scaffolding (operator-approved design round).** Lands the
  Yggdrasil-format ledger-snapshot version-tag scheme +
  `LedgerStore` trait extension that gates the snapshot-converter's
  actual conversion logic (currently surfaced as a
  `convert_snapshot_status` deferral via R439).
  Two new public surfaces in `crates/storage/src/ledger_db.rs`:
  - **LedgerSnapshotVersion(u32) newtype** with named constants:
    `MAGIC = *b"YgLS"` (4-byte sentinel for V2+ snapshots);
    `V1 = 1` (current Yggdrasil format, no header); `LATEST = V1`
    (alias bumps in future rounds). `has_header() → bool`
    predicate distinguishes V1 (no magic prefix) from V2+
    (carries header). `new(u32)` constructor preserves unknown
    future-version tags verbatim for diagnostic surface.
  - **detect_version(data: &amp;[u8]) → LedgerSnapshotVersion**: pure,
    allocation-free header-detection helper. Inspects the first
    8 bytes for the `MAGIC` prefix; falls back to V1 for
    absent / shorter-than-8-byte / non-matching input. Safe to
    call in hot loops (snapshot-converter directory scans).
  - **LedgerStore::latest_snapshot_version() → Option&lt;LedgerSnapshotVersion&gt;**:
    new trait method with default impl delegating to
    `detect_version()` over the latest snapshot's bytes.
    `InMemoryLedgerStore` + `FileLedgerStore` inherit it
    automatically.
  Wire format for V2+ snapshots (V1 stays plain CBOR):
  `[b"YgLS" (4-byte magic)][version u32 big-endian][payload bytes]`.
  Forward-compatible: future versions can carry richer headers
  behind the magic without breaking V1 readers.
  Honest re-scoping vs upstream documented in the design doc:
  upstream's 3×3 mem↔lmdb↔lsm conversion matrix collapses for
  Yggdrasil (single backend); real Yggdrasil snapshot-converter
  scope is format-version migration over time (e.g. era
  extensions). The R446 scaffolding gates that future work.
  Out-of-scope for R446 (documented as follow-on roadmap in the
  design doc): actual V1→V2 migration body (lands when V2
  format is defined); snapshot-converter binary wiring (stays
  on R439's deferral surface until V2 exists);
  `FileLedgerStore` on-disk header writing (V1 has no header for
  backwards compat).
  Tests: yggdrasil-storage 95 → 107 (+12: version constants
  canonical; has_header false for V1; has_header true for V2+;
  detect_version V1 for legacy/empty/short payloads; V2 for
  shaped header; preserves unknown future version; rejects
  almost-magic prefix; latest_snapshot_version None/V1/V2
  dispatch via `InMemoryLedgerStore`). Workspace: 5,950 → 5,962.
  Ships `docs/operational-runs/2026-05-11-round-446-snapshot-converter-format-design.md`
  with the design rationale + carve-out inventory + follow-on
  roadmap.
  Parity-matrix entry sister-tool.snapshot-converter advanced:
  next_milestone R440 → R447 + implemented_evidence bullet added
  referencing R446's scaffolding.
- **R445 — cardano-testnet: structured deferral surface
  (replicates the R439-R444 `*_status()` pattern for the era-
  aware-dispatch carve-out; completes the sister-tools structural-
  deferral sweep started at R439).** Lands
  `crates/cardano-testnet/src/status.rs` (new) + `lib.rs`
  refactor:
  - **status::Subcommand enum**: `Cardano | CreateEnv | Version` —
    3-variant identifier for the cardano-testnet top-level
    subcommands. `cli_verb()` returns the canonical CLI verb.
  - **status::EraDispatchStatus**: 4-field descriptor + helper
    `era_dispatch_status()`. Documents that the per-subcommand
    era-aware dispatch is gated on the cardano-testnet mini-arc
    (R416-R433 — LARGE; Hedgehog Process/Property modules approved
    as Rust-idiomatic carve-out using `tokio::process` + `proptest`)
    AND on yggdrasil-ledger's era surface being exposed at crate
    boundaries.
  - **RunError::SubcommandEraDispatchDeferred { subcommand }**:
    replaces the prior raw `eyre!` string.
  Tests: yggdrasil-cardano-testnet +4 from new status module
  (subcommand cli_verbs match upstream-canonical 3 verbs; Display
  matches cli_verb; era_dispatch_status describes deferral with
  cardano-testnet-mini-arc + Hedgehog + yggdrasil-ledger markers;
  status is Clone+Eq+Hash-round-trip via HashSet insertion).
  Workspace: 5,946 → 5,950. Parity-matrix entry sister-
  tool.cardano-testnet advanced: next_milestone R368 → R446.
  Completes the sister-tools structural-deferral sweep —
  cardano-tracer (R424-R429), snapshot-converter (R439),
  kes-agent-control (R440), db-synthesizer (R441), db-analyser
  (R442), kes-agent (R443), dmq-node (R444), cardano-testnet
  (R445). All sister tools with raw `eyre::eyre!` stubs now have
  structured `RunError` enums + programmatic `*_status()`
  introspection helpers.
- **R444 — dmq-node: structured deferral surface (replicates
  the R439-R443 `*_status()` pattern for the dmq-node Diffusion
  / NodeKernel / PeerSelection wiring carve-out).** Lands
  `crates/dmq-node/src/status.rs` (new) + `lib.rs` refactor:
  - **status::DiffusionWiringStatus**: 4-field descriptor +
    helper `diffusion_wiring_status()`. Documents that the
    diffusion wiring is gated on the dmq-node mini-arc
    (R450-R459 — Tier 4 sister project per the
    playful-tickling-plum.md plan).
  - **RunError::DiffusionWiringDeferred**: replaces the prior
    raw `eyre!` string. Preserves all 7 CLI-resolution markers
    (host:port, local_socket, config_file, topology_file,
    cardano_socket, cardano_magic, dmq_magic) as structured
    fields.
  Tests: yggdrasil-dmq-node +2 from new status module
  (diffusion_wiring_status describes deferral with dmq-node-mini-
  arc + R450-R459 markers; status is Clone+Eq+Hash-round-trip
  via HashSet insertion). Workspace: 5,944 → 5,946. Parity-matrix
  entry sister-tool.dmq-node advanced: next_milestone R370 → R445.
- **R443 — kes-agent: structured deferral surface (replicates
  the R439-R442 `*_status()` pattern for the kes-agent daemon
  carve-out).** Lands `crates/kes-agent/src/status.rs` (new) +
  `lib.rs` refactor:
  - **status::DaemonStatus**: 4-field descriptor + helper
    `daemon_status()`. Documents that the daemon dispatch is
    gated on the kes-agent mini-arc (R344-R354 — highest-stakes
    parity since the socket protocol must be byte-equivalent or
    live SPO setups break).
  - **RunError::DaemonDispatchDeferred**: replaces the prior
    raw `eyre!` string. No fields — the deferral is global
    rather than per-subcommand at this skeleton stage.
  Tests: yggdrasil-kes-agent unchanged → +2 from new status
  module (daemon_status describes deferral with kes-agent-mini-
  arc + crates/crypto/src/kes markers; status is
  Clone+Eq+Hash-round-trip via HashSet insertion).
  Workspace: 5,942 → 5,944. Parity-matrix entry sister-
  tool.kes-agent advanced: next_milestone R345 → R444.
- **R442 — db-analyser: structured deferral surface (replicates
  the R439-R441 `*_status()` pattern for the
  `Cardano.Tools.DBAnalyser.{HasAnalysis, Analysis, Run}`
  carve-outs).** Lands in `crates/db-analyser/src/status.rs`
  (new) + `lib.rs` refactor:
  - **status::AnalysisDispatchStatus**: 4-field descriptor
    (status, depends_on, deferred_round, upstream_reference)
    returned by [`status::analysis_dispatch_status`]. Documents
    that the per-era HasAnalysis + 13-variant Analysis.hs dispatch
    is gated on yggdrasil's per-era ImmutableStore block-iteration
    surface (Phase B.2 R391-R400 per the playful-tickling-plum.md
    plan).
  - **RunError enum**: `AnalysisDispatchDeferred { db, analysis,
    backend, limit }` — replaces the prior raw `eyre!` string,
    preserves the CLI-resolution markers as structured fields.
  Tests: yggdrasil-db-analyser 105 → 107 (+2: analysis_dispatch_status
  describes deferral with ImmutableStore + Phase-B.2 markers;
  status is Clone+Eq+Hash-round-trip via HashSet insertion).
  Workspace: 5,940 → 5,942. Parity-matrix entry sister-tool.db-
  analyser advanced: next_milestone R377 → R443.
- **R441 — db-synthesizer: structured deferral surface
  (replicates the R439-R440 `*_status()` pattern for the
  `Cardano.Tools.DBSynthesizer.{Forging, Run}` carve-outs).**
  Lands in `crates/db-synthesizer/src/status.rs` (new) +
  `lib.rs` refactor:
  - **status::ForgeLoopStatus**: 4-field descriptor (status,
    depends_on, deferred_round, upstream_reference) returned by
    [`status::forge_loop_status`]. Documents that the forge-loop
    + Run.hs supervisor are gated on the Phase C authorization
    checkpoint per the playful-tickling-plum.md plan (cardano-cli
    MVS in the parallel C-arc must complete first).
  - **RunError enum**: `ForgeLoopDeferred { config, chain_db,
    limit, mode }` — replaces the prior raw `eyre!` string.
    Preserves the operator-visible CLI-resolution markers
    (config + chain-db paths + limit + mode) as structured fields.
  Tests: yggdrasil-db-synthesizer 41 → 43 (+2: forge_loop_status
  describes deferral with Phase-C + block-producer markers;
  status is Clone+Eq+Hash-round-trip via HashSet insertion).
  Workspace: 5,938 → 5,940. Parity-matrix entry sister-
  tool.db-synthesizer advanced: next_milestone R379 → R442.
- **R440 — kes-agent-control: structured deferral surface
  (replicates the R439 `*_status()` pattern for the
  `Cardano.KESAgent.Processes.ControlClient` socket I/O carve-
  out).** Lands in `crates/kes-agent-control/src/status.rs` (new)
  + `lib.rs` refactor. Three new public types + helpers:
  - **status::ControlClientStatus**: 4-field descriptor (status,
    depends_on, deferred_round, upstream_reference) returned by
    [`status::control_client_status`]. Documents that the socket
    I/O surface is gated on the kes-agent server mini-arc landing
    first (R344-R354 per the playful-tickling-plum.md plan —
    highest-stakes parity in the sister-tools arc since the
    socket protocol must be byte-equivalent or live SPO setups
    break).
  - **status::Subcommand enum**: `GenStagedKey | ExportStagedVkey
    | DropStagedKey | InstallKey | DropKey | Info` — stable
    identifier for one of the 6 kes-agent-control subcommands.
    `cli_verb() → &'static str` returns the canonical CLI verb
    (mirror of upstream's optparse-applicative `command`
    keyword); `Display` impl uses it. Used by
    `RunError::SubcommandSocketIoDeferred` to surface the
    operator's selected subcommand without coupling to the full
    `types::ProgramOptions` payload.
  - **RunError enum**: `SubcommandSocketIoDeferred { subcommand:
    status::Subcommand }` — replaces the prior raw `eyre!`
    string. Callers can now match on the specific subcommand for
    programmatic dispatch.
  `lib.rs::run` is rewired: no behavior change (still returns
  Err) but the error is now a structured `RunError` rather than
  a free-text `eyre!` string. The error message references
  `control_client_status` for the full deferral rationale.
  Tests: yggdrasil-kes-agent-control 43 → 47 (+4:
  control_client_status describes deferral with kes-agent-server-
  arc + ControlClient markers; subcommand cli_verbs match
  upstream-canonical 6 verbs; Display impl matches cli_verb;
  status is Clone+Eq+Hash-round-trip via HashSet insertion).
  Workspace: 5,934 → 5,938. Parity-matrix entry sister-
  tool.kes-agent-control advanced: next_milestone R371 → R441.
- **R439 — snapshot-converter: structured deferral surface
  (replaces the `Err(eyre::eyre!)` stub with a typed `RunError`
  enum + programmatic `*_status()` introspection helpers).** Lands
  in `crates/snapshot-converter/src/status.rs` (new) +
  `lib.rs` refactor:
  - **RunMode enum**: `Daemon | Oneshot` — extracted from the
    inline string match in `run()`.
  - **RunError enum**: `ConvertSnapshotDeferred { mode }` — replaces
    the prior raw `eyre::eyre!` string. Callers can now match on
    the specific deferral variant for programmatic dispatch (e.g.
    a future operator-CLI shim wanting to print a structured
    "feature unavailable" page rather than a free-text error).
  - **status::ConvertSnapshotStatus**: 4-field descriptor (status,
    depends_on, deferred_round, upstream_reference) used by both
    `convert_snapshot_status()` (the mem↔lsm conversion logic) +
    `daemon_watcher_status()` (the filesystem-watcher daemon
    around it). Mirror of cardano-tracer's R424
    `TlsTerminationStatus` / R424's `*_status()` precedent.
  - **lib.rs::run** rewired: no behavior change (still returns
    Err) but the error is now a structured `RunError` rather than
    a free-text `eyre!` string. The error message references the
    `convert_snapshot_status` helper for the full deferral
    rationale.
  Tests: yggdrasil-snapshot-converter 30 → 33 (+3:
  convert_snapshot_status describes deferral with LedgerStore +
  upstream-reference markers; daemon_watcher_status describes
  deferral with notify-crate marker; statuses are
  Clone+Eq+Hash-round-trip via HashSet insertion). Workspace:
  5,931 → 5,934. Parity-matrix entry sister-tool.snapshot-
  converter advanced: next_milestone R363+ → R440 (the deferral
  is now structurally surfaced; further forward motion needs the
  yggdrasil-format LedgerStore reader/writer arc to land).
- **R438 — cardano-tracer: parity-matrix entry refresh
  (documentation cleanup; closes the R411-R437 documentation
  loop).** The `sister-tool.cardano-tracer` entry's
  `implemented_evidence` and `remaining_work` arrays were stale —
  still listing R388+ Acceptors / R389+ Logs writers / R391+
  Run.hs as outstanding work despite all three being shipped at
  R424-R427. R438 refreshes both arrays:
  - **implemented_evidence**: gains 5 new entries summarizing
    Phase 1 (R411-R415 EKG-equivalent), Phase 2 (R416-R426
    trace-forwarder + Acceptors), Phase 3 (R427-R428 supervisor),
    Phase 4 (R429-R430 TLS + closeout), and the R431-R437 follow-
    on rounds (lo_handler factory + handshake codec primitives +
    state-machine driver + Acceptors integration + TraceObject
    CBOR codec).
  - **remaining_work**: rewritten to 10 entries reflecting actual
    open carve-outs (upstream-byte-equivalence for TraceObject
    Serialise; EKG ReqResp sub-protocol; DataPoint sub-protocol;
    RemoteSocket TCP path; Logs Rotator full impl; RTView UI;
    TLS termination via axum-server-rustls; Cardano.Logging.Resources
    loop; beforeProgramStops handler; live-rehearsal +
    verified_11_0_1 promotion). Each entry references the
    corresponding `*_status()` helper for programmatic
    introspection.
  Advances `next_milestone` from R438 → R439. Workspace test
  count unchanged at 5,931 (R438 is a pure documentation round;
  no test surface change).
  Verification: `python3 scripts/check-parity-matrix.py` clean —
  20 entries validated against `.reference-haskell-cardano-node/`
  (reference tag 11.0.1).
- **R437 — cardano-tracer: TraceObject CBOR codec (synthesis
  carve-out, closes the third advisor-flagged R430 gap).**
  Yggdrasil-canonical wire codec for the upstream
  `Cardano.Logging.TraceObject` Serialise instance. Source isn't
  vendored under `.reference-haskell-cardano-node/` (the
  cardano-logging package is a Hackage dep of cardano-node, not a
  vendored sibling); R437 ships a Yggdrasil-canonical 6-field
  CBOR-array shape that round-trips internally without claiming
  byte-equivalence to upstream. Documented as a synthesis
  carve-out with operator-facing caveat.
  Three surfaces updated:
  - **crates/cardano-tracer/src/logging.rs**: adds
    `TraceObject::to_cbor` / `TraceObject::from_cbor` methods.
    Wire format: 6-element CBOR array carrying `[to_human (text or
    null), to_machine (text), to_severity_code (uint 0-7 per RFC
    5424), to_namespace (text array), to_thread_id (text),
    to_timestamp_ms (signed int)]`. Pre-1970 timestamps round-trip
    via CBOR's negative-integer encoding.
  - **crates/cardano-tracer/src/severity.rs**: adds
    `SeverityS::from_syslog_code(u8) → Option&lt;SeverityS&gt;` as the
    reverse of `syslog_code()`. Used by `from_cbor` to round-trip
    the severity field.
  - **crates/cardano-tracer/src/acceptors/{server, client}.rs**:
    replaces the R424 stub `decode_trace_objects` (which returned
    `Vec::new()`) with real per-batch decoding. Each decoder reads
    the outer CBOR array's count, then decodes each entry using
    the same 6-field shape. Bounded at 65,536 entries per batch
    to fend off a malicious peer.
  Tests: yggdrasil-cardano-tracer 420 → 429 (+9: full event
  round-trip; null `to_human` round-trip; default event round-
  trip; each of 8 severity levels round-trips; pre-epoch negative
  timestamp round-trip; empty namespace round-trip; rejects wrong
  array length; rejects invalid severity code 99; rejects trailing
  bytes). Workspace: 5,922 → 5,931. Parity-matrix entry advanced:
  next_milestone R437 → R438.
- **R436 — cardano-tracer: handshake-driver wired into Acceptors/
  Server + Client (closes advisor gap #3 — full trace-forwarder
  handshake state-machine integration).** Final integration round
  for the R432-R435 handshake foundation. Two surfaces updated:
  - **acceptors/server.rs::do_listen_to_forwarder_local**: mux
    protocol list extended from `&[TRACE_OBJECTS_NUM]` to
    `&[MiniProtocolNum::HANDSHAKE, TRACE_OBJECTS_NUM]`. Each
    accepted UnixStream now spawns a per-connection mux with both
    protocols; the spawned task takes the HANDSHAKE handle, runs
    `run_handshake_responder` against the local version table
    [V1, V2] + the operator's network magic, and gates the trace-
    objects acceptor on success. On handshake refuse / error /
    timeout the connection drops without registering in
    `connected_nodes` (no cleanup needed since registration
    happens post-handshake).
  - **acceptors/client.rs::do_connect_to_forwarder_local**: same
    mux extension (`MiniProtocolNum::HANDSHAKE` added). After
    `UnixStream::connect`, the client takes the HANDSHAKE handle
    and runs `run_handshake_initiator` proposing
    [(V1, magic), (V2, magic)]. On handshake error the client
    surfaces it via `AcceptorsServerError::LocalListener` wrapping
    a `Bind { source: handshake error message }` — operators see
    a clear signal rather than a silent connection drop.
  R436 closes the third advisor-flagged gap from the R430 closure:
  the trace-forwarder handshake state-machine driver is now
  integrated into both responder and initiator paths. End-to-end
  data flow: an operator running yggdrasil cardano-tracer in
  AcceptAt mode + yggdrasil cardano-node-equivalent forwarder
  with matching network magic will now successfully negotiate the
  handshake, multiplex trace-objects on protocol 2, and ingest
  trace-object batches through R421's `accept_trace_objects_resp`
  → R427's `default_lo_handler_factory` → R401's
  `trace_objects_handler` dispatcher.
  Tests: yggdrasil-cardano-tracer 420 → 420 (the existing
  `do_connect_to_forwarder_local_round_trips_against_local_listener`
  test was updated to spawn a real handshake responder on the
  server side; it now tests the full mux-handshake-trace path
  rather than the prior naked-mux smoke). Workspace: 5,922 →
  5,922 (no test count change; R436 strengthens an existing
  integration test). All 5 gates clean.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R436 → R437. After R436, the only remaining
  R430 advisor-flagged gap is the TraceObject CBOR codec
  (`Cardano.Logging.TraceObject` Serialise instance — synthesis
  port from the cardano-logging Hackage source, reverse-engineering
  risk noted by advisor). All other ingest pipeline gaps (default
  lo_handler, handshake codec primitives, handshake state machine)
  are now closed.
- **R435 — network: trace_object_forward_handshake_driver.rs —
  state-machine driver for the trace-forwarder handshake exchange.**
  Builds on R432-R434's codec foundation to ship the runtime
  driver that runs the ProposeVersions / AcceptVersion / Refuse
  exchange on a mux'd HANDSHAKE channel. Mirror of upstream's
  `Server.with` (responder) + `connectToNode` (initiator)
  handshake-loop semantics for the trace-forwarder pipe. New file:
  `crates/network/src/trace_object_forward_handshake_driver.rs`.
  Public surface:
  - **HandshakeOutcome**: 2-field record (version, version_data)
    returned on a successful negotiation.
  - **HandshakeError**: 8-variant error enum (Mux,
    ConnectionClosed, Decode, Unexpected, Refused, NoCompatibleVersion,
    MagicMismatch, Timeout). The `MagicMismatch` variant is new —
    surfaces upstream's `ForwardingVersionData mismatch` failure
    case as a structured value carrying both magic numbers for
    operator diagnosis.
  - **HANDSHAKE_DEADLINE = 5 seconds**: pub const matching the
    operationally-canonical 5-second budget upstream uses for the
    NtN handshake (Yggdrasil applies the same to trace-forwarder
    for symmetry).
  - **run_handshake_responder(handle, local_versions, our_magic)
    → Result&lt;HandshakeOutcome, HandshakeError&gt;**: receives
    `ProposeVersions` from the remote, picks the highest version
    we support whose network-magic matches ours (sorted highest-
    to-lowest by tag), sends `AcceptVersion` (or `Refuse`).
    Wrapped in `tokio::time::timeout(HANDSHAKE_DEADLINE, ...)`.
  - **run_handshake_initiator(handle, proposals) → Result&lt;...&gt;**:
    sends `ProposeVersions` carrying the supplied (version, data)
    table, awaits the remote's response, returns the agreed
    `HandshakeOutcome` or surfaces the remote's refuse-reason.
  Carve-outs documented in module docstring:
  - **HandshakeArguments record**: 6-field upstream config
    collapses to plain function args + module-level constants
    (codec pinned by R433/R434, accept logic by R432, tracers
    deferred).
  - **timeLimitsHandshake / noTimeLimitsHandshake**: collapses
    to a single 5-second end-to-end deadline applied on both
    sides via `HANDSHAKE_DEADLINE`.
  - **HandshakeException / Refuse exception path**: collapses to
    `Result&lt;_, HandshakeError&gt;` — callers decide whether to drop
    the connection or retry.
  Updates `lib.rs` with `pub mod trace_object_forward_handshake_driver`
  declaration + `pub use ...` re-exports for `HANDSHAKE_DEADLINE`,
  `HandshakeError as TraceForwardHandshakeError`,
  `HandshakeOutcome`, `run_handshake_initiator`,
  `run_handshake_responder`.
  Tests: yggdrasil-network 795 → 802 (+7: deadline matches
  upstream 5s; responder accepts matching version + magic;
  responder picks highest overlapping version; responder refuses
  on no overlap (initiator sees VersionMismatch); responder
  refuses on magic mismatch (initiator sees Refused with the
  upstream-faithful "ForwardingVersionData mismatch" message);
  initiator errors on unexpected-message-shape response;
  responder errors on unexpected-first-message). Workspace:
  5,915 → 5,922.
  R435 ships the driver primitives but does NOT yet wire them
  into R424's `do_listen_to_forwarder_local` — that's the final
  integration step requiring HANDSHAKE_NUM to be added to the
  mux protocol list AND the handshake to gate the trace-objects
  acceptor spawn. That integration round lands separately to keep
  R435's test surface focused on the driver itself.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R435 → R436.
- **R434 — network: handshake codec generic-refactor (operator-
  approved direction).** Refactors the handshake message-envelope
  wire encoding to be generic over a `HandshakeWireCodec` trait,
  closing the duplication R433 acknowledged in its commit
  message. Both NtN and trace-forwarder handshake variants now
  share the same version-table encode/decode logic via
  trait-dispatched per-entry codecs. New file:
  `crates/network/src/handshake/wire.rs`.
  Public surface (small, focused trait):
  - **HandshakeWireCodec trait**: 2 associated types (Version,
    VersionData) + 4 encode/decode methods. Mirror of upstream's
    `codecHandshake` parameterization over a `CodecCBORTerm`.
    Each handshake variant (NtN, trace-forwarder) supplies an
    impl plugging in the variant-specific per-entry CBOR
    encoding.
  - **encode_version_table&lt;C&gt;(enc, versions)**: shared structural
    encoder for the `{version → versionData}` CBOR map. Replaces
    the previously-duplicated logic in NtN + trace-forwarder.
  - **decode_version_table&lt;C&gt;(dec, max) → DecodeVersionTableResult&lt;C&gt;**:
    shared structural decoder, bounded by `max`.
    `DecodeVersionTableResult` type alias dodges clippy's
    `type_complexity` lint on the underlying generic shape.
  Refactored sites:
  - **handshake/codec.rs**: introduces `NtNHandshakeCodec` impl.
    `encode_version_table` + `decode_version_table` private fns
    now delegate to `wire::encode_version_table::&lt;NtNHandshakeCodec&gt;`
    + `wire::decode_version_table::&lt;NtNHandshakeCodec&gt;`. The
    helper functions `encode_version_data` + `decode_version_data`
    keep the upstream-faithful 2/3/4-element backward-compat
    decode logic in their existing form (called from the trait
    impl).
  - **protocols/trace_object_forward_handshake.rs**: introduces
    `TraceForwardHandshakeCodec` impl. The previously-hand-rolled
    `encode_version_table` + `decode_version_table` private fns
    now delegate to the same generic helpers. The duplicate
    structural logic from R433 is gone.
  - **handshake.rs**: re-exports the new `HandshakeWireCodec`
    trait at the public API surface so both call sites can
    reference the trait by short path.
  Tests: yggdrasil-network unchanged at 795 (R434 is a behavior-
  preserving refactor). Workspace unchanged at 5,915. NtN
  handshake's 31 existing tests + trace-forward handshake's 14
  existing tests all pass — verifying the trait dispatch is
  byte-identical to the previous hand-rolled codecs.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R434 → R435. R434 closes the duplication debt
  R433 introduced; the trait now gives a clean foundation for a
  future round to wire the trace-forwarder handshake state-
  machine driver into R424's `do_listen_to_forwarder_local`
  (the third advisor-flagged gap, deferred per the operator's
  R434 direction-decision).
- **R433 — network: trace_object_forward_handshake.rs — wire-
  level codec for trace-forwarder ProposeVersions / AcceptVersion
  / Refuse / QueryReply messages.** Builds on R432's
  `ForwardingVersion` + `ForwardingVersionData` primitives to
  ship the full handshake message envelope. Lands in
  `crates/network/src/protocols/trace_object_forward_handshake.rs`.
  The wire format is byte-identical to upstream's
  `handshake-node-to-node-v14.cddl` (the same envelope is reused
  for trace-forwarder), with the version-data slot specialized
  to a single CBOR-encoded unsigned u32 (network-magic).
  Public surface:
  - **TraceForwardHandshakeMessage**: 4-variant message enum
    (ProposeVersions, AcceptVersion, Refuse, QueryReply) carrying
    `ForwardingVersion` + `ForwardingVersionData` payloads.
  - **TraceForwardRefuseReason**: 3-variant refuse-reason enum
    (VersionMismatch, HandshakeDecodeError, Refused) — wire shape
    matches upstream's `RefuseReason` ADT.
  - **to_cbor() / from_cbor()** message-codec methods. Handles
    the canonical wire format including unknown-version + out-of-
    bound network-magic decode errors, surfacing the upstream's
    exact error messages (`unknown tag: N`, `networkMagic out of
    bound: N`).
  - **simple_singleton_versions(version, data) →
    TraceForwardHandshakeMessage**: builder mirror of upstream's
    `Handshake.simpleSingletonVersions` — produces a
    ProposeVersions message with a single version-data entry.
  Carve-outs documented in module docstring:
  - **`Codec.CBOR.Term.Term` value-CBOR type**: collapses since
    Yggdrasil emits canonical bytes directly.
  - **`CodecCBORTerm` typeclass parameterization**: collapses to
    inline trace-forwarder-specific encoding (the existing
    `crates/network/src/handshake/codec.rs` is hardcoded for
    NodeToNodeVersionData; refactoring it generic across
    handshake variants is out of scope for R433 — Yggdrasil
    duplicates the message-envelope structure here for the
    bounded trace-forwarder use case).
  - **Handshake state-machine driver**: this round ships only the
    message codec, not the full state-machine driver. Wiring the
    codec into R424's `do_listen_to_forwarder_local` pre-mux
    handshake step is the third advisor-flagged gap and lands
    in a follow-on round.
  Updates `protocols/mod.rs` with module declaration + 3 re-
  exports for `TraceForwardHandshakeMessage`,
  `TraceForwardRefuseReason`, `simple_singleton_versions`.
  Tests: yggdrasil-network 781 → 795 (+14: ProposeVersions
  singleton round-trip; full version-table round-trip;
  AcceptVersion round-trip; all 3 Refuse variants round-trip;
  QueryReply round-trip; simple_singleton_versions builds
  ProposeVersions with single entry; unknown outer tag errors;
  unknown version tag in propose errors; out-of-bound magic
  errors; trailing bytes errors; ProposeVersions wire format
  byte-stable [0x82, 0x00, 0xA1, 0x01, 0x01]; AcceptVersion wire
  format byte-stable [0x83, 0x01, 0x01, 0x01]). Workspace:
  5,901 → 5,915. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R433 → R434. R433 closes the wire-
  encoding half of the third advisor-flagged gap; the remaining
  half (state-machine driver wiring into R424) lands in R434+.
- **R432 — network: trace_object_forward_version.rs port of
  Trace.Forward.Utils.Version.hs (post-R430 follow-on, closes 1
  of 3 advisor-flagged gaps).** Strict-mirror port of the
  trace-forwarder handshake version codec. Lands the 2-version
  namespace (V1, V2) + the `ForwardingVersionData` payload with
  network-magic + the `Acceptable` semantics. New file:
  `crates/network/src/protocols/trace_object_forward_version.rs`.
  Public surface:
  - **ForwardingVersion enum**: `V1` (wire tag 1) + `V2` (wire
    tag 2). `ALL` const for canonical iteration order. `tag()`
    accessor returning the upstream `TInt` wire tag.
  - **encode_forwarding_version(v) → Vec&lt;u8&gt;** /
    **decode_forwarding_version(bytes) → Result&lt;_, ...&gt;**:
    CBOR `TInt` encoder/decoder pair. Mirror of upstream's
    `forwardingVersionCodec`. Emits 1 byte (`0x01` / `0x02`) per
    CBOR canonical-form for unsigned integers.
  - **ForwardingVersionData**: 1-field record (`network_magic:
    u32`). Mirror of upstream's
    `newtype ForwardingVersionData { networkMagic :: NetworkMagic }`.
  - **ForwardingVersionData::accept(local, remote) →
    AcceptForwardingVersionData**: equality-comparison-based
    negotiator. Mirror of upstream's `Acceptable
    ForwardingVersionData` instance. Returns `Accept` on match,
    `Refuse` with a human-readable mismatch message on disagreement.
  - **ForwardingVersionData::is_queryable() → bool**: always
    false. Mirror of upstream's `Queryable
    ForwardingVersionData; queryVersion _ = False`.
  - **encode_forwarding_version_data(v, data) → Vec&lt;u8&gt;** /
    **decode_forwarding_version_data(v, bytes) → Result&lt;_, ...&gt;**:
    CBOR `TInt` encoder/decoder for the version-data payload.
    Mirror of upstream's `forwardingCodecCBORTerm`. Range-checks
    the network-magic to `[0, 0xffff_ffff]` per upstream's
    explicit bound check.
  - **ForwardingVersionDecodeError** + **ForwardingVersionDataDecodeError**:
    error enums surfacing upstream's specific failure messages
    (`unknown tag: N`, `unexpected term`, `networkMagic out of
    bound: N`, `unknown encoding`).
  Carve-outs documented in module docstring:
  - **`Codec.CBOR.Term.Term` value-CBOR type**: collapses since
    Yggdrasil's port emits canonical bytes directly.
  - **`CodecCBORTerm` typeclass**: collapses to free functions.
  - **NFData / Generic deriving**: collapses to standard Rust
    derives.
  Updates `protocols/mod.rs` with module declaration + 8 re-
  exports for the public surface.
  Tests: yggdrasil-network 767 → 781 (+14: tags match upstream;
  ALL in canonical order; V1 round-trips with byte-stable
  [0x01]; V2 round-trips with byte-stable [0x02]; unknown-tag
  errors with the offending value; non-int term errors;
  matching-magic accept; mismatched-magic refuse with human-
  readable message; is_queryable always false; mainnet-magic
  round-trip; zero round-trip; max-u32 round-trip; out-of-bound
  errors at 2^32; non-int term errors). Workspace: 5,887 →
  5,901. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R432 → R433. R432 closes the second
  of advisor's three named gaps; the remaining gap (full
  trace-forwarder handshake codec + integration with R424's
  Acceptors/Server.hs) lands in R433+.
- **R431 — cardano-tracer: default lo_handler factory wired into
  the binary entry (post-R430 follow-on, closes 1 of 3
  advisor-flagged gaps).** R427 shipped a no-op default
  trace-objects handler in `lib.rs::run` that discarded payloads;
  R431 replaces it with the canonical
  [`run::default_lo_handler_factory`] which dispatches each batch
  to [`handlers::logs::trace_objects::trace_objects_handler`]
  (R401), routing per the operator's `LoggingParams` configuration.
  Two new public entry points + factory:
  - **default_lo_handler_factory(config, connected_nodes_names) →
    impl Fn(NodeId, Vec&lt;TraceObject&gt;) + Send + Sync + 'static**:
    builds a sync closure that captures the operator's `logging:
    Vec&lt;LoggingParams&gt;` + the runtime's
    `ConnectedNodesNames` map. On each invocation: empty payloads
    return immediately; non-empty payloads spawn a tokio task
    that resolves NodeId → NodeName (falling back to `NodeId::as_str`
    if no name is registered, mirroring upstream's `getNodeName`
    fallback in `Notifications/Send.hs`) and dispatches via
    `trace_objects_handler`. The sync→async bridge via
    `tokio::spawn` is necessary because the trace-forwarder
    acceptor loop calls the lo_handler as a sync `Fn` (via R421's
    `accept_trace_objects_resp` signature).
  - **run_cardano_tracer_default(params) → Result&lt;(),
    RunCardanoTracerError&gt;**: convenience entry that reads the
    config, builds the canonical handler via the factory, then
    runs the supervisor. R431 wires this as the default entry
    point for the `cardano-tracer` binary; operators wanting
    custom handlers can call `run_cardano_tracer` directly with
    their own closure.
  - **do_run_cardano_tracer_with_state(state, config, state_dir,
    lo_handler)**: variant of `do_run_cardano_tracer` that accepts
    a pre-built `AcceptorsServerState` so callers like
    `run_cardano_tracer_default` can capture references to the
    same `ConnectedNodesNames` map that the supervisor will
    populate (the original `do_run_cardano_tracer` constructed
    state internally, blocking the factory pattern).
  Updates `lib.rs::run` to call `run_cardano_tracer_default(params)`
  instead of constructing a discarding closure inline. The
  binary's default behaviour now routes incoming
  `MsgTraceObjectsReply` payloads through the canonical
  trace-objects dispatcher — once the trace-forwarder handshake
  codec + TraceObject CBOR codec follow-on rounds land, the
  end-to-end ingest path will fire automatically without operator
  re-wiring.
  Tests: yggdrasil-cardano-tracer 417 → 420 (+3:
  default_lo_handler_factory dispatches non-empty payload to the
  trace_objects_handler via spawned task; falls back to NodeId
  string when the NodeName registry is empty;
  run_cardano_tracer_default errors on missing config file).
  Workspace: 5,884 → 5,887. Parity-matrix entry sister-tool.cardano-
  tracer advanced: next_milestone R430 → R432 (R431 closes one
  follow-on item; R432+ tackles the remaining handshake codec +
  TraceObject CBOR codec gaps in named follow-on rounds).
- **R430 — cardano-tracer: R411-R430 arc closeout (Phase 4 round 2,
  final round of the 20-round arc).** Structural completion of the
  cardano-tracer port:
  - `docs/operational-runs/2026-05-10-round-430-r411-r430-closure.md`
    captures the 20-round arc summary across 4 phases (Phase 1:
    R411-R415 EKG-equivalent; Phase 2: R416-R426 trace-forwarder
    mini-arc + Acceptors leaves; Phase 3: R427-R428 supervisor +
    closure doc; Phase 4: R429-R430 TLS plan + closeout). Documents
    the 15-row carve-out inventory with `*_status()` helper
    pointers, parity-matrix delta, operational rehearsal recipe,
    and follow-on arc plan (handshake codec, TraceObject CBOR codec,
    cardano-node forwarder side).
  - `crates/cardano-tracer/AGENTS.md` refreshed: status block
    advanced from "post-R335-pattern skeleton" to "post-R411-R430
    arc — trace-forwarder TraceObject sub-protocol fully wired";
    functional-surface bullet list replaced with the 11-row
    shipped/deferred status (5 ✅ + 6 ❌); round roadmap fast-
    forwarded across all 4 phases of the arc; follow-on items
    enumerated.
  - `docs/parity-matrix.json` `sister-tool.cardano-tracer`:
    `next_milestone` advanced R430 → R430 (closeout marker;
    follow-on arcs receive their own R-numbers when scheduled).
    Status remains `partial` — `verified_11_0_1` promotion defers
    until the trace-forwarder handshake codec + TraceObject CBOR
    codec + cardano-node forwarder side land in follow-on arcs.
  Workspace tests unchanged at 5,884 (no new test code; this is a
  documentation/closure round).
  R411-R430 cumulative delivery: 20 rounds, 4 phases, +201 tests,
  13 documented carve-outs, 0 failing gates throughout. The
  yggdrasil cardano-tracer binary is now operationally bootable
  via `cargo run --bin cardano-tracer -- -c <config>` without
  falling through to the previous "unimplemented" stub. Closes the
  cardano-tracer track of the R326-R459 sister-tools port arc.
- **R429 — cardano-tracer: TLS termination integration plan +
  status descriptors (Phase 4 round 1 of R411-R430 arc).** Lands
  the TLS bind-plan documentation + `force_ssl` operator-facing
  fallback status without bloating the workspace with a heavy
  TLS server dep. Two new public helpers in
  `crates/cardano-tracer/src/handlers/http_server.rs`:
  - **tls_bind_plan_status() → &'static str**: authoritative
    deferral-plan descriptor. Documents the integration recipe
    for operators wanting TLS today (use `load_pem_certs` /
    `load_pem_key` from R408 + wire `axum-server` directly with
    a `rustls::ServerConfig`). Includes the audit checklist for
    adding `axum-server` as a workspace dep (cargo-tree no
    `openssl-sys` / `native-tls` per `deny.toml:90`,
    `docs/DEPENDENCIES.md` justification).
  - **force_ssl_unsupported_status() → &'static str**: caller-
    facing message returned when `Endpoint::force_ssl ==
    Some(true)` but the TLS bind path isn't yet wired. Operators
    get a clear signal rather than a silent fall-through to
    plain TCP.
  Updates `prometheus.rs::tls_termination_status` to point at the
  new authoritative recipe (`http_server::tls_bind_plan_status`)
  and bump `deferred_round` from `"R411+"` to `"R429+"`.
  R429 deliberately does NOT add `axum-server` to the workspace
  — adding a TLS server dep across all crates for an
  operationally-optional tracer endpoint is over-scoped vs the
  R411 plan's pacing. Operators wanting TLS today have the
  helpers (R408) + the integration recipe (R429); the
  workspace-level integration ships when the audit + dep
  justification are completed in a focused follow-on round.
  Tests: yggdrasil-cardano-tracer 415 → 417 (+2:
  tls_bind_plan_status describes deferral with axum-server +
  rustls + DEPENDENCIES references; force_ssl_unsupported_status
  describes the plain-TCP fallback). Plus 1 existing test
  loosened to substring-match the new "deferred — R429 documents
  the integration recipe" status string. Workspace: 5,882 →
  5,884. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R429 → R430.
- **R428 — cardano-tracer: R411-R427 closure documentation
  (Phase 3 round 2 of R411-R430 arc).** Operational-runs doc
  capturing the 17-round Phase 1 + Phase 2 + Phase 3-round-1
  delivery: `docs/operational-runs/2026-05-10-round-428-r411-r427-closure.md`.
  Documents:
  - Per-round delivery table (Phase 1: R411-R415 EKG-equivalent;
    Phase 2: R416-R426 trace-forwarder mini-arc + Acceptors
    leaves; Phase 3 round 1: R427 supervisor entry).
  - Workspace test growth: 5,683 → 5,882 (+199).
  - Verification-gates checkpoint at HEAD commit 67d4621.
  - 13-row carve-out inventory mapping deferred upstream surfaces
    to the corresponding `*_status()` helper functions for
    programmatic introspection.
  - Remaining R428-R430 work breakdown (R429 TLS termination via
    axum-server; R430 parity-matrix promotion).
  - Operational rehearsal recipe: minimal AcceptAt tracer-config
    boot via `cargo run --bin cardano-tracer -- -c <config>`.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R428 → R429.
- **R427 — cardano-tracer: run.rs port of Cardano.Tracer.Run.hs +
  lib.rs::run upgrade (Phase 3 round 1 of R411-R430 arc).** Top-
  level supervisor port. Wires R424's server.rs + R425's client.rs
  + R426's acceptors/run.rs into a real `cardano-tracer` binary
  entry: argv → `parser::Args` → `TracerParams` → reads operator
  config → spawns the Acceptors supervisor on a multi-thread
  tokio runtime. Earlier rounds shipped a stub returning an
  "unimplemented" error; R427 replaces that with the real
  supervisor. New file:
  `crates/cardano-tracer/src/run.rs` (mirror of upstream's
  `Cardano.Tracer.Run.hs`).
  Public API:
  - **TracerParams**: 3-field record (tracer_config, state_dir,
    log_severity). Mirror of upstream's `data TracerParams`.
  - **run_cardano_tracer(params, lo_handler) →
    Result&lt;(), RunCardanoTracerError&gt;**: top-level entry. Mirror
    of upstream's `runCardanoTracer`. Reads + parses the operator
    config, then delegates to `do_run_cardano_tracer`.
  - **do_run_cardano_tracer(config, state_dir, lo_handler)**:
    initializes the runtime state slice + spawns the Acceptors
    supervisor. Mirror of upstream's `doRunCardanoTracer`.
  - **RunCardanoTracerError**: 3-variant error enum (ReadConfig,
    ParseConfig, Acceptors).
  - **run_logs_rotator_status() / run_metrics_servers_status() /
    run_resource_stats_status()**: programmatic carve-out
    descriptors.
  Also upgrades `lib.rs::run(args)`: replaces the
  "unimplemented" stub with the real `run_cardano_tracer` call
  wrapped in a multi-thread tokio runtime
  (`Builder::new_multi_thread().enable_all().build()`). The
  default trace-objects handler is a concrete closure that
  discards payloads (R428+ wires the canonical
  `trace_objects_handler` dispatcher); operators wanting real
  ingest can call `run_cardano_tracer` directly with their own
  closure.
  Carve-outs documented in module docstring:
  - **TraceBundle / meta-trace channel**: depends on
    `Cardano.Logging` package + meta-trace channel ports.
    Collapses to no-op log calls in R427.
  - **runLogsRotator**: Logs/Rotator.hs port deferred per the
    R411 plan's pacing.
  - **runRTView**: synthesis carve-out per the original R326-R459
    plan (no Rust analog for ThreePenny GUI).
  - **Resource stats loop**: depends on Cardano.Logging.Resources;
    deferred.
  - **beforeProgramStops** (SIGINT/SIGTERM handler): deferred per
    `crate::utils::before_program_stops_status`. Supervisor
    currently shuts down via the brake flag.
  - **DataPointRequestors initialization**: deferred per the
    DataPoint sub-protocol carve-out.
  - **CurrentLogLock / CurrentDPLock**: deferred — Yggdrasil's
    `Arc&lt;RwLock&lt;...&gt;&gt;` runtime-state shape doesn't need separate
    per-resource locks for the bounded subset R427 wires.
  Updates `crates/cardano-tracer/src/lib.rs` with `pub mod run`
  declaration + the `run(args)` upgrade.
  Tests: yggdrasil-cardano-tracer 408 → 415 (+7:
  run_logs_rotator_status describes deferral; run_metrics_servers_status
  describes partial wiring; run_resource_stats_status describes
  deferral; run_cardano_tracer errors on missing config file;
  run_cardano_tracer errors on unparseable config;
  do_run_cardano_tracer with empty ConnectTo errors with NoTargets;
  end-to-end round trip with minimal AcceptAt config + abort
  cleanly within 150ms). Workspace: 5,875 → 5,882. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone
  R427 → R428. Per the R411 plan, R428 closes Phase 3 with the
  end-to-end runnable doc + operator integration assertion.
- **R426 — cardano-tracer: acceptors/run.rs port of
  Cardano.Tracer.Acceptors.Run.hs (Phase 2 round 11 of R411-R430
  arc, completes the Acceptors leaves quartet).** Strict-mirror
  port of `runAcceptors` — the trace-forwarder acceptors supervisor.
  Decides between server-mode (`AcceptAt`) and client-mode
  (`ConnectTo`) based on `TracerConfig.network`, runs per-instance
  acceptor loops with auto-restart on transport interruption,
  matches upstream's exact retry intervals (1s initial pause; 10s
  server retry; 30s client retry). Lands in
  `crates/cardano-tracer/src/acceptors/run.rs`. Public API:
  - **run_acceptors(state, config, lo_handler) →
    Result&lt;(), AcceptorsSupervisorError&gt;**: top-level supervisor.
    Mirror of upstream's `runAcceptors`. Per the R398 plan's
    TracerEnv option (b), takes the state slice + the operator's
    `TracerConfig` directly rather than coupling to TracerEnv.
  - **acceptors_configs(lo_request_num) → AcceptorConfiguration**:
    builder for the trace-objects sub-protocol's
    `AcceptorConfiguration` (R420). Mirror of upstream's
    `acceptorsConfigs p` — only the TOF tuple element is built;
    EKG + DPF slots are deferred carve-outs (see R424's server.rs).
  - **run_in_loop(state, config, lo_handler, initial_pause,
    interval, mode) → Result&lt;(), AcceptorsSupervisorError&gt;**:
    auto-restart loop body. Mirror of upstream's
    `runInLoop action onException initialPause interval`. The loop
    races against the brake flag; on transient transport errors,
    sleeps `interval`, then re-enters the body.
  - **ServerOrClient enum**: dispatch token (Server / Client) for
    `run_in_loop`'s mode arg. Public so external callers can wrap
    the loop body if needed.
  - **AcceptorsSupervisorError**: 3-variant error enum (NoTargets,
    JoinError, Server).
  - **dedup_connect_targets()**: helper deduplicating `ConnectTo`
    target lists. Mirror of upstream's `NE.nub localSocks`.
  - **INITIAL_PAUSE = 1s / SERVER_RETRY_INTERVAL = 10s /
    CLIENT_RETRY_INTERVAL = 30s / DEFAULT_LO_REQUEST_NUM = 100**:
    pub consts locking down upstream's hardcoded values.
  Carve-outs documented in module docstring:
  - **mkVerbosity** tracer wiring (depends on contra-tracer's
    Tracer typeclass — operationally an stdout closure can be
    wired by the caller).
  - EKG (`EKGF.AcceptorConfiguration`) + DataPoint
    (`DPF.AcceptorConfiguration`) configs — sub-protocols deferred.
  - `secondsToNominalDiffTime` for `requestFrequency` — applies
    only to deferred EKG config.
  - `forwarderEndpoint = EKGF.LocalPipe p` — applies only to
    deferred EKG config; upstream comment notes it's "unused in the
    context of ouroboros-network mini-protocol application".
  Updates `crates/cardano-tracer/src/acceptors.rs` with `pub mod
  run`.
  Tests: yggdrasil-cardano-tracer 403 → 408 (+5: acceptors_configs
  uses supplied lo_request_num; acceptors_configs default brake
  state is running; dedup_connect_targets collapses duplicates;
  constants match upstream intervals; run_acceptors connect_to
  empty list errors with `NoTargets`). Workspace: 5,870 → 5,875.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R426 → R427. The Acceptors leaves quartet (Server
  + Client + Utils + Run) is now structurally complete. Per the
  R411 plan, R427 ports `Cardano.Tracer.Run` — the top-level
  supervisor that wires everything (Acceptors + Logs + Metrics +
  Notifications + RTView) into the `cardano-tracer` binary entry,
  followed by R428 closeout.
- **R425 — cardano-tracer: acceptors/client.rs port of
  Cardano.Tracer.Acceptors.Client.hs (Phase 2 round 10 of R411-R430
  arc).** Strict-mirror port of `runAcceptorsClient` — the trace-
  forwarder initiator-mode client. Mirror of R424's server.rs but
  for outbound connections: cardano-tracer dials a Unix socket
  cardano-node has bound (operator's `connectTo` mode) and runs
  the same per-connection sub-protocol drivers via R421's
  `accept_trace_objects_init`. New file:
  `crates/cardano-tracer/src/acceptors/client.rs`.
  Public API:
  - **run_acceptors_client(state, how_to_connect, tf_config,
    lo_handler) → Result&lt;(), AcceptorsServerError&gt;**: top-level
    entry. Mirror of upstream's `runAcceptorsClient`. Dispatches on
    `HowToConnect`: LocalPipe → `do_connect_to_forwarder_local`;
    RemoteSocket → returns deferral error.
  - **do_connect_to_forwarder_local(state, socket_path, tf_config,
    lo_handler)**: outbound Unix-pipe connect path. Mirror of
    upstream's `doConnectToForwarderLocal`. Calls
    `tokio::net::UnixStream::connect`, initializes per-connection
    mux as `MiniProtocolDir::Initiator`, registers the connection
    with R423's `prepare_metrics_stores`, runs the trace-objects
    sub-protocol via R421's `accept_trace_objects_init`, cleans
    up via R423's `remove_disconnected_node` on shutdown.
  Carve-outs documented in module docstring (all carve-outs from
  R424's server.rs apply equivalently — the initiator side mirrors
  the responder side's deferral structure 1:1):
  - EKG sub-protocol (`runEKGAcceptorInit` / `acceptMetricsInit`)
    — `ekg-forward` Hackage package not vendored.
  - DataPoint sub-protocol (`runDataPointsAcceptorInit` /
    `acceptDataPointsInit`) — vendored, port deferred to R426+.
  - RemoteSocket TCP path — requires trace-forwarder handshake
    codec port.
  - `appInitiator` vs `appResponder` collapse into
    `mux::start_unix` with `MiniProtocolDir::Initiator`.
  Uses synthesis connection token `ConnectTo-{socket_path}-magic{N}`
  for stable NodeId derivation (the client knows where it
  connected, unlike the server which can't easily extract a peer
  address from an accept'd Unix socket).
  Updates `crates/cardano-tracer/src/acceptors.rs` with `pub mod
  client`. Updates `crates/cardano-tracer/Cargo.toml` with
  `tempfile = "3"` dev-dep.
  Tests: yggdrasil-cardano-tracer 400 → 403 (+3:
  run_acceptors_client_remote_socket_returns_deferral_error;
  do_connect_to_forwarder_local_errors_on_missing_socket;
  do_connect_to_forwarder_local_round_trips_against_local_listener
  — spawns a LocalPeerListener server, engages brake immediately,
  asserts the client connects, registers the conn, then cleans up
  via remove_disconnected_node). Workspace: 5,867 → 5,870.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R425 → R426. Per the R411 plan, R426 ports
  `Cardano.Tracer.Acceptors.Run` — the supervisor that decides
  between server vs client based on the operator config and
  handles reconnect logic, completing the Acceptors leaves of
  Phase 2.
- **R424 — cardano-tracer: acceptors/server.rs port of
  Cardano.Tracer.Acceptors.Server.hs (Phase 2 round 9 of R411-R430
  arc).** Strict-mirror port of `runAcceptorsServer` — the trace-
  forwarder responder-mode entry point. Wires R421's
  `accept_trace_objects_resp` + R423's `prepare_metrics_stores`
  / `remove_disconnected_node` into a top-level mux dispatcher
  that accepts inbound forwarder connections over a Unix pipe and
  spawns per-connection trace-object acceptors. New file:
  `crates/cardano-tracer/src/acceptors/server.rs`.
  Public API:
  - **run_acceptors_server(state, how_to_connect, tf_config,
    lo_handler) → Result&lt;(), AcceptorsServerError&gt;**: top-level
    entry. Mirror of upstream's `runAcceptorsServer`. Dispatches
    on `HowToConnect`: LocalPipe → `do_listen_to_forwarder_local`;
    RemoteSocket → returns deferral error (TCP path requires the
    trace-forwarder handshake codec port — R425+).
  - **do_listen_to_forwarder_local(state, socket_path, tf_config,
    lo_handler) → Result&lt;(), AcceptorsServerError&gt;**: Unix-pipe
    accept loop. Mirror of upstream's `doListenToForwarderLocal`.
    Binds R416's LocalPeerListener at `socket_path`, races accept
    against the global brake from `tf_config.should_we_stop`,
    spawns per-connection mux + trace-objects acceptor for each
    accepted UnixStream. The error finalizer (mirror of upstream's
    `errorHandler connId = deregisterNodeId + removeDisconnectedNode +
    notifyAboutNodeDisconnected`) is inlined as a closure that
    invokes R423's `remove_disconnected_node`.
  - **AcceptorsServerState**: 4-field state slice
    (connected_nodes, connected_nodes_names, accepted_metrics,
    network_magic). Per the R398 plan's TracerEnv option (b)
    decision, takes the slice of state directly rather than
    coupling to the full `TracerEnv` record.
  - **AcceptorsServerError**: 3-variant error enum
    (LocalListener, Mux, MissingProtocolHandle).
  - **TRACE_OBJECTS_NUM = 2 / EKG_NUM = 1 / DATA_POINTS_NUM = 3**:
    pub consts locking down the upstream sub-protocol number
    assignment — exposed even for the deferred sub-protocols so
    the canonical number-space is in place when their ports land.
  - **do_listen_to_forwarder_socket_status() /
    run_ekg_acceptor_status() /
    run_data_points_acceptor_status()**: programmatic carve-out
    descriptors.
  Carve-outs documented in module docstring:
  - **EKG sub-protocol responder (`runEKGAcceptor` /
    `acceptMetricsResp`)**: `ekg-forward` Hackage package not
    vendored. Per advisor guidance, EKG ReqResp is a synthesis
    carve-out — wire format would need to be reverse-engineered.
    Operationally cardano-tracer can run without EKG ingest (the
    per-node Prometheus/EKG endpoints from R408-R414 read from
    MetricsStore which can be fed manually).
  - **DataPoint sub-protocol responder (`runDataPointsAcceptor` /
    `acceptDataPointsResp`)**: vendored at
    `.reference-haskell-cardano-node/trace-forward/src/Trace/Forward/Run/DataPoint/Acceptor.hs`,
    port deferred to R425+.
  - **`Net.RemoteSocket host port` TCP path**: requires the trace-
    forwarder handshake codec port (R425+). LocalPipe covers the
    operationally-canonical SPO setup.
  - **Trace-forwarder handshake codec**: deferred to R425+. The
    LocalPipe path uses upstream's `Handshake.noTimeLimitsHandshake`
    which collapses to no-op for the same-host trust boundary.
    Yggdrasil's R424 port skips handshake; downstream fully-
    conformant integration with a running cardano-node forwarder
    will require the handshake port.
  - **`OuroborosApplication` + `MiniProtocol` records**: collapse
    into `mux::start_unix(stream, role, &amp;[MiniProtocolNum],
    buffer_size)` direct dispatch. The `miniProtocolStart =
    Mux.StartEagerly` and `maximumIngressQueue = maxBound` hints
    fold into Yggdrasil's `ProtocolConfig::default_for` defaults.
  - **TraceObject CBOR codec**: stub decoder returns empty list
    until the trace-dispatcher upstream package is ported (the
    full `Cardano.Logging.TraceObject` Serialise instance lives
    in upstream's `cardano-logging` package).
  Updates `crates/cardano-tracer/src/acceptors.rs` with `pub mod
  server`. Updates `crates/cardano-tracer/Cargo.toml` with
  `yggdrasil-ledger` + `yggdrasil-network` workspace dependencies.
  Tests: yggdrasil-cardano-tracer 394 → 400 (+6: protocol numbers
  match upstream wire assignment 1/2/3; do_listen_to_forwarder_socket_status
  describes deferral; run_ekg_acceptor_status describes carve-out;
  run_data_points_acceptor_status points to vendor path;
  decode_trace_objects stub returns empty list; run_acceptors_server
  remote-socket-path returns deferral error). Workspace: 5,861 →
  5,867. Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R424 → R425. Per the R411 plan, R425 ports
  `Cardano.Tracer.Acceptors.Client` (the cardano-node-side
  initiator-mode client which mirrors the responder-mode wiring),
  followed by `Cardano.Tracer.Acceptors.Run` (the supervisor that
  handles reconnect logic), completing Phase 2 of the arc.
- **R423 — cardano-tracer: acceptors/utils.rs port of
  Cardano.Tracer.Acceptors.Utils.hs (Phase 2 round 8 of R411-R430
  arc).** Strict-mirror port of the connecting tissue between
  cardano-tracer's runtime state (TracerEnv) and the trace-forwarder
  acceptor protocol drivers. Wires the existing Yggdrasil primitives
  (`ConnectedNodes`, `ConnectedNodesNames`, `AcceptedMetrics`,
  `MetricsStore`, `MetricsLocalStore`) into the upstream-named call
  surface used by `Acceptors/Server.hs` (R424 pending) and
  `Acceptors/Client.hs` (R425 pending). Two new files:
  - **crates/cardano-tracer/src/acceptors.rs**: parent shell module
    with synthesis docstring documenting the
    `Cardano.Tracer.Acceptors.{Server, Client, Utils, Run}` namespace
    layout.
  - **crates/cardano-tracer/src/acceptors/utils.rs** (mirror of
    upstream's 140-line `Acceptors/Utils.hs`): four public functions
    + 2 status descriptors:
    - **add_connected_node(connected_nodes, remote_address) → bool**:
      mirror of upstream's `addConnectedNode`. Returns true on first
      insert, false on reconnect-race no-op.
    - **prepare_metrics_stores(connected_nodes, accepted_metrics,
      remote_address) → (MetricsStore, MetricsLocalStore)**: mirror
      of upstream's `prepareMetricsStores`. Adds the new NodeId to
      connected_nodes, looks up (or creates) the per-node
      MetricsStore via R412's `get_or_insert_store`, returns the
      pair. The synthetic `ekg.server_timestamp_ms` counter
      registration is folded into `MetricsStore::insert_resp` (R412),
      so this function doesn't need an explicit registration step.
    - **remove_disconnected_node(connected_nodes,
      connected_nodes_names, accepted_metrics, remote_address)**:
      mirror of upstream's `removeDisconnectedNode`. Removes the
      NodeId from all three relevant maps. Yggdrasil performs the
      removals sequentially (each map independently locked); safe
      because the disconnect signal is the unique terminator for
      the NodeId's lifecycle. `te_dp_requestors` removal is no-op
      pending the DataPointRequestors port.
    - **store(metrics_store, metrics_local, response_metrics)**:
      mirror of upstream's `store tracerEnv (NodeId nodeId)
      (ekgStore, localStore) resp@(ResponseMetrics ms)`. Threads the
      batch through R412's `insert_resp` + the local-store delta
      tracking via `diff_and_advance`. Time-series forwarding is
      no-op pending R411 D1's deferred Option C decision.
    - **prepare_data_point_requestor_status() / notify_about_node_disconnected_status()**:
      programmatic carve-out descriptors.
  Carve-outs documented in module docstring:
  - **`prepareDataPointRequestor`**: depends on
    `Trace.Forward.Utils.DataPoint.initDataPointRequestor` (port
    deferred to R425+).
  - **`notifyAboutNodeDisconnected`** (RTView-conditional): non-
    RTView path is `pure ()`, matching Yggdrasil's no-op default.
  - **`Cardano.Timeseries.Component`**: optional time-series sink
    (R411 D1 Option C deferred).
  - **TracerEnv-record-arg**: per the R398 plan's TracerEnv
    option (b) decision, helpers take the slice of state they need
    directly rather than coupling to the full TracerEnv record.
  Updates `crates/cardano-tracer/src/lib.rs` with `pub mod acceptors`
  declaration.
  Tests: yggdrasil-cardano-tracer 385 → 394 (+9: add_connected_node
  inserts new + reports duplicate; conn-id sanitization strips
  pipe/quote chars; prepare_metrics_stores creates store for new
  node; returns existing store for reconnect (verified via
  Arc-shared inner BTreeMap); remove_disconnected_node clears all
  3 maps; idempotent on missing NodeId; store inserts response
  metrics + auto-populates synthetic timestamp counter;
  prepare_data_point_requestor_status describes deferral;
  notify_about_node_disconnected_status describes RTView carve-out).
  Workspace: 5,852 → 5,861. Parity-matrix entry sister-tool.cardano-
  tracer advanced: next_milestone R423 → R424. Per the R411 plan,
  R424 ports `Cardano.Tracer.Acceptors.Server` — the `runAcceptorsServer`
  responder-mode entry that wires R421's `accept_trace_objects_resp`
  + R423's `prepare_metrics_stores` / `remove_disconnected_node` /
  `store` into a top-level mux dispatcher. EKG + DataPoint sub-
  protocols are deferred carve-outs (the trace-object sub-protocol
  is fully wired and will exercise the end-to-end pipe).
- **R422 — network: ForwardSink + trace_object_forward_utils.rs
  port of Trace.Forward.Utils.{ForwardSink, TraceObject}.hs
  (Phase 2 round 7 of R411-R430 arc).** Strict-mirror port of the
  trace-forwarder buffering primitives (ForwardSink) + reply-list
  extractor (getTraceObjectsFromReply) + sink-init helper
  (initForwardSink). Lands two new files in
  `crates/network/src/protocols/`:
  - **forward_sink.rs** (mirror of upstream's 11-line
    `Utils/ForwardSink.hs`): `ForwardSink&lt;TraceObj&gt;` 2-field struct
    + `ForwardSinkOverflowCallback&lt;TraceObj&gt;` type alias for
    closure type (factored out to dodge clippy's
    `type_complexity`). Backing storage is `Arc&lt;Mutex&lt;VecDeque&lt;...&gt;&gt;&gt;`
    — synthesis carve-out for upstream's `TBQueue lo`. `Clone`
    impl shares the queue across clones via Arc; `Debug` impl
    redacts the callback as `&lt;Fn&gt;`. `new(callback)` constructor +
    `queue_len()` helper.
  - **trace_object_forward_utils.rs** (mirror of upstream's 138-
    line `Utils/TraceObject.hs`): three new public functions —
    - **init_forward_sink(config, overflow_callback) →
      ForwardSink&lt;TraceObj&gt;**: mirror of upstream's
      `initForwardSink :: ForwarderConfiguration lo -&gt; ([lo] -&gt; IO ())
      -&gt; IO (ForwardSink lo)`. Honours operator's `queue_size` from
      [`ForwarderConfiguration`] by preallocating the VecDeque
      capacity (mirror of `fromIntegral queueSize`).
    - **get_trace_objects_from_reply(reply: BlockingReplyList) →
      Vec&lt;TraceObj&gt;**: pure pub-fn wrapper around
      [`BlockingReplyList::into_items`]. Mirror of upstream's
      `getTraceObjectsFromReply`. Provides the upstream-named call-
      site form for parity grep across cardano-tracer's
      Acceptors/Server.hs analog.
    - **write_to_sink_status() / read_from_sink_status()**: status
      descriptors surfacing the deferred forwarder-side helpers
      (`writeToSink` / `writeToSinkSTM` / `readFromSink` /
      `readFromSinkSTM`) programmatically. The full TBQueue blocking-
      transactional semantics translate to `Arc&lt;Mutex&lt;VecDeque&gt;&gt; +
      CondVar`, but the call surface is only consumed by the
      cardano-node forwarder side (out of R411-R430 cardano-tracer
      arc scope) — port land R424+.
  Carve-outs documented in module docstrings:
  - **`Control.Concurrent.STM.TBQueue.TBQueue lo`** (bounded
    transactional queue): replaced with `Arc&lt;Mutex&lt;VecDeque&gt;&gt;` for
    R422's read-mostly use; full bounded-queue + STM blocking-write
    arrives with `writeToSink` / `readFromSink` ports later.
  - **`writeToSink` / `writeToSinkSTM` / `readFromSink` /
    `readFromSinkSTM`**: deferred per scope rationale (cardano-node
    forwarder side, not in R411-R430 scope).
  - **`Cardano.Logging.Utils.tryEvalNF`**: collapses since
    Yggdrasil's `TraceObject` is `Clone + Eq` and rendering errors
    don't surface as Haskell exceptions.
  Updates `protocols/mod.rs` with two new module declarations +
  re-exports for `ForwardSink`, `ForwardSinkOverflowCallback`,
  `get_trace_objects_from_reply`, `init_forward_sink`,
  `read_from_sink_status`, `write_to_sink_status`.
  Tests: yggdrasil-network 756 → 767 (+11: forward_sink starts
  empty; clone shares queue via Arc; overflow_callback invokable
  multiple times; Debug redacts callback; init_forward_sink creates
  empty queue; preallocates queue capacity per config; gets trace
  objects from blocking variant; gets trace objects from non-
  blocking variant; handles empty non-blocking; write_to_sink_status
  describes deferral; read_from_sink_status describes deferral).
  Workspace: 5,841 → 5,852. Parity-matrix entry sister-tool.cardano-
  tracer advanced: next_milestone R422 → R423. Per the R411 plan,
  R423 starts the EKG ReqResp sub-protocol port (number 1 of the
  3 trace-forwarder sub-protocols per upstream's
  `Cardano.Tracer.Acceptors.Server`).
- **R421 — network: trace_object_run_acceptor.rs port of
  Trace.Forward.Run.TraceObject.Acceptor.hs (Phase 2 round 6 of
  R411-R430 arc).** Strict-mirror port of the trace-forwarder
  TraceObject acceptor *runtime aggregator* — wires R420's
  `AcceptorConfiguration` + R418's codec + R419's
  `TraceObjectAcceptor` driver + caller-supplied trace-object
  handler into a single async function spawnable by the trace-
  forwarder mini-protocol layer. Implements upstream's
  `acceptorActions` recursive request-loop + `timeoutWhenStopped`
  graceful-shutdown semantics. Lands in
  `crates/network/src/trace_object_run_acceptor.rs`. Three new
  public entry points + supporting types:
  - **accept_trace_objects_resp(config, handle, decode_reply_list,
    lo_handler, peer_error_handler) → Result&lt;(),
    AcceptTraceObjectsError&gt;**: responder-mode runtime entry. Mirror
    of upstream's `acceptTraceObjectsResp`. Drives the recursive
    request → handle → check-brake loop until the brake fires; then
    sends `MsgDone` within the [`SHUTDOWN_TIMEOUT`] grace budget.
    Invokes `peer_error_handler` exactly once on transport-level
    failure (mirror of upstream's `finally peerErrorHandler ctx`
    finalizer).
  - **accept_trace_objects_init(...)**: initiator-mode entry. Mirror
    of upstream's `acceptTraceObjectsInit`. Operationally identical
    to `*_resp` — Yggdrasil's mux layer doesn't carry the
    initiator/responder role distinction in the function signature
    (upstream uses `RunMiniProtocol`'s GADT branches for that
    purpose). Both paths route through the same internal
    `run_acceptor_loop`.
  - **timeout_when_stopped(stop_flag, timeout, action) →
    Result&lt;T, AcceptTimeout&gt;**: standalone race-against-brake-flag
    helper. Mirror of upstream's `timeoutWhenStopped stopVar delay
    action`. Exposed publicly for callers that want to wrap their
    own loops in the same shutdown semantics.
  - **SHUTDOWN_TIMEOUT (15 seconds)**: pub const matching upstream's
    hardcoded `15_000` ms in `timeoutWhenStopped`.
  - **AcceptTimeout**: type-level marker (synthesis of upstream's
    `data Timeout = Timeout` exception type) surfaced through
    `AcceptTraceObjectsError::Timeout` + `timeout_when_stopped`'s
    error variant.
  - **AcceptTraceObjectsError**: 2-variant error enum
    (`Acceptor(TraceObjectAcceptorError)` + `Timeout { timeout }`).
  Carve-outs documented in module docstring:
  - **`InitiatorProtocolOnly` / `ResponderProtocolOnly`
    `RunMiniProtocol` shapes**: collapse since Yggdrasil's mux layer
    doesn't carry role distinction in the function signature.
  - **`Network.Mux.MiniProtocolCb`**: collapses to a plain
    `async fn`; callers spawn via `tokio::task::spawn` after
    acquiring the protocol handle from the mux.
  - **`Ouroboros.Network.Driver.Simple.runPeer` typed-protocol
    driver loop**: collapses since R419's `TraceObjectAcceptor`
    already exposes the per-state driver methods directly.
  Updates `lib.rs` with `pub mod trace_object_run_acceptor`
  declaration + `pub use ...` re-exports for `AcceptTimeout`,
  `AcceptTraceObjectsError`, `SHUTDOWN_TIMEOUT`,
  `accept_trace_objects_init`, `accept_trace_objects_resp`,
  `timeout_when_stopped`.
  Tests: yggdrasil-network 750 → 756 (+6: round-trip one batch
  through `accept_trace_objects_resp` then engage brake, verify
  forwarder receives MsgDone after exactly 1 batch round-trip;
  `timeout_when_stopped` returns action value when brake clear;
  completes action after brake within budget; errors when action
  overruns budget; `SHUTDOWN_TIMEOUT` matches upstream 15s; type-
  level `AcceptTimeout` marker round-trips via Debug). Workspace:
  5,835 → 5,841. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R421 → R422. Per the R411 plan, R422
  ports `Trace.Forward.Utils.TraceObject` — the
  `getTraceObjectsFromReply` helper + the forwarder-side write
  path utilities. After R422, the trace-forwarder TraceObject sub-
  protocol port is structurally complete; R423-R424 wire the EKG
  ReqResp + DataPoint sub-protocols, and R425+ wires everything
  through the cardano-tracer Acceptors/{Server, Client, Utils, Run}
  leaves.
- **R420 — network: trace_object_forward_configuration.rs port of
  Trace.Forward.Configuration.TraceObject.hs (Phase 2 round 5 of
  R411-R430 arc).** Strict-mirror port of the trace-forwarder
  configuration records used by the Run-side aggregators (R421+).
  Lands in `crates/network/src/protocols/
  trace_object_forward_configuration.rs` alongside R417's
  `trace_object_forward.rs` (type + codec). Single new module:
  - **AcceptorConfiguration**: 3-field record (acceptor_tracer,
    what_to_request, should_we_stop) mirroring upstream's
    `data AcceptorConfiguration lo`. The Yggdrasil port is payload-
    agnostic — the consuming `TraceObjectAcceptor&lt;TraceObj&gt;`
    driver carries the type parameter, so the configuration record
    can stay generic-free for ergonomic reuse. `new(n)` constructor
    creates a default-state config (no tracer, fresh stop-flag in
    the running state). `request_stop()` async helper engages the
    brake. `is_stopped()` async helper reads the brake.
  - **ForwarderConfiguration**: 2-field record (forwarder_tracer,
    queue_size) mirroring upstream's `data ForwarderConfiguration
    lo`. Used by the forwarder side (cardano-node feeding trace-
    objects to cardano-tracer); not directly exercised by R411-R430
    arc but ported in the same round to keep the two configuration
    records colocated as upstream does.
  - **TraceForwardTracer**: type alias for
    `Option&lt;Arc&lt;dyn Fn(&str) + Send + Sync&gt;&gt;` — the synthesis
    carve-out for upstream's `Tracer IO (TraceSendRecv ...)` debug-
    trace channel. Factored out as a type alias to avoid clippy's
    `type_complexity` lint on the underlying function-pointer-in-
    Option-in-Arc shape.
  Carve-outs documented in module docstring:
  - **`Tracer IO (TraceSendRecv (TraceObjectForward lo))` debug
    channel**: collapses to `TraceForwardTracer` — `contra-tracer`'s
    `Tracer` typeclass has no Rust analog without a workspace-wide
    trace-dispatcher port. Operational use cases (logging codec
    send/recv events) can be served by a closure.
  - **`TVar Bool` stop-flag**: replaced with
    `Arc&lt;tokio::sync::RwLock&lt;bool&gt;&gt;` mirroring R371's
    `ProtocolsBrake` pattern. The atomic-read semantics carry across
    cleanly; both forms are read-mostly.
  Updates `protocols/mod.rs` with module declaration + re-exports
  for `AcceptorConfiguration`, `ForwarderConfiguration`, and the
  `TraceForwardTracer` type alias.
  Tests: yggdrasil-network 743 → 750 (+7: AcceptorConfiguration
  default state — what_to_request matches, stop flag false, tracer
  None; request_stop engages brake (verified via is_stopped); clone
  shares brake state via Arc; Debug impl redacts brake value with
  `&lt;TVar Bool&gt;` placeholder; Debug shows `acceptor_tracer: true`
  when set; ForwarderConfiguration default state — queue_size
  matches, tracer None; ForwarderConfiguration Debug includes
  queue_size + redacted tracer flag). Workspace: 5,828 → 5,835.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R420 → R421. Per the R411 plan, R421 ports
  `Trace.Forward.Run.TraceObject.Acceptor` — the
  `acceptTraceObjectsResp` `RunMiniProtocol` aggregator that wires
  codec + acceptor driver + tracer-env handlers + the `acceptorActions`
  loop + `timeoutWhenStopped` semantics into something a mux can
  spawn.
- **R419 — network: trace_object_acceptor.rs port of
  Trace.Forward.Protocol.TraceObject.Acceptor.hs (Phase 2 round 4
  of R411-R430 arc).** Strict-mirror port of the trace-forwarder
  TraceObject acceptor-side typed-protocol driver. Lands as
  `TraceObjectAcceptor&lt;TraceObj&gt;` in `crates/network/src/
  trace_object_acceptor.rs` alongside `chainsync_client.rs`,
  `keepalive_client.rs`, and the other mini-protocol drivers.
  Single new module + one lib.rs re-export:
  - **TraceObjectAcceptor&lt;TraceObj&gt;**: driver struct holding
    `MessageChannel`, current `TraceObjectForwardState`, and a
    `PhantomData&lt;TraceObj&gt;` for the trace-object payload type.
    `new(handle)` constructor binds the protocol to a `ProtocolHandle`
    and starts in `StIdle`.
  - **request_blocking(n_trace_objects, decode_reply_list) →
    Result&lt;Vec&lt;TraceObj&gt;, TraceObjectAcceptorError&gt;**: sends
    `MsgTraceObjectsRequest { blocking: StBlocking, n }` and
    awaits the matching `MsgTraceObjectsReply`. Mirror of upstream's
    `SendMsgTraceObjectsRequest TokBlocking n cont` data
    constructor + the `Yield (MsgTraceObjectsRequest TokBlocking
    request) (Await ... )` peer interpretation in
    `traceObjectAcceptorPeer`.
  - **request_non_blocking(n_trace_objects, decode_reply_list) →
    Result&lt;Vec&lt;TraceObj&gt;, TraceObjectAcceptorError&gt;**: same as
    blocking variant but with `StNonBlocking`. The forwarder is
    bound to reply promptly; the reply may be empty.
  - **done(self) → Result&lt;(), TraceObjectAcceptorError&gt;**: sends
    `MsgDone` and consumes the driver. Mirror of upstream's
    `SendMsgDone (m a)` data constructor.
  - **TraceObjectAcceptorError**: 6-variant error enum (Mux,
    ConnectionClosed, Protocol, Decode, UnexpectedMessage,
    InvalidState). The InvalidState variant guards each public
    method against being called outside `StIdle` — defensive guard
    duplicating the state-machine check.
  Carve-outs documented in module docstring:
  - **Continuation-passing-style API**: upstream's
    `(BlockingReplyList blocking lo -&gt; m (TraceObjectAcceptor lo m
    a))` continuation parameter encodes a "next acceptor program"
    as an inversion-of-control callback. Rust's `async fn` makes
    this inversion unnecessary — callers just `.await`
    `request_blocking` and inspect the returned reply directly.
  - **`Network.TypedProtocol.Peer.Client` machinery**: upstream's
    `Yield`/`Await`/`Effect`/`Done` peer-construction primitives
    collapse into direct mux send/recv calls.
  Updates `lib.rs` with `pub mod trace_object_acceptor` declaration
  + `pub use TraceObjectAcceptor, TraceObjectAcceptorError`
  re-exports.
  Tests: yggdrasil-network 739 → 743 (+4: acceptor starts in
  StIdle; request_blocking round-trip with multi-element reply;
  request_non_blocking with empty reply; done terminates cleanly).
  Mux pair plumbed via `UnixStream::pair` + `start_unix` matching
  the precedent in `chainsync_client.rs`'s tests; mux handles are
  bound to `_a_mux` / `_f_mux` so they live to scope-end (drop
  aborts the mux tasks). Workspace: 5,824 → 5,828. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone
  R419 → R420. Per the R411 plan, R420 ports
  `Trace.Forward.Run.TraceObject.Acceptor` — the `RunMiniProtocol`
  aggregator that wires the codec + acceptor driver + tracer-env
  handlers into something a mux can spawn.
- **R418 — network: trace_object_forward.rs CBOR codec port of
  Trace.Forward.Protocol.TraceObject.Codec.hs (Phase 2 round 3 of
  R411-R430 arc).** Strict-mirror port of the trace-forwarder
  TraceObject CBOR encoder/decoder pair, completing the protocol-
  level wire contract before R419 wires the responder driver.
  Lands as `to_cbor` + `from_cbor_in_state` methods on
  `TraceObjectForwardMessage<TraceObj>` in the same file as R417's
  type port (matching the precedent set by `keep_alive.rs` —
  type + codec in one file). Two new methods + one new dependency
  pin:
  - **TraceObjectForwardMessage::to_cbor(&amp;self, encode_reply_list:
    F) → Vec&lt;u8&gt;**: emits the upstream wire format byte-for-byte —
    `[1, blocking_bool, n_trace_objects]` for `MsgTraceObjectsRequest`,
    `[2]` for `MsgDone`, `[3, [trace_object,…]]` for
    `MsgTraceObjectsReply`. The reply-list payload is encoded by the
    caller-supplied closure (mirror of upstream's
    `[lo] -&gt; CBOR.Encoding` parameter). `NumberOfTraceObjects` is
    hardcoded to Word16 unsigned encoding (every operational upstream
    call site uses `encodeWord16`).
  - **TraceObjectForwardMessage::from_cbor_in_state(state, data,
    decode_reply_list: F) → Result&lt;Self, LedgerError&gt;**: decodes
    according to the protocol state. The state arg is required
    because `MsgTraceObjectsReply`'s wire format does NOT carry the
    blocking flag — it is inferred from the originating
    `MsgTraceObjectsRequest`'s blocking style stored in
    `StBusy(blocking)`. Mirror of upstream's
    `stateToken :: StateToken st` decoder argument. The empty-list-
    in-blocking-reply case returns `LedgerError::CborDecodeError`
    with upstream's exact failure message
    (`codecTraceObjectForward: MsgTraceObjectsReply: empty list not
    permitted`) for byte-equivalent diagnostics. State-mismatched
    decodes (`MsgTraceObjectsRequest` in `StBusy`,
    `MsgTraceObjectsReply` in `StIdle`, any message in `StDone`) all
    surface as `CborTypeMismatch` — same information as upstream's
    `notActiveState` + per-state failure branches.
  Carve-outs documented inline:
  - **`MonadST` constraint**: collapses since
    `yggdrasil_ledger::cbor::{Encoder, Decoder}` are concrete (no
    monad-transformer). Matches the precedent set by
    `keep_alive::to_cbor` and the rest of Yggdrasil's mini-protocol
    codecs.
  - **`SomeMessage st` existential**: collapses since
    `from_cbor_in_state` returns
    `TraceObjectForwardMessage&lt;TraceObj&gt;` directly + relies on
    `TraceObjectForwardState::transition` (R417) for state-validation.
  Tests: yggdrasil-network 728 → 739 (+11: blocking-request round
  trip; non-blocking-request round trip; MsgDone round trip;
  blocking-reply round trip; non-blocking-reply with empty list ok;
  blocking-reply with empty list on the wire rejected with upstream's
  exact diagnostic; request-bytes-in-StBusy rejected; reply-bytes-
  in-StIdle rejected; decode-in-StDone always errors; request wire
  format byte-stable [0x83, 0x01, 0xF5, 0x01]; MsgDone wire format
  byte-stable [0x81, 0x02]). Workspace: 5,813 → 5,824. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone
  R418 → R419. Per the R411 plan, R419 ports
  `Trace.Forward.Protocol.TraceObject.Acceptor` — the responder side
  of the protocol's typed-protocol driver loop, wiring the codec
  into a peer-thread that consumes incoming
  `MsgTraceObjectsRequest`s and produces
  `MsgTraceObjectsReply`s on demand.
- **R417 — network: protocols/trace_object_forward.rs port of
  Trace.Forward.Protocol.TraceObject.Type.hs (Phase 2 round 2 of
  R411-R430 arc).** Strict-mirror port of the trace-forwarder
  TraceObject sub-protocol's typed state machine + message envelope.
  Lands in `crates/network/src/protocols/trace_object_forward.rs`
  alongside the existing KeepAlive / ChainSync / BlockFetch /
  TxSubmission / PeerSharing / LocalStateQuery / LocalTxMonitor /
  LocalTxSubmission state machines. Single new module:
  - **NumberOfTraceObjects(u16)**: `pub struct` newtype mirror of
    upstream's `newtype NumberOfTraceObjects { nTraceObjects :: Word16 }`
    with `new`/`n_trace_objects` accessors matching the upstream
    record-syntax shape.
  - **StBlockingStyle**: 2-variant enum `StBlocking | StNonBlocking`
    that collapses upstream's `data StBlockingStyle` + `data
    TokBlockingStyle (k :: StBlockingStyle)` (Rust enums *are*
    runtime tokens; the GADT/Singletons separation has no Rust
    analog).
  - **BlockingReplyList&lt;TraceObj&gt;**: 2-variant enum
    `Blocking(Vec) | NonBlocking(Vec)` with constructor-level
    `NonEmpty` invariant validation via `BlockingReplyList::blocking()`
    returning `Result&lt;Self, BlockingReplyListEmptyError&gt;` (mirror
    of upstream's type-level `NonEmpty lo` constraint). Helper
    methods: `style()`, `items()`, `into_items()`.
  - **TraceObjectForwardState**: 3-variant state enum
    `StIdle | StBusy(StBlockingStyle) | StDone` mirroring upstream's
    `data TraceObjectForward lo where StIdle :: ... | StBusy ::
    StBlockingStyle -> ... | StDone :: ...`.
  - **Agency**: 3-variant enum `Acceptor | Forwarder | Nobody`
    paired with `TraceObjectForwardState::agency()` reflecting
    upstream's `StateAgency` type-family clauses (StIdle →
    ClientAgency = Acceptor; StBusy _ → ServerAgency = Forwarder;
    StDone → NobodyAgency).
  - **TraceObjectForwardMessage&lt;TraceObj&gt;**: 3-variant message
    enum `MsgTraceObjectsRequest { blocking, n_trace_objects } |
    MsgTraceObjectsReply { reply } | MsgDone`. `tag()` accessor
    returning the upstream constructor name as `&'static str`.
  - **TraceObjectForwardTransitionError**: 2-variant error enum
    `IllegalTransition { from, msg_tag } | BlockingStyleMismatch
    { expected, actual }`. The mismatch variant catches
    `MsgTraceObjectsReply` arrived in `StBusy(b)` whose
    `BlockingReplyList` style disagrees with the originating request
    (a wire-level invariant upstream enforces at the type level via
    GADTs).
  - **TraceObjectForwardState::transition()**: exhaustive-match
    validator returning the next state OR a transition error.
    Matches the precedent set by `keep_alive.rs::transition`.
  Carve-outs documented in module docstring: GADT + DataKinds +
  Singletons type-level encoding (collapses to value-level enum +
  exhaustive transition validator); `Protocol` typeclass +
  `StateAgency` type family (collapses to runtime `agency()`
  method); `ShowProxy` instances (collapse to standard `Debug`
  derivation). The CBOR codec lands in R418
  (`Trace.Forward.Protocol.TraceObject.Codec` mirror), the
  responder driver in R419
  (`Trace.Forward.Protocol.TraceObject.Acceptor` mirror), and the
  `RunMiniProtocol` aggregator in R420
  (`Trace.Forward.Run.TraceObject.Acceptor` mirror).
  Updates `protocols/mod.rs` to declare the new module + re-export
  the public surface (`BlockingReplyList`, `BlockingReplyListEmptyError`,
  `NumberOfTraceObjects`, `StBlockingStyle`, `TraceObjectForwardAgency`,
  `TraceObjectForwardMessage`, `TraceObjectForwardState`,
  `TraceObjectForwardTransitionError`).
  Tests: yggdrasil-network 712 → 728 (+16: NumberOfTraceObjects
  round trip; BlockingReplyList::blocking rejects empty + accepts
  one or more; non_blocking accepts empty; style matches variant;
  into_items unifies variants; agency matches all 4 upstream
  StateAgency clauses; message tag strings match upstream
  constructor names; idle+request→busy for both blocking styles;
  idle+done→done; busy+matching-style-reply→idle; busy+mismatched-
  style→BlockingStyleMismatch; idle+reply illegal; busy+request
  illegal; done state terminal for all messages). Workspace:
  5,797 → 5,813. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R417 → R418. Per the R411 plan, R418
  ports `Trace.Forward.Protocol.TraceObject.Codec` — the CBOR
  encoder/decoder pairing for the message types, completing the
  protocol-level wire contract before R419 wires the responder
  driver.
- **R416 — network: local_listener.rs Unix-pipe LocalPeerListener
  (Phase 2 round 1 of R411-R430 arc, foundation for trace-forwarder
  Acceptors mini-arc).** Phase 2 of R411-R430 arc opens with a
  synthesis carve-out in `crates/network/`: the `LocalPeerListener`
  Unix-pipe analog of TCP `PeerListener`, mirror of the operational
  shape of `Ouroboros.Network.Snocket.localSnocket` +
  `Ouroboros.Network.Server.Simple.with` (used by upstream
  `Cardano.Tracer.Acceptors.Server::doListenToForwarderLocal`,
  Server.hs:114-143). Single new module:
  - **crates/network/src/local_listener.rs**: `LocalPeerListener`
    struct wrapping `tokio::net::UnixListener` + bound path. `bind`
    constructor performs stale-socket cleanup (mirrors
    `node/src/local_server/accept.rs`), binds via `UnixListener::bind`,
    and applies `chmod 0o660` (`SOCKET_PERMISSIONS = 0o660` const) so
    a non-root user on a multi-tenant host cannot speak trace-forward
    against a tracer running as a privileged user. `from_listener`
    constructor for tests + adoption from already-bound listeners.
    `local_path()` getter. `accept_unix()` returns a single
    `UnixStream` per call without performing the trace-forwarder
    handshake (the handshake codec lands in R420+). `Drop` impl
    removes the socket file on listener teardown so subsequent binds
    on the same path succeed cleanly. `LocalPeerListenerError` enum
    with three variants: `Bind { path, source }`,
    `SetPermissions { path, source }`, `Accept(io::Error)`.
  - **crates/network/Cargo.toml**: adds `tempfile = "3"` dev-dep
    matching the existing pin in `consensus`/`storage`/`db-truncater`/
    `node` crates.
  - **crates/network/src/lib.rs**: adds `#[cfg(unix)] pub mod
    local_listener` declaration + `#[cfg(unix)] pub use ...` re-export
    of `LocalPeerListener` and `LocalPeerListenerError`. Module is
    Unix-gated since `tokio::net::UnixListener` is Unix-only and
    cardano-tracer is operationally Unix-only.
  Carve-outs documented in module docstring: `Snocket` typeclass
  abstraction (collapses for Yggdrasil since property tests use
  `tokio::net::UnixStream::pair()` directly); `HandshakeArguments`
  threading (the trace-forwarder handshake codec lands in R420+);
  `Server.with` blocks-until-async-exception loop semantics
  (`accept_unix` returns one connection per call — caller owns the
  loop, matching `listener.rs::accept_tcp` for symmetric API design
  between the TCP and Unix-pipe listeners). Tests: yggdrasil-network
  704 → 712 (+8: bind creates socket file + bind removes stale socket
  + bind sets permissions to 0o660 + accept_unix returns connected
  stream + drop removes socket file + from_listener round trip +
  socket_permissions constant locks down 0o660 + bind error carries
  path for operator diagnosis). Workspace: 5,789 → 5,797.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R416 → R417. Per the R411 plan, R417 wires the
  trace-forward responder (Acceptor side of the trace-forwarder
  mini-protocol's TraceObject sub-protocol) on top of this listener,
  mirroring upstream's `Trace.Forward.Run.TraceObject.Acceptor::acceptTraceObjectsResp`.
- **R415 — cardano-tracer: utils.rs::load_metrics_help port
  (closes Phase 1 of R411-R430 arc).** Phase 1 round 5 (final) of
  R411-R430 arc. Single deliverable:
  - **load_metrics_help(Option<&FileOrMap>) → Vec<(String, String)>**:
    mirror of upstream `Cardano.Tracer.Run::loadMetricsHelp`
    (Run.hs:181-191). Three branches matching upstream verbatim:
    `None` returns empty; `Some(File(path))` reads + JSON-decodes
    via `serde_json::from_slice` with all IO/parse errors swallowed
    (mirroring upstream's `try $ decodeFileStrict'` shape) and falls
    back to empty on failure; `Some(Map(map))` clones the inline
    BTreeMap directly. Filters out entries with empty values per
    upstream's `M.filter (not . T.null)`. Returns BTreeMap-sorted
    (alphabetical key order) per upstream's `M.toList`. The result
    feeds the `metrics_help: Vec<(String, String)>` arg already
    threaded through `run_prometheus_server` (R413) and
    `MetricsStore::render_prometheus` per-metric `# HELP` lines.
  Tests: cardano-tracer 379 → 385 (+6: load_metrics_help returns
  empty for None + reads valid JSON file + falls back to empty for
  missing file + falls back to empty for malformed JSON + filters
  out empty values from File variant + uses inline Map directly +
  filters out empty values from Map variant + returns alphabetical
  key order). Workspace: 5,783 → 5,789. Parity-matrix entry
  sister-tool.cardano-tracer advanced: next_milestone R415 → R416.
  Phase 1 of the R411-R430 arc — EKG-equivalent MetricsStore +
  per-node Prometheus + per-node EKG monitoring + metrics_help
  parser — is now structurally complete; the metrics surface is
  ready for the R416-R424 Acceptors mini-arc to feed it real
  trace-forwarder ingest. R416 begins Phase 2: port
  `crates/network/src/local_listener.rs` — `LocalPeerListener` Unix-pipe
  analog of TCP `PeerListener`, mirror of
  `Ouroboros.Network.Snocket.localSnocket` + `Server.with` (synthesis
  carve-out documented; foundation for trace-forwarder Acceptors).
- **R414 — cardano-tracer: MetricsStore::render_ekg_html +
  monitoring.rs handle_per_node wiring (closes per-node EKG
  monitoring carve-out).** Phase 1 round 4 of R411-R430 arc. Three
  primary deliverables:
  - **MetricsStore::render_ekg_html(node_name)**: emits an EKG-style
    HTML monitoring page with a meta-refresh header (5-second
    auto-refresh), styled table of `metric name | kind | value`,
    and node name in the title. Mirror of upstream's
    `EKG.Wai`-rendered `/<slug>/?` request handler.
  - **html_escape helper**: pure-function HTML-escape for `<`, `>`,
    `&`, `"`, `'` characters. Used to escape node names + label
    values in the EKG page (since maud's compile-time templating
    doesn't compose neatly with the unbounded-N table-row loop).
  - **monitoring.rs::handle_per_node wired through**: AppState
    gains `accepted_metrics: AcceptedMetrics`. Per-node route
    resolves slug → NodeName via R407's compute_routes, then
    NodeName → NodeId via the connected_nodes_names snapshot, then
    looks up the per-node MetricsStore and renders. Run-monitoring-
    server signature gains the new `accepted_metrics` arg.
  Tests: cardano-tracer 373 → 379 (+6: html_escape replaces special
  chars + passes through safe chars; render_ekg_html emits valid
  HTML with meta-refresh + escapes node name in title + escapes
  label values + renders empty store without table rows;
  run_monitoring_server binds with new arg). Workspace:
  5,777 → 5,783. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R414 → R415. Per the R411 plan, R415
  lands the metrics_help.json parser + Run.hs::loadMetricsHelp
  mirror, completing Phase 1 of the R411-R430 arc.
- **R413 — cardano-tracer: MetricsStore::render_prometheus + wire
  into prometheus.rs handle_per_node (closes ExpositionStatus
  carve-out from R408).** Phase 1 round 3 of R411-R430 arc. Three
  primary deliverables:
  - **MetricsStore::render_prometheus(no_suffix, &[(name, help)])**:
    emits Prometheus text exposition (3-line block per metric:
    `# HELP <name> <text>` (when help slice has entry) +
    `# TYPE <name> <kind>` + `<name> <value>`). Mirror of upstream
    `Cardano.Logging.Prometheus.Exposition.renderExpositionFromSampleWith`.
    Uses `MetricValue::prometheus_kind()` + `prometheus_value()`
    helpers from R411.
  - **strip_prom_suffix + sanitize_prom_metric_name helpers**:
    private helpers handling `metricsNoSuffix` flag (strips `_int`
    / `_real` suffix) + sanitizing forbidden characters
    (notably `.`) per Prometheus identifier rules
    `[a-zA-Z_:][a-zA-Z0-9_:]*`.
  - **prometheus.rs::handle_per_node wired through**: AppState
    gains `accepted_metrics: AcceptedMetrics` + `metrics_help:
    Vec<(String, String)>` fields. Per-node route now resolves
    slug → NodeName via R407's compute_routes, then
    NodeName → NodeId via the connected_nodes_names snapshot, then
    looks up the per-node MetricsStore and renders. Run-prometheus-server
    signature gains the two new args.
  - **ExpositionStatus** struct upgraded from a deferral descriptor
    to a closure marker: `status: "closed at R413"`.
  Tests: cardano-tracer 362 → 373 (+11: render_prometheus emits
  3-line block per metric + emits HELP when help slice supplies
  text + strips _int suffix when no_suffix=true + sanitizes dotted
  synthetic timestamp + empty store returns empty string;
  strip_prom_suffix drops _int/_real + passes through unsuffixed;
  sanitize_prom_metric_name replaces dots with underscores +
  preserves alphanumeric_underscore + replaces leading digit with
  underscore; ExpositionStatus describes closure;
  run_prometheus_server binds with new args). Workspace:
  5,766 → 5,777. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R413 → R414. Per the R411 plan, R414
  lands MetricsStore::render_ekg_html + monitoring.rs handle_per_node
  wiring (closes per-node EKG monitoring carve-out).
- **R412 — cardano-tracer: MetricsStore::insert_resp + MetricsLocalStore
  delta tracking (Phase 1 round 2 of R411-R430 arc).** Lands the
  Response::ResponseMetrics ingestion path that
  `Acceptors/Utils.hs::store` (R422 pending) calls per incoming
  EKG ReqResp protocol response. Three primary deliverables:
  - **MetricsStore::insert_resp(Vec<(String, MetricValue)>)**:
    accepts an upstream `Response::ResponseMetrics` batch and
    replaces all matching entries. Mirror of upstream
    `System.Metrics.Store.Acceptor::storeMetrics`. Always
    populates the synthetic `ekg.server_timestamp_ms` counter
    using `crate::time::get_time_ms()` (mirror of upstream's
    `Acceptors/Utils.hs:70` `getTimeMs >>= EKG.set` invocation —
    the EKG Wai frontend expects this counter in every store).
  - **MetricsStore::delta_since(&previous_snapshot)**: returns a
    delta map of entries that have been added or modified since
    the supplied snapshot. The synthetic `ekg.server_timestamp_ms`
    counter is excluded from the diff (it always changes;
    surfacing it would mask other-metric churn).
  - **MetricsLocalStore struct + diff_and_advance + reset
    methods**: per-node delta-tracking state for the
    `GetUpdatedMetrics` mode of the EKG ReqResp protocol. Holds
    the most recent snapshot returned to the upstream forwarder.
    First-call returns full contents (minus synthetic timestamp);
    subsequent calls return only diffs. Reset on node disconnect
    + reconnect.
  - **EKG_SERVER_TIMESTAMP_MS const**: the canonical synthetic-
    counter name (`"ekg.server_timestamp_ms"`) populated by every
    `insert_resp` call.
  Tests: cardano-tracer 352 → 362 (+10: insert_resp writes batch
  + synthetic timestamp; replaces prior values; empty batch still
  updates timestamp; delta_since excludes synthetic timestamp +
  includes changed value + includes new metric;
  metrics_local_store first-call returns full contents +
  subsequent-call returns only changes + reset clears snapshot;
  EKG_SERVER_TIMESTAMP_MS constant matches upstream).
  Workspace: 5,756 → 5,766. Parity-matrix entry
  sister-tool.cardano-tracer advanced: next_milestone R412 → R413.
  Per the R411 plan, R413 lands MetricsStore::render_prometheus
  (closes ExpositionStatus carve-out from R408).
- **R411 — cardano-tracer: EKG-equivalent MetricsStore (R411-R430
  arc plan + Phase 1 round 1; D3 from R411 plan; closes EKG-store-shape
  carve-out).** Lands the per-node metrics aggregator that the
  R422+ Acceptors mini-arc + the per-node Prometheus + Monitoring
  exposition bodies depend on. Operator-approved planning at the
  R411 entry: see `docs/operational-runs/2026-05-10-round-411-arc-plan-r411-r430.md`
  for the full 20-round R411-R430 plan with per-decision tradeoff
  matrices + risk register. Three primary deliverables:
  - **`crates/cardano-tracer/src/metrics_store.rs`**: new
    synthesis-stand-in for upstream's `System.Metrics.Store` (from
    the unvendored Hackage `ekg-core`). MetricValue enum
    (Counter/Gauge/Label) mirroring upstream's
    `System.Metrics.ReqResp.MetricValue` typed-message surface.
    MetricsStore = `Arc<RwLock<BTreeMap<String, MetricValue>>>`
    schema-flexible aggregator (cardano-tracer is *passive* —
    stores whatever names the forwarder delivers; no name-locked
    schema needed). 8 inherent methods: register_counter,
    register_gauge, register_label, set_counter, set_gauge,
    get, snapshot, len/is_empty.
  - **AcceptedMetrics type**: alias
    `Arc<RwLock<BTreeMap<NodeId, MetricsStore>>>` mirroring upstream's
    `TVar (Map NodeId (TVar EKG.Store))`. Replaces R393's unit-struct
    placeholder. Companion helpers: `new_accepted_metrics`,
    `get_or_insert_store` (mirror of `Acceptors/Utils.hs::prepareMetricsStores`),
    `remove_store` (mirror of `removeDisconnectedNode`).
  - **MetricValue rendering helpers**: prometheus_kind() returns
    "counter"/"gauge" for the Prometheus exposition `# TYPE` line;
    prometheus_value() returns the i64 value (Labels render as 0
    since Prometheus has no native string-metric type).
  Carve-outs documented:
  - `System.Metrics.Distribution` distribution-histogram metric
    type deferred — wait for ekg-core vendor; meanwhile any
    incoming Distribution variants surface as synthetic Label
    entries.
  - `sampleAll` Sample-frozen-snapshot semantics replaced with
    direct cloned-map snapshot — same semantics, simpler shape.
  Tests: cardano-tracer 336 → 352 (+16: store default empty;
  register_counter inserts then replaces; register_gauge round-trip;
  register_label round-trip; set_counter updates existing + returns
  false when not-a-counter / when missing; set_gauge updates;
  snapshot clones full map; prometheus_kind matches variant;
  prometheus_value returns i64 for each kind; new_accepted_metrics
  starts empty; get_or_insert_store creates then reuses + separates
  per-node; remove_store returns then drops node + returns None for
  unknown). Workspace: 5,740 → 5,756. Parity-matrix entry
  sister-tool.cardano-tracer advanced: next_milestone R411 → R412.

  R411-R430 arc plan (full details in operational-runs):
  - Phase 1 R411-R415: EKG MetricsStore (D3 inline; critical path)
  - Phase 2 R416-R424: Acceptors mini-arc (9 rounds — Unix-pipe
    responder + trace-forward 4 sub-protocols + EKG ReqResp + 4
    Acceptors leaves)
  - Phase 3 R425-R428: Run.hs supervisor + closeout (cardano-tracer
    end-to-end runnable at R428)
  - Phase 4 R429-R430: TLS termination via axum-server + closeout

  Decisions: D1 Timeseries → defer (Option C; routing-only shell at
  R426); D2 TLS → axum-server 0.7 at R429; D3 EKG-equivalent →
  inline MetricsStore (this round). Only R429 bumps
  workspace.dependencies in the entire 20-round arc.
- **R410 — cardano-tracer: Metrics/Monitoring.hs port (EKG-style
  monitoring HTTP server).** Lands the EKG-style monitoring server
  using R408's axum 0.8 stack + R407's compute_routes + R406's
  render_html / render_json, mirroring the same pattern as R409's
  Prometheus port. New `handlers/metrics/monitoring.rs` module
  ports the upstream Cardano.Tracer.Handlers.Metrics.Monitoring
  surface:
  - run_monitoring_server(ConnectedNodesNames, Endpoint) async →
    std::io::Result<JoinHandle<()>>. Mirror of upstream's
    runMonitoringServer signature; takes the slice of state per
    R398 plan option (b) rather than full TracerEnv.
  - 200ms `tokio::time::sleep` stagger before listener bind
    (mirrors upstream's `sleep 0.2` — 0.1s offset from R409
    Prometheus's 0.1s to prevent listening-banner collisions on
    stdout).
  - Two routes attached to an axum::Router:
    - `GET /` — content-negotiated index (HTML or JSON based on
      `Accept` header). Uses R406's render_html / render_json.
    - `GET /{slug}` — per-node EKG monitoring page (HTML
      placeholder pending the EKG-equivalent metrics surface).
  - wants_json(&HeaderMap) helper for content negotiation
    (duplicated from R409's prometheus.rs since each server
    handles its own content negotiation independently per upstream's
    per-server design).
  Carve-outs documented:
  - System.Metrics.Store EKG store deferred — same blocker as
    R409 Prometheus's per-node exposition.
  - TLS termination via tlsCertificate.epForceSSL deferred —
    references R409's tls_termination_status carve-out.
  Tests: cardano-tracer 333 → 336 (+3: wants_json content-negotiation
  for application/json + text/html; run_monitoring_server binds
  on ephemeral port without panicking). Workspace: 5,737 → 5,740.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R410 → R411.

  R398 plan completion status: R398 (planning) + R399-R402 (Logs
  pipeline) + R403-R405 (Notifications pipeline closure) + R406-R407
  (HTML + compute_routes) + R408-R410 (HTTP server stack +
  Prometheus + Monitoring) all shipped. Remaining R398-arc items:
  TimeseriesServer.hs port + Servers.hs orchestration (R411+); full
  TLS termination + EKG-equivalent metrics surface (R412+ tightening
  rounds gated on follow-up workspace-dep approvals or hand-rolled
  alternatives).
- **R409 — cardano-tracer: Metrics/Prometheus.hs port (HTTP server
  with content negotiation + per-node routes).** Lands the
  Prometheus exporter HTTP server using R408's axum 0.8 stack +
  R407's compute_routes + R406's render_html / render_json. New
  `handlers/metrics/prometheus.rs` module ports the upstream
  Cardano.Tracer.Handlers.Metrics.Prometheus surface:
  - run_prometheus_server(ConnectedNodesNames, Endpoint,
    BTreeMap<String, String>, bool) async →
    std::io::Result<JoinHandle<()>>. Mirror of upstream's
    runPrometheusServer signature; takes the slice of state per
    R398 plan option (b) rather than full TracerEnv.
  - 100ms `tokio::time::sleep` stagger before listener bind to
    avoid concurrent listening-banner collisions (mirrors
    upstream's `sleep 0.1`).
  - Three routes attached to an axum::Router:
    - `GET /` — content-negotiated index (HTML or JSON based on
      `Accept` header). Uses R406's render_html / render_json.
    - `GET /targets` — Prometheus HTTP-SD service-discovery JSON.
    - `GET /{slug}` — per-node OpenMetrics exposition (placeholder
      pending EKG-equivalent metrics surface).
  - PrometheusServiceDiscovery newtype with serde-derived JSON.
  - wants_json(&HeaderMap) helper for content negotiation.
  - ExpositionStatus + TlsTerminationStatus deferral descriptors
    + helpers exposing the two pending pieces programmatically.
  Carve-outs documented:
  - Per-node OpenMetrics exposition body deferred — depends on
    EKG-equivalent metrics surface (Cardano.Tracer.Types.AcceptedMetrics
    + Cardano.Logging.Prometheus.Exposition.renderExpositionFromSampleWith;
    both unported).
  - TLS termination via tlsCertificate.epForceSSL deferred —
    needs axum-server-rustls (or hyper-rustls) integration with
    R408's load_pem_certs / load_pem_key.
  Tests: cardano-tracer 325 → 333 (+8: PrometheusServiceDiscovery
  serializes with targets + labels; wants_json content-negotiation
  for application/json + text/html + missing Accept + combined
  Accept; exposition_status describes deferral; tls_termination_status
  describes deferral; run_prometheus_server binds on ephemeral
  port without panicking). Workspace: 5,729 → 5,737. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone R409 →
  R410. Per the R398 plan, this leaves R410 (Metrics/Monitoring +
  TimeseriesServer + Servers orchestration) as the final round in
  the cardano-tracer R398-R410 sub-arc.
- **R408 — cardano-tracer: axum + tower + rustls-pemfile workspace
  deps + HTTP-server skeleton (D2 main land per R398 plan).** Lands
  the HTTP server stack that R398 carved out for the Metrics
  handler suite (Prometheus / Monitoring / TimeseriesServer /
  Servers). Three workspace surfaces touched:
  - **Cargo.toml workspace.dependencies**: adds `axum = "0.8"`
    (default-features off, features `["http1", "tokio", "json"]`),
    `tower = "0.5"` (default-features off, features `["util"]`),
    and `rustls-pemfile = "2"`. License: all MIT. Note the version
    bump to axum 0.8 from the R398-recommended 0.7 (latest stable
    release; same feature pin). hyper 1 is a transitive dep of
    axum 0.8.
  - **Audit verification**:
    `cargo tree -p yggdrasil-cardano-tracer | grep -iE "openssl|native-tls"`
    returned zero hits — transitive tree clean of all three banned
    crates per `deny.toml:88-91`.
  - **handlers/http_server.rs**: new synthesis-shell module with
    common HTTP-server scaffolding for the upcoming Metrics handler
    suite. Ships:
    - `build_router()` — empty `axum::Router` builder.
    - `serve_router(SocketAddr, Router)` — binds + spawns a tokio
      task serving until aborted.
    - `load_pem_certs(&Path)` / `load_pem_key(&Path)` — rustls-pemfile
      helpers for upcoming per-server TLS termination per upstream's
      `tlsSettingsChain` semantics.
    - `PemLoadError` enum.
  Carve-outs documented:
  - Strict mirror: none — upstream has no single HTTP-server module;
    each Metrics handler ships its own warp setup. Yggdrasil's port
    consolidates the scaffolding here so each per-server impl
    (R409+) focuses on its routing/handler logic.
  Tests: cardano-tracer 318 → 325 (+7: build_router returns empty
  Router type-shape; serve_router binds and aborts cleanly;
  load_pem_certs returns IO error for missing file; returns empty
  Vec for empty PEM file; parses valid PEM block; load_pem_key
  returns error for missing file + when no key found in file).
  Workspace: 5,722 → 5,729. Parity-matrix entry
  sister-tool.cardano-tracer advanced: next_milestone R408 → R409.
  Per the R398 plan, this leaves R409 (Metrics/Prometheus.hs port)
  + R410 (Metrics/Monitoring.hs + TimeseriesServer + Servers
  orchestration) unblocked next.
- **R407 — cardano-tracer: compute_routes direct-arg pass-through
  (closes R391 ComputeRoutesStatus carve-out per R398 plan option
  b).** Lands the per-node URL routing-table builder. New
  entry-point in `handlers/metrics/utils.rs`:
  - compute_routes(&ConnectedNodesNames, &AcceptedMetrics) async
    → RouteDictionary. Mirror of upstream
    `computeRoutes :: TracerEnv -> IO RouteDictionary`.
  - Per the R398 plan's TracerEnv option (b), takes the
    connected-nodes-names slice directly rather than the full
    14-field TracerEnv record. The `_accepted_metrics` parameter
    is reserved for the upcoming EKG-equivalent metrics surface
    — until that ships, the function returns routes for *all*
    connected nodes (upstream's `Map.intersectionWith` filter is
    a no-op when AcceptedMetrics is a placeholder).
  - Uses R391's slugify for the per-node URL slug; preserves
    snapshot iteration order from R371's ConnectedNodesNames.
  - ComputeRoutesStatus struct upgraded from a deferral
    descriptor to a closure marker: `status: "closed at R407"`.
  Carve-outs documented:
  - TracerEnv coupling deferred per R398 option (b); function
    takes Arc-shared state directly.
  - Per-node metrics-presence filter deferred until EKG-equivalent
    metrics surface ships.
  Tests: cardano-tracer 316 → 318 (+2: empty-when-no-nodes-connected;
  one-entry-per-connected-node with slugified URLs). Workspace:
  5,720 → 5,722. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R407 → R408.
- **R406 — cardano-tracer: maud HTML renderer (D2-prime from R398
  plan; closes R391 RenderHtmlStatus carve-out).** Lands the
  `renderListOfConnectedNodes` HTML index page. Three workspace
  surfaces touched:
  - **Cargo.toml workspace.dependencies**: adds `maud = "0.27"`.
    License: MIT. Verified zero transitive deps via
    `cargo tree` — only adds `maud_macros` (proc-macro).
  - **RouteDictionary::render_html(&str) → Vec<u8>** in
    `handlers/metrics/utils.rs`: emits an HTML page with `<title>`
    (operator-supplied) + `<ul>` of `<li><a href="/<slug>">name</a></li>`
    per connected node. Empty-dictionary short-circuit returns the
    canonical upstream
    `"There are no connected nodes yet."` text verbatim. Auto-
    escapes user-supplied node names via maud's compile-time
    template syntax (no XSS risk from operator-supplied node-name
    strings).
  - **RenderHtmlStatus** struct upgraded from a deferral descriptor
    to a closure marker: `status: "closed at R406"`.
  Note: the broader R398 D2 (axum + hyper + tower + rustls-pemfile)
  HTTP server suite is split off to a separate round (R407) since
  it requires more careful TLS/feature integration. R406 ships
  D2-prime (maud) standalone since the maud audit is genuinely
  zero-deps and closes the RenderHtmlStatus carve-out cleanly.
  Tests: cardano-tracer 312 → 316 (+4: render_html with empty
  dictionary returns no-nodes message; with one node emits
  canonical HTML page with title + per-node link; with multiple
  nodes emits each link with slugified hrefs; auto-escapes
  user-supplied node names [`<script>` becomes `&lt;script&gt;`];
  status describes closure). Workspace: 5,716 → 5,720. Parity-
  matrix entry sister-tool.cardano-tracer advanced: next_milestone
  R406 → R407.
- **R405 — cardano-tracer: initEventsQueues orchestration (closes
  R385 InitEventsQueuesStatus carve-out — Notifications subsystem
  fully complete).** Lands the entry-point that bootstraps the
  notification engine on tracer start. Uses every prior round's
  surface: R384's settings persistence + R386's Timer + R403's
  lettre + R404's makeAndSendNotification. New entry-point in
  `crates/cardano-tracer/src/handlers/notifications/utils.rs`:
  - init_events_queues(Option<&Path>, NodeIdResolver)
    async → (EventsQueues, EventsSenders). Mirror of upstream
    `initEventsQueues`.
  - Reads EmailSettings from disk (R384's read_saved_email_settings).
  - Returns empty queues + senders when email config is incomplete
    (mirror of upstream's `if incompleteEmailSettings emailSettings
    then pure []`).
  - Reads EventsSettings from disk (R384's
    read_saved_events_settings).
  - Creates 6 per-group queues (Warnings / Errors / Criticals /
    Alerts / Emergencies / NodeDisconnected), each backed by a
    Timer (R386) whose periodic action calls
    make_and_send_notification (R404) on the corresponding queue.
  - Timer action wraps the async make_and_send_notification call
    in a `tokio::spawn` since Timer's action signature is
    `Fn() + Send + Sync + 'static` (sync). Each spawn closes over
    cloned Arc-shared state (resolver / settings / last_time /
    queues / group).
  - InitEventsQueuesStatus struct upgraded from a deferral
    descriptor to a closure marker: `status: "closed at R405"`.
  Carve-outs documented:
  - askNodeNameRaw chain (upstream's `nodesNames` + `dpReqs` +
    `curDPLock` triple) replaced with a `Fn(&NodeId) -> NodeName +
    Clone + Send + Sync + 'static` closure injection per R398's
    plan option (b).
  - Upstream's `newTBQueueIO 2000` (bounded queue with capacity
    2000) replaced with `tokio::sync::mpsc::unbounded_channel`
    per the existing R380 EventsQueue carve-out — bounded-queue
    swap available if a future round needs back-pressure semantics.
  Tests: cardano-tracer 311 → 312 (+1: init_events_queues returns
  empty when email is incomplete; status describes closure).
  Workspace: 5,715 → 5,716. Parity-matrix entry
  sister-tool.cardano-tracer advanced: next_milestone R405 → R406.
  **Notifications subsystem now structurally + functionally
  complete**: all 7 leaves (Types, Check, Settings, Utils, Timer,
  Email, Send) ported with all carve-outs closed.
- **R404 — cardano-tracer: makeAndSendNotification orchestration
  (closes R389 MakeAndSendNotificationStatus carve-out, unblocked
  by R403's lettre).** Lands the orchestration that drains
  per-event-group queues, filters by last-seen timestamp, resolves
  node names, and sends notifications through R403's lettre-backed
  SMTP path. New entry-point in
  `crates/cardano-tracer/src/handlers/notifications/send.rs`:
  - make_and_send_notification(EmailSettings, EventsQueues,
    EventGroup, &Arc<Mutex<i64>>, NodeIdResolver) async →
    StatusMessage. Mirror of upstream `makeAndSendNotification`.
  - Drains the per-group queue via R385's get_new_events.
  - Filters to events strictly newer than `*last_time_ms.lock()`.
  - Builds a unique (NodeId, NodeName) pair list via the
    caller-supplied resolver closure (mirror of upstream's
    askNodeNameRaw chain — kept as a closure per R398 plan's
    option (b) TracerEnv decision).
  - Formats the body via R389's format_notification_body.
  - Updates last_time_ms to max(event.time_ms) before send.
  - Calls R403's create_and_send_email and returns the
    StatusMessage.
  Carve-outs documented:
  - askNodeNameRaw data-point requestor chain replaced with
    closure injection (R398 option (b)).
  - Two short-circuits return STATUS_SUCCESS without an SMTP send:
    empty queue + all-events-older-than-last-seen.
  - MakeAndSendNotificationStatus struct upgraded from a deferral
    descriptor to a closure marker: `status: "closed at R404"`.
  Tests: cardano-tracer 309 → 311 (+2: short-circuits on empty
  queue [unchanged last_time]; skips events older than last-seen
  [unchanged last_time]; status describes closure).
  Workspace: 5,713 → 5,715. Parity-matrix entry
  sister-tool.cardano-tracer advanced: next_milestone R404 → R405.
  Per the R398 plan, this leaves R405 (initEventsQueues, the
  Notifications/Utils.hs entry deferred at R385) unblocked next.
- **R403 — cardano-tracer: lettre SMTP wired (D1 from R398 plan;
  closes R388 SmtpSendStatus carve-out).** Lands the SMTP send
  path that R388 carved out pending lettre workspace dependency
  approval. Three workspace surfaces touched:
  - **Cargo.toml workspace.dependencies**: adds `lettre = { version
    = "0.11", default-features = false, features = ["smtp-transport",
    "tokio1-rustls", "ring", "webpki-roots", "builder"] }`. The
    R398-recommended feature list was extended at land time with
    `ring` (rustls crypto provider) + `webpki-roots` (Mozilla CA
    bundle) — both required by lettre's `tokio1-rustls` dependency
    at compile time.
  - **Audit verification**:
    `cargo tree -p yggdrasil-cardano-tracer | grep -iE "openssl|native-tls"`
    returned zero hits — transitive tree clean of all three banned
    crates per `deny.toml:88-91`. (cargo-deny isn't installed in
    this environment; the tree-grep audit covers the same surface
    as the typical deny-list ban check.)
  - **handlers/notifications/email.rs full SMTP send path**:
    create_and_send_email + create_and_send_test_email + send_email
    + explain_smtp_error helper. SSL dispatch on EmailSettings::ssl:
    Tls → AsyncSmtpTransport::relay; Starttls → starttls_relay;
    NoSSL → builder_dangerous. All wrapped in run_io_with_watchdog
    with the upstream's exact 10-second timeout +
    "✗ Unable to send: timeout" message. Error explanation mirrors
    upstream's `getAddrInfo` / `user error` substring matching with
    the same "check SMTP host" / "check your name, password or SSL"
    replacement strings.
  - **SmtpSendStatus** struct upgraded from a deferral descriptor to
    a closure marker: `status: "closed at R403"`.
  - **docs/DEPENDENCIES.md** updated to mark lettre as LANDED at
    R403 with the actual feature list + transitive-tree audit
    result.
  Carve-outs documented:
  - Data.Text.Lazy.Builder Mail body → lettre's Message::builder()
    (strict-text equivalent).
  - getAddrInfo / user error string-matching preserved with the
    same upstream replacement strings, but operates on
    lettre::Error Display string.
  Tests: cardano-tracer 308 → 309 (+1: create_and_send_email returns
  send-failure prefix when given an unreachable host; smtp_send_status
  test updated from "deferred" assertion to "closed at R403"
  closure assertion). Workspace tests: 5,712 → 5,713. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone R403 →
  R404. Per the R398 plan, this unblocks R404
  (makeAndSendNotification orchestration) and R405 (initEventsQueues).
- **R402 — cardano-tracer: createOrUpdateEmptyLog real impl (closes
  R390 LogRotationStatus carve-out + upgrades HandleRegistry to hold
  real file handles).** Lands the IO orchestration that was deferred
  at R390. Three workspace surfaces touched:
  - **HandleRegistry value-type upgrade** in `crates/cardano-tracer/src/types.rs`:
    previously `Registry<HandleRegistryKey, ((), PathBuf)>`; now
    `Registry<HandleRegistryKey, (SharedLogFile, PathBuf)>` where
    `SharedLogFile = Arc<tokio::sync::Mutex<tokio::fs::File>>`. Arc
    cloning is cheap; per-write operations acquire the inner Mutex
    to serialize bytes onto the underlying file descriptor.
  - **`format_log_timestamp(i64) → String`** in `handlers/logs/utils.rs`:
    inverse of `get_timestamp_from_log`'s parser. Mirrors upstream's
    `formatTime defaultTimeLocale timeStampFormat` shape.
  - **`create_or_update_empty_log` + `create_empty_log_rotation`**:
    full IO orchestration mirroring upstream. Acquires
    `Arc<tokio::sync::Mutex<()>>` (matches upstream's
    `Control.Concurrent.Extra.Lock`); mints `node-YYYY-MM-DDTHH-MM-SS.<ext>`
    filename; opens file write-only with truncation;
    drops any previous registry entry (closes old fd via Arc
    drop); inserts new entry; atomically swaps the `node.<ext>`
    symlink via `update_symlink_atomically` helper (uses
    `std::os::unix::fs::symlink` on Unix; non-Unix branch falls
    back to writing the target path as plain text per the
    cardano-tracer-is-Unix-only operational convention).
  - **`update_symlink_atomically` helper** runs symlink replace +
    rename inside `tokio::task::spawn_blocking` (since
    `std::fs::rename` and `std::os::unix::fs::symlink` are blocking).
  - **`LogRotationStatus`** struct upgraded from a deferral
    descriptor to a closure marker: `status: "closed at R402"` +
    `closed_at_round: "R402"`.
  - **Workspace `tokio` features** gain `"fs"` (was missing) — needed
    for `tokio::fs::File` + `tokio::fs::OpenOptions`.
  Carve-outs documented:
  - Windows `createFileLink` (NTFS junctions) replaced with
    plain-text fallback per workspace Unix-only convention.
  - Data.Time.Clock.UTCTime → Unix-epoch ms (consistent with
    `crate::time::get_time_ms`).
  Tests: cardano-tracer 303 → 308 (+5: log_rotation_status describes
  closure; format_log_timestamp is inverse of parser
  [round-trip via filename]; format_log_timestamp at Unix epoch;
  create_or_update_empty_log creates file + symlink + registers in
  registry; create_empty_log_rotation creates missing subdir;
  create_or_update_empty_log replaces previous handle [verifies
  registry holds new entry pointing at second file]).
  Workspace tests: 5,707 → 5,712. Parity-matrix entry
  sister-tool.cardano-tracer advanced: next_milestone R401 → R403.
  Per the R398 plan, this closes the R390 LogRotationStatus
  carve-out + the R400 WriteTraceObjectsToFileStatus carve-out's
  upstream blocker; the file-side write path can now be wired in a
  follow-up tightening round once `Arc<Mutex<File>>` write semantics
  are exercised in production.
- **R401 — cardano-tracer: Logs/TraceObjects.hs port (dispatcher
  routing).** Lands the per-LoggingParams trace-object dispatcher
  that fans out incoming objects to the appropriate sink (journal
  or file). New handlers/logs/trace_objects.rs module ports the
  upstream Cardano.Tracer.Handlers.Logs.TraceObjects bounded subset:
  - DispatchOutcome enum (Journal | FilePending | Skipped) describing
    the routing outcome per LoggingParams entry.
  - trace_objects_handler(&NodeName, &[LoggingParams],
    &[TraceObject]) async → Vec<DispatchOutcome> mirroring upstream's
    `traceObjectsHandler` dispatcher. Skips dispatch on empty
    trace-object input (returns vec![Skipped]); otherwise routes
    each LoggingParams to the matching sink:
    - JournalMode → calls write_trace_objects_to_journal (R382 no-op
      currently; preserved in the call graph for when the
      systemd-binding port lands).
    - FileMode → computes the line-encoded payload via R400's
      prepare_lines + returns FilePending with the byte count
      (actual file write defers to R402).
  - DeregisterNodeIdStatus + helper exposing the deferred-
    `deregisterNodeId` rationale programmatically.
  Carve-outs documented:
  - TracerEnv-record-arg + askNodeName lookup → caller passes
    pre-resolved NodeName (per R398's TracerEnv option (b)
    decision).
  - forConcurrently_ parallel fan-out → sequential per-LoggingParams
    iteration (acceptable until R402's actual file write happens;
    swap to tokio::task::join_all if soak shows contention).
  - teReforwardTraceObjects callback → deferred (depends on
    trace-forwarder mini-protocol acceptors at R411+).
  - #if RTVIEW saveTraceObjects arm → workspace RTView UI carve-out;
    never ported.
  - deregisterNodeId → deferred to R402 alongside the
    createOrUpdateEmptyLog port (depends on modifyRegistry_ +
    System.IO.hClose on registry-stored handles).
  Tests: cardano-tracer 294 → 303 (+9: empty trace_objects returns
  Skipped; empty logging_params with events returns no outcomes;
  JournalMode dispatches to Journal outcome; FileMode dispatches to
  FilePending with prepared_bytes matching prepare_lines output;
  mixed logging_params routes each independently; multi-event
  batches preserve count; ForMachine vs ForHuman produce different
  byte counts; deregister_node_id_status describes deferral;
  Skipped equality). Workspace: 5,698 → 5,707. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone R401
  → R402.
- **R400 — cardano-tracer: Logs/File.hs port (bounded subset; pure
  line-encoders).** Lands the per-format trace-object converters
  + line-encoding helpers used by the file-rotation IO orchestration.
  New handlers/logs/file.rs module ports the upstream
  Cardano.Tracer.Handlers.Logs.File bounded subset:
  - trace_text_for_human(&TraceObject) → &str — wraps R399's
    TraceObject::render_for_human (which falls back to to_machine
    when to_human is None, mirroring upstream's
    `fromMaybe toMachine toHuman`).
  - trace_text_for_machine(&TraceObject) → &str — always returns
    to_machine.
  - prepare_lines(LogFormat, &[TraceObject]) → Vec<u8> — UTF-8
    payload that would be appended to the log file: leading `\n`
    + per-format-converted lines joined with `\n` (mirror of
    upstream's `preparedLines = TE.encodeUtf8 (nl `T.append`
    T.intercalate nl itemsToWrite)`). Returns empty Vec for empty
    input (matches upstream's `unless (null itemsToWrite) do ...`
    guard).
  - WriteTraceObjectsToFileStatus + helper exposing the deferred-
    orchestration rationale programmatically.
  Carve-outs documented:
  - writeTraceObjectsToFile IO orchestration deferred — depends on
    super::utils::log_rotation_status (createOrUpdateEmptyLog)
    which is itself blocked on Cardano.Tracer.Utils.modifyRegistry_;
    resolves at R402 per the R398 plan.
  - Cardano.Tracer.Utils.nl replaced with crate::utils::NL (`"\n"`
    Unix; matches upstream Unix-only operational convention).
  Tests: cardano-tracer 282 → 294 (+12: trace_text_for_human uses
  to_human when present + falls back to machine when None;
  trace_text_for_machine always returns machine for both kinds;
  prepare_lines empty input returns empty; human starts with
  newline; human renders to_human text [excludes machine];
  machine renders to_machine text [excludes human]; intercalates
  multi-event with newline; human falls back per object;
  single-event byte-exact round-trip; handles empty to_machine;
  status describes deferral). Workspace: 5,686 → 5,698. Parity-
  matrix entry sister-tool.cardano-tracer advanced: next_milestone
  R400 → R401. Per the R398 plan, this leaves R401
  (Logs/TraceObjects.hs) unblocked next; R402 closes the
  WriteTraceObjectsToFileStatus + LogRotationStatus carve-outs.
- **R399 — cardano-tracer: TraceObject 6-field inline port (D3 from
  R398 plan; unblocks Logs/{File, TraceObjects} + acceptors).**
  Lands the canonical TraceObject record replacing R382's unit-
  struct placeholder. New logging.rs module synthesizes the
  `Cardano.Logging.TraceObject` shape (the upstream `trace-dispatcher`
  package is NOT vendored at .reference-haskell-cardano-node/; the
  field set was recovered from upstream's exhaustive field-accesses
  in `Logs/Journal/Systemd.hs::mkJournalFields` +
  `Logs/File.hs::traceTextForHuman/traceTextForMachine`):
  - 6 fields: to_human (Option<String>), to_machine (String),
    to_severity (SeverityS reused from R380), to_namespace
    (Vec<String> hierarchical path), to_thread_id (String —
    `tokio::task::id()`-formatted at use site), to_timestamp_ms
    (i64 Unix-epoch ms matching crate::time::get_time_ms convention).
  - new(...) all-explicit constructor for production sites.
  - render_for_human() — Option<&str> with fallback to to_machine
    (mirror of upstream's `fromMaybe toMachine toHuman`).
  - render_for_machine() — always returns to_machine.
  - namespace_dotted() — joins the namespace path with `.`
    separator (e.g. "BlockFetch.Server.Acquired") for the
    journal/systemd-journal `namespace` field.
  - Default impl returns an all-zeros / empty-strings placeholder
    suitable for tests + synthesis sites.
  Migration: existing unit-struct placeholder at
  handlers/logs/journal/no_systemd.rs:45 replaced with
  `pub use crate::logging::TraceObject` re-export. Existing call
  sites updated:
  - environment.rs::tests::no_op_reforward_does_not_panic_on_non_empty_input
    swapped from `&[TraceObject, TraceObject]` to
    `&[TraceObject::default(), TraceObject::default()]`.
  - handlers/logs/journal/no_systemd.rs::tests swapped similarly.
  Carve-outs documented:
  - LogFormatting typeclass methods (forHuman/forMachine) live on
    the *source* type per upstream; Yggdrasil-side equivalents are
    local to each emit site.
  - Data.Time.UTCTime nanosecond precision dropped to milliseconds
    (matches the rest of the cardano-tracer crate's wall-clock
    convention; sub-millisecond precision unused operationally).
  Tests: cardano-tracer 272 → 282 (+10: new builds with all fields;
  default uses Debug severity + empty strings + 0 timestamp;
  render_for_human uses to_human when present; falls back to
  to_machine when to_human is None; render_for_machine always
  returns to_machine; namespace_dotted joins with periods + handles
  empty namespace + handles single element; equality across clones;
  inequality when severity differs). Workspace: 5,676 → 5,686.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R399 → R400. Per the R398 audit, this unblocks
  R400 (Logs/File.hs writeTraceObjectsToFile) + R401 (Logs/TraceObjects.hs
  traceObjectsHandler) + R411+ trace-forwarder mini-protocol
  acceptors.
- **R398 — cardano-tracer: dependency audit + TracerEnv decision
  (R398-R410 sub-arc prep).** Documentation-only round preparing
  the next 12 rounds of cardano-tracer subsystem build-out. Three
  architectural decisions identified by the advisor + planning
  agent for the R398-R410 sub-arc:
  - **D1 — `lettre` 0.11 SMTP client (R403)**: closes
    `SmtpSendStatus` (R388). Pin features to `["smtp-transport",
    "tokio1-rustls", "builder"]` (default-features off) —
    mandatory to avoid `native-tls` blocked by `deny.toml`.
    ~30 transitive deps; MIT.
  - **D2 — `axum` 0.7 + `hyper` 1 + `tower` 0.5 + `rustls-pemfile`
    2 (R406)**: chosen over raw-tokio (the cardano-submit-api
    precedent) because cardano-tracer ships 4 separate HTTP servers
    + per-server TLS termination + content negotiation + per-node
    dynamic routing. Hand-rolling rustls integration 4 times is
    structurally wrong here. Closes `RenderHtmlStatus` (R391) +
    `ComputeRoutesStatus` (R391) at R406+R407.
  - **D2-prime — `maud` 0.27 HTML templating (R406)**: zero
    transitive deps (proc-macro only); replaces upstream's
    `Text.Blaze.Html` for `RouteDictionary::render_html`.
    Fallback: hand-rolled inline renderer if maud audit fails.
  - **D3 — TraceObject 6-field inline port (R399, no new deps)**:
    chosen over Option B (vendor `trace-dispatcher`, multi-quarter)
    + Option C (defer entirely, blocks too much). 6 fields:
    to_human / to_machine / to_severity / to_namespace /
    to_thread_id / to_timestamp_ms.
  - **TracerEnv 14-field record sub-decision**: option (b)
    tactical direct-arg pass-through chosen over (a) full record
    port. Per-helper signatures take only the slice of state they
    need; full record port deferred until `Cardano.Logging` +
    `Cardano.Timeseries` vendor.
  Adds 3 entries to `docs/DEPENDENCIES.md` under a new "Sister-tools
  port arc — R398 audit" section + a comprehensive operational-runs
  entry at `docs/operational-runs/2026-05-10-round-398-dep-audit-tracerenv-decision.md`
  with the per-decision audit + rejected-alternatives + side-by-side
  comparison vs cardano-submit-api precedent + risk register +
  R398-R410 round-by-round breakdown.
  No code changes; no `[workspace.dependencies]` bumps yet (those
  land at R403 / R406 with the actual `cargo deny check` against
  the resolved Cargo.lock). Workspace tests held at 5,676 (same
  as R397). All 5 cargo gates clean; all 3 parity validators clean.
  Phase A.5 cardano-tracer arc end-of-arc target shifts from R385
  (original) to R415 (post-buffer absorption); end-of-plan target
  shifts R459 → R464 (+5 rounds, within ±10 buffer per plan
  acceptance).
- **R397 — cardano-tracer: MetaTrace.hs port (TracerTrace 25-variant
  enum + supporting types).** Lands the trace-event taxonomy for
  the cardano-tracer's own self-tracing — the enum that every
  emit site dispatches through. New meta_trace.rs module ports
  the upstream Cardano.Tracer.MetaTrace surface:
  - TracerTrace enum with all 25 variants (TracerBuildInfo,
    TracerParamsAre, TracerConfigIs, TracerInitStarted, ...,
    TracerForwardingInterrupted) carrying upstream's exact field
    set + JSON `kind`-discriminated tag (`#[serde(tag = "kind")]`)
    and per-field upstream-name renames (`builtWithRTView`,
    `connectionIncomingAt`, `AcceptorsAddr`, etc.).
  - TraceBundle struct (assorted + timeseries) with closure-based
    Trace<T> tracers; default = null tracers.
  - Trace<T> = Arc<dyn Fn(&T) + Send + Sync> placeholder type
    alias mirroring upstream's `Trace IO TracerTrace`.
  - null_tracer<T>() constructor for default no-op tracer fields.
  - RT_VIEW_CONFIG_WARNING constant matching upstream verbatim.
  - ResourceStats + TimeseriesTrace placeholder types (full
    Cardano.Logging.Resources / Cardano.Timeseries.Component.Trace
    not vendored).
  - TracerTrace::for_human() — matches upstream's selective
    forHuman behavior (only emits non-empty for ConfigIs +
    ForwardingInterrupted variants).
  - TracerTrace::for_machine() — JSON value via serde
    serialization.
  - NodeId gains `serde::Serialize` + `serde::Deserialize` derives
    (with `#[serde(transparent)]`) to support the
    TracerAddNewNodeIdMapping variant.
  Carve-outs documented:
  - MetaTrace TracerTrace typeclass instance (severity +
    documentation classification per variant) deferred — Rust
    equivalent would be a trait with severity()/docs() methods;
    full table lands when trace-dispatcher is vendored.
  - Trace IO TracerTrace replaced with Arc<dyn Fn> closure
    (sync-only — async sinks would require BoxFuture upgrade).
  - ResourceStats + TimeseriesTrace placeholder types.
  - JSON forMachine flattening for TracerResource variant — Rust
    keeps the `"kind"` discriminant for serde tagging consistency
    (sites that need byte-equivalent flattened output post-process
    the JSON manually).
  Tests: cardano-tracer 256 → 272 (+16: RT_VIEW_CONFIG_WARNING
  matches upstream verbatim; null_tracer doesn't panic;
  TracerTrace serializes with kind discriminant + camelCase fields
  + error field + listenAt field + AddNewNodeIdMapping; for_human
  empty for init events / returns warning for ConfigIs / renders
  ForwardingInterrupted; for_machine returns JSON value; round-trips
  through JSON for 5 simple variants; SockConnecting uses upstream-
  typo'd `connectionIncomingAt` key; TraceBundle default uses null
  tracers + Debug renders closures as placeholders;
  meta_trace_instance_status describes deferral). Workspace: 5,660
  → 5,676. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R397 → R398.
- **R396 — cardano-tracer: Utils.hs port (bounded subset; runtime-state
  init helpers + connection-id sanitizer + Registry wrappers).**
  Lands the cross-cutting helper surface used by every cardano-tracer
  subsystem. New utils.rs module ports the upstream
  Cardano.Tracer.Utils bounded subset:
  - NL constant (`"\n"` for Unix; the Windows `"\r\n"` variant
    deferred per the cardano-tracer-is-Unix-only operational
    convention).
  - init_connected_nodes / init_connected_nodes_names /
    init_accepted_metrics / init_data_point_requestors /
    init_protocols_brake — return-default constructors for each
    runtime-state type.
  - apply_brake(&ProtocolsBrake) — engages the protocols brake.
  - conn_id_to_node_id(&str) → NodeId — string-sanitization for
    upstream's `connIdToNodeId`. Strips LocalAddress prefix +
    pipe/. (Windows) substrings, replaces \ / " space with `-`,
    collapses `--` runs, trims leading/trailing dashes.
  - get_process_id() — std::process::id() wrapper mirroring
    upstream's `getProcessId :: IO Word32`.
  - new_registry / member_registry / lookup_registry / read_registry
    / modify_registry — generic Registry<K, V> wrappers mirroring
    upstream's `newRegistry / memberRegistry / lookupRegistry /
    readRegistry / modifyRegistry_`.
  - Registry::snapshot() inherent method added to types.rs's R371
    Registry to support the new wrappers.
  - 3 deferral status descriptors (AskNodeNameStatus,
    BeforeProgramStopsStatus, SequenceConcurrentlyStatus) +
    helpers for downstream sites that surface deferral state.
  Carve-outs documented:
  - askNodeName / askNodeNameRaw / askNodeId — depend on data-point
    mini-protocol surface (askDataPoint, DataPointRequestor) +
    tracer-trace channel (Trace IO TracerTrace from MetaTrace.hs).
  - showProblemIfAny — same tracer-trace dependency.
  - beforeProgramStops — Unix signal handler installation; needs
    Run.hs supervisor task lifetime in scope.
  - sequenceConcurrently_ — no clean Rust 1:1 mirror; Rust uses
    tokio::join! / futures::future::join_all instead.
  - clearRegistry / elemsRegistry / showRegistry — depend on
    System.IO.hClose semantics (close stored file handle); deferred
    until Logs/File.hs ports a real handle type.
  - forMM / forMM_ — Rust uses native `for x in iter { ... }`;
    synthesis-only.
  Tests: cardano-tracer 234 → 256 (+22: NL canonical Unix newline;
  6 init_* default-state checks; apply_brake engages brake; 5
  conn_id_to_node_id sanitization edges [strip LocalAddress / drop
  path separators / strip leading-trailing dashes / drop quotes /
  collapse double-dashes]; get_process_id positive; 6 Registry
  wrapper tests [new empty / member false-empty / member true-after-
  insert / lookup composite-key returns value / lookup composite-key
  returns None / read snapshot / modify replaces contents / modify
  no-op preserves]; 3 deferral-status descriptors). Workspace:
  5,638 → 5,660. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R395 → R397.
- **R395 — closure-status doc refresh covering R378–R394.** Updates
  the [`docs/PARITY_SUMMARY.md`](docs/PARITY_SUMMARY.md) banner +
  current-implementation-status preamble to reflect the 17-round
  cardano-tracer subsystem build-out shipped since the R377 banner.
  Banner reads "**394+ parity rounds completed**" (was "376+") with
  workspace-test count refreshed from 5,443 → 5,638 (+195 across
  17 rounds: R378 +13, R379 +4, R380 +19, R381 +6, R382 +4, R383
  +16, R384 +13, R385 +7, R386 +11, R387 +6, R388 +11, R389 +15,
  R390 +24, R391 +19, R392 +0 [doc-only], R393 +10, R394 +17).
  Per-round summary added inline:
  - R378 db-synthesizer Orphans (JSON deserialization +
    AdjustFilePaths trait).
  - R379 cardano-tracer Time helper (EKG epoch-millis).
  - R380-R381 Notifications/{Types, Check} + SeverityS synthesis.
  - R382 Logs/Journal pair (CPP-dispatcher + no_systemd).
  - R383 Handlers/System path-resolution (XDG fallback).
  - R384-R385+R387 Notifications/{Settings, Utils — full surface}.
  - R386 Notifications/Timer (full periodic-action scheduler with
    tokio task + Mutex-shared state).
  - R388-R389 Notifications/{Email, Send} bounded subsets (SMTP
    send + orchestration deferred).
  - R390 Logs/Utils (log-naming + timestamp parser).
  - R391 Metrics/Utils (Content-Type constants + RouteDictionary +
    slugify).
  - R392 workspace structure cleanup (architecture review
    follow-through; AGENTS.md inventory + Cargo.toml semantic
    grouping + skeleton-only crate annotations).
  - R393 Environment.hs (TracerEnv 14-field record — unblocks
    downstream subsystems).
  - R394 Logs/Rotator pure rotation policy helpers.
  Status of cardano-tracer subsystem coverage: Notifications
  structurally complete (all 7 leaves: Types + Check + Settings +
  Utils + Timer + Email + Send); Logs has Journal pair + Utils +
  Rotator (3 of ~6 leaves); Metrics has Utils (1 of 5 leaves);
  System + Time + SeverityS + Environment foundational helpers
  shipped. RTView UI is the workspace-wide carve-out per plan.
  No code changes; doc-only round mirroring the cadence of R346 /
  R352 / R357 / R360 / R368 / R377 closure refreshes.
- **R394 — cardano-tracer: Logs/Rotator.hs port (bounded subset:
  pure rotation policy helpers).** Lands the pure rotation-policy
  surface — log-mode filtering, sort-by-timestamp, retention-count,
  age-threshold removal candidate selection. New
  handlers/logs/rotator.rs module ports the upstream
  Cardano.Tracer.Handlers.Logs.Rotator surface:
  - logging_params_for_files(&[LoggingParams]) — filters to
    FileMode-only entries and dedups (mirror of upstream
    `nub (NE.filter filesOnly logging)`).
  - log_is_full(current_size_bytes, max_size_in_bytes) — pure size
    threshold check (mirror of upstream's `logIsFull`); IO-bound
    `hTell handle >>= ...` chain split out.
  - check_if_there_are_old_logs(sorted_logs, max_age_minutes,
    keep_files_num, now_ms) — full removal-candidate computation
    with retention-count + age-threshold + early-exit-on-young-log
    (mirror of upstream's `checkIfThereAreOldLogs` walk lines
    153-172). Skip-on-malformed-timestamp matches upstream's
    continue-on-Nothing fall-through.
  - logs_to_remove(sorted_logs, keep_n) — drops the newest N from
    a list (mirror of upstream's `dropEnd keepFilesNum`).
  - sort_logs_oldest_first(logs) — orders by parsed timestamp
    with unparseable entries pushed last (mirror of upstream's
    `sort logs` over timestamp-bearing names).
  - RunLogsRotatorStatus + run_logs_rotator_status() helper
    exposing the deferred-orchestration rationale programmatically.
  Carve-outs documented:
  - runLogsRotator / launchRotator / checkRootDir / checkLogs /
    checkIfCurrentLogIsFull (IO-bound) — depend on
    Cardano.Tracer.Utils.{showProblemIfAny, readRegistry} (both
    unported), tracer-trace channel from MetaTrace.hs (unported),
    + the deferred createOrUpdateEmptyLog from super::utils.
  - Data.Time.diffUTCTime → Unix-epoch milliseconds (matches
    super::utils::get_timestamp_from_log convention; eliminates
    upstream's `NominalDiffTime` / picosecond intermediate).
  Tests: cardano-tracer 217 → 234 (+17: 3 logging_params_for_files
  cases [keeps FileMode only / dedups identical / empty when no
  FileMode]; 2 log_is_full cases [true at threshold / false below];
  4 logs_to_remove cases [drops N newest / keep>=total returns
  empty / keep==total returns empty / keep=0 returns all]; 5
  check_if_there_are_old_logs cases [removes old files at correct
  threshold / keeps newest N / empty input / skips unparseable
  timestamps / stops at first young log]; 2 sort_logs_oldest_first
  cases [orders by timestamp / pushes unparseable to end];
  run_logs_rotator_status describes deferral). Workspace: 5,621 →
  5,638. Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R394 → R395.
- **R393 — cardano-tracer: Environment.hs port (TracerEnv 14-field
  record, unblocks downstream subsystems).** Lands the runtime
  environment record threaded through every cardano-tracer
  subsystem. New environment.rs module ports the upstream
  Cardano.Tracer.Environment surface:
  - TracerEnv 14-field struct with each field tagged to its
    upstream `te*` name + Haskell type. Reuses already-ported
    types from R358 (TracerConfig) + R371 (ConnectedNodes,
    ConnectedNodesNames, ProtocolsBrake, HandleRegistry).
  - Placeholder types for 4 unported field types (AcceptedMetrics,
    DataPointRequestors, TracerTrace, TimeseriesHandle) — each
    documented as a deferred carve-out with the upstream
    Haskell type signature on file.
  - Lock fields use `Arc<tokio::sync::Mutex<()>>` (single-acquirer
    semantics matching upstream's Control.Concurrent.Extra.Lock).
  - ReforwardTraceObjects type alias around
    `Arc<dyn Fn(&[TraceObject]) + Send + Sync>` mirroring upstream's
    `[TraceObject] -> IO ()` callback. no_op_reforward() default
    constructor returns a stub that's safe in all sites.
  - TracerEnv::new(TracerConfig) constructor + with_state_dir()
    builder method. Production wiring in the future Run.hs
    supervisor will populate the runtime-state fields after
    construction.
  - Custom Debug impl that renders Mutex + closure fields as
    `<Mutex>` / `<closure>` placeholders (since neither derives
    Debug naturally).
  - TracerEnvRTView intentionally empty (mirrors upstream's
    `#else` branch when RTVIEW build flag is off — the entire
    RTView UI is the workspace-wide carve-out).
  Carve-outs documented:
  - 4 unported field types: AcceptedMetrics (TVar Map NodeId TVar
    EKG.Store; pending EKG-equivalent), DataPointRequestors (TVar
    Map NodeId DataPointRequestor IO; pending datapoint mini-
    protocol), TracerTrace (Trace IO TracerTrace; pending
    MetaTrace.hs port), TimeseriesHandle (Cardano.Timeseries.Component;
    pending cardano-timeseries-io vendoring).
  - reforward closure ships as no-op until Acceptors/Run.hs lands.
  Tests: cardano-tracer 207 → 217 (+10: tracer_env_new uses
  supplied config + default-initializes runtime-state fields;
  locks acquire independently [tokio::test]; with_state_dir
  override + clear-to-None; Debug renders all fields with
  `<Mutex>` / `<closure>` placeholders for non-derivable fields;
  no_op_reforward doesn't panic on empty + non-empty input;
  placeholder unit-struct types construct; clone produces
  independent value sharing the same Mutex Arc). Workspace:
  5,611 → 5,621. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R392 → R394. Unblocks computeRoutes
  (Metrics/Utils) + Logs/Rotator + Acceptors/* + Run.hs supervisor
  + Metrics handlers (Servers / Monitoring / Prometheus /
  TimeseriesServer) — all of which take TracerEnv as their
  primary parameter.
- **R392 — workspace structure cleanup (architecture review
  follow-through).** Operator-requested critical review of the 19
  workspace crates concluded that the count is correct (each crate
  mirrors a separate upstream Haskell package per the strict 1:1
  file-mirror policy) but readability could be improved without
  touching any code. Three concrete cleanups shipped:
  1. **`crates/AGENTS.md` expanded** with a full 19-crate inventory
     table (was: 6 core crates only). Each crate row carries
     LOC + upstream-package mapping + current status (skeleton /
     partial / verified_11_0_1) grouped by Tier 1 / 2 / 3 / 4 per
     the R326-R459 sister-tools plan. Adds a "do not propose
     merging crates" rule to maintenance guidance — the parity
     contract is per-upstream-package, not per-LOC.
  2. **Workspace `members` list reordered semantically** in the
     root `Cargo.toml`. Previously two arbitrary blocks ("Existing
     crates" + "Sister-tool crates"); now grouped as
     core / cardano-cli / node binary / Tier 1 / Tier 2 / Tier 3 /
     Tier 4 with per-block heading comments showing the R-arc
     ownership and per-crate status comments at line-end. The file
     reads as a status board now without cross-referencing the
     parity-matrix.
  3. **Skeleton-only outliers documented in `Cargo.toml`
     description fields**. `kes-agent` (~150 LOC) now reads
     "SKELETON STUB awaiting Phase A.3 entry at R344+ —
     HIGHEST-STAKES sister-tool work; gated on upstream
     socket-protocol byte-equivalence fixture capture per the R344
     risk register entry." `tx-generator` (~150 LOC) reads
     "SKELETON STUB gated on Phase C entry at R408+ — entry
     depends on the cardano-cli C-arc CLI-MVS subset (keys / tx /
     query / genesis / governance) reaching verified_11_0_1
     status before tx-generator's submit driver can be wired."
     The package-level comment block on each crate's Cargo.toml
     also now cites the specific gating dependency.
  No code changes; doc + workspace-structure round. All 5 cargo
  gates clean; workspace tests held at 5,611 (same as R391
  closeout). Strict-mirror gate clean (0 violations); parity-matrix
  clean (20 entries validated against tag 11.0.1).
- **R391 — cardano-tracer: Metrics/Utils.hs port (bounded subset:
  Content-Type constants + RouteDictionary + slugify).** Lands the
  metrics-server utility surface — Content-Type response headers,
  per-node URL routing table, slug helper. New
  handlers/metrics.rs parent-shell module + handlers/metrics/utils.rs
  port the upstream Cardano.Tracer.Handlers.Metrics.Utils surface:
  - 5 Content-Type response-header constants
    (CONTENT_HDR_JSON / CONTENT_HDR_OPEN_METRICS /
    CONTENT_HDR_UTF8_HTML / CONTENT_HDR_UTF8_TEXT /
    CONTENT_HDR_PROMETHEUS) as `(&'static str, &'static str)`
    tuples carrying upstream's exact MIME strings.
  - RouteDictionary newtype (slug → node-name pair list) with
    new() constructor + node_names() + render_json() returning a
    BTreeMap-sorted `node_name → "/slug"` JSON object (matches
    upstream's Data.Map.fromList semantics).
  - slugify(&str) → String inline implementation — lowercases
    ASCII alphanumeric, replaces non-alphanumeric runs with `-`,
    strips leading/trailing dashes, drops non-ASCII chars (matches
    upstream Text.Slugify default settings).
  - ComputeRoutesStatus + RenderHtmlStatus + helpers exposing the
    deferred entry-point rationale programmatically.
  Carve-outs documented:
  - computeRoutes — depends on the unported TracerEnv 14-field
    record (teConnectedNodesNames + teAcceptedMetrics TVars) and
    EKG.Store equivalent.
  - renderListOfConnectedNodes HTML page — depends on a Text.Blaze
    equivalent (e.g. maud / markup / horrorshow) pending
    docs/DEPENDENCIES.md justification, OR hand-rolled inline
    renderer.
  - System.Metrics.Store dropped from RouteDictionary tuple
    pending EKG-equivalent metrics surface.
  - Network.HTTP.Types.ResponseHeaders → `(&str, &str)` (name,
    value) tuples; callers wrap to axum/hyper HeaderMap at use
    site.
  Tests: cardano-tracer 188 → 207 (+19: 5 content-header
  constants verbatim; RouteDictionary default empty + new
  round-trip; render_json node→route mapping + empty dictionary +
  alphabetical key ordering; slugify lowercases + replaces spaces
  + collapses non-alphanumeric runs + strips leading/trailing
  dashes + drops non-ASCII [café→caf, ümlaut→mlaut] + empty
  string + only-punctuation yields empty; 2 deferral-status
  helpers describe carve-outs). Workspace: 5,592 → 5,611.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R391 → R392.
- **R390 — cardano-tracer: Logs/Utils.hs port (bounded subset:
  pure log-naming + timestamp parser).** Lands the log-file naming
  + timestamp-parsing helpers shared between the file-writer and
  rotator subsystems. New handlers/logs/utils.rs module ports the
  upstream Cardano.Tracer.Handlers.Logs.Utils surface:
  - LOG_PREFIX const ("node-") + TIMESTAMP_FORMAT const
    ("%Y-%m-%dT%H-%M-%S") matching upstream verbatim.
  - log_extension(LogFormat) → &'static str (.log for ForHuman,
    .json for ForMachine).
  - sym_link_name(LogFormat) → String ("node.log" / "node.json").
  - is_it_log(LogFormat, &Path) → bool — validates prefix +
    extension + parseable timestamp + valid calendar date
    (mirror of upstream's `isItLog`).
  - get_timestamp_from_log(&Path) → Option<i64> — extracts the
    timestamp portion from a rotated log filename and parses it to
    Unix-epoch milliseconds.
  - parse_log_timestamp + ymd_to_days_since_epoch helpers using
    Howard Hinnant's public-domain `days_from_civil` (inverse of
    R389's `format_event_timestamp` epoch-arithmetic) for
    chrono-free date math.
  - is_valid_date with the standard Gregorian leap-year rule
    (`year % 4 == 0 && year % 100 != 0 || year % 400 == 0`).
  - LogRotationStatus + log_rotation_status() helper exposing the
    deferred-rotation rationale programmatically.
  Carve-outs documented:
  - createEmptyLogRotation / createOrUpdateEmptyLog deferred —
    depend on Cardano.Tracer.Utils.modifyRegistry_ (atomic
    read-modify-write under Lock) which isn't ported. Yggdrasil's
    HandleRegistry from R371 is Arc<RwLock<HashMap<...>>> — once
    modifyRegistry_ ships, the rotation helpers will use
    tokio::sync::RwLock::write_lock().
  - Data.Time.Clock.UTCTime → Option<i64> Unix-epoch ms (matches
    crate::time::get_time_ms convention; same information content;
    sites that need a structured datetime can use R389's
    format_event_timestamp).
  Tests: cardano-tracer 164 → 188 (+24: LOG_PREFIX + TIMESTAMP_FORMAT
  canonical strings; log_extension for ForHuman + ForMachine;
  sym_link_name for both formats; is_it_log accepts canonical
  human/machine logs + rejects wrong extension / missing prefix /
  malformed timestamp / invalid calendar date / invalid hour / no
  extension; get_timestamp_from_log Unix epoch + 2023-11-14 known
  value + with directory prefix + returns None for malformed /
  invalid date / missing prefix; leap-year Feb 29 round-trips on
  2020 + rejects Feb 29 in non-leap 2021; ymd_to_days round-trips
  with R389's format_event_timestamp; log_rotation_status
  describes deferral). Workspace: 5,568 → 5,592. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone R390 →
  R391.
- **R389 — cardano-tracer: Notifications/Send.hs port (bounded
  subset; orchestration deferred).** Lands the notification-send
  body-formatting layer. Notifications subsystem is now structurally
  complete (all 7 leaves ported as bounded subsets where dependencies
  warrant). New handlers/notifications/send.rs module ports the
  upstream Cardano.Tracer.Handlers.Notifications.Send surface:
  - format_notification_body(&[Event], &[(NodeId, NodeName)]) →
    String — mirror of upstream `sendNotification`'s `preface <>
    events` body-construction with singular/plural "event/events"
    word + per-event `[ts] [node-name] [sev] [msg]` line + canonical
    "This is a notification from Cardano RTView service." preface
    + no trailing newline (matching upstream's `T.intercalate nl`).
  - format_event_timestamp(i64) → String — emits upstream's
    `%F %T %Z` shape as "YYYY-MM-DD HH:MM:SS UTC" using a manual
    format string + an inline days-since-epoch-to-(year, month, day)
    algorithm based on Howard Hinnant's public-domain
    `civil_from_days` (no chrono workspace dependency needed).
  - get_node_name(NodeId, &[(NodeId, NodeName)]) → String — pure
    lookup-with-fallback helper mirroring upstream's `getNodeName`
    inline closure inside `sendNotification`.
  - format_severity(SeverityS) → &'static str — maps each of the
    8 severity variants to its variant-name string (mirrors
    upstream's `showT sev`).
  - MakeAndSendNotificationStatus + make_and_send_notification_status()
    helper exposing the deferred-orchestration rationale
    programmatically.
  Carve-outs documented:
  - makeAndSendNotification orchestration deferred — depends on
    DataPointRequestors (unported), Trace IO TracerTrace
    (unported), Cardano.Tracer.Utils.askNodeNameRaw (unported),
    and the SMTP send-path which is itself a carve-out per
    super::email::smtp_send_status.
  - Upstream's locale-dependent `%Z` timezone abbreviation
    hard-coded to "UTC" for parity (operational tracer hosts run
    in UTC; chrono-free implementation avoids extra workspace dep).
  Tests: cardano-tracer 149 → 164 (+15: format_event_timestamp at
  Unix epoch + 2023-11-14 known value + Y2038 threshold;
  get_node_name registered + unregistered + empty-map; 7
  format_notification_body cases [empty events, singular,
  plural, name-when-registered, name-when-unregistered, canonical-
  preface, no-trailing-newline]; make_and_send_notification_status
  deferral; format_severity for all 8 variants). Workspace: 5,553
  → 5,568. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R389 → R390.
- **R388 — cardano-tracer: Notifications/Email.hs port (bounded
  subset; SMTP send carved out pending lettre approval).** Lands the
  notification-engine email helpers — bounded subset that doesn't
  require an SMTP transport. New handlers/notifications/email.rs
  module ports the upstream
  Cardano.Tracer.Handlers.Notifications.Email surface:
  - StatusMessage = String type alias.
  - STATUS_SUCCESS + STATUS_TIMEOUT constants matching upstream's
    exact `✓ Yay! Notification is sent.` + `✗ Unable to send:
    timeout` strings (preserved verbatim so status_is_ok can grep
    for the same substring).
  - status_is_ok(&str) -> bool — case-sensitive `Yay` substring
    match mirroring upstream's `T.isInfixOf "Yay"` semantics.
  - test_notification_body() returning upstream's exact
    "Congrats: your email settings are correct!" template.
  - run_io_with_watchdog<F: Future, T>(timeout_secs: f64,
    timeout_value: T, action: F) async helper using
    tokio::time::timeout (mirror of upstream's
    `Control.Concurrent.Async.race` pattern).
  - SmtpSendStatus struct + smtp_send_status() helper exposing the
    deferred-SMTP-send rationale programmatically.
  Carve-outs documented:
  - Network.Mail.SMTP + Network.Mail.Mime full SMTP transport
    (createAndSendEmail / createAndSendTestEmail / sendEmail with
    TLS/STARTTLS/NoSSL dispatch) deferred pending lettre crate (or
    equivalent SMTP client) workspace-dependency approval per
    docs/DEPENDENCIES.md. lettre is ~30 transitive deps, MIT,
    pure-Rust; once approved, the SMTP send-path lands in a
    follow-up round without changing the rest of this module's
    surface.
  - Upstream's getAddrInfo / "user error" Haskell-specific
    error-string matching deferred — Rust's lettre crate will
    surface error categories more cleanly via lettre::error::Error
    variants when added.
  Tests: cardano-tracer 138 → 149 (+11: STATUS_SUCCESS contains
  "Yay"; STATUS_TIMEOUT does not; status_is_ok true for
  STATUS_SUCCESS + false for STATUS_TIMEOUT + false for error
  message + case-sensitive [lowercase "yay" rejected];
  test_notification_body matches upstream verbatim;
  run_io_with_watchdog passes through fast result + returns
  timeout_value when slow + works with full STATUS_TIMEOUT string;
  smtp_send_status describes deferral with lettre dependency).
  Workspace: 5,542 → 5,553. Parity-matrix entry sister-tool.cardano-
  tracer advanced: next_milestone R388 → R389.
- **R387 — cardano-tracer: re-enable Notifications/Utils.hs
  timer-bound helpers (now unblocked by R386).** Lands the three
  Utils.hs helpers that R385 had stub-and-deferred pending the full
  Timer surface. Functions added to handlers/notifications/utils.rs:
  - change_timer_state<F: AsyncFn(&Timer)>(&EventsQueues,
    EventGroup, setter) async → bool — applies a per-Timer transform
    to the timer registered under a group (read-lock; closure runs
    while holding it). Returns true if the timer was found,
    false otherwise. Mirror of upstream
    `changeTimerState :: (Timer -> IO ()) -> EventsQueues ->
    EventGroup -> IO ()`.
  - update_notifications_events(&EventsQueues, EventGroup, bool)
    async → bool — dispatches to start_timer / stop_timer based
    on the bool. Mirror of upstream's two-arm pattern-match.
  - update_notifications_periods(&EventsQueues, EventGroup,
    PeriodInSec) async → bool — calls set_call_period on the
    matching timer. Mirror of upstream
    `\`setCallPeriod\` period`-style invocation.
  Module docstring updated to remove the deferred-status entries
  for these three helpers; only initEventsQueues remains deferred
  (gated on Send.hs + DataPointRequestors + tracer-trace channel
  ports — see init_events_queues_status).
  Tests: cardano-tracer 132 → 138 (+6: change_timer_state returns
  false for unregistered group + runs closure for registered
  group; update_notifications_events starts and stops a registered
  Timer [verifies is_running before/after]; update_notifications_events
  returns false for unregistered group; update_notifications_periods
  swaps call_period in flight [verified via timer.call_period()
  reader]; update_notifications_periods returns false for
  unregistered group). Workspace: 5,536 → 5,542. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone R387 →
  R388.
- **R386 — cardano-tracer: Notifications/Timer.hs port (full
  periodic-action scheduler).** Lands the full Timer surface
  replacing types.rs's R380 placeholder unit struct. New
  handlers/notifications/timer.rs module ports the upstream
  Cardano.Tracer.Handlers.Notifications.Timer surface:
  - Timer struct holding Arc<Mutex<PeriodInSec>> call_period +
    Arc<Mutex<bool>> is_running + Option<Arc<JoinHandle<()>>>
    (None for placeholders). 5 inherent methods mirror upstream's
    5-record fields:
    - is_alive() (threadAlive), kill() (threadKill),
      set_call_period(P) async (setCallPeriod), start_timer() /
      stop_timer() async (startTimer / stopTimer).
    - Plus call_period() / is_running() readers for tests.
  - 5 constructors:
    - Timer::new (full mkTimer with on_failure_message callback)
    - Timer::new_stderr (mkTimerStderr — stderr-logging variant)
    - Timer::new_die_on_failure (mkTimerDieOnFailure)
    - Timer::new_stderr_die_on_failure (mkTimerStderrDieOnFailure)
    - Timer::placeholder (no-op no-task variant for EventsQueues
      default).
  - CHECK_PERIOD_SECS const = 1 mirroring upstream's
    `checkPeriod = 1` granularity.
  - Spawn-loop semantics: every CHECK_PERIOD_SECS seconds, check
    is_running flag → if false skip, else accumulate elapsed_time
    (kept in closure-local Mutex, not on struct, since lifetime is
    bounded by task) → when elapsed >= period, run action via
    tokio::task::spawn_blocking + std::panic::catch_unwind, reset
    elapsed on success, fire on_failure_message + optional break
    on panic.
  Carve-outs documented:
  - Trace IO TracerTrace replaced with Box<dyn Fn(&str) + Send +
    Sync> failure-message callback (Yggdrasil-side tracer-trace
    surface not yet ported).
  - killThread =<< myThreadId (upstream's die-on-failure pattern
    that kills the calling thread) replaced with abort-only-the-
    timer-loop semantics. Operationally safer in a multi-tenant
    tokio runtime — the periodic action stops running but the
    surrounding tracer process keeps going.
  - Control.Exception.try → std::panic::catch_unwind.
  Updates types.rs Timer to be a `pub use super::timer::Timer`
  re-export. Downstream test sites in check.rs / utils.rs / types.rs
  swap from `Timer` unit struct construction to `Timer::placeholder()`.
  Tests: cardano-tracer 121 → 132 (+11: CHECK_PERIOD_SECS canonical
  1; placeholder default not-alive; default constructs to
  placeholder; placeholder methods are safe no-ops; new timer
  invokes action after period elapses [4-second wait]; stop_timer
  pauses invocations [3-second test, counter stays 0]; start_timer
  resumes invocations [initial-stopped + 2-second wait + start +
  3-second wait]; set_call_period updates in flight;
  is_running flag round-trips; kill aborts task with eventual
  is_alive=false [50ms-poll loop, 20-iter ceiling]; die_on_failure
  aborts loop after action panics [counter exactly 1, failure
  callback received the panic message]). Workspace: 5,525 →
  5,536. Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R386 → R387.
- **R385 — cardano-tracer: Notifications/Utils.hs port (bounded
  subset).** Lands the bounded subset of upstream's notification-engine
  utility helpers — the two functions whose dependencies are already
  ported (`addNewEvent`, `getNewEvents`) plus stub-and-defer markers
  for the timer-bound entries. New handlers/notifications/utils.rs
  module ports the upstream
  Cardano.Tracer.Handlers.Notifications.Utils surface:
  - add_new_event(&EventsSenders, EventGroup, Event) async → bool —
    push event to per-group queue; returns true if routed, false if
    group has no registered sender (mirror of upstream's silent
    no-op when M.lookup eventGroup queues = Nothing). Takes the
    EventsSenders auxiliary type added in R381 (producer side)
    rather than upstream's bidirectional EventsQueues.
  - get_new_events(&EventsQueues, EventGroup) async → Vec<Event> —
    drains all currently-queued events for a group in FIFO order
    via try_recv loop (mirror of upstream's `atomically $
    flushTBQueue queue`).
  - InitEventsQueuesStatus struct + init_events_queues_status()
    helper exposing the deferral rationale programmatically; sites
    wiring up a partial cardano-tracer runtime can reference this
    type to surface "feature deferred" status without duplicating
    the rationale string.
  Stub-and-defer markers documented for:
  - initEventsQueues — depends on full Timer surface (forkIO +
    killThread closures + setCallPeriod), pending in a future round.
  - updateNotificationsEvents / updateNotificationsPeriods /
    changeTimerState — same Timer dependency.
  Carve-outs documented:
  - Cardano.Tracer.MetaTrace.TracerTrace channel not yet ported;
    upstream's initEventsQueues writes trace events during init
    that Yggdrasil-side equivalents will eventually consume.
  - isFullTBQueue bounded-queue check not applicable; Yggdrasil's
    EventsQueue is mpsc::UnboundedSender (per the R380 carve-out
    documentation). If a future round needs strict bounded-queue
    semantics, swap to mpsc::Receiver<Event> (bounded) and observe
    try_send Err(Full) at the add_new_event call site.
  Tests: cardano-tracer 114 → 121 (+7: add_new_event returns true
  when group registered + false when not registered; get_new_events
  drains all queued events FIFO; returns empty when group not
  registered; returns empty when queue is empty; second drain after
  first yields empty; init_events_queues_status describes deferral
  with Timer dependency). Workspace: 5,518 → 5,525. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone R385 →
  R386.
- **R384 — cardano-tracer: Notifications/Settings.hs port (settings
  persistence).** Lands the persistence layer for the notification
  engine. New handlers/notifications/settings.rs module ports the
  upstream Cardano.Tracer.Handlers.Notifications.Settings surface
  (now unblocked by R383's Handlers/System.hs):
  - read_saved_email_settings(Option<&Path>) → EmailSettings —
    loads from `{config}/notifications/email`, falling back to
    [`default_email_settings`] on any IO or parse error (matches
    upstream's `try_` + `decodeStrict'` cascade).
  - read_saved_events_settings(Option<&Path>) → EventsSettings —
    same pattern for `{config}/notifications/events`.
  - save_email_settings_on_disk(Option<&Path>, &EmailSettings) —
    writes JSON-encoded settings to disk; IO errors silently
    ignored (matches upstream's `ignore do ...` wrapper).
  - save_events_settings_on_disk(Option<&Path>, &EventsSettings) —
    same pattern for events settings.
  - incomplete_email_settings(&EmailSettings) → bool — true when
    smtp_host is empty (mirror of upstream's
    `T.null $ esSMTPHost emailSettings`).
  - default_email_settings() — port=-1 sentinel + empty strings +
    Tls.
  - default_events_settings() — all 6 groups set to (false, 1800).
  - default_events_state() — (false, 1800) helper.
  Carve-outs documented:
  - TracerEnv-record-arg → Option<&Path> per R383's pattern.
  - Control.Exception.Extra.try_ + ignore → Result::ok() / `let _
    = ...` mirroring upstream's silent-ignore behavior on missing /
    unwritable settings file.
  - Encryption layer (commented out in upstream Settings.hs) NOT
    ported — Yggdrasil mirrors upstream's actual plain-JSON
    behavior. If a future round adds encryption it lands as a
    separate port + carve-out close.
  Tests: cardano-tracer 101 → 114 (+13: default_events_state matches
  upstream sentinel; default_events_settings uses default_state for
  all 6 groups; default_email_settings sentinels [port=-1, ssl=Tls,
  all-empty strings]; incomplete_email_settings true-for-default +
  false-when-host-set; read fallback when no file + on unparsable
  JSON; save+read round-trip for both EmailSettings and
  EventsSettings; save creates notifications subdir for both kinds;
  save overwrites previous file). Workspace: 5,505 → 5,518.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R384 → R385.
- **R383 — cardano-tracer: Handlers/System.hs port (path-resolution
  helpers with XDG fallback).** Lands the path-resolution surface
  that future Notifications/Settings + RTView state-management
  sites need to locate per-tracer config + data directories. New
  handlers/system.rs module ports the upstream
  Cardano.Tracer.Handlers.System surface:
  - RT_VIEW_ROOT_DIR const ("cardano-rt-view") mirroring upstream
    `rtViewRootDir :: FilePath`.
  - XdgKind enum (Config | Data) — narrow subset of upstream's
    System.Directory.XdgDirectory used by cardano-tracer (cache +
    state variants intentionally omitted; upstream doesn't use
    them either).
  - get_state_dir(Option<&Path>, XdgKind) → PathBuf returning the
    operator-supplied state dir if set, else falling back to the
    XDG-base-dir for the requested kind. Mirror of upstream
    `getStateDir`.
  - xdg_dir_with_env_lookup<E, H>(XdgKind, env_lookup, home_lookup)
    test-friendly helper that decouples env/home resolution from
    the live process environment (mirroring R370's optFromEnv
    closure-injection pattern).
  - get_path_to_config_dir / get_paths_to_notifications_settings /
    get_path_to_charts_config / get_path_to_theme_config /
    get_path_to_logs_live_view_font_config /
    get_path_to_chart_colors_dir / get_path_to_backup_dir — direct
    ports of the 6 upstream `getPathTo*` helpers, each calling
    `std::fs::create_dir_all` for the directory variants and
    returning the resolved path.
  Carve-outs documented:
  - TracerEnv-record-arg replaced with Option<&Path> directly. The
    upstream helpers take `TracerEnv` and pluck `teStateDir` out;
    Yggdrasil's TracerEnv 14-field record is pending (depends on
    Cardano.Logging + Timeseries vendoring). Once TracerEnv lands,
    thin wrappers can pluck `te_state_dir` and call into these
    lower-level helpers without changing their signatures.
  - System.Directory.XdgDirectory replaced with manual
    $XDG_CONFIG_HOME / $XDG_DATA_HOME env-var lookup with
    $HOME/.config + $HOME/.local/share fallback. Linux/Unix subset
    only — cardano-tracer is Unix-only in operator practice.
    Empty-string XDG vars are treated as unset (matches POSIX
    env-var semantics).
  Tests: cardano-tracer 85 → 101 (+16: RT_VIEW_ROOT_DIR canonical-
  string + 2 get_state_dir override tests for both XdgKind variants
  + 7 xdg_dir_with_env_lookup tests covering both XDG vars set,
  both fallbacks to $HOME suffix, missing-HOME relative fallback,
  empty-XDG-var-as-unset handling + 6 path-helper integration tests
  using ephemeral tempdirs that verify ends_with + exists + is_dir
  invariants for each helper). Workspace: 5,489 → 5,505. Parity-
  matrix entry sister-tool.cardano-tracer advanced: next_milestone
  R383 → R384. Unblocks Notifications/Settings.hs port for R384.
- **R382 — cardano-tracer: Logs/Journal pair port (CPP-dispatcher +
  no_systemd no-op).** Lands the journal log-sink surface as a
  bounded pair: handlers/logs.rs (parent-shell synthesis), the
  CPP-style dispatcher, and the no-op no-systemd impl. New files:
  - handlers/logs.rs — parent shell with `**Strict mirror:** none.`
    synthesis declaration + layout map; pub mod journal exported.
  - handlers/logs/journal.rs — direct port of upstream's
    CPP-conditional `module Impl` re-export; Yggdrasil always
    selects the NoSystemd path per the workspace policy banning
    systemd-specific dependencies (no-FFI policy). pub use
    no_systemd::write_trace_objects_to_journal preserves upstream's
    flat call surface so callers see the same API regardless of the
    underlying impl.
  - handlers/logs/journal/no_systemd.rs — direct port of upstream's
    `writeTraceObjectsToJournal :: LogFormat -> NodeName ->
    [TraceObject] -> IO ()`; Rust signature is
    `write_trace_objects_to_journal(LogFormat, &NodeName, &[TraceObject])
    -> std::io::Result<()>` returning Ok(()) unconditionally
    (matches upstream's `pure ()` semantics).
  - TraceObject placeholder unit struct documented as a deferred
    port (full upstream type carries toHuman/toMachine/toSeverity/
    toNamespace/toThreadId/toTimestamp per the field accesses in
    Logs/Journal/Systemd.hs::mkJournalFields).
  Carve-outs documented:
  - Logs/Journal/Systemd.hs upstream impl carved out — uses the
    libsystemd-journal Haskell binding which itself wraps the C
    libsystemd-journal library. Yggdrasil's no-FFI policy forbids
    that path. Operators wanting journald output can run the tracer
    behind systemd's `StandardOutput=journal` redirect.
  - Cardano.Logging.TraceObject deferred (synthesis placeholder
    used; full port lands when trace-dispatcher is vendored).
  Includes a `.gitignore` fix anchoring the `logs/` runtime-output
  ignore-rule to repo root (`/logs/`) plus explicit
  `**/run/logs/` + `**/*-runtime-logs/` patterns. Without the
  anchor, the rule was swallowing the new source-tree
  `crates/cardano-tracer/src/handlers/logs/` directory — same drift
  class as R310/R311 (over-broad gitignore patterns hiding production
  source files). The strict-mirror gate (R311 index-vs-tree drift
  check) caught this in this same round; it warned that two new
  files existed locally but weren't tracked, the .gitignore was
  patched, and the gate cleared.
  Tests: cardano-tracer 81 → 85 (+4: write_trace_objects_to_journal
  no-op-returning-Ok with ForMachine format + ForHuman format +
  non-empty trace-object list + TraceObject default construction).
  Workspace: 5,485 → 5,489. Parity-matrix entry sister-tool.cardano-
  tracer advanced: next_milestone R382 → R383.
- **R381 — cardano-tracer: Notifications/Check.hs port (severity
  dispatcher).** Lands the per-event-group severity dispatcher that
  routes incoming trace events to the correct event-group queue. New
  handlers/notifications/check.rs module ports the single-function
  upstream Notifications/Check.hs surface (now unblocked by R380's
  SeverityS + Event + EventGroup foundation):
  - EventsSenders auxiliary type (Arc<RwLock<BTreeMap<EventGroup,
    mpsc::UnboundedSender<Event>>>>) bridging upstream's
    bidirectional STM TBQueue to tokio's split-channel pattern. The
    sender side lives separately because tokio's
    mpsc::UnboundedSender must be cloned to be sharable across
    producers; upstream's TBQueue is bidirectional and stored once.
  - new_events_senders() empty-map constructor.
  - check_common_errors(node_id, message, severity, time_ms,
    &EventsSenders) async dispatcher mirroring upstream's
    `checkCommonErrors :: NodeId -> TraceObjectInfo -> EventsQueues
    -> IO ()`. Returns bool: true if routed to a queue, false if
    severity is below Warning OR if the corresponding event-group
    isn't registered.
  Carve-outs documented:
  - TraceObjectInfo (msg, sev, ts) tuple flattened to 3 separate
    parameters since upstream's TraceObject type is not yet ported
    (deferred per types.rs's docstring).
  - addNewEvent from Notifications/Utils inlined since Utils.hs port
    lands in a later round; once Utils.hs lands the Rust function
    will be refactored to call the helper.
  Tests: cardano-tracer 75 → 81 (+6: routes Warning to EventWarnings
  queue with full event-payload assertions; routes each high severity
  [Error/Critical/Alert/Emergency] to its correct group via parametric
  loop; drops low-severity events [Info] leaving rx empty checked via
  TryRecvError::Empty; drops Debug/Info/Notice sweep; returns false
  when group is not registered; new_events_senders starts empty).
  Workspace: 5,479 → 5,485. Parity-matrix entry sister-tool.cardano
  -tracer advanced: next_milestone R381 → R382. The Notifications
  subsystem now has Types + Check ported; remaining leaves
  (Settings, Send, Email, Timer, Utils) follow in subsequent rounds.
- **R380 — cardano-tracer: SeverityS synthesis + Notifications/Types.hs
  port.** Lands the trace-event severity ladder + notification-engine
  data-record surface. New severity.rs synthesizes the
  Cardano.Logging.SeverityS enum (the upstream `trace-dispatcher`
  package is not vendored at .reference-haskell-cardano-node/, so this
  is a carved-out synthesis with the variant set recovered from
  upstream's exhaustive pattern matches in
  Journal/Systemd.hs::mkPriority and Notifications/Check.hs):
  - SeverityS enum (8 variants: Debug, Info, Notice, Warning, Error,
    Critical, Alert, Emergency) deriving Serialize+Deserialize +
    Default=Debug. JSON serialization emits each variant's name
    verbatim (matching upstream's Aeson deriving).
  - syslog_code() inherent method returning the RFC 5424 §6.2.1
    numeric severity (Emergency=0, Debug=7).
  - is_notification_worthy() inherent method returning true for
    Warning-and-above (matches Check.hs::checkCommonErrors which
    only adds events for Warning/Error/Critical/Alert/Emergency).
  - Derived Ord follows declaration order (Debug → Emergency, the
    inverse of syslog_code).
  Adds parent-shell modules handlers.rs (mirroring the upstream
  `Cardano.Tracer.Handlers.*` namespace) and
  handlers/notifications.rs (mirroring the upstream
  `Cardano.Tracer.Handlers.Notifications.*` namespace), both with
  `**Strict mirror:** none.` synthesis declarations explaining the
  parent-shell role (no upstream aggregate `Handlers.hs` /
  `Notifications.hs` exists).
  Adds handlers/notifications/types.rs as a 1:1 port of upstream
  Notifications/Types.hs:
  - EmailSSL enum (TLS | STARTTLS | NoSSL, NoSSL default).
  - EmailSettings (8-field SMTP envelope record with upstream JSON
    keys: esSMTPHost / esSMTPPort / esUsername / esPassword / esSSL /
    esEmailFrom / esEmailTo / esSubject).
  - EventsSettings (6-field per-event-group enable+period record:
    evsWarnings / evsErrors / evsCriticals / evsAlerts /
    evsEmergencies / evsNodeDisconnected, each (bool, PeriodInSec)).
  - Event (4-field record: node_id + time_ms + severity + message)
    with constructor.
  - EventGroup enum (6 variants: EventWarnings / EventErrors /
    EventCriticals / EventAlerts / EventEmergencies /
    EventNodeDisconnected) with from_severity(SeverityS) →
    Option<Self> dispatcher returning None for Debug/Info/Notice.
  - EventsQueue type alias (tokio::sync::mpsc::UnboundedReceiver<Event>).
  - Timer placeholder unit struct (full port deferred until
    R382+ when Timer.hs is ported).
  - EventsQueues type alias
    (Arc<RwLock<BTreeMap<EventGroup, (EventsQueue, Timer)>>>).
  - new_events_queues() helper.
  - PeriodInSec u32 type alias (promoted to types.rs from upstream
    Timer.hs).
  Carve-outs documented:
  - Cardano.Logging.SeverityS unported package (severity.rs is the
    synthesis stand-in).
  - Control.Concurrent.STM.TBQueue → tokio::sync::mpsc::UnboundedReceiver.
  - Control.Concurrent.STM.TVar → Arc<RwLock<...>> (same pattern as
    R371 ConnectedNodes).
  - Notifications/Timer.hs full port (112 lines wrapping
    forkIO/killThread/IORef) deferred to a later round; until then
    Timer is a placeholder unit struct.
  Adds tokio workspace dependency to cardano-tracer Cargo.toml.
  Tests: cardano-tracer 56 → 75 (+19: 6 severity tests [default
  Debug, syslog_code RFC 5424, ord declaration-order, is_notification
  _worthy worthy/not-worthy, JSON variant-name + round-trip] + 13
  notification types tests [EmailSSL default + JSON variant-name,
  EmailSettings round-trip + upstream-key fidelity, EventsSettings
  default zeros + upstream-keys, Event constructor, EventGroup
  from_severity for 5 worthy + 3 unworthy + ord declaration-order,
  new_events_queues empty + register-a-group async-tested with
  tokio::test]). Workspace: 5,460 → 5,479. Parity-matrix entry
  sister-tool.cardano-tracer advanced: next_milestone R380 → R381.
  Unblocks Notifications/Check.hs port for R381 (advisor-recommended
  stub-and-build pattern).
- **R379 — cardano-tracer: Time.hs port (EKG epoch-millis helper).**
  Lands the small wall-clock helper used by the cardano-tracer EKG
  metric backend. New time.rs module ports the upstream
  Cardano.Tracer.Time surface verbatim:
  - `get_time_ms() -> i64` mirror of upstream
    `getTimeMs :: IO Int64; getTimeMs = (round . (* 1000)) `fmap`
    getPOSIXTime`. Uses std::time::SystemTime::now().duration_since(
    UNIX_EPOCH).as_millis() with i64 cast for upstream Int64 width
    parity.
  - Upstream's `--` docstring linking to ekg-wai's
    System/Remote/Monitoring/Wai.hs source preserved verbatim in the
    module docstring for context.
  Tests: cardano-tracer 52 → 56 (+4: positive-integer past 1999-12-31
  + sandwich-within-2-seconds-of-system-now + monotonic-within-short-
  window + i64-return-type assertion). Workspace: 5,456 → 5,460.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R372 → R380.
- **R378 — db-synthesizer: Orphans.hs port (JSON deserialization +
  AdjustFilePaths trait).** Lands the JSON-deserialization +
  file-path-adjustment surface used by the db-synthesizer typed config
  types. New orphans.rs module ports the upstream
  Cardano.Tools.DBSynthesizer.Orphans surface:
  - AdjustFilePaths trait mirroring upstream's
    `class AdjustFilePaths a where adjustFilePaths :: (FilePath -> FilePath) -> a -> a`.
    The single `adjust_file_paths<F: Fn(PathBuf) -> PathBuf>` method
    walks every embedded `PathBuf` and returns a new value with the
    transform applied — used to canonicalize relative paths inside a
    parsed node-config JSON against the directory the JSON file
    itself lives in.
  - AdjustFilePaths impl on NodeConfigStub adjusting alonzo /
    shelley / byron / conway / dijkstra (Option) paths.
  - AdjustFilePaths impl on NodeCredentials adjusting cert / VRF /
    KES / bulk (all Option<PathBuf>) paths.
  - Custom serde::Deserialize for NodeConfigStub enforcing upstream's
    `Protocol = Cardano` assertion. Error messages match upstream's
    exact wording ("nodeConfig.Protocol expected: Cardano; found: X").
  - parse_node_config_stub(serde_json::Value) public entry-point
    mirroring upstream's `parseJSON val = withObject "NodeConfigStub"
    (parse' val) val` pattern.
  - NodeConfigStubParseError enum (5 variants: ProtocolMissing,
    ProtocolMismatch(String), RequiredPathMissing { field },
    InvalidPathType { field }, NotAnObject(String)) with
    thiserror::Error derives.
  Carve-outs documented:
  - NodeHardForkProtocolConfiguration + NodeByronProtocolConfiguration
    FromJSON instances are NOT ported. Upstream's own comment marks
    them as DUPLICATE — a re-implementation to avoid an import
    dependency on Cardano.Node.Configuration.POM. Yggdrasil-side
    parallels live in node/src/config.rs. db-synthesizer operates on
    the raw serde_json::Value stashed in
    NodeConfigStub::node_config and feeds that to the runtime layer.
  - The hard-coded Byron application name "cardano-sl" carve-out
    documented for the eventual node-runtime-side
    NodeByronProtocolConfiguration port.
  Tests: db-synthesizer 29 → 42 (+13: complete-stub round-trip,
  missing-Dijkstra, explicit-null-Dijkstra, non-object rejection,
  wrong-Protocol with upstream-exact error wording, missing-Protocol,
  missing-required-genesis, non-string-path, node_config preservation
  on the raw Value, adjust_file_paths for NodeConfigStub
  all-paths-applied + None-Dijkstra pass-through, adjust_file_paths
  for NodeCredentials all-present + all-None pass-through). Workspace:
  5,443 → 5,456. Adds serde workspace dependency to db-synthesizer
  Cargo.toml (was serde_json-only). Parity-matrix entry
  sister-tool.db-synthesizer advanced: next_milestone R365 → R379.
- **R377 — closure-status doc refresh covering R369–R376.** Updates
  the [`docs/PARITY_SUMMARY.md`](docs/PARITY_SUMMARY.md) banner +
  current-implementation-status preamble to reflect the deeper-layer
  sub-arc shipped since the R368 banner. Banner now reads
  "**376+ parity rounds completed**" (was "367+") with workspace-test
  count refreshed from 5,358 → 5,443 (+85 across 8 rounds: +10 R369 +
  4 R370 + 16 R371 + 13 R372 + 9 R373 + 10 R374 + 7 R375 + 17 R376).
  Strict-mirror audit table held stable at 257 (a) + 215 (c) = 472
  graded files (R369–R376 leaves are docstring-graded inline at the
  strict-mirror gate; no audit-table regeneration required).
  Per-round summary inline: R369 dmq-node Configuration.hs::readConfigurationFile
  port; R370 kes-agent-control optFromEnv env-var derivation; R371
  cardano-tracer runtime-state types (NodeId/NodeName/ProtocolsBrake/
  ConnectedNodes/ConnectedNodesNames/Registry/HandleRegistry); R372
  db-analyser CSV writers; R373 db-analyser HasAnalysis trait surface;
  R374 db-analyser BenchmarkLedgerOps SlotDataPoint record; R375
  db-analyser BenchmarkLedgerOps Metadata record; R376 db-analyser
  BenchmarkLedgerOps FileWriting dispatch — closes the
  BenchmarkLedgerOps leaf trio. Refreshed sister-tool partial-status
  inventory: 4 typed-parser-wired tools have deeper-layer extensions
  in flight (dmq-node, kes-agent-control, cardano-tracer, db-analyser);
  3 are typed-parser-wired without deeper extensions yet
  (snapshot-converter, db-synthesizer, cardano-testnet). No code
  changes; doc-only round mirroring the cadence of R346/R352/R357/
  R360/R368 closure refreshes.
- **R376 — db-analyser: BenchmarkLedgerOps file writers (port of
  FileWriting.hs).** Closes the BenchmarkLedgerOps leaf trio
  (SlotDataPoint + Metadata + FileWriting). New
  analysis/benchmark_ledger_ops/file_writing.rs module ports the
  upstream Cardano.Tools.DBAnalyser.Analysis.BenchmarkLedgerOps.FileWriting
  surface that ties together the BenchmarkLedgerOps row data,
  preamble, and csv.rs writers:
  - OutputFormat enum (Csv | Json) with Csv as Default, mirroring
    upstream `data OutputFormat = CSV | JSON`.
  - csv_separator() returning Separator::tab() — module constant
    mirror of upstream `csvSeparator :: TextBuilder; csvSeparator = "\t"`.
  - get_output_format(Option<&Path>, &mut W) — test-friendly variant
    with stderr-sink injection so the dispatch logic is unit-testable
    without spawning a process. Path-extension dispatch: `csv` →
    Csv, `json` → Json, anything else → Csv with a stderr warning
    matching upstream's exact text "Unsupported extension '.X'.
    Defaulting to CSV.".
  - get_output_format_io(Option<&Path>) — IO-driven counterpart that
    writes the warning to std::io::stderr() (matching upstream's
    `IO.hPutStr IO.stderr ...` byte-for-byte).
  - write_header(&mut W, OutputFormat) — Csv emits 15-column header
    via the same builder list as the data rows; Json no-op (matches
    upstream's `writeHeader _ JSON = pure ()`).
  - write_data_point(&mut W, OutputFormat, &SlotDataPoint) — Csv
    emits 14 fixed columns + variable trailing era-specific stats
    columns (the BlockStats list is intercalated with tabs and
    expanded inline, matching upstream's `Builder.intercalate
    csvSeparator . unBlockStats` semantics); Json emits compact JSON
    via serde_json::to_writer with no trailing newline (matches
    upstream's `BSL.hPut h (Aeson.encode x)`).
  - write_metadata(&mut W, OutputFormat, LedgerApplicationMode) —
    Csv no-op; Json emits Metadata::collect()'s output via
    serde_json::to_writer.
  - data_point_csv_builder() returning Vec<DataPointCsvBuilder>
    where `DataPointCsvBuilder = (&'static str, fn(&SlotDataPoint) ->
    String)`. Type alias keeps the function signature within
    workspace clippy::type_complexity bounds. The 15-element list
    preserves upstream's exact column order so any tooling grading
    BenchmarkLedgerOps output by column position continues to work.
  Carve-outs documented:
  - Data.ByteString.Lazy.hPut + Aeson.encode → serde_json::to_writer
    (byte-identical for compact JSON output).
  - System.FilePath.Posix.takeExtension (which returns ".csv" with
    leading dot) → Path::extension() (which strips the dot). The
    warning message re-adds the dot prefix for byte-equivalent
    stderr output.
  Tests: db-analyser 89 → 106 (+17: OutputFormat default + csv_separator
  + 5 get_output_format dispatch variants [None, .csv, .json,
  unsupported with warning, no extension with warning] + write_header
  CSV/JSON [2 tests] + write_data_point CSV/JSON [2 tests] +
  empty-block-stats edge case + write_metadata CSV/JSON [2 tests] +
  data_point_csv_builder column-name order + per-column rendering +
  round-trip header+data sequencing). Workspace: 5,426 → 5,443.
  Parity-matrix entry sister-tool.db-analyser advanced:
  next_milestone R376 → R377. The BenchmarkLedgerOps leaf trio is
  now structurally complete (SlotDataPoint + Metadata + FileWriting);
  the remaining db-analyser work shifts to per-era HasAnalysis
  instances + Analysis.hs dispatch + Run.hs supervisor.
- **R375 — db-analyser: BenchmarkLedgerOps Metadata record (port of
  Metadata.hs).** Lands the run-environment metadata record
  accompanying the BenchmarkLedgerOps JSON output. New
  analysis/benchmark_ledger_ops/metadata.rs module ports the upstream
  Cardano.Tools.DBAnalyser.Analysis.BenchmarkLedgerOps.Metadata
  surface:
  - Metadata 10-field struct (rts_gc_max_stk_size,
    rts_gc_max_heap_size, rts_concurrent_ctxt_switch_time,
    rts_par_n_capabilities, compiler_version, compiler_name,
    operating_system, machine_architecture, git_revison,
    ledger_application_mode). Field-order preserves the upstream
    record-syntax declaration; `#[serde(rename = "...")]` annotations
    preserve upstream's exact JSON key names — including the
    upstream typo `gitRevison` (sic), preserved for byte-equivalent
    output.
  - Metadata::collect(LedgerApplicationMode) constructor mirroring
    upstream's `getMetadata :: LedgerApplicationMode -> IO Metadata`.
  - render_ledger_application_mode(LedgerApplicationMode) helper
    rendering LedgerApply → "full-application" and LedgerReapply →
    "reapplication" (matching upstream's exact strings).
  - rustc_version_string() helper reading option_env!(
    "YGGDRASIL_RUSTC_VERSION") with the rust-toolchain.toml-pinned
    "rustc 1.95.0 (yggdrasil pinned toolchain)" fallback.
  - git_rev_string() helper reading option_env!("YGGDRASIL_GIT_REV")
    with upstream's exact "unavailable (git info missing at build
    time)" fallback for byte-equivalent output.
  - Reuses LedgerApplicationMode from types.rs (R351 surface).
  Carve-outs documented:
  - GHC.RTS.Flags has no Rust analog. The four RTS-flag fields
    (rts_gc_max_stk_size, rts_gc_max_heap_size,
    rts_concurrent_ctxt_switch_time, rts_par_n_capabilities) are
    kept for JSON key-shape parity but zero-populated. Future round
    can wire to crates such as `tikv-jemalloc-ctl` if RSS-pressure
    observability becomes relevant.
  - Cardano.Tools.GitRev.gitRev (TemplateHaskell tGitInfoCwdTry
    splice) → option_env!("YGGDRASIL_GIT_REV") read at build time.
  - Data.Version.showVersion System.Info.compilerVersion (Haskell
    compiler version) → option_env!("YGGDRASIL_RUSTC_VERSION")
    capturing the rustc version at build time.
  Tests: db-analyser 82 → 89 (+7: Metadata default zeroes/empty
  strings + render_ledger_application_mode for both LedgerApply +
  LedgerReapply variants + Metadata::collect() round-trip for both
  modes + camelCase JSON serialization with gitRevison typo
  preserved + field-order preservation across the RTS group).
  Workspace: 5,419 → 5,426. Parity-matrix entry
  sister-tool.db-analyser advanced: next_milestone R375 → R376.
- **R374 — db-analyser: BenchmarkLedgerOps SlotDataPoint record
  (port of SlotDataPoint.hs).** Lands the per-slot timing/allocation
  data-point record fed into the BenchmarkLedgerOps analysis CSV/JSON
  output streams. New analysis/benchmark_ledger_ops/slot_data_point.rs
  module ports the upstream
  Cardano.Tools.DBAnalyser.Analysis.BenchmarkLedgerOps.SlotDataPoint
  surface:
  - SlotDataPoint 15-field struct (slot, slot_gap, total_time, mut_,
    gc, maj_gc_count, min_gc_count, allocated_bytes, mut_forecast,
    mut_header_tick, mut_header_apply, mut_block_tick, mut_block_apply,
    block_byte_size, block_stats). Field-order preserves the upstream
    record-syntax declaration so JSON output emits keys in the same
    order. `#[serde(rename = "...")]` annotations preserve upstream's
    camelCase JSON key names (slotGap, totalTime, majGcCount, etc.).
  - BlockStats(Vec<String>) newtype mirroring upstream
    `newtype BlockStats = BlockStats { unBlockStats :: [TextBuilder] }`.
    `#[serde(transparent)]` ensures byte-identical JSON-array output
    matching upstream's `toJSON . fmap Builder.toText . unBlockStats`.
    Inherent constructors: empty(), from_strings(IntoIterator),
    as_slice(), len(), is_empty().
  - SlotDataPoint::empty(slot) inherent helper for zero-initialized
    construction at a given slot.
  - Adds analysis.rs + analysis/benchmark_ledger_ops.rs parent-shell
    modules with `**Strict mirror:** none.` synthesis declarations.
    Analysis.hs body (1057 lines, 13-variant dispatch) is pending.
    The BenchmarkLedgerOps namespace has no upstream aggregate
    module — it's a directory of three peer leaves — so the shell
    has no .hs counterpart.
  - Adds `serde` + `serde_json` workspace dependencies to db-analyser
    Cargo.toml (mirroring the existing usage in yggdrasil-ledger and
    other crates).
  Carve-outs documented:
  - TextBuilder → String (consistent with csv.rs).
  - Cardano.Slotting.Slot.SlotNo → yggdrasil_ledger::SlotNo (already
    pinned by types::DBAnalyserConfig from R351).
  - Aeson.genericToEncoding → serde_json with `#[derive(Serialize)]`,
    declaration-order-preserving for the field set, with decimal
    integer formatting matching upstream Aeson defaults.
  Tests: db-analyser 72 → 82 (+10: BlockStats default/empty-helper/
  from_strings round-trip + 2 JSON-serialization edges [string array,
  empty array]; SlotDataPoint default zeroes/empty-helper/round-trip-
  through-struct/full JSON with camelCase renames/negative-timing
  serialization). Workspace: 5,409 → 5,419. Parity-matrix entry
  sister-tool.db-analyser advanced: next_milestone R374 → R375.
- **R373 — db-analyser: HasAnalysis trait surface (port of
  HasAnalysis.hs).** Lands the per-block analysis trait that every
  era-specific block must satisfy for the db-analyser dispatch arms
  to operate on it. New has_analysis.rs module ports the upstream
  Cardano.Tools.DBAnalyser.HasAnalysis surface:
  - WithLedgerState<Blk, State> struct mirroring upstream
    `data WithLedgerState blk = WithLedgerState { wlsBlk, wlsStateBefore, wlsStateAfter }`,
    generic over the block type and the ledger-state-with-values
    type (era-specific instantiation deferred to per-era rounds).
  - SizeInBytes type alias (u64) mirroring upstream's `Word32`
    re-export from Ouroboros.Consensus.Storage.Serialisation; widened
    to u64 for headroom on per-tx-size measurements.
  - HasAnalysis trait declaring count_tx_outputs / block_tx_sizes /
    known_ebbs / emit_traces / block_stats / block_application_metrics,
    with associated types HeaderHash / ChainHash / LedgerStateValues.
    The trait is left open (no superclass bounds) — concrete per-era
    implementors will add their own HasAnnTip / GetPrevHash / Condense
    bounds when the era-aware ledger surface is exposed at crate
    boundaries.
  - BlockApplicationMetric<Blk> closure-tuple type mirroring
    upstream's `(TextBuilder, WithLedgerState blk -> IO TextBuilder)`,
    feeding directly into csv.rs's compute_and_write_line_io.
  - HasProtocolInfo trait with associated types Args / ProtocolInfo /
    Error and a make_protocol_info constructor mirroring upstream's
    `class HasProtocolInfo blk where { data Args blk; mkProtocolInfo :: Args blk -> IO (ProtocolInfo blk) }`.
    The data-family Args becomes an associated type; ProtocolInfo
    stays opaque pending the consensus-layer surface exposure.
  Carve-outs documented in module docstring:
  - HasAnnTip / GetPrevHash / Condense (HeaderHash blk) superclass
    constraints — left open, picked up by per-era HasAnalysis
    implementors when era-aware ledger types are exposed.
  - Ouroboros.Consensus.Node.ProtocolInfo blk — collapsed to an
    opaque associated type until the era surface lands.
  - TextBuilder → String, same carve-out as csv.rs.
  Tests: db-analyser 63 → 72 (+9: WithLedgerState round-trip,
  count_tx_outputs / block_tx_sizes / known_ebbs / block_stats trait
  method exercises against a StubBlock, emit_traces state-diff
  rendering, block_application_metrics CSV-emission with positive +
  negative deltas, HasProtocolInfo args pass-through).
  Workspace: 5,400 → 5,409. Parity-matrix entry sister-tool.db-analyser
  advanced: next_milestone R373 → R374.
- **R372 — db-analyser: CSV output writers (port of CSV.hs).**
  Lands the CSV-emission helpers used by db-analyser's
  BenchmarkLedgerOps and GetBlockApplicationMetrics analyses. New
  csv.rs module ports the upstream Cardano.Tools.DBAnalyser.CSV
  surface:
  - Separator(String) newtype mirroring upstream
    `newtype Separator = Separator { unSeparator :: TextBuilder }`
    with `comma()` / `tab()` / `from(impl Into<String>)` constructors.
  - write_header_line(writer, separator, headers): writeHeaderLine.
  - write_line(writer, separator, columns): writeLine.
  - compute_and_write_line_pure(writer, separator, builders, value):
    computeAndWriteLinePure.
  - compute_and_write_line_io(writer, separator, builders, value):
    computeAndWriteLine (Result-based; short-circuits on first
    builder error).
  - compute_columns_pure / compute_columns_io: same Pure / IO
    counterparts for column-only computation.
  - CsvWriteError<E> enum (Builder | Io) for the fallible emit path.
  Carve-out documented:
  - TextBuilder (upstream's `text-builder` crate) replaced by plain
    String. Adequate for the analyzers' output volume (~hundreds of
    thousands of rows max). Higher-throughput backends can be layered
    on without changing the public API.
  Tests: db-analyser 50 → 63 (+13: 4 Separator constructors and
  defaults + 2 write_header_line variants [comma + tab separators] +
  1 write_line emits data row + 1 compute_columns_pure applies
  builders + 1 compute_and_write_line_pure full row + 1
  compute_columns_io short-circuits on first error + 1
  compute_and_write_line_io propagates builder error + 2 edge cases
  [empty columns list, single-column header]). Workspace:
  5,387 → 5,400. Parity-matrix entry sister-tool.db-analyser
  advanced: next_milestone R366 → R373.
- **R371 — cardano-tracer: runtime-state types (port of Types.hs).**
  Lands the runtime-state types for cardano-tracer. New types.rs
  module ports the upstream Cardano.Tracer.Types surface — type
  aliases + newtypes describing the tracer's mutable runtime state:
  - NodeId(String) newtype mirroring upstream NodeId Text.
  - NodeName = String type alias.
  - ProtocolsBrake (Arc<RwLock<bool>>) — stop-signal for protocols
    on the acceptor's side; engage() / is_engaged() inherent methods.
  - ConnectedNodes (Arc<RwLock<HashSet<NodeId>>>) — canonical
    source-of-truth set with insert/remove/contains/snapshot helpers.
  - ConnectedNodesNames — bidirectional NodeId↔NodeName map mirroring
    upstream's `Bimap NodeId NodeName`. Replaces the bimap ecosystem
    crate with two parallel HashMaps (forward + reverse), preserving
    Data.Bimap.insert's replace-both-sides semantic.
  - Registry<Key, Value> generic — Mutex<HashMap<Key, Value>> wrapper
    mirroring upstream `newtype Registry a b = Registry { getRegistry
    :: MVar (Map a b) }`. Used by the logs-handler subsystem.
  - HandleRegistryKey = (NodeName, LoggingParams) type alias.
  - HandleRegistry = Registry<HandleRegistryKey, ((), PathBuf)> —
    open-log-file-handle registry; the upstream System.IO.Handle is
    a `()` placeholder pending the file-rotator round.
  Carve-outs documented:
  - System.Metrics.EKG.Store + MetricsLocalStore: lands when the
    EKG-equivalent metrics aggregation layer is ported.
  - Trace.Forward.Utils.DataPoint.DataPointRequestor: lands when
    the trace-forwarder mini-protocol port is wired.
  - Data.Bimap.Bimap: replaced by ConnectedNodesNames forward+reverse
    HashMap pair (no `bimap` ecosystem dep).
  Tests: cardano-tracer 40 → 52 (+12: NodeId round-trip + ord
  lexicographic; ProtocolsBrake disengaged-default + engage; 4
  ConnectedNodes operations [insert / re-insert returns false /
  remove / snapshot]; 5 ConnectedNodesNames operations [bidirectional-
  lookup / replace-id-clears-old-name / replace-name-clears-old-id /
  remove-id-clears-both-directions]; 1 Registry insert/get/remove +
  1 HandleRegistry round-trip with LoggingParams key). Workspace:
  5,375 → 5,387. Parity-matrix entry sister-tool.cardano-tracer
  advanced: next_milestone R367 → R372.
- **R370 — kes-agent-control: env-var derivation (port of optFromEnv).**
  Lands the environment-variable threading layer for kes-agent-control.
  CommonOptions gains two helpers mirroring upstream's
  `optFromEnv :: IO CommonOptions`:
  - `from_env()` — reads the process environment.
  - `from_env_lookup<F>(lookup)` — test-friendly variant accepting a
    closure for the lookup, useful for unit tests that need to seed
    specific values without mutating the process-wide environment.
  Reads:
  - `KES_AGENT_CONTROL_PATH` → control_path
  - `KES_AGENT_CONTROL_RETRY_INTERVAL` → retry_delay (fails open
    on malformed numbers, matching upstream's `(>>= readMaybe)`)
  - `KES_AGENT_CONTROL_RETRY_ATTEMPTS` → retry_attempts (same
    fail-open behavior)
  Verbosity and retry-exponential are NOT env-derivable upstream;
  those fields stay None.
  lib.rs::run_main() now layers the resolution order:
  1. CLI-derived ProgramOptions (highest priority)
  2. Environment-derived CommonOptions (mid-priority)
  3. CommonOptions::defaults (lowest priority — fills any field
     still unset)
  Resolution wired via:
    let env_options = types::CommonOptions::from_env();
    let resolved_common = env_options.merge(CommonOptions::defaults());
    let program_options = cli_options.with_common_options(resolved_common);
  Tests: kes-agent-control 36 → 43 (+7: 1 from-env-no-vars-set
  all-None + 1 control-path-from-env + 1 retry-interval-from-env +
  1 retry-attempts-from-env + 1 silently-drops-malformed-numbers
  [matches upstream readMaybe semantics] + 1 verbosity-and-retry-
  exponential-not-env-derivable + 1 full-resolution-chain CLI >
  env > defaults). Workspace: 5,368 → 5,375. Parity-matrix entry
  sister-tool.kes-agent-control advanced: next_milestone R363 → R371.
- **R369 — dmq-node: configuration-file load + CLI/file/defaults resolution (port of Configuration.hs::readConfigurationFile).**
  Lands the configuration-file loader for dmq-node. New
  configuration.rs module ports upstream's `readConfigurationFile`
  and the resolution-order helper. PartialConfig + LocalAddress +
  NetworkMagic gain Serialize/Deserialize derives with
  `serde(rename_all = "camelCase")` to match upstream's
  Generic-derived FromJSON field names exactly (hostAddr / hostIpv6Addr
  / portNumber / localAddress / configFile / topologyFile /
  cardanoNodeSocket / cardanoNetworkMagic / networkMagic / showVersion).
  Resolution order (left-priority merge):
  1. CLI-derived PartialConfig (highest priority)
  2. File-derived PartialConfig if cli.config_file is set
  3. Configuration::defaults for any field still unset
  ConfigError variants: Io { path, source } / Parse { path, source }.
  lib.rs::run_main() now invokes resolve_configuration(args) which
  reads the file (if --configuration-file was supplied), merges
  with CLI overrides, and resolves to a fully-applied Configuration.
  Carve-outs documented:
  - mkDiffusionConfiguration (builds upstream
    Ouroboros.Network.Diffusion.DiffusionConfiguration record with
    peer-selection / connection-manager / churn-interval tunables)
    deferred to the diffusion mux wiring round.
  - YAML parsing not yet wired (only JSON); serde_yaml can be
    layered on when an operator workflow needs it.
  Cargo deps: serde + serde_json added.
  Tests: dmq-node 33 → 43 (+10: 2 read-config-file round-trips
  [minimal + full 10-field JSON] + 2 read-config-file error paths
  [missing file → Io / malformed JSON → Parse] + 4 resolve-
  configuration cases [no-file uses cli+defaults / cli-overrides-
  file-overrides-defaults / config-file-missing-error / local-
  address-from-file] + 2 PartialConfig serde behavior [camelCase
  field names + JSON round-trip]). Workspace: 5,358 → 5,368.
  Parity-matrix entry sister-tool.dmq-node advanced: next_milestone
  R362 → R370.
- **R367 — cardano-testnet: typed CLI parser (subcommand dispatch from Parsers/Run.hs::commands).**
  Lands the top-level subcommand dispatch for cardano-testnet,
  replacing the R335 passthrough Args. parser.rs ports upstream's
  `commands :: EnvCli -> Parser CardanoTestnetCommands` — a 4-way
  subcommand recognition layer covering `cardano`, `create-env`,
  `version`, `help`. Each subcommand variant currently carries an
  opaque PassthroughArgs (raw post-subcommand argv tail); the deep
  era-aware option records (CardanoTestnetCliOptions,
  CardanoTestnetCreateEnvOptions, VersionOptions) are carved out
  pending yggdrasil-ledger's era surface being exposed at crate
  boundaries — subsequent rounds will replace PassthroughArgs with
  the typed records. Top-level --help / --version short-circuit
  anywhere in argv (matches upstream's helper combinator behavior).
  ParseError variants: HelpRequested / VersionRequested /
  MissingSubcommand / UnknownSubcommand. Carve-outs documented:
  CardanoTestnetCliOptions/CreateEnvOptions payloads (era-aware
  records depending on Cardano.Api machinery); EnvCli env-var
  threading. lib.rs::run_main() wires parser → Command → run() chain
  end-to-end. lib.rs::run() returns a sentinel reporting which
  subcommand was selected + roadmap pointer.
  Tests: cardano-testnet 22 → 32 (+10: 3 help/version detection +
  1 missing-subcommand + 1 unknown-subcommand + 4 subcommand-
  dispatch verifications [cardano, create-env, version, help] +
  3 passthrough-args round-trips [cardano with flags, create-env
  with flags, version with no args] + 1 help-inside-subcommand-
  window short-circuits + 1 PassthroughArgs default check).
  Workspace: 5,348 → 5,358. Parity-matrix entry sister-tool.cardano-
  testnet advanced: next_milestone R360 → R368.
- **R366 — cardano-tracer: typed CLI parser (port of CLI.hs::parseTracerParams).**
  Lands the typed parser dispatcher for cardano-tracer. parser.rs ports
  upstream's `parseTracerParams :: Parser TracerParams` — a thin
  3-flag CLI shell since the bulk of the operator surface lives in
  the YAML config file (parsed at startup via
  configuration::parse_tracer_config_json).
  Flags:
  - `-c` / `--config FILEPATH` — mandatory; tracer's YAML/JSON config.
  - `--state-dir FILEPATH` — optional; RTView state directory (RTView
    itself is carved out per the plan; the flag is parsed verbatim).
  - `--min-log-severity SEVERITY` — optional; per-message severity
    floor.
  New SeverityS enum (Debug | Info | Notice | Warning | Error |
  Critical | Alert | Emergency) mirroring upstream
  Cardano.Logging.SeverityS — distinct from the existing
  configuration::Verbosity (Minimum | ErrorsOnly | Maximum) which
  controls the tracer's own verbosity rather than per-message
  severity floor. SeverityS::from_str_strict parses the
  upstream-canonical Haskell constructor names case-sensitively
  (matching `option auto`'s Read instance).
  ParseError variants: MissingConfig / InvalidSeverity /
  UnknownFlag / MissingValue / HelpRequested / VersionRequested.
  lib.rs::run_main() wires parser → Args → run() chain end-to-end.
  lib.rs::run() returns a sentinel reporting the resolved config /
  state-dir / min-log-severity + roadmap pointer (config-file load +
  Acceptors/Handlers/Logs/Metrics wiring land in subsequent rounds).
  Tests: cardano-tracer 21 → 40 (+19: 3 help/version + 1 minimal-
  config-only + 1 config-short-form + 1 state-dir + 1 all-8-severity-
  levels [single test that loops] + 2 unknown-severity / case-
  sensitive-rejection + 1 full-canonical + 3 error rejections
  [missing-config / unknown-flag / missing-value] + 2 SeverityS
  default+ordering checks). Workspace: 5,337 → 5,348. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone R359 →
  R367.
- **R365 — db-analyser: typed CLI parser (port of DBAnalyser/Parsers.hs::parseDBAnalyserConfig).**
  Lands the typed parser dispatcher for db-analyser, replacing
  the R335 passthrough Args. parser.rs ports upstream's
  `parseDBAnalyserConfig :: Parser DBAnalyserConfig` and the per-section
  helpers (parseSelectDB / parseValidationPolicy / parseAnalysis /
  parseLimit + per-analysis sub-parsers).

  Mandatory flag: `--db PATH`.

  Optional flags: `--verbose`, `--analyse-from SLOT_NUMBER`
  (default Origin), `--db-validation {validate-all-blocks,
  minimum-block-validation}`, `--num-blocks-to-process INT`
  (default Limit::Unlimited).

  LedgerDB backend (mutually exclusive; one required):
  `--in-mem` (V2InMem) | `--lsm` (V2LSM).

  Analysis-name dispatch (mutually exclusive; default OnlyValidation
  when none supplied — matches upstream's `pure OnlyValidation`
  last-resort branch):
  --show-slot-block-no / --count-tx-outputs / --show-block-header-size
  / --show-block-txs-size / --show-ebbs / --count-blocks /
  --trace-ledger / --store-ledger SLOT [--full-ledger-validation] /
  --checkThunks N / --repro-mempool-and-forge INT /
  --benchmark-ledger-ops [--out-file PATH] [--full-ledger-validation] /
  --get-block-application-metrics N [--out-file PATH].

  Carve-out documented: parseCardanoArgs / CardanoBlockArgs (era-aware
  Byron/Shelley/Cardano block-construction args) deferred until
  yggdrasil-ledger's era surface is exposed at crate boundaries; the
  current parser ignores any era-specific flags and the deeper round
  wires them in alongside per-era HasAnalysis dispatch.

  ParseError variants: MissingDb / MissingLedgerDbBackend /
  ConflictingLedgerDbBackend / ConflictingAnalysisName /
  InvalidDbValidation / UnknownFlag / MissingValue / InvalidValue.

  lib.rs::run_main() wires parser → DBAnalyserConfig → run() chain
  end-to-end. lib.rs::run() returns a sentinel reporting the
  resolved db/analysis/backend/limit + roadmap pointer (per-era
  HasAnalysis + Analysis.hs dispatch land in subsequent rounds).

  Tests: db-analyser 22 → 50 (+28: 3 help/version + 13 analysis-name
  variants [show-slot-block-no, count-tx-outputs, show-block-header-
  size, show-block-txs-size, show-ebbs, count-blocks, trace-ledger,
  store-ledger default-reapply, store-ledger with-full-validation,
  check-thunks, repro-mempool-and-forge, benchmark-ledger-ops with
  and without --out-file/--full-ledger-validation,
  get-block-application-metrics with and without --out-file] + 5
  optional-flag round-trips [verbose, lsm-backend, analyse-from,
  db-validation modes, num-blocks-to-process] + 7 rejection paths
  [missing-db, missing-backend, conflicting-backends, conflicting-
  analysis-flags, invalid-db-validation, unknown-flag, missing-value,
  invalid-slot-value]). Workspace: 5,307 → 5,337. Parity-matrix
  entry sister-tool.db-analyser advanced: next_milestone R352 → R366.
- **R364 — db-synthesizer: typed CLI parser (port of DBSynthesizer/Parsers.hs::parserCommandLine).**
  Lands the typed parser dispatcher for db-synthesizer, replacing
  the R335 passthrough Args. parser.rs ports upstream's
  `parserCommandLine :: Parser (NodeFilePaths, NodeCredentials,
  DBSynthesizerOptions)`. Args struct collapses the upstream tuple
  into a single record. Mandatory flags: --config FILE + --db PATH.
  Mutually-exclusive forge-limit flags: -s/--slots / -b/--blocks /
  -e/--epochs. Mutually-exclusive open-mode flags: -f (force) / -a
  (append); default OpenCreate. Optional credential flags:
  --shelley-operational-certificate, --shelley-vrf-key,
  --shelley-kes-key, --bulk-credentials-file. ParseError variants:
  MissingConfig, MissingDb, MissingForgeLimit,
  ConflictingForgeLimits, ConflictingOpenModes, plus the standard
  HelpRequested/VersionRequested/UnknownFlag/MissingValue/
  InvalidValue. lib.rs::run_main() wires parser → Args → run() chain
  end-to-end. lib.rs::run() returns a sentinel reporting the
  resolved config/db/limit/open-mode + roadmap pointer (Forging.hs +
  Run.hs land in subsequent rounds gated on Phase C entry per the
  plan's Phase C authorization checkpoint).
  Tests: db-synthesizer 16 → 29 (+13: 3 help/version + 1 minimal
  canonical + 2 alternate-forge-limit forms [blocks / epochs short-
  form] + 2 open-mode-overrides [force / append] + 1 all-credentials
  + 4 missing-flag rejections [config / db / forge-limit / conflict
  ing-forge-limits + conflicting-open-modes] + 3 unknown-flag /
  missing-value / invalid-slot-number rejections). Workspace:
  5,294 → 5,307. Parity-matrix entry sister-tool.db-synthesizer
  advanced: next_milestone R355 → R365.
- **R363 — snapshot-converter: typed CLI parser (port of snapshot-converter.hs::parseConfig).**
  Lands the typed parser dispatcher for snapshot-converter, replacing
  the R335 passthrough Args. parser.rs ports upstream's
  `parseConfig :: Parser Config` — the mutually-exclusive daemon-vs-
  oneshot mode dispatch.

  Daemon mode (3 required flags):
  - `--monitor-lsm-snapshots-in PATH` — directory to watch
  - `--lsm-database PATH` — backing LSM database file
  - `--output-mem-snapshots-in PATH` — output directory

  Oneshot mode (input + output, each from mutually-exclusive forms):
  - input: `--input-mem PATH` OR `--input-lsm-snapshot PATH +
    --input-lsm-database PATH`
  - output: `--output-mem PATH` OR `--output-lsm-snapshot PATH +
    --output-lsm-database PATH`

  ParseError variants for the dispatch failure modes:
  MissingMode / ConflictingModes / MissingDaemonFlag /
  MissingOneshotInput / MissingOneshotOutput /
  ConflictingOneshotInput / ConflictingOneshotOutput /
  LsmInputMissingDatabase / LsmOutputMissingDatabase / UnknownFlag /
  MissingValue. lib.rs::run_main() wires parser → Config → run()
  chain end-to-end. lib.rs::run() returns a sentinel reporting which
  mode was selected + roadmap pointer (convertSnapshot LSM/Mem
  conversion logic + filesystem-watcher daemon land in subsequent
  rounds gated on yggdrasil-format LedgerStore reader/writer being
  available).

  Tests: snapshot-converter 23 → 30 (+7 net after the 7 unused-pre-R363
  passthrough tests dropped + 14 new parser unit tests):
  - 3 help/version detection
  - 1 missing-mode (empty argv) + 1 unknown-flag + 1 missing-value
  - 1 daemon canonical 3-flag invocation + 2 daemon-missing-flag rejections
  - 4 oneshot canonical: mem-to-lsm / lsm-to-mem / mem-to-mem /
    lsm-to-lsm
  - 6 oneshot rejection paths: conflicting-modes /
    conflicting-input / conflicting-output /
    lsm-input-missing-database /
    lsm-output-missing-database / missing-output

  Workspace: 5,279 → 5,294. Parity-matrix entry sister-tool.snapshot-
  converter advanced: next_milestone R354 → R364.
- **R362 — kes-agent-control: typed CLI parser (port of ControlMain.hs::pProgramOptions).**
  Lands the typed parser dispatcher for kes-agent-control, replacing
  the R335 passthrough Args. parser.rs ports upstream's
  pProgramOptions + per-subcommand parsers (pCommonOptions /
  pGenKeyOptions / pQueryKeyOptions / pDropStagedKeyOptions /
  pDropKeyOptions / pInstallKeyOptions). Two-pass walk:
  (1) Locate the subcommand keyword (gen-staged-key /
      export-staged-vkey / drop-staged-key / install-key / drop-key /
      info); split argv into before/subcommand/after windows.
  (2) Parse common options from both before- and after- windows
      (filtering per-subcommand flags out of the after-window before
      passing to the common-options parser).
  (3) Apply common-options overrides to the chosen subcommand via
      ProgramOptions::with_common_options.
  Common options dispatched from any position around the subcommand:
  -c / --control-address, -v / --verbose, --retry-interval (alias
  --retry-delay), --retry-exponential (boolean switch),
  --retry-attempts. Per-subcommand options: --kes-vkey for
  gen-staged-key + export-staged-vkey; --op-cert for install-key.
  --help and --version short-circuit at parse time matching upstream's
  helper combinator behavior. lib.rs::run_main() wires parser →
  ProgramOptions → run() chain end-to-end. lib.rs::run() returns a
  sentinel reporting the chosen subcommand + roadmap pointer
  (per-subcommand ControlClient socket I/O lands when the kes-agent
  server mini-arc completes). Tests: kes-agent-control 17 → 36 (+19:
  3 help/version detection + 1 missing-subcommand + 1 unknown-
  subcommand + 6 per-subcommand-minimal parses + 4 common-options-
  before/after/short-form/retry-exponential + 3 retry-interval /
  retry-delay alias / retry-attempts + 3 missing-value /
  invalid-verbosity / unknown-flag rejections + 1 full canonical
  install-key invocation). Workspace: 5,260 → 5,279. Parity-matrix
  entry sister-tool.kes-agent-control advanced: next_milestone R356
  → R363; runtime ControlClient socket I/O still pending kes-agent
  server mini-arc per the per-tool roadmap.
- **R361 — dmq-node: typed CLI parser (port of CLIOptions.hs::parseCLIOptions).**
  Lands the typed parser dispatcher for dmq-node, replacing the R335
  passthrough Args. parser.rs ports upstream's
  `parseCLIOptions :: Parser PartialConfig` — the 10-flag grammar
  (--host-addr, --host-ipv6-addr, -p/--port, --local-socket,
  -c/--configuration-file, -t/--topology-file, --cardano-node-socket,
  --cardano-network-magic, --dmq-network-magic, -v/--version,
  -h/--help) maps each flag to the matching field in
  types::PartialConfig. `--version` is an in-grammar switch (sets
  show_version: Some(true) for downstream dispatch; not a parser
  short-circuit) matching upstream's optparse-applicative behavior;
  `--help` short-circuits via ParseError::HelpRequested. ParseError
  enum gains UnknownFlag/MissingValue/InvalidValue variants for
  the typed-flag dispatch failure modes. lib.rs::run_main() now
  wires the parser → resolve → run() chain: HelpRequested emits
  HELP_TEXT and exits 0; show_version emits VERSION_TEXT and exits
  0; otherwise resolves PartialConfig → Configuration via
  PartialConfig::resolve and hands off to run(&Configuration).
  lib.rs::run() returns a sentinel reporting resolved-config field
  values + roadmap pointer (R362+ for Diffusion/NodeKernel/
  PeerSelection wiring). Tests: dmq-node 14 → 33 (+19: 1 detect-help-
  long + 1 detect-help-short + 2 version-flag-sets-show-version +
  9 individual-flag round-trips + 1 full canonical invocation + 4
  rejection paths [unknown-flag / missing-value / invalid-port /
  invalid-network-magic] + 2 resolve-end-to-end checks). Workspace:
  5,241 → 5,260. Parity-matrix entry sister-tool.dmq-node advanced:
  next_milestone R357 → R362; remaining_work narrowed
  (Configuration.hs port → mux wiring → Diffusion/NodeKernel +
  Tracer + integration + closeout).
- **R359 — cardano-testnet: simple-types port (operator-facing knobs from Start/Types.hs).**
  Lands the simple operator-facing types from upstream's
  Testnet/Start/Types.hs that don't pull in the deeper Cardano.Api /
  Cardano.Ledger.* machinery. New types.rs module ports:
  - DEFAULT_TESTNET_MAGIC = 42 const (matches upstream
    defaultTestnetMagic).
  - NodeId(i32), NumPools(i32), NumRelays(i32), NumDReps(i32) — numeric
    newtypes with Ord/PartialOrd for natural sorting.
  - InputNodeConfigFile(PathBuf) — user-provided node-config file path.
  - UpdateTimestamps (UpdateTimestamps [default] | DontUpdateTimestamps).
  - RpcSupport (RpcDisabled [default] | RpcEnabled).
  - NodeLoggingFormat (AsJson [default] | AsText) with from_string
    mirroring upstream readNodeLoggingFormat (case-insensitive
    "json"/"text" parse, error otherwise).
  - GenesisHashesPolicy (WithHashes [default] | WithoutHashes).
  - PraosCredentialsSource (UseKesKeyFile [default] | UseKesSocket).
  - UserProvidedData<A> generic wrapper (UserProvidedData a |
    NoUserProvidedData [default]) with as_ref + into_option helpers.
  Carve-outs documented in module docstring:
  - Cardano.Api era machinery (cardanoEra / AnyShelleyBasedEra /
    AnyCardanoEra) — port lands when yggdrasil-ledger era surface is
    exposed at crate boundaries.
  - Cardano.Ledger.Alonzo.Genesis.AlonzoGenesis +
    Cardano.Ledger.Conway.Genesis.ConwayGenesis — kept as
    serde_json::Value at this surface; typed parsing happens at
    use-site in yggdrasil-ledger.
  - Hedgehog.MonadTest — carved out per the plan's pre-approved
    Process/Property module carve-out (Rust uses tokio::process +
    proptest equivalents at R363+).
  Tests: cardano-testnet 8 → 22 (+14: 1 default-magic + 4 numeric-
  newtype round-trips + 1 NodeId-Ord + 1 InputNodeConfigFile + 5
  default-impl checks + 3 NodeLoggingFormat::from_string + 2
  UserProvidedData round-trips). Workspace: 5,225 → 5,241.
  Parity-matrix entry sister-tool.cardano-testnet advanced:
  next_milestone R417 → R360; rust_surface description updated to
  reflect R359 simple-types + per-module-roadmap; remaining_work
  refreshed (Filepath/Conf/Process → era-aware records →
  Testnet/Types.hs runtime types → Process/Property carve-outs →
  per-subcommand wiring → integration + closeout).
- **R358 — cardano-tracer: typed configuration surface (port of Configuration.hs).**
  Lands the typed configuration surface for cardano-tracer. New
  configuration.rs module ports the full upstream
  Cardano.Tracer.Configuration surface:
  - TracerConfig (17-field record with serde renames matching
    upstream's Aeson-derived FromJSON field names exactly:
    networkMagic, loRequestNum, ekgRequestFreq, hasEKG,
    hasPrometheus, hasRTView, hasTimeseries, tlsCertificate,
    hasForwarding, logging, rotation, verbosity, metricsNoSuffix,
    metricsHelp, resourceFreq, ekgRequestFull, prometheusLabels).
  - HowToConnect (LocalPipe FilePath | RemoteSocket host port)
    untagged sum mirroring Cardano.Logging.Types.HowToConnect; type
    alias Address = HowToConnect.
  - Endpoint (host + port + optional force_ssl); is_null predicate
    mirroring upstream's nullEndpoint.
  - Certificate (file + key_file + optional chain).
  - RotationParams (frequency_secs / log_limit_bytes /
    max_age_minutes / keep_files_num) with defaults matching
    upstream's hand-written FromJSON: frequency_secs = 60,
    max_age_minutes = 24 * 60.
  - LogMode (FileMode | JournalMode), LogFormat (ForHuman | ForMachine).
  - LoggingParams (root + mode + format) with is_invalid_file_mode
    predicate.
  - Network (AcceptAt accept_at | ConnectTo connect_to) untagged sum.
  - Verbosity (Minimum | ErrorsOnly | Maximum).
  - FileOrMap untagged sum (File PathBuf | Map BTreeMap<String,
    String>).
  - HasForwarding (network + optional path-prefixes + options).
  - TraceOptionForwarder JsonValue placeholder; typed parsing lands
    when the trace-forwarder mini-protocol port is wired.
  - well_formed validator mirroring upstream's wellFormed:
    catMaybes-then-intercalate ', '-shaped problem-list reporting
    AcceptAt-not-empty + ConnectTo-not-all-empty + logRoot-non-empty-
    when-FileMode + duplicate-port-detection + non-empty-hosts on
    hasEKG/hasPrometheus/hasRTView.
  - parse_tracer_config_json runs serde_json + nubLogging dedup +
    well_formed validation.
  Carve-outs documented in module docstring:
  - Network.Wai.Handler.Warp.HostPreference/Port/Settings replaced
    by String/u16 + std::net::SocketAddr at use-sites.
  - Cardano.Logging.Types.TraceOptionForwarder kept as
    serde_json::Value at this layer.
  - readTracerConfig IO-with-die replaced by parse_tracer_config_json
    returning a Result.
  - YAML parsing not yet wired (only JSON); serde_yaml can be added
    when operator-side wiring lands.
  Cargo deps: serde + serde_json added (eyre/thiserror were already
  present).
  Tests: cardano-tracer 8 → 21 (+13: 2 HowToConnect is_null + 1
  Endpoint is_null + 2 RotationParams serde-with-defaults + 2
  LoggingParams invalid-mode + 7 well_formed cases + 2
  parse_tracer_config_json round-trips/error-paths + 4 serde
  round-trips for FileOrMap/Verbosity/LogMode/LogFormat).
  Workspace: 5,202 → 5,225.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R361 → R359; rust_surface description updated to
  reflect R358 + per-module roadmap; remaining_work refreshed
  (Types.hs → CLI.hs → Acceptors → Handlers/Logs → Handlers/Metrics
  → Handlers/Notifications → Run.hs → RTView carve-out → integration
  + closeout).
- **R356 — dmq-node: typed config surface (port of CLIOptions.hs CLI shape).**
  Lands the operator-facing CLI shape for dmq-node. New types.rs
  module ports the partial-vs-resolved Configuration distinction:
  - LocalAddress(PathBuf) newtype (mirrors upstream's
    `LocalAddress` from Configuration.hs).
  - NetworkMagic(u32) newtype.
  - PartialConfig (10-field record, all Option<_>) — represents the
    CLI-derived partial configuration before merging with file-derived
    defaults. Mirrors upstream's `Configuration' Last`.
  - Configuration (10-field record, all concrete) — the fully-resolved
    runtime config. Mirrors upstream's `Configuration' Identity`.
  - PartialConfig::merge: left-priority field-level merge mirroring
    the Haskell Generic-derived Semigroup instance.
  - PartialConfig::resolve: fills in defaults for missing fields,
    yielding a fully-applied Configuration.
  - Configuration::defaults: matches upstream's defaultConfiguration
    (host=0.0.0.0/IPv6=::, port=3001, local_address=dmq-node.socket,
    config_file=dmq-node.json, topology_file=dmq-node-topology.json,
    cardano_node_socket=node.socket, cardano_network_magic=mainnet,
    network_magic=0).
  Carve-outs documented:
  - Generic-derived Semigroup/Monoid via gmappend/gmempty replaced by
    explicit merge() helper (one line per field; same shape as
    kes-agent-control's CommonOptions).
  - Data.Act action-on-types machinery (gpact derivation) replaced by
    straight field-level merging — the action abstraction has no
    semantic role at this surface.
  - Higher-level Configuration.hs functions (mkDiffusionConfiguration,
    readConfigurationFile, etc.) deferred — they touch
    Ouroboros.Network.Diffusion which is a substantial separate port.
  Tests: dmq-node 8 → 14 (+6: 1 LocalAddress round-trip + 1 NetworkMagic
  + 1 PartialConfig default-all-None + 1 merge-left-priority + 1
  defaults-match-upstream + 1 resolve-uses-defaults + 1 round-trip-all-
  supplied + 1 resolve-empty-yields-defaults). Workspace: 5,194 → 5,202.
  Parity-matrix entry sister-tool.dmq-node advanced: next_milestone
  R451 → R357; remaining_work refreshed with the per-module roadmap
  (Parsers → Configuration.hs port → mux wiring → Diffusion/NodeKernel
  → Tracer + integration + closeout).
- **R355 — kes-agent-control: typed config surface (port of ControlMain.hs option types).**
  Lands the typed configuration surface for kes-agent-control. New
  types.rs module ports the full upstream ControlMain.hs option types:
  - CommonOptions (5-field record: control_path, verbosity,
    retry_delay, retry_exponential, retry_attempts; all Option<_>
    so env-var-derived defaults can merge with CLI-flag-derived
    overrides via `merge()` mirror of the Haskell Semigroup instance).
  - GenKeyOptions, QueryKeyOptions, DropStagedKeyOptions,
    DropKeyOptions, InstallKeyOptions per-subcommand records (each
    carries CommonOptions + subcommand-specific paths).
  - ProgramOptions 6-variant sum (RunGenKey / RunQueryKey /
    RunDropStagedKey / RunInstallKey / RunDropKey / RunGetInfo).
  - with_common_options method mirroring upstream's WithCommonOptions
    typeclass + programOptionsWithCommonOptions dispatcher.
  Defaults match upstream's defXyzOptions byte-for-byte:
  control_path = "/tmp/kes-agent-control.socket", verbosity = 0,
  kes_verification_key_file = "kes.vkey", op_cert_file = "kes.vkey"
  (the latter is preserved verbatim from upstream — likely an
  upstream bug since logically it should default to a node.cert
  path; documented as such in the variant docstring).
  Carve-outs documented:
  - Haskell Semigroup instance for CommonOptions / GenKey / etc.
    replaced by explicit `merge()` inherent method (left wins per
    field; first non-None wins).
  - WithCommonOptions typeclass replaced by per-options-struct
    `with_common_options` helpers (each implementation is one-line;
    a trait would be over-engineering).
  Tests: kes-agent-control 8 → 17 (+9: 1 CommonOptions defaults
  match upstream + 1 merge-left-priority + 4 per-subcommand defaults
  + 1 install-key-options upstream-quirk preservation + 2
  ProgramOptions round-trips + 2 with_common_options applications).
  Workspace: 5,183 → 5,194.
  Parity-matrix entry sister-tool.kes-agent-control: rust_surface
  description + implemented_evidence refreshed; next_milestone stays
  at R356 (per-subcommand runtime ControlClient socket logic still
  pending — gated on kes-agent server mini-arc).
- **R354 — db-synthesizer: typed config surface (port of Types.hs).**
  Lands the typed configuration surface for db-synthesizer. New types.rs
  module ports the full upstream Cardano.Tools.DBSynthesizer.Types
  surface:
  - NodeConfigStub (6-field record: node_config, alonzo/shelley/byron/
    conway/dijkstra-genesis-file paths; dijkstra optional).
  - NodeFilePaths (config + chain_db PathBufs).
  - NodeCredentials (cert/VRF/KES/bulk Option<PathBuf> 4-tuple, default
    all None).
  - ForgeLimit (Block u64 | Slot SlotNo | Epoch u64).
  - ForgeResult newtype (forged: i64).
  - DBSynthesizerOpenMode (OpenCreate [default] | OpenCreateForce |
    OpenAppend).
  - DBSynthesizerOptions (limit + open_mode).
  - DBSynthesizerConfig (5-field record: config_stub, options,
    protocol_credentials, shelley_genesis, db_dir).
  Carve-outs documented in module docstring:
  - Aeson.Value → serde_json::Value (untyped JSON storage).
  - ProtocolFilepaths (from Cardano.Node.Types) collapsed to
    NodeCredentials struct of optional paths — db-synthesizer only
    consumes path values, not the typed credential machinery.
  - ShelleyGenesis (from Ouroboros.Consensus.Shelley.Node) kept as
    serde_json::Value at the surface layer; typed parsing happens in
    yggdrasil-ledger's genesis module at use-site.
  Cargo deps: yggdrasil-ledger added (for SlotNo); serde_json added
  (for the untyped JSON fields).
  Tests: db-synthesizer 8 → 16 (+8: 1 NodeConfigStub + 1 NodeFilePaths
  + 1 NodeCredentials default + 3 ForgeLimit variants + 1 ForgeResult
  + 1 default-impl + 1 DBSynthesizerOptions + 1 DBSynthesizerConfig).
  Workspace: 5,173 → 5,183.
  Parity-matrix entry sister-tool.db-synthesizer advanced:
  next_milestone R409 → R355; remaining_work refreshed with the
  per-module roadmap (Parsers → Forging → Run + integration + closeout).
- **R353 — snapshot-converter: typed config surface from CLI shape.**
  Lands the typed configuration surface for snapshot-converter. New
  types.rs module ports the operator-facing data declarations from
  upstream's app/snapshot-converter.hs:
  - SnapshotsDirectory(PathBuf), LsmDatabaseFilePath(PathBuf) PathBuf
    newtypes (re-exported upstream from
    Ouroboros.Consensus.Storage.LedgerDB.Snapshots).
  - StandaloneFormat (Mem [default] | Lsm) — only Mem is currently
    CLI-reachable; Lsm reserved for future parity.
  - SnapshotsDirectoryWithFormat (LsmSnapshot { directory, database })
    used by daemon mode.
  - SnapshotSpec (Standalone { path, format } | Lsm { path, database })
    — renamed from upstream's `Snapshot'` (with prime; Rust does not
    allow apostrophes in identifiers). The upstream `Snapshot`
    (no prime) is a different type defined in the LedgerDB module
    and is carved out — yggdrasil's parser-side surface stops at
    SnapshotSpec; the slot-name parsing + Snapshot pairing lands
    when the conversion logic is ported.
  - Config (Daemon { watch, output } | Oneshot { input, output }).
  Carve-outs documented:
  - convertSnapshot mem↔lsm logic carved-out — operates on upstream's
    ledger-DB on-disk format which yggdrasil does not implement
    (yggdrasil's LedgerStore uses a different on-disk layout under
    data_dir/ledger/). Future round paths: (a) yggdrasil-format ↔
    upstream-mem-format converter (semantic parity); or (b) implement
    upstream LSM/mem readers/writers as separate compat-snapshot crate.
  - Daemon-mode filesystem watcher (withManager / watchTree from
    System.FSNotify) — port-able but needs Rust `notify` crate
    equivalent. Tracked separately.
  Tests: snapshot-converter 8 → 23 (+15: 1 SnapshotsDirectory + 1
  LsmDatabaseFilePath + 1 StandaloneFormat default + 1
  SnapshotsDirectoryWithFormat + 2 SnapshotSpec constructors + 1
  SnapshotSpec path accessor + 2 Config round-trips + 7 implicit
  round-trip checks). Workspace: 5,164 → 5,173.

  Parity-matrix entry sister-tool.snapshot-converter advanced:
  next_milestone R402 → R354; remaining_work refreshed with the
  per-module roadmap (Parsers → Conversion → Daemon → Run +
  integration + closeout).
- **R352 — closure-status refresh for R346-R351 multi-arc work.**
  Updates docs/PARITY_SUMMARY.md Status banner: 345+ → 351+ rounds,
  prepared/updated date 2026-05-10, R347-R350 db-truncater Phase B.1
  + R351 db-analyser Phase B.2 added to closed-arcs list. Workspace
  test count refreshed 5,115 → 5,164. Audit table unchanged.
- **R351 — db-analyser: typed config surface (port of Types.hs).**
  Lands the typed configuration surface for db-analyser. New types.rs
  module ports the full upstream Cardano.Tools.DBAnalyser.Types
  surface: DBAnalyserConfig (7-field record: db_dir, verbose, select_db,
  validation, analysis, conf_limit, ldb_backend), AnalysisName (13-variant
  sum covering every analysis mode the upstream binary exposes —
  ShowSlotBlockNo / CountTxOutputs / ShowBlockHeaderSize /
  ShowBlockTxsSize / ShowEBBs / OnlyValidation / StoreLedgerStateAt /
  CountBlocks / CheckNoThunksEvery / TraceLedgerProcessing /
  BenchmarkLedgerOps / ReproMempoolAndForge / GetBlockApplicationMetrics),
  AnalysisResult (ResultCountBlock | ResultMaxHeaderSize), NumberOfBlocks
  newtype, Limit (Limit u64 | Unlimited), LedgerDBBackend (V2InMem |
  V2LSM), ValidateBlocks (ValidateAllBlocks | MinimumBlockValidation),
  LedgerApplicationMode (LedgerReapply | LedgerApply), SelectDB
  (SelectImmutableDB), WithOrigin<A> (Origin | At a). Reuses
  yggdrasil_ledger::SlotNo. Default impls match upstream's documented
  semantics (LedgerDBBackend=V2InMem, ValidateBlocks=
  MinimumBlockValidation, LedgerApplicationMode=LedgerReapply). Cargo
  deps: yggdrasil-ledger added. db-analyser tests: 8 → 22 (+14: 1
  WithOrigin + 1 SelectDB + 1 Limit + 3 default-impl + 4 AnalysisName
  variants + 2 AnalysisResult + 1 DBAnalyserConfig + 1 NumberOfBlocks
  ord). Workspace: 5,150 → 5,164. Parity-matrix entry
  sister-tool.db-analyser advanced: next_milestone R392 → R352;
  remaining_work refreshed with the per-module roadmap (Parsers →
  HasAnalysis → Analysis → CSV → Run + integration + closeout).
- **R350 — db-truncater: comparison harness for operator soak vs upstream.**
  Ships node/scripts/compare_db_truncater_to_upstream.sh — 200-line
  bash script for verification of yggdrasil-db-truncater against the
  upstream Haskell binary across the canonical surface. Three stages:
  (1) byte-equivalent --help / --version (already pinned by R335
  golden tests; re-checked here as smoke); (2) error-input rejection
  shape parity (missing --db, missing truncate target, conflicting
  truncate targets); (3) post-truncate semantic parity — operator
  supplies an upstream-format ChainDB + a yggdrasil-format ChainDB,
  the script copies both, runs the corresponding binary's truncate,
  and verifies both report completion. Storage-format divergence
  acknowledged: yggdrasil's ChainDB on-disk format diverges from
  upstream's chunked-binary-index layout, so the two binaries
  cannot operate on the same DB; semantic parity (both truncate
  successfully, both report a count) is verified instead. The
  script is operator-runnable, not CI-runnable: it requires the
  vendored upstream binary plus per-format synthesized ChainDBs.
  Promotion of the parity-matrix entry to verified_11_0_1 (R351
  closeout) is gated on operator running this script and reporting
  all stages passed. Stage 3 is skip-able (UPSTREAM_DB / YGGDRASIL_DB
  unset → CLI-only smoke). Parity-matrix entry sister-tool.db-
  truncater advanced: next_milestone R350 → R351.
- **R349 — db-truncater: Run.hs equivalent (functional binary).**
  Lands the operator-facing run procedure for db-truncater. New run.rs
  module mirrors upstream Cardano.Tools.DBTruncater.Run: resolve_target
  maps TruncateAfter::TruncateAfterSlot through verbatim, scans the
  immutable DB for the matching block on TruncateAfter::TruncateAfterBlock
  (errors on BlockNumberNotFound). run_with_store is generic over any
  ImmutableStore impl for unit-testability against InMemoryImmutable;
  run() opens FileImmutable at config.db_dir and delegates. lib.rs::run()
  now calls run::run() and reports `truncated immutable DB at slot N:
  K block(s) removed` to stderr (verbose-mode adds open + resolve
  trace lines). The binary is now end-to-end functional: an operator
  can invoke `db-truncater --db /path --truncate-after-slot N` (or
  --truncate-after-block N) and the on-disk immutable DB is rewound
  to that point. Carve-outs documented: upstream's async ChainDB
  bracket collapsed (FileImmutable is sync); upstream's
  Ouroboros.Consensus.Block.Abstract.Cardano type-level dispatch
  collapsed (Yggdrasil operates on era-tagged CBOR pass-through).
  Cargo deps: tempfile dev-dep added for the tempdir-backed
  FileImmutable smoke test. Tests: db-truncater 22 → 30 (+8: 3
  resolve_target + 4 run_with_store + 1 tempdir-backed FileImmutable
  smoke). Workspace: 5,142 → 5,150.
- **R348 — db-truncater: typed config surface + into_config validation.**
  Lands the typed CLI surface for db-truncater. New types.rs module
  mirrors upstream Cardano.Tools.DBTruncater.Types: DBTruncaterConfig
  (db_dir, truncate_after, verbose) + TruncateAfter (TruncateAfterSlot
  | TruncateAfterBlock) reusing yggdrasil_ledger::SlotNo + BlockNo as
  the canonical workspace types. parser.rs replaces the R335 raw-
  passthrough Args with typed fields (--db PATH, --truncate-after-slot
  SLOT_NUMBER, --truncate-after-block BLOCK_NUMBER, --verbose) +
  into_config(args) validation: errors on MissingDb, MissingTruncate-
  Target (neither truncate flag), or ConflictingTruncateTargets (both).
  lib.rs::run_main now invokes into_config after parsing to surface
  missing-flag errors clearly; lib.rs::run() returns a sentinel
  noting that R349 (Run.hs equivalent) is pending. Cargo deps:
  yggdrasil-ledger + yggdrasil-storage added (the storage dep is for
  R349; included now to keep dep wiring in one round). Tests: db-
  truncater 8 → 22 (+14: 3 types.rs round-trip + 11 parser unit + new
  into_config validation cases). Workspace: 5,126 → 5,142.
- **R347 — storage: ImmutableStore::trim_after_slot extension (db-truncater unblock).**
  Extends the ImmutableStore trait with trim_after_slot — the inverse
  of the existing trim_before_slot GC primitive. Removes all immutable
  blocks with slots strictly after the given slot; blocks at the slot
  or earlier are retained. Implementations on both InMemoryImmutable
  (simple retain) and FileImmutable (full crash-safe variant with
  mark-dirty / delete CBOR + legacy-JSON / mark-clean ceremony,
  mirroring the existing trim_before_slot pattern). ChainDb wrapper:
  truncate_immutable_after_slot delegates to the storage primitive
  with a docstring warning that callers must coordinate volatile +
  ledger cleanup separately. This unblocks Phase B.1 (db-truncater)
  by providing the storage-level truncation primitive that
  db-truncater's Run.hs-equivalent implementation needs at R388+.
  Tests: +11 (7 InMemoryImmutable cases including
  inverse_of_trim_before_slot invariant; 2 FileImmutable cases with
  crash-safe re-open verification; 2 ChainDb cases including
  volatile/ledger isolation contract). Workspace: 5,115 → 5,126.
- **R346 — closure-status refresh for R338-R345 cardano-submit-api Phase A.2 arc.**
  Updates docs/PARITY_SUMMARY.md Status banner: 336+ → 345+ rounds,
  prepared/updated date 2026-05-10, Phase A.2 cardano-submit-api
  implementation arc added to closed-arcs list. Workspace test count
  refreshed 4,982 → 5,115 (+133 in this session). Audit table
  unchanged at 257 (a) + 215 (c) = 472 graded files (R338-R345
  populated already-tracked stub files rather than adding new ones).
  Notes: cardano-submit-api closeout to verified_11_0_1 gated on
  operator running node/scripts/compare_submit_api_to_upstream.sh
  and reporting an empty diff.
- **R345 — cardano-submit-api comparison harness: operator-runnable soak vs upstream.**
  Ships node/scripts/compare_submit_api_to_upstream.sh — 175-line bash
  script that POSTs canonical inputs (empty body, malformed CBOR) to
  both upstream and yggdrasil binaries and diffs HTTP status + response
  body, then scrapes /metrics from both and diffs the # HELP / # TYPE
  shape. Counter line ordering and inner-reason-bytes for malformed-
  CBOR failures are documented as legitimate divergences (counter
  emit-order vs registry-insertion-order; mempool-reason hex bytes vs
  rendered cardano-api Show string). The script is operator-runnable,
  not CI-runnable: it requires a live cardano-node socket + the
  upstream binary, neither available in CI. Promotion of the parity-
  matrix entry to verified_11_0_1 (R346 closeout) is gated on an
  operator running this script and reporting an empty diff. Round-doc
  documents the procedure with a sample bringup (preview testnet) and
  expected output. Parity-matrix entry `sister-tool.cardano-submit-api`
  `next_milestone` advanced R345 → R346.
- **R344 — cardano-submit-api Prometheus metrics: registry, port-retry server, tracer composition.**
  Lands the Prometheus metrics surface for cardano-submit-api,
  mirroring upstream Cardano.TxSubmit.Metrics. New metrics.rs
  module: MetricsRegistry with atomic AtomicU64 counters for
  tx_submit / tx_submit_fail; apply(MetricUpdate[]) +
  observe(TraceSubmitApi) wiring; render_prometheus emits
  `# HELP / # TYPE counter / <name> <value>` shape byte-equivalent
  to upstream serveMetrics; register_metrics_server with port-
  occupied retry up to MAX_PORT_OFFSET=1000 adjacent ports + tracing
  of MetricsServerError/Started/PortOccupied/PortNotBound events
  (matches upstream's "metrics endpoint disabled" semantic if every
  retry fails); spawned per-request tokio task serving
  GET /metrics → 200 OK with text/plain; version=0.0.4 / other → 404.
  web.rs::run_tx_submit_server_from_params now spawns both the HTTP
  server and the metrics server concurrently;
  make_metrics_aware_tracer wraps the operator tracer with registry
  observation so counter updates ride the same trace stream the
  operator's logger sees — no separate counter-bumping path. The
  ApplicationInitializeMetrics event applies a counter zero-set at
  startup matching upstream's forMachine semantic. Carve-outs
  documented: System.Metrics.Prometheus.Http.Scrape.serveMetrics +
  RegistrySample replaced by raw-tokio HTTP + AtomicU64 — no
  prometheus-client ecosystem dependency. Workspace tests:
  5,100 → 5,115 (+15: 13 metrics.rs tests + 2 web.rs
  metrics_aware_tracer tests). Crate total: 133 → 148.
  Parity-matrix entry `sister-tool.cardano-submit-api`
  `next_milestone` advanced R344 → R345.
- **R343 — cardano-submit-api LocalTxSubmission wiring: async Handler + ntc_connect integration.**
  Completes the Phase A.2 web round: the placeholder 503 response from
  R342 is replaced with real NtC LocalTxSubmission integration, and
  lib.rs::run() now spins a tokio runtime + binds + serves until the
  listener exits. The cardano-submit-api binary is now end-to-end
  functional against a real cardano-node socket. Diff inventory:
  Cargo.toml adds `hex` + `yggdrasil-network` deps; rest/web.rs
  refactors `Handler` type alias to `Arc<dyn Fn(HttpRequest) ->
  Pin<Box<dyn Future<Output=HttpResponse> + Send>> + Send + Sync>`
  with sync_handler test helper; web.rs `tx_submit_post` is now async
  and calls new `submit_via_ntc` which opens ntc_connect per request,
  extracts NTC_LOCAL_TX_SUBMISSION ProtocolHandle, drives
  LocalTxSubmissionClient::submit, maps MsgAcceptTx → 202 / MsgRejectTx
  → 400 (TxCmdTxSubmitValidationError) / connect|protocol failure →
  503 (TxCmdTxSubmitConnectionError); MAINNET_NETWORK_MAGIC =
  764824073 constant exposed; lib.rs::run() builds tokio multi-thread
  runtime + Arc tracer forwarding to stderr via render_human + calls
  runtime.block_on(web::run_tx_submit_server_from_params). Carve-outs
  documented: Cardano.Api.deserialiseFromCBOR + multi-era FromSomeType
  table NOT ported (raw bytes pass through to NtC; cardano-node returns
  MsgRejectTx for malformed bytes — equivalent observable behavior);
  Cardano.Api.getTxId NOT ported (Yggdrasil returns empty 'OK' success
  body — operators can compute Blake2b-256 client-side; future
  enhancement). Workspace tests: 5,099 → 5,100 (+1 net). Crate total:
  132 → 133. Parity-matrix entry `sister-tool.cardano-submit-api`
  `next_milestone` advanced R343 → R344.
- **R342 — cardano-submit-api web server: raw-tokio HTTP listener + tx_submit_app dispatch.**
  Lands the HTTP server core for cardano-submit-api. Two production
  modules graduate from R335 stub-only to working web server:
  `rest/web.rs` (HttpRequest, HttpResponse types with response
  constructors for 202/400/404/405/413/503; parse_request scanning
  Content-Length / Content-Type / Transfer-Encoding with chunked-
  rejection + 32 KiB MAX_REQUEST_BYTES cap; encode emitting RFC 7230
  wire format with Connection: close; run_settings TCP listener
  tracing EndpointListeningOnPort + spawning per-connection handlers);
  `web.rs` (run_tx_submit_server outer supervisor mirroring
  runTxSubmitServer; tx_submit_app dispatch closure routing
  POST /api/submit/tx and emitting 404/405 for off-path / wrong-method;
  tx_submit_post placeholder returning 503 with byte-equivalent
  TxSubmitFail JSON for non-empty bodies and 400 TxSubmitEmpty for
  empty bodies). The raw-tokio TCP approach matches the existing
  node/src/metrics_server.rs pattern; NO axum / hyper / tower / warp
  dependency added — just the workspace tokio dep already present.
  Carve-outs documented in strict-mirror docstrings:
  Network.Wai.Handler.Warp.runSettingsSocket / bindPortTCP →
  tokio::net::TcpListener::bind; Servant.Application → Handler type
  alias + path-prefix dispatch; chunked transfer-encoding rejected
  (clients always send Content-Length). Real LocalTxSubmission
  integration deferred to R343 (currently a placeholder body wired
  with byte-equivalent JSON shape so client integrations can be
  tested against this binary now). Workspace tests: 5,076 → 5,099
  (+23: 4 encode + 11 parse + 2 run_settings #[tokio::test] + 6
  web.rs handlers/routing). Crate total: 109 → 132. Parity-matrix
  entry `sister-tool.cardano-submit-api` `next_milestone` advanced
  R342 → R343.
- **R341 — cardano-submit-api trace surface: for_machine, as_metrics, Namespace tables.**
  Completes the trace surface for cardano-submit-api by porting upstream
  `LogFormatting` and `MetaTrace` instance methods. R339 had landed
  the data-only TraceSubmitApi enum + render_human (mirror of
  `forHuman`); R341 adds: `for_machine` (forMachine — JSON shape per
  event byte-equivalent to upstream Aeson), `as_metrics` (asMetrics —
  counter increment instructions), `namespace_for` (namespaceFor — 11-
  variant Namespace enum), Namespace::segments / severity / metrics_doc
  inherent methods (mirror of MetaTrace.segments/severityFor/metricsDocFor),
  ALL_NAMESPACES const (allNamespaces). Three supporting types:
  `Severity` (Debug | Info | Warning | Error mirroring
  Cardano.Logging.SeverityS), `Namespace` (11-variant closed enum),
  `MetricUpdate` (CounterInc | CounterSet mirroring
  Cardano.Logging.MetricM's CounterM name (Maybe v) shape). The Rust
  port intentionally does not implement `LogFormatting` /
  `MetaTrace` typeclasses (no Rust analog under our backend-agnostic
  tracing layer); data is exposed via inherent methods + constants
  so callers can map to whatever tracing backend (`tracing`, `slog`,
  cardano-tracer NtN protocol) is wired at runtime. Workspace tests:
  5,052 → 5,076 (+24: 7 for_machine + 4 as_metrics + 3 namespace +
  8 severity/metrics_doc + 2 MetricUpdate). Parity-matrix entry
  `sister-tool.cardano-submit-api` `next_milestone` advanced R341 →
  R342.
- **R340 — cardano-submit-api type bridges: cli/types, cli/parsers, rest/types, rest/parsers.**
  Bridges Yggdrasil's flat `parser::Args` argv representation to
  upstream's typed parser surface (`TxSubmitCommand`/`TxSubmitNodeParams`).
  Four production modules graduate from R335 stub-only to full upstream
  port: `cli/types.rs` (ConfigFile/GenesisFile/SocketPath PathBuf
  newtypes; ConsensusModeParams Cardano-only enum with `#[default]`;
  NetworkId Mainnet|Testnet sum with From<NetworkMagic> glue;
  TxSubmitNodeParams 6-field record; TxSubmitCommand Run|Version sum);
  `cli/parsers.rs` (`into_command(&Args) → Result<TxSubmitCommand,
  CommandError>` mirroring upstream `pTxSubmit envCli`; per-field
  bridge fns `config_file_from_args` / `socket_path_from_args` /
  `network_id_from_args` / `metrics_port_from_args`; default
  constants `DEFAULT_WEBSERVER_PORT=8090`, `DEFAULT_METRICS_PORT=8081`);
  `rest/types.rs` (`WebserverConfig { host, port }` + `to_socket_addr`
  with wildcard support `*`/`0.0.0.0`/`::` → unspecified IPv4); `rest/
  parsers.rs` (`from_args(&Args, default_port) → WebserverConfig`
  mirroring `pWebserverConfig`). `lib.rs::run()` now validates argv
  → TxSubmitCommand before its sentinel error so missing-flag errors
  surface clearly to operators even before R341 lands the actual
  HTTP listener. Carve-outs documented in strict-mirror docstrings:
  `Cardano.CLI.Environment.EnvCli` (Yggdrasil parser is environment-
  blind), `Options.Applicative.Parser` combinators (centralized in
  `parse_args`), `Warp.HostPreference`/`Warp.Settings` (axum uses
  SocketAddr), `Cardano.Api.SocketPath`'s polymorphic `File 'Out`
  envelope (collapsed to direct PathBuf newtype). Workspace tests:
  5,023 → 5,052 (+29: 7 cli/types.rs + 11 cli/parsers.rs + 7
  rest/types.rs + 4 rest/parsers.rs). Parity-matrix entry
  `sister-tool.cardano-submit-api` `next_milestone` advanced
  R340 → R341.
- **R339 — cardano-submit-api foundations: Types, Util, TraceSubmitApi data enum.**
  Lands the dependency-closed foundation of the cardano-submit-api
  crate ahead of the R340 web round. Three production modules graduate
  from R335 stub-only to full upstream port: `types.rs` (TxSubmitPort,
  RawCborDecodeError, DecoderError, EnvSocketError, TxCmdError,
  TxSubmitWebApiError, render_tx_cmd_error — JSON shapes byte-
  equivalent to upstream Aeson via serde tag/content + transparent +
  untagged); `tracing/trace_submit_api.rs` (data-only TraceSubmitApi
  enum + MediumTxId helper + render_human strings byte-matching
  upstream forHuman); `util.rs` (log_exception generic over
  FnOnce(TraceSubmitApi)). Servant API types (`TxSubmitApi`,
  `TxSubmitApiRecord`, `CBORStream`) carved out under a `**Strict
  mirror:** none.` synthesis docstring with rationale: axum's
  router-based design has no Servant analog; CBOR content-type
  negotiation is handled inline at handler in R340. The
  `LogFormatting`/`MetaTrace`/`forMachine`/`asMetrics` tables for
  TraceSubmitApi are intentionally deferred to R340 alongside web
  round (when trace-receiver wiring is decided). Workspace tests:
  4,982 → 5,023 (+41: 24 in types.rs + 13 in trace_submit_api.rs +
  4 in util.rs). Parity-matrix entry `sister-tool.cardano-submit-api`
  evidence/remaining_work refreshed.
- **R335-R336 — Phase A skeleton milestone: 12/12 sister tools deployable.**
  R335 lands cardano-submit-api skeleton + parser (14 file-mirror
  tree, byte-equivalent --help/--version, 7-flag parser). The bulk
  skeleton commit brings 10 more sister-tool crates from `absent`
  → `partial`: cardano-testnet, cardano-tracer, db-analyser,
  db-synthesizer, db-truncater, dmq-node, kes-agent,
  kes-agent-control, snapshot-converter, tx-generator. Each ships
  lib.rs + main.rs + parser.rs (with `HELP_TEXT`/`VERSION_TEXT`
  fixtures captured from upstream binary) + golden test pinning
  byte-equivalence. R336 round-doc records the milestone:
  **all 12 sister tools have deployable Rust binaries** with
  byte-equivalent --help/--version output, 126 sister-tool tests
  total (31 bech32 + 15 cardano-submit-api + 80 bulk skeleton),
  workspace test count 4,856 → 4,982 (+126).
- **R326-R334 — sister-tools port arc Phase Prep + Phase A.1 (bech32) shipped.**
  Prep block (R326-R330) bootstrapped the 12-tool sister-tools port
  arc: vendored bech32 + kes-agent + dmq-node sources (R326b);
  created 12 sister-tool skeleton crates (R327); extended parity-
  matrix +12 entries + upstream_pins +3 SHAs + drift detector
  cross-org URL support (R328); landed `node/scripts/run-tools.sh`
  12-binary dispatcher (R329); added `bech32 v0.11` workspace dep
  (R330). Phase A.1 (R331-R334) shipped the **first sister tool with
  full deployment-ready 100% parity**: `bech32` is now drop-in
  byte-equivalent to upstream `IntersectMBO/bech32 1.1.10` for every
  documented CLI surface (`--help`, `--version`, base16/bech32/base58
  encoding detection, encode/decode dispatch via stdin). Workspace
  test count: 4,856 → 4,887 (+31 new bech32 tests). Parity-matrix
  entry `sister-tool.bech32` advanced from `absent` to
  `verified_11_0_1`.
- **R321 — closure-status doc triad refresh for R313–R320.**
  Refreshes the four canonical closure-status documents
  (`PARITY_SUMMARY.md`, `PARITY_PROOF.md`, `AGENTS.md` Current
  Phase, `UPSTREAM_PARITY.md`) to reflect the R313–R320 docstring-
  classification cleanup arc. Status banner round count `306+ →
  320+`, test count `4,855 → 4,856`, audit table `230 (a) +
  215 (c) = 445 → 262 (a) + 186 (c) = 448`.
- **R322 — CHANGELOG.md backfill for R303–R321.** The CHANGELOG's
  `[Unreleased]` section's last comprehensive update was R302
  (covering R273–R301). After R302–R321 (20 rounds), R322 adds
  19 new bullets (one per round, ordered chronologically after
  the existing R302 bullet) and refreshes the `[Unreleased]`
  header summary. Without these entries the next tagged release
  would have shipped without mention of operationally-important
  rounds like R310 (gitignore CI failure fix) and R311
  (drift-detection hardening).
- **R323 — eliminate `(a) DIRECT_MIRROR (auto)` bucket via
  explicit declarations.** Closes the gap where the audit grader
  was relying on basename-match heuristic + crate-affinity filter
  alone for 25 files. Each file hand-audited against actual
  content: 17 promoted to canonical strict-mirror declarations
  (`blake2b.rs`, `bls12_381.rs`, `collateral.rs`, era files,
  state-rule files, `bearer.rs`, `root_peers.rs`, storage layer,
  protocol Type files); 8 reclassified to synthesis where the
  basename match was misleading (`secp256k1.rs` aggregating ECDSA
  + Schnorr, `epoch_boundary.rs` cross-era processor not Byron
  block boundary, `cost_model.rs` runtime parameter table not
  Agda metatheory, etc.). After R323, the `(a) auto` sub-bucket
  is empty.
- **R324 — eliminate `(a) DIRECT_MIRROR (auto (affinity-filtered))`
  bucket; audit table now binary.** Closes the last auto-graded
  sub-bucket of 18 files. 10 promoted to canonical strict-mirror
  declarations (Praos `Header.hs` + `VRF.hs`, `Ed25519.hs`,
  `KES/Sum.hs`, `VRF.hs` umbrella, `Alonzo.hs` + `Shelley.hs`
  era files, `LocalStateQuery/Type.hs`, `ChainDB.hs` + `LedgerDB.hs`
  storage). 8 reclassified to synthesis (`kes.rs` aggregator over
  Single + CompactSingle + Simple variants; `cbor.rs` workspace-
  wide helper not Byron-only; node-binary integration files).
  After R324 the audit table has **exactly two canonical
  verdicts**: `(a) declares strict mirror` (246 files) and
  `(c) declares synthesis` (202 files); 448 graded files total;
  zero auto-graded-by-basename. Cumulative R313–R324 closure:
  `(c) unspecified` 41 → 0; `(c) strict-partial` peaked at 17 → 0;
  `(a) auto` 25 → 0; `(a) auto (affinity-filtered)` 18 → 0;
  `(a) declares strict mirror` 187 → 246; `(c) strict-none`
  174 → 202.

## [0.2.0] - 2026-05-01

Public code-level parity closure release for the 2026-Q2 audit cycle.
This release includes the operational-parity arc from Rounds 144 → 245.
Highlights: full cardano-cli 10.16 query parity at preprod (Shelley
era) and preview (Alonzo era), **every Conway-era LSQ subcommand**
(constitution, gov-state, drep-state, drep-stake-distribution,
committee-state, treasury, spo-stake-distribution, proposals,
ratify-state, future-pparams, stake-pool-default-vote,
ledger-peer-snapshot) decoding end-to-end, multi-round
sync-speed and apply-correctness fixes, exact ChainDepState rollback
sidecar recovery, all four preset genesis hashes verified at startup,
and the Conway BBODY `HeaderProtVerTooHigh` testnet grace matched
through Dijkstra.  Workspace tests: **4.7K+ passing, 0 failing**.

### Added

- **R236 — Live `PoolDistr` for `GetStakeDistribution` and
  `GetSPOStakeDistr`**.  `encode_stake_distribution_map` and
  `encode_spo_stake_distribution_for_lsq` now source per-pool
  active stake from `LedgerStateSnapshot::stake_snapshots()`'s
  `set` snapshot (matching upstream `nesPd`).
  `cardano-cli conway query stake-distribution` and `query
  spo-stake-distribution` now render real per-pool data
  post-epoch-rotation; the empty-snapshot fallback (`0x82 0xa0
  0x01`) is preserved for pre-rotation chains.  The
  `IndividualPoolStake` 3-tuple wire shape `[Rational stake_share,
  CompactCoin pool_stake, VRFKeyHash]` matches upstream
  `Cardano.Protocol.TPraos.API.IndividualPoolStake.encCBOR`.
  Closes a Phase A.3 LSQ data-plumbing gap.
- **R238 — ChainDepState rollback sidecar hardening.** Startup,
  rollback, and LocalStateQuery recovery now restore the exact
  slot-indexed nonce/OpCert bundle from `chain_dep_state/<slot>.cbor`
  and replay stored blocks from that point. Persistent non-origin
  rollback fails closed when the required bundle history is missing.
- **R239/R243/R245 — upstream pin refreshes.** All six documentary
  IntersectMBO pins are in sync with live upstream heads; R245 updates
  `cardano-ledger` through the BBODY/GOV drift.
- **R240 — reproducible parallel BlockFetch soak harness.**
  `node/scripts/parallel_blockfetch_soak.sh` captures the §6.5
  multi-peer BlockFetch default-flip evidence path instead of relying
  on hand-assembled operator notes.
- **R242 — optional upstream `cardano-node-tests` wrapper workflow.**
  The workflow is manual-only and documented as an external parity
  harness, not a required CI gate.
- **R244 — Byron genesis hash parity.** `validate-config` now verifies
  Byron genesis files with upstream Canonical JSON hashing while
  Shelley-family genesis files continue to use raw-byte SHA256.
- **R245 — Conway BBODY testnet grace.** `HeaderProtVerTooHigh` is
  enforced on mainnet, suppressed on testnets before Dijkstra
  protocol major 12, and re-enabled on testnets from protocol major
  12 onward.

- **`cardano-cli query` end-to-end parity at preprod (Shelley)
  and preview (Alonzo)**.  All 11 working cardano-cli
  operations — `tip`, `protocol-parameters` (Shelley/Alonzo/
  Babbage/Conway shapes), `era-history`, `slot-number`,
  `utxo --whole-utxo`, `utxo --address X`, `utxo --tx-in T#i`,
  `tx-mempool info` / `next-tx` / `tx-exists`, `submit-tx` —
  decode end-to-end against yggdrasil's NtC socket.
  Verified Rounds 144–164.
- **`YGG_LSQ_ERA_FLOOR=N` env var (Round 178).**  Operator
  opt-in floor on the LSQ-reported era so cardano-cli's
  client-side Babbage+ gate can be bypassed on partial-sync
  chains.  With `YGG_LSQ_ERA_FLOOR=6` the era-gated queries
  (`stake-pools`, `stake-distribution`, `pool-state`,
  `stake-snapshot`, `stake-address-info`) become reachable
  without waiting for the natural Babbage hard-fork.
- **Conway-era LSQ queries (Rounds 180–189) — complete.**
  Every `cardano-cli conway query` subcommand decodes
  end-to-end against yggdrasil:
  `constitution`, `gov-state` (R188, full 7-element
  `ConwayGovState`),
  `drep-state --all-dreps`, `drep-stake-distribution`,
  `committee-state`, `treasury` (via `GetAccountState`),
  `spo-stake-distribution`, `proposals`, `ratify-state` (R187,
  real EnactState with constitution + 31-element PParams +
  treasury), `future-pparams` (R183, `Maybe Nothing`),
  `stake-pool-default-vote`, `ledger-peer-snapshot` (R189,
  V2 form `{"bigLedgerPools": [], "slotNo": "origin",
  "version": 2}`).  Constitution returns real Conway data
  from the chain; the rest return correct empty/placeholder
  shapes for fresh-sync chains.  R184 surfaced a 3-call flow
  inside `query spo-stake-distribution`: SPOStakeDistr (tag
  30) → GetCBOR(GetPoolState) (9→19) →
  GetFilteredVoteDelegatees (tag 28); all three dispatchers
  added in one round.  **The Conway-era LSQ wire-protocol
  gap is now closed entirely.**
- **`yggdrasil_current_era` Prometheus gauge (Round 169)**
  reports the wire era ordinal (`0=Byron, 1=Shelley, …,
  6=Conway`) of the latest applied block.
- **Per-era applied-block counters (Round 170)** —
  `yggdrasil_blocks_byron`, `…_shelley`, `…_allegra`, `…_mary`,
  `…_alonzo`, `…_babbage`, `…_conway` Prometheus counters
  let dashboards graph the share of blocks applied per era
  during a long sync.

### Changed

- **Default `--batch-size` 10 → 30 → 50** (Rounds 165, 166).
  Out-of-the-box preprod sync improves from ~5 blocks/sec at
  the original default to ~14 blocks/sec at the new default
  by amortising per-batch overhead and unblocking the
  initial-sync rollback fast path.  Past 50 the throughput
  plateaus on peer-side fetch latency.
- **Initial-sync rollback fast path** (Round 166) skips the
  heavy `recover_ledger_state_chaindb` replay when the
  rollback target is `Origin` and the base ledger state is
  empty, letting the boundary-aware forward-apply path fire
  epoch transitions correctly.
- **LSQ era-specific tag table re-corrected (Round 179)** —
  R163's tag numbers for `GetStakePools` (was 13, upstream
  is 16), `GetStakePoolParams` (was 14, upstream is 17),
  `GetPoolState` (was 17, upstream is 19), `GetStakeSnapshots`
  (was 18, upstream is 20) are now aligned with cardano-node
  10.7.x's `Ouroboros.Consensus.Shelley.Ledger.Query
  .encodeShelleyQuery`.  Bug masked R163-R178 because
  cardano-cli's client-side era gate refused to send these
  queries.

### Fixed

- **Mid-sync rollback epoch fixup (Round 167)** — when
  `recover_ledger_state` replays the volatile suffix via
  `apply_block` (no boundary detection), `current_epoch` is
  now patched post-recovery to match the recovered tip's
  slot.  Prevents PPUP validation errors on cross-epoch
  rollback.
- **`yggdrasil_active_peers` metric reported 0 during active
  sync** (Round 168).  Bootstrap sync peer is now marked
  `PeerHot` in the registry at session establishment and
  demoted at teardown so `/metrics` reflects the actual
  active session.  Round 175 added cooling at the missed
  `KeepAlive`-failure and session-switching mux-abort sites.
- **Era blockage end-to-end fix (Round 179)**.  Three
  independent bugs unblocked: (1) wrong tag numbers
  (R163-R178); (2) `cardano-cli query stake-distribution`
  uses tag 37 `GetStakeDistribution2` (post-Conway no-VRF
  variant) returning `[map, NonZero Coin]` not bare map;
  (3) `query pool-state` and `query stake-snapshot` use tag
  9 `GetCBOR` wrapper.  All five era-gated queries now
  decode end-to-end against cardano-cli 10.16 with
  `YGG_LSQ_ERA_FLOOR=6`.
- **Decoder strictness (Rounds 174, 176)** — five CBOR
  set-decoder helpers (`decode_pool_hash_set`,
  `decode_stake_credential_set`, `decode_address_set`,
  `decode_txin_set`, `decode_maybe_pool_hash_set`) now
  enforce CIP-21 tag 258 strictly and `Maybe Nothing`
  shortcut requires bare `null` (`0xf6`) rather than any
  CBOR major-7 byte.  Pre-fix malformed payloads were
  silently mis-parsed.
- **`encode_filtered_delegations_and_rewards` correctness
  (Round 177)** — three independent bugs: non-deterministic
  HashSet iteration order, O(N·M) inner search per
  credential, and reward-account lookup mis-matched on hash
  bytes alone (stripping AddrKey-vs-Script discriminator).
  Fixed via sort-then-iterate, `BTreeMap::get`, and
  `find_account_by_credential` (full credential match).
- **`DrepState` LSQ map shape (Round 181)** —
  `GetDRepState` now emits a CBOR map (`encCBOR @(Map a b)`)
  instead of the storage-format array-of-pairs.  cardano-cli
  no longer rejects with `expected map len or indef`.

### Operational notes

- The R178 `YGG_LSQ_ERA_FLOOR` env var is opt-in and
  documented; default behaviour is unchanged.
- The R179 tag-table correction is the major user-visible
  unblocker.  Operators on partial-sync chains (preprod /
  preview before reaching natural Babbage) can now exercise
  the full Conway governance query surface.
- Sync default `--batch-size 50` is safe (boundary-aware
  apply path); legacy operators wanting the old behaviour
  can pass `--batch-size 10` explicitly.


## 0.2.0 candidate checkpoint - 2026-04-27

This was an internal local checkpoint before the public `v0.2.0`
release tag. Later R211→R245 rounds closed the known issue noted in
this checkpoint and are included in the public `v0.2.0` section above.

Operational-parity, byzantine-path, and recovery-correctness release on
top of v0.1.0.  Highlights: full byzantine-path audit closure
(Rounds 87 / 88 / 89), multi-peer BlockFetch dispatch wiring (with
Round 90 closing the session-handoff `RollbackPointNotFound` crash),
zero-copy `Block.raw_cbor` clone via `Arc<[u8]>` (F-2), single-shot
`BlockTxRawSpans` cache shared by the eviction + apply + ledger-advance
consumers (F-1), sealed `ShelleyCompatibleSubmittedTx` /
`AlonzoCompatibleSubmittedTx` invariants (Q-1), `cargo fmt --check`
enforcement in CI, and a self-contained devcontainer that pre-installs
the upstream IntersectMBO Haskell `cardano-cli` + `cardano-node`
binaries for the §5 / §6.5b operator rehearsals.

Workspace tests: 4 210 (v0.1.0) → **4 640 passing, 0 failing**.

The Round 91 multi-peer storage-persistence livelock (Gap BN) remains
open and is documented as a Known Issue below.  The production default
`max_concurrent_block_fetch_peers = 1` keeps the legacy single-peer
path active until that closes.

### Fixed

- **Fee-validation parity bug at the preprod Byron→Shelley boundary
  (slot ~518 460).** Previously `*_block_to_block` in
  `node/src/sync.rs` re-serialised typed `ShelleyTxBody` /
  `ShelleyWitnessSet` to compute `tx_size`, which produced
  byte-canonical CBOR that did not always match the on-wire encoding
  the block author chose (definite vs indefinite length, set vs
  array, integer-width canonicalisation).  The 10-byte drift was
  enough to shift `min_fee = 44 · txSize + 155 381` past the declared
  fee on a real preprod transaction (440 lovelace gap; ~0.2 %).  Fix:
  new helper `yggdrasil_ledger::extract_block_tx_byte_spans` walks
  the outer block CBOR and returns the on-wire byte spans for every
  `transaction_body` / `transaction_witness_set`; the four era
  converters (`shelley`/`alonzo`/`babbage`/`conway`) now take
  `raw_block_bytes: &[u8]` and use those spans for `tx.body`,
  `tx.witnesses`, and `tx_id` hashing.  `TypedSyncStep::RollForward`
  and `MultiEraSyncStep::RollForward` thread raw bytes alongside the
  typed values, sourced from the existing
  `BlockFetchClient::request_range_collect_points_raw_with` API.
  4 new regression tests in `crates/ledger/src/cbor.rs` exercise the
  helper, including a deliberately mismatched indefinite-length-array
  case that proves on-wire byte preservation.  Surfaced in the
  2026-04-27 operational quality-check pass; details in
  [`docs/REAL_PREPROD_POOL_VERIFICATION.md`](docs/REAL_PREPROD_POOL_VERIFICATION.md).

### Changed

- **Submitted-tx invariant hardening (`Q-1`).**  The `raw_body` and
  `raw_cbor` fields on `ShelleyCompatibleSubmittedTx<TxBody>` and
  `AlonzoCompatibleSubmittedTx<TxBody>` were demoted from `pub` to
  `pub(crate)` to prevent external code from mutating `body` and
  silently desyncing the canonical-bytes invariant that `tx_id()` and
  fee `tx_size` rely on.  New public read accessors `raw_body() ->
  &[u8]` and `raw_cbor() -> &[u8]` replace direct field access.
  External code that previously read these fields directly must now
  use the accessors; external constructors (struct literals) must use
  the existing `::new(...)` constructors instead.

- **Authoritative `tx_id` derivation centralised on `raw_body`.**
  `MultiEraSubmittedTx::Shelley` now wraps
  `ShelleyCompatibleSubmittedTx<ShelleyTxBody>` (preserving the on-
  wire `raw_body` / `raw_cbor` byte spans, like every other era arm
  already did), and `MultiEraSubmittedTx::tx_id()` delegates uniformly
  to each variant's `tx.tx_id()`.  Three ledger-side validation sites
  in `crates/ledger/src/state.rs` switched from
  `tx.body.to_cbor_bytes()` / `tx.to_cbor_bytes().len()` to
  `tx.raw_body` / `tx.raw_cbor.len()`, removing one O(n) re-encode +
  alloc per submitted transaction in the mempool admission and apply
  paths.  New regression test
  `shelley_submitted_tx_id_uses_on_wire_bytes_not_re_encoded` in
  `crates/ledger/tests/integration/shelley.rs` decodes a deliberately
  non-canonical Shelley tx (over-long `uint64` for `fee`) and verifies
  `tx_id() == hash(raw_body) ≠ hash(body.to_cbor_bytes())`, locking in
  the on-wire-byte contract against future regressions.

### Performance

- **One-shot `BlockTxRawSpans` cache on `MultiEraSyncStep::RollForward`.**
  Span extraction is now performed exactly once per block at sync-step
  construction (`node::sync::extract_spans_per_block`) and shared by
  all three roll-forward consumers (mempool eviction via
  `extract_tx_ids`, volatile-store apply via
  `apply_multi_era_step_to_volatile`, and ledger-state advance via
  `advance_ledger_state_with_progress`).  Before this change, every
  confirmed block triggered three independent
  `yggdrasil_ledger::extract_block_tx_byte_spans` walks of the same
  CBOR; the cache cuts that to one.  Implementation:
  `MultiEraSyncStep::RollForward` gained a parallel
  `block_spans: Vec<BlockTxRawSpans>` field; new public
  `*_block_to_block_with_spans` variants for Shelley / Alonzo /
  Babbage / Conway / multi-era consume pre-extracted spans;
  `extract_tx_ids` signature changed from `(block, &[u8])` to
  `(block, Option<&BlockTxRawSpans>)`; the closure passed to
  `for_each_roll_forward_block` now receives spans alongside the
  block and raw bytes.  The three Alonzo-family `*_with_spans`
  helpers (60 lines each, identical modulo era tag and typed block
  struct) are generated by a single `alonzo_family_block_to_block_with_spans!`
  macro to keep the duplication-eliminated.  Test count grew by 1
  (the L-1 fixture above); workspace remains green at 4 636 passing.
- **Zero-copy `Block.raw_cbor` cloning (`F-2`).**  `Block.raw_cbor:
  Option<Vec<u8>>` switched to `Option<Arc<[u8]>>`.  Storage's per-
  block clone (volatile-DB `prefix_up_to`, immutable-DB `suffix_after`,
  `chain_db.append_block`) and the per-apply assignment in
  `node/src/sync.rs::apply_multi_era_step_to_volatile` are now atomic
  refcount bumps instead of full ~80 KB heap copies for typical Conway
  blocks.  The BlockFetch trait boundary (`BlockProvider::get_block_range`
  -> `Vec<Vec<u8>>`) still pays one `Arc::to_vec()` at re-serve time,
  so the net win is one fewer alloc per block per re-serve.  On-disk
  CBOR encoding is unchanged: `serde/rc` is now enabled workspace-wide
  and `Arc<[u8]>` serializes to the same RFC 8949 byte-string as
  `Vec<u8>`.  New regression test `block_raw_cbor_arc_serde_round_trip`
  in `crates/storage/tests/integration.rs` locks the byte-equivalence.
- **CI now enforces `cargo fmt --all -- --check`.**  Previously the
  workflow installed `rustfmt` but never ran it; format drift could
  reach `main` undetected.

### Documentation

- **CI-gate prose alignment.**  `CLAUDE.md`, `docs/CONTRIBUTING.md`, and
  `docs/archive/code-audit.md` now list all four CI gates
  (`fmt --all -- --check`, `check-all`, `test-all`, `lint`) — previously
  three files claimed only the trio (`check-all` / `test-all` / `lint`)
  even though `cargo fmt --check` has been a CI step since iteration 1.
- **Arithmetic conventions documented in `crates/ledger/AGENTS.md`.**
  Audit pass over the 164 `saturating_*` call sites across 11 ledger
  files confirmed each is bounded by validated protocol parameters,
  total-ADA-supply caps, or fixed parser depth.  The convention
  (`checked_*` for value-preservation paths surfacing
  `LedgerError::ValueOverflow`; `saturating_*` everywhere the upper
  bound is upstream-enforced) is now codified in the crate AGENTS.md
  with a pointer to the canonical rationale at
  [`crates/ledger/src/fees.rs:14-22`](crates/ledger/src/fees.rs).
- **Round 84 parity-audit-history entry.**  `docs/PARITY_SUMMARY.md`
  records the Q-1 / F-2 closure with anchored upstream references.

### Known Issues

- **§6.5a multi-peer dispatch — `ChainState` advances but `volatile`
  storage stays empty (Round 91 Gap BN, OPEN).**  After Round 90
  closed the hard-crash path, the same §6.5a rehearsal reveals that
  multi-peer dispatch advances the in-memory chain (`from_point` at
  ~slot 102 240) but **persists no blocks to `volatile/` /
  `immutable/` / `ledger/`** — the per-peer `FetchWorkerPool`
  reassembly is not feeding into `apply_multi_era_step_to_volatile`.
  The Round 90 realignment now keeps the node alive across this
  livelock (5 successful handoffs + 0 crashes confirmed on the
  2026-04-27 90-second rehearsal), but the node re-syncs from Origin
  on every session handoff, so it never reaches a stable steady-
  state.  Investigation entry points:
  `node/src/sync.rs::dispatch_range_with_tentative`,
  `execute_multi_peer_blockfetch_plan`, the reorder-buffer →
  apply-step seam.  Production default
  `max_concurrent_block_fetch_peers = 1` MUST stay until this also
  closes.

### Fixed

- **§6.5a multi-peer dispatch — session-handoff `RollbackPointNotFound`
  crash (Round 90 Gap BM).**  With
  `--max-concurrent-block-fetch-peers 2` and ≥ 3 `localRoots`, the
  multi-peer BlockFetch worker pool activates correctly
  (`yggdrasil_blockfetch_workers_registered = 3`,
  `_migrated_total = 3`) but within ~30 s of preprod sync the
  governor's `Net.PeerSelection: switching sync session to
  higher-tip hot peer` path triggered a reconnect, the re-established
  session resumed from `fromPoint=BlockPoint(N, H)`, and
  `roll_backward` on the in-memory `ChainState` returned
  `RollbackPointNotFound { slot: N, hash: H }` — crashing the node.
  Not the Round 88 fresh-restart bug — `ChainState` was the same
  in-memory object across the reconnect loop, but `from_point` had
  advanced past whatever the volatile store actually held (e.g.,
  `from_point` at slot 102 240 vs storage tip at Origin, observed
  live).  Fix: at the top of every reconnect-loop iteration in both
  `run_reconnecting_verified_sync_service_chaindb_inner` and
  `run_reconnecting_verified_sync_service_shared_chaindb_inner`,
  re-seed `chain_state` from the volatile DB AND realign
  `from_point` to `chain_state.tip()` — emitting
  `Net.PeerSelection: realigning from_point to volatile storage tip
  before reconnect` whenever they differ.  This makes the resume
  self-consistent regardless of what diverged in the prior session:
  the next peer's `RollBackward(from_point)` confirmation always
  finds the target.  Verified end-to-end on the 2026-04-27 §6.5a
  rehearsal — 5 realignments handled cleanly + 0 crashes over
  1 m 31 s, was crashing at 30 s pre-fix.  Forensic log:
  `/tmp/ygg-multi-peer-rollback-crash-2026-04-27.log`.

### Added

- **CLI override for `max_concurrent_block_fetch_peers`.**  New
  `--max-concurrent-block-fetch-peers <N>` flag on the `run` subcommand,
  matching the existing override pattern for `--peer`, `--port`,
  `--metrics-port`.  Lets the §6.5 multi-peer BlockFetch rehearsal
  flip the knob without editing the vendored config files; replaces
  the previously-documented (but unimplemented)
  `NODE_CONFIG_OVERRIDE_max_concurrent_block_fetch_peers` env-var
  pattern in the runbook.
- **Devcontainer setup for the full operator-rehearsal toolchain.**
  `.devcontainer/devcontainer.json` now declares the Rust 1.95.0
  feature, common-utils feature, port forwards for `3001` (NtN) +
  `9001/9099/9101` (metrics), VSCode extensions
  (`rust-analyzer`, `vadimcn.vscode-lldb`,
  `tamasfe.even-better-toml`), and a `postCreateCommand` that runs
  `node/scripts/install_haskell_cardano_node.sh` to fetch the
  upstream IntersectMBO Haskell `cardano-node` + `cardano-cli`
  binaries (10.7.1+) into `~/.local/bin/`.  This unblocks the §5
  hash-comparison and §6.5b parallel-fetch parity checks in a fresh
  devcontainer with no manual operator setup.  The installer is
  idempotent — subsequent rebuilds skip the ~217 MB download.

### Fixed

- **Restart-resilience cycle-2 crash: `RollbackPointNotFound` after
  recovery (Round 88 operational parity).**  On node restart,
  `ChainState` was always constructed via `ChainState::new(k)` —
  empty.  The next ChainSync session immediately received
  `RollBackward(recovered_tip)` (the peer's confirmation of the
  resume point) and our `roll_backward` searched the empty `entries`
  vec, returning `RollbackPointNotFound` and crashing the node:

  ```text
  Notice  Node.Recovery       point=BlockPoint(SlotNo(88840), …)
  Notice  ConnectionManager   verified sync session established fromPoint=BlockPoint(SlotNo(88840), …)
  Error   Node.Sync           rollback point not found: slot 88840 …
  ```

  Surfaced by §6 restart-resilience operator rehearsal as a cycle-2
  failure on a real preprod sync.  Fix: new
  `ChainState::seed_from_entries` API + new node-side helper
  `crate::sync::seed_chain_state_from_volatile` that reads the
  volatile DB at restart and seeds the `ChainState` window with the
  most-recent k entries.  Wired into all 5 sync entry points
  (chaindb, shared-chaindb, with-tracer, run_verified_sync_service,
  run_verified_sync_service_chaindb) via a small
  `ChainDbVolatileAccess` trait so both `&mut ChainDb<I, V, L>` and
  `&Arc<RwLock<ChainDb<I, V, L>>>` access modes get the same seed.
  3 unit tests in `crates/consensus/src/chain_state.rs` lock the
  invariant; 3 integration tests in `node/tests/runtime.rs` were
  updated to provide chain-contiguous block-number / prev-hash
  fixtures (they previously relied on the empty-`ChainState` bug to
  bypass the chain validation).

  Reference: upstream `Ouroboros.Consensus.Storage.ChainDB.Init` /
  `getCurrentChain` rebuilds the in-memory chain fragment from the
  volatile DB on start-up.

  End-to-end verification: `node/scripts/restart_resilience.sh`
  with `CYCLES=2` against a real preprod peer now reports
  `[ok] all 2 cycles + final recovery completed monotonic tip
  progression`.

- **Vendored `peer-snapshot.json` placeholders for mainnet + preview
  (operator preflight).**  Both `node/configuration/mainnet/topology.json`
  and `node/configuration/preview/topology.json` referenced
  `peerSnapshotFile: "peer-snapshot.json"` but the actual files were
  missing, so `validate-config --network mainnet|preview` reported
  `peer_snapshot.status = "unavailable"` with a "could not be loaded"
  warning out of the box.  Vendored placeholder files matching the
  preprod skeleton (slot=0, single bootstrap-pool entry per network);
  preflight now reports `peer_snapshot.status = "loaded"` for all
  three networks.

### Security

- **Byzantine-path closures (Round 87 parity audit).**  Two upstream
  `Word8` / size-bound parity gaps fixed:
  - **PeerSharing amount cap.**  `MsgShareRequest` carries the
    requested amount as `u16` on our wire (HandshakeVersion-bound),
    but upstream `Ouroboros.Network.PeerSelection.PeerSharing`
    transports it as `Word8` (max 255).  Our
    `SharedPeerSharingProvider::shareable_peers` previously honoured
    the full `u16` range, so a malicious peer requesting `u16::MAX`
    forced the provider to walk the entire registry per request.
    Fixed: cap at `PEER_SHARING_MAX_AMOUNT = 255` BEFORE the registry
    walk in `node/src/server.rs`, plus a regression test
    `shared_peer_sharing_provider_clamps_to_upstream_word8_max` that
    populates 300 peers and asserts `u16::MAX` requests return ≤ 255.
  - **LocalTxSubmission decode-byte ceiling.**  The NtC
    `LocalTxSubmission` server in `node/src/local_server.rs` accepted
    arbitrary CBOR `tx_bytes` and only rejected oversized payloads
    AFTER the full mempool admission decode + `validate_max_tx_size`
    check (mainnet `max_tx_size = 16 384 B` Conway PV 10).  A
    malicious local client could submit a multi-MB well-formed-but-
    oversized CBOR blob and force the allocation before rejection.
    Fixed: explicit `LOCAL_TX_SUBMIT_MAX_BYTES = 64 KiB` ceiling at
    the wire boundary (~4× the protocol max for headroom), reject
    with structured reason before any decode.
- **Code audit C-1/H-1/H-2 + M-1..M-8 + L-1..L-9 closed.**  See
  [`docs/archive/code-audit.md`](docs/archive/code-audit.md) for the source audit;
  remediation summary:
  - **C-1 / H-1** — every CBOR decoder that allocates from a
    peer-supplied `count` field now goes through
    `vec_with_safe_capacity` (soft cap) or `vec_with_strict_capacity`
    (hard cap) defined in [`crates/ledger/src/cbor.rs`](crates/ledger/src/cbor.rs);
    per-protocol bounds live in
    [`crates/network/src/protocol_size_limits.rs`](crates/network/src/protocol_size_limits.rs).
    Fixes a pre-auth remote DoS via `Vec::with_capacity(u64::MAX)`.
  - **H-2** — `PeerListener::accept_peer` split into `accept_tcp` +
    `handshake_on` with a 5 s `HANDSHAKE_DEADLINE`.  Inbound rate-
    limit decision now runs **before** the handshake, so a hard-limit
    rejection costs only a TCP accept.
  - **M-1** — mux ingress-queue limit checked **before** the per-frame
    payload allocation in [`crates/network/src/mux.rs`](crates/network/src/mux.rs).
  - **M-3** — NtC Unix socket bound at `0o660` (was `0o755` from
    default umask) in [`node/src/local_server.rs`](node/src/local_server.rs).
  - **M-6 / L-8 / L-9** — value-preservation arithmetic in
    [`crates/ledger/src/utxo.rs`](crates/ledger/src/utxo.rs) now uses
    `checked_add` (new `LedgerError::ValueOverflow`); plutus
    `ExBudget::spend` uses `checked_sub`; mempool capacity arithmetic
    uses `checked_add`.  Closes the silent saturating-on-overflow
    path that diverged from upstream Haskell `Integer` arithmetic.
  - **M-8** — genesis-hash gate hard-fails on unpaired
    `(genesis-file, declared-hash)` in
    [`node/src/config.rs`](node/src/config.rs); previously a missing
    `*GenesisHash` skipped verification silently.
  - **L-6** — KES/VRF/cold key files rejected unless
    `mode & 0o077 == 0` in [`node/src/block_producer.rs`](node/src/block_producer.rs).
  - **M-4 / M-5** — `serde_yaml` (advisory-db #2132) and `serde_yml`
    (RUSTSEC-2025-0068) replaced with `serde_norway = "0.9"`;
    trace-forwarder migrated from `serde_cbor 0.11` (RUSTSEC-2021-0127)
    to `ciborium 0.2`.  `serde_cbor` retained transitionally for
    storage on-disk format only, ignored in `deny.toml` with rationale.
  - **L-4** — `cargo deny check` runs in CI on every push and PR.
  - **L-1 / L-2 / L-7** — release verification + maintainer signing
    sections in [`SECURITY.md`](SECURITY.md); `restart_resilience.sh`
    now uses `mktemp -d` + ephemeral ports so concurrent runs don't
    collide.

### Changed

- **Toolchain bumped from Rust 1.85.0 → 1.95.0** ([rust-toolchain.toml](rust-toolchain.toml),
  workspace `rust-version`).  All new 1.95 clippy lints are clean
  (`manual_is_multiple_of`, `manual_div_ceil`, `manual_abs_diff`,
  `manual_contains`, `manual_ok`, `cloned_ref_to_slice_refs`,
  `unnecessary_sort_by`, `useless_vec`, `single_match_else`,
  `manual_while_let_some`, `derivable_impls`, `doc_overindented_list_items`,
  `doc_list_items_indentation`).  Stylistic-bulk lints
  (`collapsible_if`, `result_large_err`, `large_enum_variant`)
  explicitly carried forward as `allow` in
  [`Cargo.toml`](Cargo.toml) `[workspace.lints.clippy]` with
  documented rationale.

- **Docs site converted to dark-only mode** with the YggdrasilNode
  branding.  `docs/_sass/color_schemes/yggdrasil.scss` is a
  self-contained dark scheme (no fragile `@import "./dark"` that
  broke under `remote_theme:`); `docs/_sass/custom/custom.scss`
  design tokens and per-component backgrounds rebound to
  dark-friendly values; the YggdrasilNode banner appears as a
  landing-page hero via `docs/_includes/header_custom.html` (gated
  by `hero: true` front-matter).  Sidebar logo wired via
  `_config.yml` `logo:`; favicon and Open Graph image set in
  `docs/_includes/head_custom.html`.

### Tests

- **4 634 passing, 0 failing** (was 4 630).  `+4` from the new
  `extract_block_tx_byte_spans_*` regression tests in
  [`crates/ledger/src/cbor.rs`](crates/ledger/src/cbor.rs).

## [0.1.0] — 2026-04-27

### Yggdrasil 1.0 closure

First feature-complete release after the 2026-Q2 parity audit. Every
confirmed-active parity slice is closed; every runtime integration
originally tracked as a follow-up has landed.

### Operator deliverables

- Documentation site published at <https://yggdrasil-node.github.io/Cardano-node/>
  with the user manual (install, configure, run, monitor, troubleshoot,
  block production, releases) and reference docs.
- Release workflow that builds Linux x86_64 + aarch64 binaries on `v*` tag
  push, computes SHA256 checksums, and publishes a GitHub Release.
- `Dockerfile` + `docker-compose.yml` + `.dockerignore` for container
  deployments.
- Operator scripts: `install_from_release.sh` (with build-from-source
  fallback), `healthcheck.sh`, `backup_db.sh`, `restart_resilience.sh`,
  `compare_tip_to_haskell.sh`, `check_upstream_drift.sh`, plus a
  systemd unit template.
- Issue templates, PR template, CODEOWNERS, dependabot config (with
  RustCrypto digest-ecosystem grouping).
- `SECURITY.md` with vulnerability disclosure policy.
- Operator-facing Prometheus metric names normalized across the manual,
  runbook, healthcheck, restart-resilience and pool-producer scripts:
  `yggdrasil_current_block_number`, `yggdrasil_reconnects`,
  `yggdrasil_rollbacks`, `yggdrasil_stable_blocks_promoted`,
  `yggdrasil_batches_completed`, `yggdrasil_mempool_tx_added`,
  `yggdrasil_mempool_tx_rejected`, `yggdrasil_inbound_connections_accepted`,
  `yggdrasil_inbound_connections_rejected`, `yggdrasil_active_peers`,
  `yggdrasil_blocks_synced`, `yggdrasil_current_slot`.

### Closure cycle slices

- **Slice B** — CDDL parser range constraints (`N..M`, `.le`, `.ge`,
  `.lt`, `.gt`, `.size N..M`).
- **Slice D** — `HotPeerScheduling` per-mini-protocol weight table
  mirroring upstream `Ouroboros.Network.PeerSelection.Governor.HotPeers`.
- **Slice E (foundation)** — `effective_block_fetch_concurrency` +
  `partition_fetch_range_across_peers` + `BlockFetchAssignment`
  primitives.
- **Slice GD** — genesis density tracking primitive
  (`crates/consensus/src/genesis_density.rs::DensityWindow`,
  `DEFAULT_SLOT_WINDOW = 6480`, `DEFAULT_LOW_DENSITY_THRESHOLD = 0.6`).
- **Slice GD-RT** — ChainSync header density observation hook
  (`DensityRegistry`).
- **Slice GD-Governor** — density-biased hot demotion in `PeerMetrics`.
- **Slice GD-Final** — runtime data flow unifying the density seam.
- **Slice D-Scheduler** — `HotPeerScheduling`-driven mux egress weights.
- **Slice E-Dispatch** — `execute_multi_peer_blockfetch_plan`
  parallel executor with `tokio::JoinSet` + `ReorderBuffer`.
- **Slice E-Tentative** — `dispatch_range_with_tentative` consensus-
  correctness contract.
- **Slice E-Phase6-Seam** — `OutboundPeerManager` hot-peer accessors.
- **Slice E-Inline** — non-spawning multi-peer dispatcher
  (`execute_multi_peer_blockfetch_plan_inline`).
- **Slice E-Workers** — per-peer fetch worker primitive
  (`FetchWorkerHandle`, `FetchWorkerPool`) mirroring upstream
  `Ouroboros.Network.BlockFetch.ClientRegistry`.
- **Slice E-Production-Spawn** —
  `FetchWorkerHandle::spawn_with_block_fetch_client` wiring real
  `BlockFetchClient` into a worker.
- **Slice E-Migration** — `PeerSession.block_fetch: Option<...>` plus
  `migrate_session_to_worker` / `unregister_worker`.
- **Slice E-Wire** — sync-loop multi-peer dispatch branch +
  `MultiPeerDispatchContext`.
- **Slice E-Promote** — governor migrates `BlockFetchClient` on
  `promote_to_warm` when the operator knob is `> 1`.
- **Phase 6 observability** — Prometheus counters
  `yggdrasil_blockfetch_workers_registered` (gauge) and
  `yggdrasil_blockfetch_workers_migrated_total` (counter).

### Operator surface

- `max_concurrent_block_fetch_peers` config knob (default `1`,
  flippable to `2` after §6.5 rehearsal).
- §6.5 parallel-fetch rehearsal added to the manual test runbook.

### Test count

- 4,630 tests passing across the workspace, 0 failing (post-v0.1.0
  the count rose to 4,634 with the fee-validation regression tests
  added in the next cycle).
- All four gates clean: `cargo check-all`, `cargo test-all`,
  `cargo lint`, `cargo doc --workspace --no-deps`.

[Unreleased]: https://github.com/yggdrasil-node/Cardano-node/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/yggdrasil-node/Cardano-node/releases/tag/v0.2.0
[0.1.0]: https://github.com/yggdrasil-node/Cardano-node/releases/tag/v0.1.0
