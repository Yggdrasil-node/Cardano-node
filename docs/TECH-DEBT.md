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

- ✅ **4 drifted flags removed** (Phase 5 follow-on): the Phase 5.4
  audit found `yggdrasil-consensus/experimental-genesis`,
  `yggdrasil-network/serde-traces`, and
  `yggdrasil-storage/{lmdb, mem-only}` had drifted premises — each
  gated zero `#[cfg]` sites and could not be meaningfully wired
  (genesis-density tracking became load-bearing in the verified
  Praos path; the network crate has no trace-type `serde` derives;
  the storage backends are both always-compiled and additive, not
  a compile-time choice). Re-scope outcome: the dead declarations
  were removed rather than carried as decorative flags.

- ✅ **`yggdrasil-plutus/{secp256k1, bls12-381}`** (verified 2026-05-17):
  both already gate real `#[cfg]` sites — the per-builtin dispatch arms
  in `crates/plutus/src/builtins.rs` carry `#[cfg(feature = "...")]` with
  `#[cfg(not(feature = "..."))]` fallback-error paths, the crypto helper
  fns and the builtin tests are gated, and `cargo check` /
  `cargo lint-no-default` both pass under `--no-default-features`. This
  entry previously listed them as pending; that was stale.

- ✅ Genuinely-inert flags — all removed. `yggdrasil-network/ntn` (R591),
  `yggdrasil-ledger/plutus` (R592), and `yggdrasil-network/ntc` (R770)
  each carried 0 `#[cfg]` sites. Wiring `ntc` would have scattered
  `#[cfg(feature = "ntc")]` across the `yggdrasil-network` NtC module
  tree, the `yggdrasil-node-ntc-server` crate, and `cardano-cli`'s
  LocalStateQuery surface — `cargo lint-no-default` builds the whole
  workspace with `--no-default-features`, so a partial gating breaks
  it — and the payoff is only a niche relay-only build that omits the
  local socket. Removal matched the R591/R592 inert-flag precedent and
  the "no decorative feature flags" rule. No inert feature flags
  remain in the workspace. The binary declares only `forge` /
  `relay-only`; the earlier mention of `yggdrasil-node/{plutus,
  ntc-socket, tracer-forwarder}` flags here was aspirational — they do
  not exist.

**Desired end state.** Each flag actually conditionally compiles
the code paths it names. Per-flag follow-on PRs land
incrementally as operator demand surfaces (e.g., a sister tool
that needs `--no-default-features --features=slim` to drop
Plutus would drive the `plutus` flag work).

**Scope.** Per-flag PRs. `forge`, `secp256k1`, and `bls12-381` are
closed (wired); the 4 drifted flags and the 3 genuinely-inert flags
(`ntn` R591, `yggdrasil-ledger/plutus` R592, `ntc` R770) are closed by
removal. No feature-flag debt remains.

## Tracking conventions

- New tech-debt entries follow this header structure: **Owner**,
  **State today**, **Desired end state**, **Scope**.
- An entry is closed (deleted from this file) when the
  consolidation PR lands.
- Issues that are blocking ship — broken tests, security bugs,
  parity-matrix violations — DO NOT belong here; they go to
  Issues / SECURITY.md / the strict-mirror gate.
