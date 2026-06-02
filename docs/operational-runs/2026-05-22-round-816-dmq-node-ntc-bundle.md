---
title: "Round 816 dmq-node NtC mux bundle"
parent: Reference
---

# Round 816 dmq-node NtC mux bundle

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 11:
the node-to-client mux mini-protocol bundle.

## What shipped

`crates/tools/dmq-node/src/node_to_client.rs`:

- `dmq_ntc_bundle` — the DMQ node-to-client mux mini-protocol bundle
  (`OuroborosBundle`), mirror of the DMQ NtC protocol assignment:
  the established-tier `LocalMsgSubmission` (14) and
  `LocalMsgNotification` (15). Node-to-client connections are
  responder-only — every protocol is established-tier, with no hot
  or warm tier.

1 unit test covers the established-only tier assignment.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 203 lib (+1 vs R815's 202) +
  2 golden, all green.

## dmq-node run() integration — components complete

With both mux bundles ported, every component of the dmq-node `run()`
integration is in place: the protocols, drivers, governor, mempool,
`NodeKernel` (R805-R814), and the NtN / NtC `OuroborosBundle`s
(R815-R816). What remains is the `run()` event loop itself —
assembling the `crates/network` diffusion components (connection
manager, mux, accept loop) with the `NodeKernel` and the per-protocol
driver runners.
