---
title: "Round 773 cardano-testnet KeyPair + key-kind types (runtime_types.rs)"
parent: Reference
---

# Round 773 cardano-testnet KeyPair + key-kind types (runtime_types.rs)

Date: 2026-05-22

## Scope

Continues the cardano-testnet arc — opens `runtime_types.rs`, the
yggdrasil-side port of upstream `Testnet/Types.hs`.

## What shipped

`crates/tools/cardano-testnet/src/runtime_types.rs` — new file:

- `KeyPair<K>` — a verification + signing key-file pair, phantom-typed
  by key kind, mirror of upstream `data KeyPair k`. `new`,
  `verification_key_fp` / `signing_key_fp` (mirror of upstream
  `verificationKeyFp` / `signingKeyFp`).
- The six key-kind markers — `VrfKey`, `StakePoolKey`, `StakeKey`,
  `PaymentKey`, `KesKey`, `DRepKey` — the `k` parameter values,
  giving compile-time safety against mixing key pairs of different
  kinds.

Upstream's `VKey` / `SKey` `File`-tag phantoms have no Rust
counterpart (yggdrasil's `KeyPair` stores `PathBuf` directly). The
process-handle-backed runtime types (`TestnetRuntime`, `TestnetNode`,
`TestnetKesAgent`) land with the testnet-harness rounds.

`lib.rs` gains `pub mod runtime_types;`.

3 unit tests: the path accessors, equality-by-path, and constructing
a `KeyPair` of every kind.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 42 lib (+3 vs R772's
  39), all green.

## Remaining (cardano-testnet)

- `runtime_types.rs`: the process-handle runtime types
  (`TestnetRuntime`, `TestnetNode`, `TestnetKesAgent`), the IP
  helpers (`testnetDefaultIpv4Address`, `showIpv4Address`),
  `LeadershipSlot`.
- `Start/Types.hs`: the deeper era-aware option records.
