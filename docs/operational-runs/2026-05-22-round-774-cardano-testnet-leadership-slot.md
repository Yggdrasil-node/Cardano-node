---
title: "Round 774 cardano-testnet LeadershipSlot + default IPv4"
parent: Reference
---

# Round 774 cardano-testnet LeadershipSlot + default IPv4

Date: 2026-05-22

## Scope

Continues the cardano-testnet `runtime_types.rs` port (upstream
`Testnet/Types.hs`).

## What shipped

`crates/tools/cardano-testnet/src/runtime_types.rs`:

- `TESTNET_DEFAULT_IPV4_ADDRESS` — the hard-coded local-host testnet
  IPv4 (`127.0.0.1`), mirror of upstream `testnetDefaultIpv4Address`.
  Upstream's separate `showIpv4Address` renderer has no Rust
  counterpart — `std::net::Ipv4Addr`'s `Display` already produces the
  dotted form.
- `LeadershipSlot` — `slot_number` / `slot_time`, mirror of upstream
  `data LeadershipSlot`. Upstream derives Aeson `FromJSON` (keyed on
  the `slotNumber` / `slotTime` field names); the Rust port derives
  `serde::Deserialize` with `rename_all = "camelCase"` to reproduce
  those JSON keys.

`crates/tools/cardano-testnet/Cargo.toml` gains `serde` (an
already-workspace-accepted dependency) and a `serde_json`
dev-dependency for the JSON-key parity test.

2 unit tests: the default IPv4 value/rendering, and `LeadershipSlot`
parsing the upstream `slotNumber`/`slotTime` JSON keys.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 44 lib (+2 vs R773's
  42), all green.

## Remaining (cardano-testnet `runtime_types.rs`)

- The process-handle-backed runtime types — `TestnetRuntime`,
  `TestnetNode`, `TestnetKesAgent`, `SpoNodeKeys`, `PaymentKeyInfo`,
  `Delegator` — land with the testnet-harness rounds.
