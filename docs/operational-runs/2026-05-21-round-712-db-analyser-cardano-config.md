---
title: "Round 712 db-analyser CardanoConfig (genesis-bootstrap arc, slice 3a)"
parent: Reference
---

# Round 712 db-analyser CardanoConfig (genesis-bootstrap arc, slice 3a)

Date: 2026-05-21

## Scope

Sub-slice 3a of the db-analyser genesis-bootstrap arc. Adds the
`CardanoConfig` node-`config.json` type to
`crates/tools/db-analyser/src/has_analysis.rs`, with serde
`Deserialize` and config-relative path adjustment.

## Parity plan

Authored before implementation (in-conversation, per the
`parity-plan` skill — slice 3 touches genesis-config parsing).
The plan reviewed upstream `Block/Cardano.hs:130-296`
(`mkProtocolInfo`, `data CardanoConfig`, `AdjustFilePaths`,
`FromJSON`) and decomposed slice 3 into 3a (the `CardanoConfig`
type, this round) and 3b (the `HasProtocolInfo` impl that loads the
genesis bundle).

R712 corrected the R708 roadmap guess: `CardanoConfig` is **not**
in `Cardano.Node.Types` — it is defined inside upstream
`DBAnalyser/Block/Cardano.hs` itself (line 194). Since yggdrasil
collapses `Block/Cardano.hs` into `has_analysis.rs`, the
`CardanoConfig` mirror lives there, alongside `CardanoBlockArgs`
(R710).

## What shipped

`crates/tools/db-analyser/src/has_analysis.rs`:

- `CardanoHardForkTriggers` — 7 `Option<u64>` per-era
  `Test*HardForkAtEpoch` epochs. Flattened mirror of upstream's
  typed-`NP` `CardanoHardForkTriggers`; `None` =
  `CardanoTriggerHardForkAtDefaultVersion`, `Some(e)` =
  `CardanoTriggerHardForkAtEpoch e`.
- `CardanoConfig` — 9 fields mirroring upstream `data CardanoConfig`
  (network magic, 4 required genesis paths, 2 optional genesis
  hashes, optional Dijkstra path, hard-fork triggers).
- `CardanoConfig::adjust_file_paths` — mirror of upstream
  `instance AdjustFilePaths CardanoConfig`.
- `parse_cardano_config` + custom `Deserialize` — mirror of
  upstream `instance FromJSON CardanoConfig`'s
  `withObject "CardanoConfigFile"`, including the
  `Test*HardForkAtEpoch` **monotonicity check** (a later era's
  trigger requires every earlier era's trigger; upstream's `isBad`).
  Extra keys (e.g. `AlonzoGenesisHash`) are tolerated, as upstream's
  `withObject` does.
- `CardanoConfigParseError` — typed parse failures.

`crates/tools/db-analyser/Cargo.toml`:

- New workspace-internal dependency `yggdrasil-node-config` (for
  `RequiresNetworkMagic`). Tools-→node-config is an established edge
  (`db-synthesizer` already depends on it); no external dependency,
  no `docs/DEPENDENCIES.md` entry needed.

8 new unit tests: full / minimal config, missing required path,
missing network magic, non-object, non-monotone triggers rejected,
`adjust_file_paths` over every genesis path, and a parse of the real
vendored `configuration/preview/config.json` (the arc's validation
gate target).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-db-analyser` — 209 lib (+8 vs R711's
  201) + 20 end-to-end + 2 golden, all green.

## Remaining (db-analyser genesis-bootstrap arc)

- Slice 3b — `HasProtocolInfo for yggdrasil_ledger::Block`:
  `make_protocol_info` resolves config-relative genesis paths and
  loads the genesis bundle (mirror of `mkProtocolInfo`; new dep
  `yggdrasil-node-genesis`).
- Slice 4 — load the genesis-seeded `LedgerState` in `run`.
- Slice 5 — thread it into the analysis runner.
