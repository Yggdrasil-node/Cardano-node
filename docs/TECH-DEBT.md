# Technical debt tracker

Operator-visible items that work today but want consolidation. Each
entry names the owning subsystem, the current state, the desired
end state, and the rough scope of the consolidation PR.

## EKG-parity metrics: field-mapping consolidated (formerly dual paths)

**Owner:** observability (Wave 6 PR 16 follow-on consolidation)

**State today.** The 110-line field-mapping table now lives in ONE
place — `yggdrasil_metrics::render_ekg_parity_prometheus_text` —
via the `yggdrasil_metrics::EkgParitySource` trait. The binary's
`MetricsSnapshot::to_ekg_parity_prometheus_text` is a one-line
delegation to that function plus an `impl EkgParitySource for
MetricsSnapshot` block (ten 1:1 field accessors). The
`/metrics` route's concatenation behaviour is unchanged from the
operator perspective; only the rendering ownership moved.

**Remaining secondary debt.** `yggdrasil_metrics::install_prometheus_exporter`
still spawns its own HTTP listener (originally meant as a parallel
Prometheus scrape endpoint). It currently has NO live consumer in
the binary — `serve_metrics` owns the port. Sister tools (cardano-
tracer, future cardano-submit-api) that want their own scrape
endpoint can call `install_prometheus_exporter`; the binary
continues to use the `render_ekg_parity_prometheus_text` path so
both legacy `yggdrasil_*` and EKG names appear on the same port.

**Desired end state for the secondary item.** The binary's
`NodeMetrics` update sites call `metrics::*` macros so values flow
through the global `metrics` facade. `install_prometheus_exporter`
no longer binds its own port (operator chooses the port via
`--metrics-port`). `to_ekg_parity_prometheus_text` becomes a thin
adapter over `handle.render()`. Histogram-shaped metrics
(`blockProcessingTime`) become real Prometheus histograms.

**Scope of the remaining item.** ~30 `NodeMetrics` update sites in
`yggdrasil_node_tracer` either bridged via a tick-loop (cheaper)
or rewritten as direct `metrics::*` calls (cleaner). Best done
once a sister tool actually consumes the `install_prometheus_exporter`
surface so the integration has a real driver.

## cardano-tracer Mux Layer 2/3 (R502)

**Owner:** observability (Wave 6 PR 17)

**State today.** **11 of 12 sub-items landed across Phase 2.B
(May 2026 session, 12+ commits).** The forwarder-side
pipeline is fully wireable in ~10 binary-setup lines (see the
example block at the top of
`crates/node/tracer/src/trace_forwarder.rs`).

Shipped components:

- **Layer 1** (TraceObject CBOR codec): encoder
  (`trace_forwarder.rs::TraceObject::to_cbor`) + decoder
  (`trace_forwarder.rs::TraceObject::from_cbor_bytes` —
  commit `f0bc5a9`).
- **Layer 2** (TraceForward mini-protocol CBOR codec):
  `mini_protocol.rs` — `MsgTraceObjectsRequest` /
  `MsgTraceObjectsReply` / `MsgDone` + full round-trip pinned
  in `nonempty_reply_round_trip`.
- **Layer 3** (Network.Mux SDU codec): `mux.rs` — 8-byte
  big-endian header (timestamp + dir-and-protonum + length) +
  mini-protocol number constants (Handshake=0, EKG=1,
  TraceObject=2, DataPoint=3).
- **AF_UNIX SOCK_STREAM bearer** (`bearer.rs`, commit
  `ee7d496`): `Bearer<S>` generic over any
  `tokio::io::AsyncRead + AsyncWrite + Unpin + Send`
  transport, with `read_sdu` / `write_sdu` + 4 round-trip
  tests pinned against `tokio::io::DuplexStream`.
- **Handshake mini-protocol codec** (`handshake.rs`, commit
  `fe9c520`): `MsgProposeVersions` / `MsgReplyVersions` /
  `MsgAcceptVersion` / `MsgRefuse` + all three RefuseReason
  variants.
- **Handshake initiator state-machine driver**
  (`handshake_driver.rs`, commit `c868f73`):
  `run_initiator_handshake(&mut bearer, versions)` performs
  the Idle→Confirm→Done flow with structured error variants.
- **`tracing::Event` → `TraceObject` builder**
  (`event_builder.rs`, commit `5464f8a`): pure-Rust
  civil-date arithmetic, Level→SeverityS mapping, field-set
  JSON serialisation.
- **Write-only forwarding task** (`forwarding_task.rs`,
  commit `02f7ce0`): `run` / `run_via_mux` drain a tokio
  mpsc::UnboundedReceiver<TraceObject> and batch-emit
  `MsgTraceObjectsReply` SDUs.
- **`tracing_subscriber::Layer<S>` adapter** (`layer.rs`,
  commit `92fc2df`): `TraceForwardingLayer::new(tx, hostname)`
  bridges sync `on_event` callbacks to the async forwarding
  pipeline.
- **Minimal bidirectional Mux dispatcher**
  (`mux_connection.rs`, commit `30111c5`):
  `MuxConnection<S>` wraps `Bearer<S>` with per-mini-protocol
  `mpsc` subscription channels + a serialised write side.
- **Composition glue** (`MuxConnection::run_initiator_handshake`,
  commit `5a8b662`; `forwarding_task::run_via_mux`, commit
  `0d40fb6`): handshake_driver and forwarding_task can both
  ride on a shared `Arc<MuxConnection<S>>` without exclusivity.

**Only remaining sub-item:**

- Full Network.Mux semantics: per-mini-protocol ingress/egress
  queue limits, scheduler fairness (round-robin among ready
  writers, weighted-priority for hot protocols), and
  bearer-task supervision (cohesive shutdown when any
  sub-task fails). Today's `MuxConnection` serialises through
  a single bearer mutex; sufficient for the cardano-tracer
  use case (Handshake first, then TraceObject forwarding
  until shutdown), but doesn't match upstream Network.Mux
  semantics under concurrent activity.
- Conformance test against the vendored
  `.reference-haskell-cardano-node/install/bin/cardano-tracer`
  binary — needs `scripts/setup-reference.sh` without
  `--sources-only` so the install tarball materialises.

**Desired end state.** Verified_11_0_1 promotion of the new
`node.tracer.cardano-tracer-forwarder` parity-matrix entry
(`docs/parity-matrix.json`, added in commit `8e79bc7`). Gated
on the full-Mux work + a 24h operator soak forwarding live
mainnet/preprod traces to a real cardano-tracer endpoint
without protocol errors.

**Scope.** Multi-day for full Mux semantics; the conformance
soak is operator-driven work.

## Wave 3 / Wave 5 feature flags: per-flag gating progress

**Owner:** packaging (Wave 3 PR 5, Wave 5 PR 7+, Phase 5)

**State today.**

- ✅ **`forge`** (the canonical first target): fully gated as of
  Phase 5.1 (commits `d526613` runtime side + `ba7119d` binary
  side, May 2026). `cargo build --no-default-features
  --features=relay-only` produces a smaller binary that excludes
  the `yggdrasil-node-block-producer` crate from the dep graph
  entirely. The runtime's `forge` feature gates `forge.rs` +
  `block_producer_loop.rs` + the `block_producer_ledger_state_judgement`
  helper; the binary's `forge` feature pulls in the runtime's
  `forge` plus the block-producer crate as a direct optional
  dep; `ntc-server` / `ntn-server` consume runtime with the
  workspace-default `forge` OFF so they don't pull the
  block-producer crate transitively.

- ⏳ Other declared-but-not-gating flags: `yggdrasil-ledger/plutus`,
  `yggdrasil-consensus/experimental-genesis`,
  `yggdrasil-network/{ntn, ntc, serde-traces}`,
  `yggdrasil-storage/{lmdb, mem-only}`,
  `yggdrasil-plutus/{secp256k1, bls12-381}`,
  binary's `yggdrasil-node/{plutus, ntc-socket, tracer-forwarder}`.
  Each is a separate per-flag PR. `plutus` is the next-biggest
  individual scope (gating the Alonzo+ phase-2 witness paths in
  per-era ledger apply rules); `ntn` / `ntc` gate full mini-
  protocol module trees in `yggdrasil-network`; `secp256k1` /
  `bls12-381` gate per-builtin dispatch arms in
  `yggdrasil-plutus::builtins`. `serde-traces` is decorative
  today (no trace-type serde derives currently in the network
  crate); a follow-on round will surface derives on the trace
  types newly added under `crates/network/src/protocols/`.

**Desired end state.** Each flag actually conditionally compiles
the code paths it names. Per-flag follow-on PRs land
incrementally as operator demand surfaces (e.g., a sister tool
that needs `--no-default-features --features=slim` to drop
Plutus would drive the `plutus` flag work).

**Scope.** Per-flag PRs. `forge` is closed; per remaining flag:
~1-3 days each depending on the cross-crate coupling. `plutus`
is multi-day because Plutus types are referenced from ~8 ledger
files including era-specific apply rules.

## yggdrasil-cardano-cli — 3-subcommand surface closed, broader migration is the open arc

**Owner:** packaging (sister-tools layout)

**State today (R506 closure, May 2026).** All three subcommands that
the library's `Command` enum exposes are now operationally wired in
the standalone `yggdrasil-cardano-cli` binary. `cargo install --path
crates/tools/cardano-cli` produces a feature-complete binary; the
slim build (`--no-default-features`) trades the LSQ-touching
`query-tip` capability for a smaller dep footprint.

Operator-visible coverage today (3 of 3 surface commands ✅):

- ✅ `yggdrasil-cardano-cli version` (R296 helpers, R503 dispatcher) —
  emits the canonical `helper::version_info()` banner.
- ✅ `yggdrasil-cardano-cli --version` / `--help` — clap-standard
  output to stdout, exit 0.
- ✅ `yggdrasil-cardano-cli show-upstream-config --network <preset>`
  (R297 helpers, R504 dispatcher) — resolves config + topology
  paths against the vendored
  `crates/node/yggdrasil-node/configuration/<preset>/` tree (or
  operator-supplied `--upstream-config-root`), extracts the
  network magic from `config.json` (or falls back to the
  well-known constant per preset), emits operator JSON
  byte-equivalent to the node binary's wrapper.
- ✅ `yggdrasil-cardano-cli query-tip --socket-path <socket>
  [--network-magic <magic>]` (R505 trait, R506 concrete impl) —
  default build (`lsq-tokio` feature on) opens a Unix-socket NtC
  connection via `yggdrasil_network::ntc_connect`, drives the
  LocalStateQuery mini-protocol to acquire VolatileTip + send
  the CBOR `[3]` `GetChainPoint` query, prints the JSON envelope
  the node binary's wrapper already emits. Slim build
  (`--no-default-features`) falls back to the structured
  deferral message pointing at `yggdrasil-node cardano-cli
  query-tip`.

**Remaining (the actual open arc).** Migrating the broader
cardano-cli subcommand surface (~150 upstream `.hs` files, ~30
operator-essential subcommands beyond the current 3) into the
library so `yggdrasil-cardano-cli <any_subcommand>` works
standalone. The node binary's `cardano-cli` subcommand group
(35 commands) is the parity reference; the C-arc migration
tracked in
`crates/tools/cardano-cli/AGENTS.md`'s "Migration roadmap
(R298+ deferred)" section is the multi-week port plan.

**Scope.** Per-subcommand follow-on rounds, each ~1-3 days
depending on what auxiliary deps the subcommand brings (tokio +
yggdrasil-network for socket-touching commands — already gated
behind the `lsq-tokio` feature post-R506; bech32 for address
commands; yggdrasil-crypto for key-management commands; full
tx-builder primitives for `transaction build` /
`transaction build-raw`).

## cardano-submit-api validation error: structured mapping (Phase 1 — raw-bytes carrier landed)

**Owner:** sister-tools (Wave 6 PR 18 / cardano-submit-api web round, Phase 4.A)

**State today.** `TxCmdError::TxCmdTxSubmitValidationError` now carries
a `TxSubmitValidationError` struct (in
`crates/tools/cardano-submit-api/src/types.rs`) holding BOTH the raw
CBOR-encoded era-specific `ApplyTxError` payload AND a string
rendering. The custom `Serialize` impl on `TxSubmitValidationError`
emits only the rendered string so the upstream JSON wire shape
(`{"tag":"...","contents":"<rendered>"}`) stays byte-equivalent.
The `LocalTxSubmissionClientError::TransactionRejected(reason)` path
in `web.rs` now plumbs the raw reject bytes through.

**Remaining work** (Phase 2 — structured-enum decoder, deferred):
the rendered string is still a hex-dump of the reject bytes; the
structured `ApplyTxError` enum mirroring upstream's per-era variant
sum (`FeeTooSmall`, `ValueNotConservedUTxO`, `OutsideValidityInterval`,
`BadInputsUTxO`, `OutputTooSmall`, `WrongNetwork`, …; multiplied by
6 eras Shelley→Conway) is not yet built. Once it lands, the
renderer becomes a `Display` impl on the structured form and
operators can pattern-match on individual rejection variants
without a CBOR re-walk.

**Scope of Phase 2.** ~400 lines of new enum (per-era ApplyTxError
sum + UtxoErr/UtxowErr/DelegsErr sub-sums + Conway governance
errors), the per-era CBOR decoders that map the wire shape to
the typed enum, and the `Display` impl that re-renders. Best
landed when a sister tool actually needs to pattern-match (e.g.,
tx-generator surfacing typed rejection reasons in its 1-hour TPS
soak summary).

**Tracking.** Also referenced from `docs/parity-matrix.json` under
the cardano-submit-api entry's `next_milestone` field; the doc-
comment in `types.rs` points here.

## Tracking conventions

- New tech-debt entries follow this header structure: **Owner**,
  **State today**, **Desired end state**, **Scope**.
- An entry is closed (deleted from this file) when the
  consolidation PR lands.
- Issues that are blocking ship — broken tests, security bugs,
  parity-matrix violations — DO NOT belong here; they go to
  Issues / SECURITY.md / the strict-mirror gate.
