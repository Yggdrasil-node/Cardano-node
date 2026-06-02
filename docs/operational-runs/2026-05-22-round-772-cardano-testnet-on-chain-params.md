---
title: "Round 772 cardano-testnet UpdateTimestamps default fix + TestnetOnChainParams"
parent: Reference
---

# Round 772 cardano-testnet UpdateTimestamps default fix + TestnetOnChainParams

Date: 2026-05-22

## Scope

Continues the cardano-testnet `Testnet/Start/Types.hs` port —
completes the era-free option types R359 left, and fixes a parity bug
found while reviewing them.

## What shipped

`crates/tools/cardano-testnet/src/types.rs`:

- **Parity bug fix — `UpdateTimestamps` default.** Upstream
  `instance Default UpdateTimestamps where def = DontUpdateTimestamps`.
  The R359 port had `#[default]` on `UpdateTimestamps` (the wrong
  variant), so `UpdateTimestamps::default()` returned the opposite of
  upstream's `def`. `#[default]` moved to `DontUpdateTimestamps`; the
  doc comment and test corrected.
- `TestnetOnChainParams` — the on-chain-parameters selector
  (`DefaultParams` / `OnChainParamsFile(PathBuf)` /
  `OnChainParamsMainnet`), mirror of upstream `data
  TestnetOnChainParams` with `Default = DefaultParams`. An era-free
  enum R359 had left out.
- `MAINNET_PARAMS_URL` — the Blockfrost-format mainnet
  on-chain-parameters file URL, mirror of the target of upstream
  `mainnetParamsRequest`.

4 unit tests: both `Default` impls verified against upstream, the
`OnChainParamsFile` payload, and the URL shape.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 39 lib (+4 vs the prior
  35), all green.

## Remaining (cardano-testnet `Start/Types.hs`)

The deeper option records (`CardanoTestnetCliOptions`,
`TestnetCreationOptions`, `GenesisOptions`, `NodeOption`,
`TestnetRuntimeOptions`, `Conf`) carry era-aware fields gated on
yggdrasil-ledger's era surface; `Testnet/Types.hs` runtime/key types
(`KeyPair`, the key-kind tags, `TestnetNode`) are the pending
`runtime_types.rs`.
