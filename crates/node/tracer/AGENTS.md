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
  Phase 2.B (Wave 6 PR 17 / R502) has landed: Layer 1 codecs
  (encoder + decoder in `trace_forwarder.rs`), Layer 2 TraceForward
  CBOR codec (`mini_protocol.rs`), Layer 3 SDU codec (`mux.rs`),
  AF_UNIX bearer (`bearer.rs`), `tracing::Event`→`TraceObject`
  builder (`event_builder.rs`), write-only forwarding task
  (`forwarding_task.rs`), `tracing-subscriber::Layer<S>` adapter
  (`layer.rs`), Handshake mini-protocol codec (`handshake.rs`),
  Handshake initiator state-machine driver (`handshake_driver.rs`),
  and `MuxConnection` (`mux_connection.rs`). Both live conformance
  tests against the upstream `cardano-tracer 11.0.1` binary are
  GREEN — the handshake and the full TraceObject-delivery pipeline
  (see "Live conformance tests" below). Remaining: a fully
  bidirectional Mux state-machine driver (ingress/egress scheduler
  for per-mini-protocol Request/Reply pacing — the current
  forwarding task is write-only and works against a request-
  tolerant acceptor like cardano-tracer).

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
- `trace_objects_delivered_to_upstream_cardano_tracer` — GREEN
  (task #19 closeout, outcome a). Drives the TraceForward
  mini-protocol (num 2) end-to-end via
  `forwarding_task::run_via_mux`: cardano-tracer accepts the
  `MsgTraceObjectsReply` SDU and writes every forwarded
  TraceObject's `to_machine` text into its `FileMode` log. It was
  un-`ignore`d once three trace-forward CBOR codecs were corrected
  to the upstream `Codec.Serialise` byte shapes (read from the
  now-vendored `trace-dispatcher` source — see below).

  Both tests self-skip when the binary is absent (override with
  `YGGDRASIL_CARDANO_TRACER_BIN`); CI's `--sources-only` reference
  tree has no `install/bin/`, so the live tests skip there while
  staying green.

### trace-forward CBOR codec — upstream byte shapes (task #19 closeout)

The trace-forward codecs are wired with `Codec.Serialise`'s generic
`encode`/`decode`. Three shapes were corrected to match upstream
byte-for-byte; all are pinned by unit tests + the live conformance
test:

1. **`NumberOfTraceObjects`** (`mini_protocol.rs`) — the upstream
   `newtype { nTraceObjects :: Word16 }` with
   `deriving anyclass Serialise` is generic-encoded by `cborg`'s
   `GSerialiseEncode (K1 i a)` as the 2-element array
   `[0, word16]`, NOT a bare CBOR uint. On-wire confirmation:
   cardano-tracer's `MsgTraceObjectsRequest` for 100 objects is
   `83 01 f5 82 00 18 64` = `[1, true, [0, 100]]`.
2. **`TraceObject`** (`trace_forwarder.rs`) — the 8-field
   single-constructor record (`Cardano.Logging.Types.TraceObject`,
   `deriving anyclass Serialise`) is generic-encoded by `cborg`'s
   `GSerialiseEncode (f :*: g)` as a **9-element** array
   `[0, …8 fields…]` (constructor tag `0` then the fields). Field
   shapes: `Maybe a` → `[]`/`[x]`; `[Text]` → `array(0)` (empty) or
   indefinite list `0x9f…0xff` (non-empty); `SeverityS`/
   `DetailLevel` → nullary-sum `[idx]`; `UTCTime` → extended-time
   `tag(1000) {1: secs, -12: psecs}`. `TraceObject::to_timestamp`
   is therefore `(posix_seconds, picoseconds_of_second)`.
3. **Reply list** (`mini_protocol.rs::encode_reply`) — the
   `MsgTraceObjectsReply` list is `Serialise [TraceObject]`;
   `cborg`'s `defaultEncodeList` encodes a non-empty list as the
   indefinite-length form `0x9f…0xff` (an empty list stays the
   definite `array(0)`).

Upstream source for the codec shapes is vendored at
`.reference-haskell-cardano-node/deps/hermod-tracing/trace-dispatcher/src/Cardano/Logging/Types.hs`
(the `trace-dispatcher` package — extracted from `cardano-node`
into the standalone `IntersectMBO/hermod-tracing` repo as of
`trace-dispatcher 2.12.x`); the generic-`Serialise` instances are
in `well-typed/cborg`'s `Codec/Serialise/Class.hs`.
