---
title: "Round 801 dmq-node inbound-V2 collectTxsImpl"
parent: Reference
---

# Round 801 dmq-node inbound-V2 collectTxsImpl

Date: 2026-05-22

## Scope

Completes the dmq-node inbound-V2 governor — the `collect_txs_impl`
inbound handler.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `CONST_MAX_TX_SIZE_DISCREPANCY` — the allowed advertised-vs-received
  size discrepancy (32 bytes), mirror of upstream
  `const_MAX_TX_SIZE_DISCREPANCY`.
- `TxSubmissionProtocolError` — the inbound governor's protocol-error
  enum (`ProtocolErrorTxNotRequested` / `…TxIdsNotRequested` /
  `…TxSizeError`), mirror of upstream `data TxSubmissionProtocolError`
  (`Inbound/V2/Types.hs`).
- `collect_txs_impl` — mirror of upstream `collectTxsImpl`
  (`State.hs`), the `MsgReplyTxs` inbound handler: every received tx
  must agree with its advertised size to within the discrepancy bound
  (else `ProtocolErrorTxSizeError`); received txs are recorded in
  `downloaded_txs`, requested-but-undelivered txids move to
  `unknown_txs` (intersected with the live set), and the requested
  txids are cleared from the peer's in-flight tracking and the shared
  `inflight_txs` multiplicity map.

1 unit test covers the undelivered-request path (txids → unknown,
in-flight tracking cleared on every level).

## dmq-node inbound-V2 governor complete

`inbound_v2.rs` now ports the full inbound-V2 governor — the state
surface, all seven decision-path functions, and both inbound handlers
(`received_tx_ids_impl`, `collect_txs_impl`). Fourteen slices
(R788-R801).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 181 lib (+1 vs R800's 180) +
  2 golden, all green.
