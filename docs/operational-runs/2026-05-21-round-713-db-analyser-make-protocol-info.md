---
title: "Round 713 db-analyser HasProtocolInfo (genesis-bootstrap arc, slice 3b)"
parent: Reference
---

# Round 713 db-analyser HasProtocolInfo (genesis-bootstrap arc, slice 3b)

Date: 2026-05-21

## Scope

Sub-slice 3b of the db-analyser genesis-bootstrap arc. Implements
`HasProtocolInfo for yggdrasil_ledger::Block` in `has_analysis.rs` —
`make_protocol_info` reads the node `config.json` and loads the
per-era genesis into a typed bundle.

## What shipped

`crates/tools/db-analyser/src/has_analysis.rs`:

- `CardanoGenesisBundle` — the per-era genesis (Byron UTxO entries,
  Shelley / Alonzo / Conway genesis) plus the initial Praos nonce.
  db-analyser's `HasProtocolInfo::ProtocolInfo`; a db-analyser-scoped
  intermediate of upstream `mkProtocolInfo`'s inline genesis reads
  (`**Strict mirror:** none`).
- `MakeProtocolInfoError` — typed config-read / config-JSON /
  config-parse / genesis-load failures.
- `impl HasProtocolInfo for yggdrasil_ledger::Block` —
  `make_protocol_info` mirrors the genesis-reading half of upstream
  `Block/Cardano.hs::mkProtocolInfo`: read the config, resolve
  `relativeToConfig` (genesis paths config-dir-relative), decode the
  `CardanoConfig` (R712), `adjust_file_paths`, load the four era
  genesis files, and derive the initial Praos nonce — preferring the
  configured `ShelleyGenesisHash`, else the Shelley genesis file
  hash (upstream's `initialNonce` case split).

`crates/tools/db-analyser/Cargo.toml`:

- New workspace-internal dependency `yggdrasil-node-genesis` (the
  genesis loaders + `GenesisLoadError`). `db-synthesizer` already
  depends on it; no external dependency.

6 new unit tests: bundle loads from a tempdir config, genesis paths
resolve relative to the config dir, the configured `ShelleyGenesisHash`
is preferred, missing-config and missing-genesis-file error paths,
and an end-to-end load of the real vendored
`configuration/preview/` genesis bundle (the arc's validation-gate
evidence — every preview era genesis parses through the loaders).

## Carve-out (deferred, by design)

`make_protocol_info` produces the genesis bundle only. The
`CardanoBlockArgs::threshold` → `mkCardanoProtocolInfo` Byron PBFT
wiring and the `mkLatestTransitionConfig` fold are not part of the
bundle — folding the bundle into a genesis-seeded `LedgerState` is
slice 4 (`yggdrasil_node_genesis::build_base_ledger_state`).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-db-analyser` — 215 lib (+6 vs R712's
  209) + 20 end-to-end + 2 golden, all green.

## Remaining (db-analyser genesis-bootstrap arc)

- Slice 4 — fold `CardanoGenesisBundle` into a genesis-seeded
  `LedgerState` in `run` via `build_base_ledger_state`.
- Slice 5 — thread it into the analysis runner so the 6
  ledger-applying analyses bootstrap from it.
