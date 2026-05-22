---
title: "Round 783 cardano-testnet era-tag enums"
parent: Reference
---

# Round 783 cardano-testnet era-tag enums

Date: 2026-05-22

## Scope

Continues the cardano-testnet arc — the era-tag enums and
`eraToString` conversions, correcting an over-stated R359 carve-out.

## What shipped

`crates/tools/cardano-testnet/src/types.rs`:

- `CardanoEra` — the Cardano ledger era tag (Byron through Conway,
  matching `yggdrasil_ledger::eras::Era`), with `era_to_string`
  (mirror of upstream `eraToString` / `anyEraToString` — the
  lower-case era name).
- `ShelleyBasedEra` — the Shelley-onward era tag, with `era_to_string`
  (mirror of `eraToString` / `anyShelleyBasedEraToString`) and a
  `From<ShelleyBasedEra> for CardanoEra` widening.

The R359 `types.rs` carve-out claimed the era types "need the full
yggdrasil-ledger era surface". That was over-stated: the era
*selector* (`creationEra : AnyShelleyBasedEra`) is a 7-/6-variant tag
enum, not the heavy per-era genesis surface — `eraToString` is just
`map toLower . pretty`. Upstream's `AnyCardanoEra` /
`AnyShelleyBasedEra` GADT-erased wrappers reduce to plain Rust enums.
This unblocks the era-aware option records (`TestnetCreationOptions`
etc., whose other fields are already ported) for later rounds.

3 unit tests cover the era-name conversions, the widening, and the
era ordering.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 72 lib (+3 vs R782's
  69), all green.

## Remaining (cardano-testnet)

The era-aware option records (`TestnetCreationOptions`,
`CardanoTestnetCliOptions`, `Conf`) — now unblocked — plus
`Defaults.hs` per-era genesis (still genuinely genesis-surface
gated) and the process harness.
