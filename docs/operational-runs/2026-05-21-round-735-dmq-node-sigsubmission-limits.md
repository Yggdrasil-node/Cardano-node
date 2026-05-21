---
title: "Round 735 dmq-node SigSubmission protocol limits (dmq-node arc, slice 17)"
parent: Reference
---

# Round 735 dmq-node SigSubmission protocol limits (dmq-node arc, slice 17)

Date: 2026-05-21

## Scope

Slice 17 of the dmq-node arc (SigSubmission protocol slice 4). Adds
the `SigSubmission` per-state time and size limits.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `SigSubmissionState::time_limit` — the inactivity timeout per
  protocol state (`None` is `waitForever`). Mirror of upstream
  `Codec.hs::timeLimitsSigSubmission`: `StInit` / `StIdle` / blocking
  `StTxIds` wait forever; non-blocking `StTxIds` and `StTxs` use
  `shortWait` (10 s).
- `SigSubmissionState::byte_limit` — the maximum inbound-message
  size per state. Mirror of `byteLimitsSigSubmission` —
  `smallByteLimit` (`0xffff`) for every state.
- `SHORT_WAIT` / `SMALL_BYTE_LIMIT` constants, matching upstream
  `Ouroboros.Network.Protocol.Limits` (`shortWait = Just 10`,
  `smallByteLimit = 0xffff`) and `crates/network`'s `SHORT_WAIT`.

2 unit tests covering both limit tables across every state.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 89 lib (+2 vs R734's 87) +
  2 golden, all green.

## dmq-node SigSubmission mini-protocol — surface complete

The dmq-node-local `SigSubmission` mini-protocol now has its state
machine, transition validation, message types, full CBOR message
codec, and the time / byte limit tables. Remaining dmq-node work —
the client / server protocol drivers, the `LocalMsg*` mini-protocols,
and the `Diffusion/*` wiring — is the run-loop / mux integration
layer.
