---
title: "Round 754 dmq-node SigSubmissionV2 protocol limits (dmq-node arc, slice 36)"
parent: Reference
---

# Round 754 dmq-node SigSubmissionV2 protocol limits (dmq-node arc, slice 36)

Date: 2026-05-21

## Scope

Slice 36 of the dmq-node arc — the `SigSubmissionV2` per-state time
and size limits.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission_v2.rs`:

- `SigSubmissionV2State::time_limit` — mirror of upstream
  `timeLimitsSigSubmissionV2`: `StIdle` waits forever (`None`); a
  blocking `StSigIds` uses 20 s; a non-blocking `StSigIds` and
  `StSigs` use `shortWait` (10 s).
- `SigSubmissionV2State::byte_limit` — mirror of upstream
  `byteLimitsSigSubmissionV2`: `StIdle` uses `smallByteLimit`
  (`0xffff`); the reply states (`StSigIds`, `StSigs`) use
  `largeByteLimit` (`2_500_000`).
- `SHORT_WAIT` / `BLOCKING_SIG_IDS_WAIT` / `SMALL_BYTE_LIMIT` /
  `LARGE_BYTE_LIMIT` constants, matching upstream
  `Ouroboros.Network.Protocol.Limits` and `Codec.hs`'s `Just 20`.

2 unit tests covering both limit tables across every state.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 129 lib (+2 vs R753's 127) +
  2 golden, all green.

## SigSubmissionV2 mini-protocol — protocol-definition surface complete

`SigSubmissionV2` now has its full state machine, transition
validation, message types, CBOR codec, and limit tables — the same
surface `SigSubmission` reached.

## Remaining (dmq-node arc)

- The `SigSubmissionV2` `Inbound` / `Outbound` driver halves; the
  client / server protocol drivers; the `NodeKernel` / `Diffusion/*`
  run-loop wiring; the NtN / NtC protocol bundles; `Tracer.hs`.
