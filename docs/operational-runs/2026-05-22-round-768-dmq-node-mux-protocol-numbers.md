---
title: "Round 768 dmq-node mux protocol numbers (dmq-node runtime sub-arc, slice 10)"
parent: Reference
---

# Round 768 dmq-node mux protocol numbers (dmq-node runtime sub-arc, slice 10)

Date: 2026-05-22

## Scope

Slice 10 of the dmq-node runtime/diffusion sub-arc — the
self-contained constants of the NtN / NtC mux bundles.

## What shipped

`crates/tools/dmq-node/src/node_to_node.rs`:

- `SIG_SUBMISSION_MINI_PROTOCOL_NUM` (11),
  `KEEP_ALIVE_MINI_PROTOCOL_NUM` (12),
  `PEER_SHARING_MINI_PROTOCOL_NUM` (13) — the NtN mux mini-protocol
  numbers, mirror of upstream `DMQ/NodeToNode.hs`
  (`sigSubmissionMiniProtocolNum` etc.). Typed as
  `yggdrasil_network::MiniProtocolNum`.

`crates/tools/dmq-node/src/node_to_client.rs`:

- `LOCAL_MSG_SUBMISSION_MINI_PROTOCOL_NUM` (14),
  `LOCAL_MSG_NOTIFICATION_MINI_PROTOCOL_NUM` (15) — the NtC mux
  mini-protocol numbers, mirror of upstream `DMQ/NodeToClient.hs`.
- `NTC_MAX_SIGS_TO_ACK` (1000) — the `LocalMsgNotification`
  single-reply batch cap, mirror of upstream `_ntc_MAX_SIGS_TO_ACK`.

The `ntnApps` / `ntcApps` mux-application wiring of `NodeToNode.hs` /
`NodeToClient.hs` is runtime integration deferred to the `run()` loop.

3 unit tests pin the protocol numbers and the batch cap.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 154 lib (+3 vs R767's 151) +
  2 golden, all green.

## Remaining (dmq-node runtime sub-arc)

The decomposable runtime surface — all six peer drivers, the full
NtN/NtC version surfaces, and the mux protocol numbers — is now
complete. What remains is the entangled mux/diffusion integration:
the `ntnApps` / `ntcApps` application wiring, `Diffusion/*`,
`NodeKernel`, `tracer.rs`, and the `run()` loop replacing
`RunError::DiffusionWiringDeferred` — one effort that needs the mux
event loop running to be exercised.
