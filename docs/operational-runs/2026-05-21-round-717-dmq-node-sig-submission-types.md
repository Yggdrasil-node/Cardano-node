---
title: "Round 717 dmq-node SigSubmission protocol types (dmq-node arc, slice 1)"
parent: Reference
---

# Round 717 dmq-node SigSubmission protocol types (dmq-node arc, slice 1)

Date: 2026-05-21

## Scope

Opens the `dmq-node` sister-tool build-out arc (A4). First slice: the
`SigSubmission` mini-protocol byte-wrapper newtypes.

## Why dmq-node

The db-analyser genesis-bootstrap arc closed at R716. Surveying the
roadmap's open Category-A work: the 8 `partial` parity-matrix entries
are all sister tools; `snapshot-converter` is gated on a not-yet-defined
V2 format; `kes-agent` / `kes-agent-control` are blocked on an upstream
socket-protocol fixture capture; `cardano-testnet` sits behind a deeper
prerequisite (era-surface exposure). `dmq-node` is the one A4 tool
shaped like the genesis-bootstrap arc — a real vendored upstream
(`deps/dmq-node`, ~40 `.hs` files), existing `crates/network/`
mini-protocol infrastructure to leverage, code-only (no operator soak,
no process spawning), and not gated on a deeper prerequisite.

DMQ is the Decentralized Message Queue; its core is the `SigSubmission`
mini-protocol — `type SigSubmission crypto = TxSubmission2 SigId (Sig
crypto)` — which diffuses signatures (e.g. Mithril signatures) across
the network by reusing the `TxSubmission2` mini-protocol.

## What shipped

`crates/tools/dmq-node/src/protocol.rs` — new module-tree parent for
the upstream `DMQ/Protocol/` directory.

`crates/tools/dmq-node/src/protocol/sig_submission.rs` — new file
collapsing upstream `DMQ/Protocol/SigSubmission/{Type,Codec,Validate}.hs`
(the `crates/network/src/protocols/` one-file-per-mini-protocol
pattern). This slice ports the `Type.hs` byte-wrapper newtypes:

- `SigHash` — `newtype SigHash = SigHash ByteString`; `Debug` mirrors
  upstream `Show` (first 10 bytes as hex, ≤20 chars).
- `SigId` — `newtype SigId = SigId SigHash`, the `txid`-analog in the
  `TxSubmission2`-based protocol.
- `SigBody` — `newtype SigBody = SigBody ByteString`.
- `CborBytes` — `newtype CBORBytes = CBORBytes LBS.ByteString`;
  `Debug` mirrors upstream `Show` (the full byte string as hex).

7 unit tests cover the hex-rendering behavior (10-byte truncation vs
full), `SigId` wrapping + ordering, and round-trips.

`lib.rs` gains `pub mod protocol;` and a `Protocol/*` layout-map row;
`AGENTS.md` records the in-progress `protocol/sig_submission.rs`.

No `parity-plan` was required — this slice is a filename-mirror
skeleton of plain type definitions (round-extraction territory); the
`parity-plan` lands when the CBOR codec slice does.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (the audit TSV was rebuilt via `audit-strict-mirror.py`,
  picking up the 2 new files plus pre-existing tx-generator row drift).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 52 lib (+7 vs the R716
  baseline of 45) + 2 golden, all green.

`check-stale-placement.py` exits 1 only on pre-existing nested `.git`
metadata inside the vendored `.reference-haskell-cardano-node/` tree —
unrelated to this round's files.

## Remaining (dmq-node arc)

- `SigSubmission` `SigRaw` / `Sig` crypto-parameterized payload types
  (KES signature, OpCert, cold key).
- `SigValidationError` / `SigValidationTrace` validation-error tree.
- `SigSubmission` CBOR codec (warrants a `parity-plan`).
- `SigSubmission` validator (`Validate.hs`).
- NodeToClient / NodeToNode protocols, Diffusion wiring (the
  `DiffusionWiringDeferred` carve-out).
