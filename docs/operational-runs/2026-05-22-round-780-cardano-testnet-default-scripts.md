---
title: "Round 780 cardano-testnet default test scripts (defaults.rs)"
parent: Reference
---

# Round 780 cardano-testnet default test scripts (defaults.rs)

Date: 2026-05-22

## Scope

Continues the cardano-testnet arc — opens `defaults.rs`, the port of
upstream `Testnet/Defaults.hs`, with its era-free script values.

## What shipped

`crates/tools/cardano-testnet/src/defaults.rs` — new file
(`defaults.rs` basename-mirrors `Defaults.hs`):

- `simple_script` — builds a native-script JSON envelope requiring a
  single signer, mirror of upstream `simpleScript :: Text -> Text`.
- `PLUTUS_V2_SCRIPT` / `PLUTUS_V3_SCRIPT` — the always-succeeds
  Plutus V2 / V3 test scripts (text-envelope JSON), mirror of
  upstream `plutusV2Script` / `plutusV3Script`.

`Defaults.hs` is otherwise era / ledger-coupled (per-era default
genesis records, default key pairs, topology), gated on the
yggdrasil-ledger era surface. The two large Plutus blobs
(`plutusV3SupplementalDatumScript`, `plutusV2StakeScript`) land in a
follow-up round. `lib.rs` gains `pub mod defaults;`.

3 unit tests cover the `simple_script` shape and that the Plutus
constants are valid text-envelope JSON.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 65 lib (+3 vs R779's
  62), all green.

## Remaining (cardano-testnet `Defaults.hs`)

The two large Plutus script blobs; the era-coupled default genesis
records, key pairs, and topology values.
