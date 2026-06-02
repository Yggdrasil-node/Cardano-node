---
title: "Round 710 db-analyser CardanoBlockArgs (genesis-bootstrap arc, slice 1 re-scoped)"
parent: Reference
---

# Round 710 db-analyser CardanoBlockArgs (genesis-bootstrap arc, slice 1 re-scoped)

Date: 2026-05-21

## Scope

First slice of the db-analyser genesis-bootstrap arc, **re-scoped**
from the R708 plan. Adds the `CardanoBlockArgs` config-args type to
`crates/tools/db-analyser/src/has_analysis.rs`.

## Why the re-scope

R708 planned slice 1 as "extract the db-synthesizer config→genesis
loaders into `yggdrasil-node-genesis`". R709 found that move
entangled three db-synthesizer **strict-mirror files**
(`types.rs` / `orphans.rs` / `run.rs`, mirroring upstream
`DBSynthesizer/{Types,Orphans,Run}.hs`) and flagged it as needing a
`parity-plan`.

R710 found *why* that shape was wrong: upstream `db-analyser` does
**not** borrow db-synthesizer's loaders. It has its own
`DBAnalyser/Block/Cardano.hs`, whose
`Args (CardanoBlock StandardCrypto) = CardanoBlockArgs { configFile,
threshold }` plus `mkProtocolInfo` is the config→genesis path
(`.reference-haskell-cardano-node/.../Cardano/Tools/DBAnalyser/Block/Cardano.hs:131-148`).

yggdrasil deliberately has **no `block/` subtree**: the three
upstream `Block/{Byron,Shelley,Cardano}.hs` `HasAnalysis` instances
are collapsed into `has_analysis.rs` because `yggdrasil_ledger::Block`
is a unified era-tagged type (documented in the `HasAnalysis` impl
docstring there). The re-scoped arc extends that collapse — the
`CardanoBlockArgs` surface lives in `has_analysis.rs` — so
db-synthesizer's strict-mirror files are **not touched at all**. The
R709 entanglement is dissolved: no extraction, no `parity-plan`
(filename-mirror restructuring is `round-extraction` territory).

## What shipped

`crates/tools/db-analyser/src/has_analysis.rs`:

- `pub struct CardanoBlockArgs { config_file: PathBuf, threshold:
  Option<f64> }` — mirror of upstream
  `Block/Cardano.hs::Args (CardanoBlock StandardCrypto)`.
  `config_file` is the operator's node `config.json`; `threshold` is
  the optional Byron PBFT signature threshold (upstream
  `PBftSignatureThreshold` is a `Double` newtype → `f64`, matching
  `db-synthesizer/src/types.rs::byron_pbft_signature_thresh`).
- 2 unit tests: construction with `threshold: None` and
  `threshold: Some(_)`, plus the derived-`PartialEq`/`Clone` check.

This is the real config-args type that the slice-2 `--config PATH`
parser flag populates.

## Slice-3 prerequisite recorded

R710 verified (`grep -rn 'CardanoConfig' crates/`) that **no
`Cardano.Node.Types::CardanoConfig` mirror exists** anywhere in
`crates/`. Upstream `Block/Cardano.hs:140` deserializes the config
file into a `CardanoConfig` (per-era genesis paths + `AdjustFilePaths`
instance), not into db-synthesizer's `NodeConfigStub`. Porting that
`CardanoConfig` mirror is therefore a slice-3 prerequisite, and
slice 3 warrants a `parity-plan` (it touches genesis-config parsing).

## Changes

- `crates/tools/db-analyser/src/has_analysis.rs` — `PathBuf` import +
  `CardanoBlockArgs` struct + 2 tests.
- `docs/COMPLETION_ROADMAP.md` — recorded the slice-1 re-scope and
  the new 5-slice decomposition.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (no new files; an addition to an existing
  strict-mirror file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-db-analyser` — 195 lib (+2 vs R707's
  193) + 20 end-to-end + 2 golden, all green.

## Remaining (db-analyser genesis-bootstrap arc)

- Slice 2 — `db-analyser --config PATH` parser flag populating
  `CardanoBlockArgs`.
- Slice 3 — `CardanoConfig` mirror + config→genesis-bundle loading
  (`HasProtocolInfo for yggdrasil_ledger::Block`).
- Slice 4 — load the genesis-seeded `LedgerState` in `run`.
- Slice 5 — thread it into the analysis runner.
