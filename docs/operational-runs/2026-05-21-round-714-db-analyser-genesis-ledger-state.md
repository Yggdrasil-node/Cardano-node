---
title: "Round 714 db-analyser genesis-seeded LedgerState builder (genesis-bootstrap arc, slice 4)"
parent: Reference
---

# Round 714 db-analyser genesis-seeded LedgerState builder (genesis-bootstrap arc, slice 4)

Date: 2026-05-21

## Scope

Slice 4 of the db-analyser genesis-bootstrap arc. Adds
`build_genesis_ledger_state` to `has_analysis.rs` — folds a
`CardanoGenesisBundle` (R713) into the genesis-seeded initial
`LedgerState`.

## What shipped

`crates/tools/db-analyser/src/has_analysis.rs`:

- `build_genesis_ledger_state(&CardanoGenesisBundle) ->
  Result<LedgerState, GenesisLoadError>` — assembles a
  `BaseLedgerStateInputs` from the bundle (Byron UTxO entries,
  Shelley genesis bootstrap, protocol parameters, Conway genesis
  enact state, Shelley `active_slots_coeff` / `security_param`) and
  folds it through the shared
  `yggdrasil_node_genesis::build_base_ledger_state`.

The wiring mirrors `db-synthesizer`'s `build_initial_forge_state` —
the same `BaseLedgerStateInputs` fed to the same shared builder, so
db-analyser and db-synthesizer seed a **byte-identical** initial
ledger state — minus the nonce / stake-snapshot fields, which a
chain *analyser* (as opposed to a *forger*) does not need. This is
the db-analyser projection of upstream `ProtocolInfo`'s
`pInfoInitLedger` (`Block/Cardano.hs::mkProtocolInfo` →
`mkCardanoProtocolInfo` → `protocolInfoCardano`).

2 new unit tests: a minimal tempdir config and the real vendored
`configuration/preview/config.json` both fold to a Byron-rooted
genesis-seeded `LedgerState` (the preview test exercises
`build_shelley_genesis_bootstrap` decoding the real preview
`initialFunds` addresses).

## Scope boundary

`build_genesis_ledger_state` is a pure function. `run` does not call
it yet — wiring `make_protocol_info` + `build_genesis_ledger_state`
into `run` and threading the resulting `LedgerState` into the
analysis runner (so the 6 ledger-applying analyses bootstrap from it
instead of `LedgerState::new()`) is slice 5.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-db-analyser` — 217 lib (+2 vs R713's
  215) + 20 end-to-end + 2 golden, all green.

## Remaining (db-analyser genesis-bootstrap arc)

- Slice 5 — thread the genesis-seeded `LedgerState` through `run`
  into the analysis runner; the 6 ledger-applying analyses bootstrap
  from it. Closes the arc's validation gate.
