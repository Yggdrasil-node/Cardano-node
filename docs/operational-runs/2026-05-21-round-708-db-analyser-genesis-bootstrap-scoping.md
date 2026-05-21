---
title: "Round 708 Scope the db-analyser genesis-bootstrap arc (A3)"
parent: Reference
---

# Round 708 Scope the db-analyser genesis-bootstrap arc (A3)

Date: 2026-05-21

## Scope

Decomposes the db-analyser genesis-bootstrap arc — the R488
deferral that R3c-6 named as its remaining blocker — into a
verified 4-slice plan, recorded in `docs/COMPLETION_ROADMAP.md`.

## Rationale

R705-R707 completed A3 R3c-6's three structural slices (ledger
checkpoint persist, canonical `immutable/` layout, snapshot
consistency guard). R707 honestly recorded that the full
"db-analyser validates the synthesized chain" goal is blocked:
db-analyser's 6 ledger-applying analyses bootstrap an empty
`LedgerState::new()`, so real on-chain blocks fail at apply
time.

Closing that needs a genesis-bootstrap arc, not a one-round
slice. Starting it naively would have shipped a dead `--config`
flag (a CLI surface with no behavior). Instead this round maps
the arc properly before any code lands.

## Findings

- Upstream `db-analyser` takes `--config PATH`
  (`DBAnalyser/Parsers.hs:253-266`) and builds a genesis-seeded
  initial `LedgerState` from it.
- The loaders already exist: `load_genesis_bundle` /
  `load_consensus_protocol` / `build_initial_forge_state` /
  `load_initial_forge_state` in `crates/tools/db-synthesizer/
  src/run.rs`, and the shared `build_base_ledger_state` +
  `BaseLedgerStateInputs` in `crates/node/genesis/src/lib.rs`.
- The arc is therefore mostly *wiring* + one *extraction* (move
  the db-synthesizer loaders to `yggdrasil-node-genesis` so
  db-analyser reuses them without a sister-tool cross-dep).

## Deliverable

A 4-slice decomposition added to `docs/COMPLETION_ROADMAP.md`
under "db-analyser genesis-bootstrap arc":

1. Extract the shared config→genesis loaders into
   `yggdrasil-node-genesis`.
2. Add the `db-analyser --config PATH` parser flag.
3. Load the genesis-seeded `LedgerState` in `run`.
4. Thread it into the analysis runner so the 6 ledger-applying
   analyses bootstrap from it.

Validation gate: `db-analyser --config <preview-config> --db
<synthesized-chain>` runs the apply-loop analyses without
empty-`LedgerState::new()` apply failures.

## Validation

- `cargo fmt --all -- --check` — green (doc-only round).
- No source change.

## Status

A3 R3c-6 structural work is complete (R705-R707). The
genesis-bootstrap arc is now scoped and ready for
slice-by-slice implementation; slice 1 (the shared-loader
extraction) is the natural entry point and warrants a
`parity-plan` as it touches genesis-config parsing.
