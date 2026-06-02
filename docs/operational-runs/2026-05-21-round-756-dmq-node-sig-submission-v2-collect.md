---
title: "Round 756 dmq-node SigSubmissionV2 Collect type (dmq-node arc, slice 37)"
parent: Reference
---

# Round 756 dmq-node SigSubmissionV2 Collect type (dmq-node arc, slice 37)

Date: 2026-05-21

## Scope

Slice 37 of the dmq-node arc — the `Collect` pipelined-result type
from `Protocol/SigSubmissionV2/Inbound.hs`.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission_v2.rs`:

- `Collect` — the `SigSubmissionV2` inbound peer's pipelined-result
  sum, mirror of upstream `data Collect sigId sig`:
  - `CollectSigIds { requested: NumIdsReq, ids: Vec<SigIdAndSize> }`
    — the result of a pipelined `MsgRequestSigIds`.
  - `CollectSigs { requested: BTreeMap<SigId, u32>, sigs: Vec<Sig> }`
    — the result of a pipelined `MsgRequestSigs` (the requested
    `sigId → size` map paired with the returned signatures).

`Collect` is a pure data type — testable independently of the
deferred mux / diffusion runtime. The surrounding continuation-style
inbound peer (`InboundStIdle`, `sigSubmissionV2InboundPeerPipelined`)
is *not* standalone-portable: it is `typed-protocols` framework
plumbing only meaningful inside the mux, and ships with the runtime
sub-arc.

1 unit test covers both variants.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 130 lib (+1 vs R755's 129) +
  2 golden, all green.

## dmq-node arc — at the runtime wall

With the protocol-definition surface complete (R717-R754) and the
last standalone-portable data type (`Collect`) ported, the remaining
dmq-node work is exclusively the runtime / diffusion integration
sub-arc: the typed-protocols peer drivers (continuation-style,
framework-bound) plus `NodeKernel` / `Diffusion/*` / the NtN-NtC mux
bundles / `Tracer.hs`. These are entangled (a peer driver is only
exercisable inside the mux) and reuse `crates/network`'s mux +
peer-selection machinery — one deliberate `crates/network`-integration
arc warranting its own `parity-plan`, not standalone slices.
