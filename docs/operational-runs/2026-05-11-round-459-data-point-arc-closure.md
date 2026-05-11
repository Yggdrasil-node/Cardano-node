---
title: 'R459 closeout: cardano-tracer DataPoint sub-protocol R452-R459 arc complete'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-459-data-point-arc-closure/
---

# R459 — DataPoint sub-protocol arc closure

**Date:** 2026-05-11
**Predecessor:** [`R430 closeout`](2026-05-10-round-430-r411-r430-closure.md).
**Closure scope:** R452-R458 fully shipped; R459 = closeout (AGENTS.md
refresh, parity-matrix evidence, this operational-runs doc).

## Arc summary

R452-R459 shipped 8 rounds porting the upstream trace-forward
DataPoint sub-protocol from
`.reference-haskell-cardano-node/trace-forward/src/Trace/Forward/`
into Yggdrasil's `crates/network/` and integrating it into
`crates/tools/cardano-tracer/src/acceptors/`.

Per-round delivery:

| Round | Source mirror | Yggdrasil delivery |
|-------|---------------|--------------------|
| R452  | `Protocol/DataPoint/Type.hs` | `crates/network/src/protocols/data_point_forward.rs` (Type half) |
| R453  | `Protocol/DataPoint/Codec.hs` | (same file — Codec half + wire-byte-stable tests) |
| R454  | `Protocol/DataPoint/Acceptor.hs` | `crates/network/src/data_point_acceptor.rs` (DataPointAcceptor driver) |
| R455  | `Configuration/DataPoint.hs` | `crates/network/src/protocols/data_point_forward_configuration.rs` |
| R456  | `Utils/DataPoint.hs` (acceptor-side subset) | `crates/network/src/protocols/data_point_forward_utils.rs` (DataPointRequestor) |
| R457  | `Run/DataPoint/Acceptor.hs` | `crates/network/src/data_point_run_acceptor.rs` |
| R458  | `cardano-tracer/Acceptors/{Server, Client, Utils}.hs` integration | `crates/tools/cardano-tracer/src/acceptors/{server, client, utils}.rs` updates |
| R459  | (closeout) | this doc + parity-matrix + AGENTS.md refresh |

**Workspace tests:** 5,962 → 6,024 (+62 across 8 rounds).
**Verification gates:** all five clean at HEAD (`cargo fmt`,
`cargo check-all`, `cargo test-all`, `cargo lint`,
`scripts/check-strict-mirror.py --fail-on-violation`).
**Deferral status descriptors closed:**
- `crates/tools/cardano-tracer/src/acceptors/utils.rs::prepare_data_point_requestor_status`
  — R423 deferral closed (real function shipped).
- `crates/tools/cardano-tracer/src/acceptors/server.rs::run_data_points_acceptor_status`
  — R424 deferral closed (real driver wired).

## Functional shippable surface

After R458, the cardano-tracer per-connection mux multiplexes:

1. **HANDSHAKE** (mini-protocol number 0) — trace-forwarder
   version negotiation (R433-R436).
2. **TRACE_OBJECTS** (mini-protocol number 2) — incoming trace-
   object batches via R417-R424's TraceObject sub-protocol.
3. **DATA_POINTS** (mini-protocol number 3) — node-info data-point
   queries via R452-R458's DataPoint sub-protocol. **New in this arc.**

Both sub-protocol drivers spawn concurrently via `tokio::join!` in
the per-connection acceptor task. Both share the same connection-
level brake flag — a single stop trip terminates both cleanly
within ~50ms via the brake-aware `wait_for_ask` racing in
`run_until_stopped`.

The `DataPointRequestor` coordination primitive (R456) provides
the STM-mirror handle that external context (e.g. a future
node-info RPC dispatcher) uses to push name lists into the
acceptor loop and receive `(name, maybe-bytes)` replies, with a
10-second timeout fallback matching upstream's `tenSeconds`.

## Wire format parity

CBOR wire format mirrors upstream's `codecDataPointForward`:

| Tag | Shape | Message |
|-----|-------|---------|
| 1   | `[1, [name, ...]]` | MsgDataPointsRequest |
| 2   | `[2]` | MsgDone |
| 3   | `[3, [(name, maybe-bytes), ...]]` | MsgDataPointsReply |

Per-value `Maybe` encoding mirrors cborg's `Serialise (Maybe a)`
canonical shape (`Nothing → array(0)`, `Just v → [1, bytes(v)]`).
The encoder emits definite-length CBOR arrays; the decoder accepts
both definite- AND indefinite-length list encodings so messages
from upstream cardano-node forwarders deserialize correctly. This
matches the wire-canonicalization carve-out from R418's TraceObject
codec.

## Concurrency safety

The R457 loop uses `tokio::select!` to race the wait-for-external-
ask against a 50ms poll of the brake flag — a synthesis improvement
over upstream, which relies on either the external context also
raising the ask flag or the mux shutting the channel to wake a
brake-tripped loop. Yggdrasil's racing variant makes brake-driven
shutdown robust regardless of external-context behavior.

## Carve-outs surviving R459

- **DataPoint forwarder side** (`Run/DataPoint/Forwarder.hs`,
  `Protocol/DataPoint/Forwarder.hs`, `DataPointStore` /
  `read_from_store` / `write_to_store`): the acceptor side shipped
  in R452-R458; the forwarder side runs in cardano-node (which
  Yggdrasil isn't building yet as cardano-tracer's forwarder
  counterpart). Vendored but pending — not blocking
  cardano-tracer's operational role as the acceptor.
- **EKG ReqResp sub-protocol** — synthesis carve-out
  (`ekg-forward` Hackage package not vendored). Operationally
  cardano-tracer runs without EKG ingest; per-node Prometheus /
  EKG endpoints from R408-R414 read from MetricsStore.
- **TraceObject CBOR codec byte-equivalence** — upstream's
  Cardano.Logging.TraceObject Serialise instance is not vendored;
  Yggdrasil ships a Yggdrasil-canonical 6-field array shape
  (R437) that decodes the upstream wire only when reverse-
  engineered against the cardano-logging Hackage source. Not
  blocking the DataPoint arc.
- **RemoteSocket TCP path** — defers pending trace-forwarder
  handshake-over-socket integration. The LocalPipe (Unix-domain
  socket) path is the operationally-canonical SPO deployment
  shape.

## Verification log

```
cargo fmt --all -- --check                 # clean
cargo check-all                             # clean
cargo test-all                              # 6,024 passing (was 5,962 pre-R452)
cargo lint                                  # clean
python3 scripts/check-strict-mirror.py --fail-on-violation  # 0 violations
python3 scripts/check-parity-matrix.py     # clean
```

24/24 cardano-tracer acceptors tests pass at HEAD.

## Follow-on rounds

After R459 the next R460+ candidates remain:

- **EKG ReqResp sub-protocol synthesis** — requires `ekg-forward`
  Hackage-source reverse engineering. Separate arc.
- **DataPoint forwarder side** — needed only when yggdrasil-node-
  equivalent forwarding lands.
- **TraceObject CBOR upstream-byte-equivalence** — requires
  cardano-logging Hackage source.
- **Logs Rotator full impl** — bounded 3-5 round arc.
- **axum-server-rustls TLS integration** — needs dep-audit per
  `docs/DEPENDENCIES.md`.

Each follow-on advances a documented `*_status()`-tracked carve-out
without blocking cardano-tracer's structurally-complete operational
shape.
