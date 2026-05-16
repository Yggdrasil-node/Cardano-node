# yggdrasil-node-tracer — node tracer + metrics + cardano-tracer forwarder

## Scope

Extracted from `yggdrasil-node` in Wave 5 PR 7 (alongside config /
genesis) so the trace + metrics surface is consumable by sister tools
and Wave 6 observability scaffolding without linking the runtime.

The crate ships:

- `NodeTracer` — the tracing dispatch entry point used by every
  runtime sub-loop (sync, mempool, block-producer, governor, …).
- `NodeMetrics` + `MetricsSnapshot` + `MetricsSnapshot::to_prometheus_text` —
  the atomic-counter store and Prometheus-text serializer that
  Wave 6 PR 16 swaps to `metrics-exporter-prometheus`.
- `trace_fields` — the field-name constants source-of-truth
  (mirrored by `yggdrasil-telemetry::trace_fields` in Wave 2).
- `metrics_server` — the raw-TCP HTTP server on the operator-
  configurable `--metrics-port` (Wave 6 PR 16 replaces this with
  `PrometheusBuilder::with_http_listener`).
- `trace_forwarder` — the cardano-tracer Unix-socket forwarder.
  Phase 2.B (Wave 6 PR 17 / R502) has now landed nine of the
  eleven sub-items: Layer 1 codecs (encoder + decoder in
  `trace_forwarder.rs`), Layer 2 TraceForward CBOR codec
  (`mini_protocol.rs`), Layer 3 SDU codec (`mux.rs`), AF_UNIX
  bearer (`bearer.rs`), `tracing::Event`→`TraceObject` builder
  (`event_builder.rs`), write-only forwarding task
  (`forwarding_task.rs`), `tracing-subscriber::Layer<S>` adapter
  (`layer.rs`), Handshake mini-protocol codec (`handshake.rs`),
  Handshake initiator state-machine driver
  (`handshake_driver.rs`). Remaining: bidirectional Mux state-
  machine driver (ingress/egress scheduler for per-mini-protocol
  Request/Reply pacing — distinct from the one-shot Handshake
  driver) and live conformance test against the upstream
  `cardano-tracer` binary.

## Rules — Non-Negotiable

- **Tier-1 stability for trace_fields + EKG-parity metric names.**
  Operators key off these. See `docs/COMPATIBILITY.md`.
- **No runtime dependency on sibling node crates** (sync, mempool,
  block-producer, runtime). The tracer must be addable from any
  sub-loop without re-introducing the coupling Wave 5 broke.
- **Depends on yggdrasil-node-config only for NodeConfigFile /
  TraceNamespaceConfig.** Adding a deeper config dependency
  re-introduces the monolithic coupling.

## Naming parity

The lib.rs (former `node/src/tracer.rs`) carries the parity stanza.
`metrics_server.rs` and `trace_forwarder.rs` are synthesis (no
upstream mirror); the `## Naming parity` blocks in those files
declare so.

## R-arc tracking

Wave 5 PR 7 (extracted). Wave 6 PR 14-17 (R502) refactors the
metrics_server + trace_forwarder surfaces to use `metrics-exporter-
prometheus` and finish the cardano-tracer Mux Layer 2/3 protocol.

### Phase 2.B follow-on commits (cardano-tracer Mux Layer 2/3)

- `3d78362` — Layer 2/3 codecs (mini_protocol.rs + mux.rs).
- `01cdc53` — fix SDU direction-bit inversion in mux.rs (advisor-
  caught; encoder and decoder both had Initiator/Responder swapped
  vs the implementation in upstream `Network.Mux.Codec.hs`).
- `f0bc5a9` — `TraceObject` Layer-1 CBOR decoder + non-empty
  `MsgTraceObjectsReply` round-trip.
- `ee7d496` — AF_UNIX `Bearer<S>` adapter.
- `5464f8a` — `tracing::Event` → `TraceObject` builder.
- `02f7ce0` — write-only forwarding task that batches into
  `MsgTraceObjectsReply` SDUs.
- `92fc2df` — `TraceForwardingLayer` (`tracing_subscriber::Layer<S>`).
- `fe9c520` — Handshake mini-protocol codec (Propose / Reply /
  Accept / Refuse + all three RefuseReason variants).
- `c868f73` — Handshake initiator state-machine driver
  (`run_initiator_handshake`).

Remaining pre-`verified_11_0_1`: bidirectional Mux state-machine
driver (multi-day; the ingress/egress scheduler that runs every
mini-protocol concurrently on a shared bearer) and operator
conformance soak against
`.reference-haskell-cardano-node/install/bin/cardano-tracer`
(needs `setup-reference.sh` without `--sources-only`).

### Live conformance tests — `tests/cardano_tracer_conformance.rs`

- `handshake_completes_against_upstream_cardano_tracer` — GREEN.
  Spawns the vendored `cardano-tracer 11.0.1` and drives
  `MuxConnection::run_initiator_handshake` (Mux mini-protocol 0).
- `trace_objects_delivered_to_upstream_cardano_tracer` — `#[ignore]`d
  (task #19 outcome b, documented parity gap). Drives the
  TraceForward mini-protocol (num 2) end-to-end via
  `forwarding_task::run_via_mux`. Run live, cardano-tracer accepts
  the `MsgTraceObjectsReply` SDU but logs 0 bytes — its `runPeer`
  decoder silently rejects the reply. Two upstream-CBOR shape
  mismatches were pinned from on-wire capture and are recorded in
  the test docstring:
  1. **`NumberOfTraceObjects` codec bug** in `mini_protocol.rs`
     (`encode_request` / `decode_message`): the upstream
     `newtype { nTraceObjects :: Word16 }` with
     `deriving anyclass Serialise` is generic-wrapped as
     `[constructor_tag(0), word16]`, NOT a bare CBOR uint. Captured
     request: `83 01 f5 82 00 18 64` = `[1, true, [0,100]]`.
     **Separately actionable** — fix the request codec.
  2. **`Serialise TraceObject` shape unverifiable**: the instance
     lives in `Cardano.Logging.Types` in the `trace-dispatcher`
     package, which is NOT vendored under
     `.reference-haskell-cardano-node/`. Yggdrasil's `to_cbor`
     8-element array is a best-effort guess. UNBLOCK: vendor
     `trace-dispatcher` (extend `setup-reference.sh`), mirror the
     real `Serialise` instance, fix `TraceObject::to_cbor`, then
     un-`ignore` the test.

  Both tests self-skip when the binary is absent; CI stays green
  (`#[ignore]` skips the delivery test under `cargo test-all`).
