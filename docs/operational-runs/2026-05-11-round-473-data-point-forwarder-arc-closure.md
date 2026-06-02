---
title: 'R473 closeout: DataPoint forwarder-side R471-R473 arc complete'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-473-data-point-forwarder-arc-closure/
---

# R473 — DataPoint forwarder-side arc closure

**Date:** 2026-05-11
**Predecessor:** [`R459 DataPoint acceptor-side arc`](2026-05-11-round-459-data-point-arc-closure.md).
**Closure scope:** R471-R472 fully shipped; R473 = closeout (this
doc + parity-matrix evidence + CHANGELOG full arc summary).

## Arc summary

R471-R473 closed the DataPoint *forwarder*-side surface in
`crates/network/`, completing the trace-forward DataPoint
sub-protocol port (the R452-R459 arc shipped the acceptor side
only). Per-round delivery:

| Round | Source mirror | Yggdrasil delivery |
|-------|---------------|--------------------|
| R471  | `Protocol/DataPoint/Forwarder.hs` (50 lines) | `crates/network/src/data_point_forwarder.rs` — `DataPointForwarder` driver + `DataPointForwarderEvent { Request, Done }` |
| R472  | `Utils/DataPoint.hs` (forwarder-side subset) | `crates/network/src/protocols/data_point_forward_utils.rs` extended with `DataPointStore`, `init_data_point_store`, `write_to_store`, `read_from_store` |
| R473  | `Run/DataPoint/Forwarder.hs` (46 lines) | `crates/network/src/data_point_run_forwarder.rs` — `forward_data_points_{init,resp}` mux-level entries pairing R471 driver + R472 DataPointStore |

**Workspace tests:** 6,063 → 6,080 (+17 across 3 rounds).
**Verification gates:** all five clean at HEAD on every round
commit.

## Functional shippable surface

After R473, Yggdrasil's network crate ships **both sides** of the
trace-forward DataPoint sub-protocol:

- **Acceptor side** (R452-R459, cardano-tracer): requests
  data-points by name → receives `(name, maybe-bytes)` pairs.
  Uses `DataPointRequestor` external-context coordination
  primitive (R456).
- **Forwarder side** (R471-R473, cardano-node analog): receives
  requests from the acceptor → looks up names in a per-node
  `DataPointStore` → sends `(name, maybe-bytes)` pairs reply.

The two sides interoperate over a single mux'd
`MiniProtocolNum(3)` channel (the canonical upstream wire-format
assignment). The R460 integration smoke confirmed the
acceptor-side multiplexing alongside HANDSHAKE + TRACE_OBJECTS;
the new forwarder-side surface is ready to be wired into a
node-binary trace-source when that lands.

## Driver-pattern symmetry

R471's `DataPointForwarder` mirrors R454's `DataPointAcceptor`
driver shape but with inverted control:

| Operation | Acceptor (R454) | Forwarder (R471) |
|-----------|-----------------|------------------|
| Initiate request | `acceptor.request(names) -> Result<DataPointValues, _>` | (receives — no caller-initiated method) |
| Receive request | (no — acceptor sends) | `forwarder.wait_for_request() -> Result<Event, _>` returning `Request(names)` / `Done` |
| Send reply | (no — forwarder sends) | `forwarder.send_reply(values) -> Result<(), _>` |
| Terminate | `acceptor.done() -> Result<(), _>` | (receives `Done` event from acceptor) |

Both drivers maintain the same `DataPointForwardState` state
machine (`StIdle ↔ StBusy → StDone`) and reject method calls
made in the wrong state with `InvalidState` errors.

## DataPointStore design notes

Upstream's `DataPointStore` is
`TVar (Map DataPointName DataPoint)` where
`DataPoint = forall a. ToJSON a => DataPoint a`. The existential
encodes-on-lookup via `\NodeInfo{niName} -> Just $ encode v`.

Yggdrasil's `DataPointStore` is
`Arc<RwLock<HashMap<DataPointName, DataPointValue>>>` —
storing the **already-encoded** JSON bytes directly. This is
operationally equivalent (the upstream pattern always encodes at
lookup time anyway) and avoids needing a `Box<dyn Serialize>`
trait-object dance for the existential. Producer call sites just
`serde_json::to_vec(&node_info).unwrap_or_default()` once before
calling `write_to_store`.

## Verification log

```
cargo fmt --all -- --check                          # clean
cargo check-all                                     # clean
cargo test-all                                      # 6,080 passing (was 6,063 pre-R471)
cargo lint                                          # clean
python3 dev/test/check-strict-mirror.py --fail-on-violation  # 0 violations
python3 dev/test/check-parity-matrix.py             # clean
```

Per-round test breakdown:
- R471: +6 (driver round-trips, error paths, sequential rounds, invalid-state guards)
- R472: +7 (store init, write/overwrite, read known/unknown, order preservation, clone-shared state)
- R473: +4 (resp + init route to same loop, multi-round, transport-error propagation)

## Carve-outs surviving R473

- **The DataPoint forwarder side is not wired into a node-binary
  trace-source yet.** Yggdrasil's `node/` crate doesn't currently
  generate trace objects to forward, so the forwarder-side
  surface ships as a callable library API without a production
  invocation. When `node/` grows trace-emission, callers will
  hold a `DataPointStore`, populate it via `write_to_store` on
  state changes, and spawn `forward_data_points_resp` against
  cardano-tracer's mux'd HANDSHAKE pipe.

- **Cross-arc dependencies remain unchanged**: the EKG ReqResp
  sub-protocol synthesis carve-out (Hackage), the
  RemoteSocket TCP path (handshake-over-socket codec), the
  TraceObject CBOR upstream-byte-equivalence (cardano-logging
  Hackage), and the dmq-node / kes-agent / db-analyser
  sister-tool arcs are all unaffected by this arc.

## Follow-on observation

The trace-forward 2-sided port is now structurally complete in
Yggdrasil's network crate. The DataPoint sub-protocol — both
sides — mirrors upstream's `trace-forward` library at the
protocol level. The remaining wire-format-byte-equivalence gap
(Yggdrasil's TraceObject codec being a 6-field array synthesis
rather than cardano-logging's Hackage Serialise) is the only
documented divergence from upstream cardano-node interoperability;
this gap also affects R458's mux-multiplexed pipe and is
documented in `crates/tools/cardano-tracer/AGENTS.md`.
