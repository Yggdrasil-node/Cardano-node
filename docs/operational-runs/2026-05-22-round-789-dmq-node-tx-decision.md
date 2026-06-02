---
title: "Round 789 dmq-node inbound-V2 TxDecision record"
parent: Reference
---

# Round 789 dmq-node inbound-V2 TxDecision record

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `TxDecision` record (the
governor's per-peer decision output) and its supporting types.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `NumTxIdsToAck` / `NumTxIdsToReq` — `Word16` count newtypes, mirror
  of upstream `Protocol/TxSubmission2/Type`.
- `TxsToMempool` — the `(SigId, Sig)` pairs ready to submit to the
  mempool, mirror of upstream `newtype TxsToMempool txid tx`
  (concrete over the DMQ `SigId` / `Sig`).
- `TxDecision` — the inbound governor's per-peer decision, mirror of
  upstream `data TxDecision txid tx`: txids to acknowledge, txids to
  request, the pipeline flag, the `SigId → size` request map, and the
  `TxsToMempool`.

2 unit tests cover `TxDecision` construction/equality and the count
newtypes.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 163 lib (+2 vs R788's 161) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

`PeerTxState` (the per-peer governor state), `SharedTxState` (the
shared state across peers), and the governor decision functions.
