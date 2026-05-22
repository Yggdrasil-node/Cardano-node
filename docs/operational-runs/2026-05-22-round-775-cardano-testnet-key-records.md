---
title: "Round 775 cardano-testnet key-record types"
parent: Reference
---

# Round 775 cardano-testnet key-record types

Date: 2026-05-22

## Scope

Continues the cardano-testnet `runtime_types.rs` port (upstream
`Testnet/Types.hs`) — the `KeyPair`-composed key-record types.

## What shipped

`crates/tools/cardano-testnet/src/runtime_types.rs`:

- `SpoNodeKeys` — the cold / VRF / staking key pairs of a
  stake-pool-operator node, mirror of upstream `data SpoNodeKeys`.
- `PaymentKeyInfo` — a payment key pair plus its derived address,
  mirror of upstream `data PaymentKeyInfo`.
- `Delegator` — a payment key pair and the staking key pair it
  delegates with, mirror of upstream `data Delegator`.

All three are `KeyPair`-composed records — no process handles, so
they port cleanly. Upstream's `MonoFunctor SpoNodeKeys` instance (a
typeclass for mapping over the contained file paths) is
Haskell-specific machinery with no Rust counterpart; only the records
are ported.

3 unit tests cover field access, equality, and the kinded key pairs.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 47 lib (+3 vs R774's
  44), all green.

## Remaining (cardano-testnet `runtime_types.rs`)

The process-handle-backed runtime types — `TestnetRuntime`,
`TestnetNode`, `TestnetKesAgent` — land with the testnet-harness
rounds (they hold OS process handles and stdio handles).
