---
title: "Round 706 db tools — canonical immutable/ ChainDb subdir (A3 R3c-6 slice 2)"
parent: Reference
---

# Round 706 db tools — canonical immutable/ ChainDb subdir (A3 R3c-6 slice 2)

Date: 2026-05-21

## Scope

Second slice of A3 R3c-6: migrate the db-synthesizer,
db-analyser, and db-truncater to the canonical ChainDb layout —
immutable blocks under a `<db>/immutable/` subdir.

## Rationale

The node already constructs its ChainDb at
`<storage>/{immutable,volatile,ledger}/`
(`crates/node/cardano-node/src/commands/run.rs:358-360`). The
db-* sister tools, by contrast, wrote/read the immutable blocks
*flat* in the `--db` directory. That divergence meant:

- the db-synthesizer produced a non-canonical ChainDb;
- `db-analyser` / `db-truncater` could not read a node-produced
  ChainDb (whose blocks live under `immutable/`).

R705 already placed the synthesizer's ledger snapshot at
`<db>/ledger/`; this slice aligns the immutable blocks too.

## Changes

- `db-synthesizer/src/run.rs` — `synthesize` /
  `synthesize_with_forge_state` open
  `FileImmutable::open(db_dir.join("immutable"))`.
- `db-analyser/src/lib.rs::run` — opens
  `FileImmutable::open(config.db_dir.join("immutable"))`.
- `db-truncater/src/run.rs::run` — opens
  `FileImmutable::open(config.db_dir.join("immutable"))`.
- Test fixtures across the three crates updated in lockstep
  (db-synthesizer unit + integration tests, db-analyser
  `lib::run` end-to-end tests, db-truncater `run` smoke test) so
  each builds its `FileImmutable` fixture under the same
  `immutable/` subdir the tool now reads.
- `db-truncater` / `db-analyser` AGENTS.md updated.

All three tools now agree on the canonical
`<db>/{immutable,ledger}/` layout and can read a node-produced
ChainDb directly. `pre_open_chain_db`'s ChainDb-subdir heuristic
(`["immutable","ledger","volatile","gsm"]`) is now satisfied by
the synthesizer's own output.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `check-strict-mirror.py --fail-on-violation` — 0 violations
- `cargo test -p yggdrasil-db-synthesizer -p yggdrasil-db-analyser
  -p yggdrasil-db-truncater` — db-synthesizer 96 lib + 8
  integration, db-analyser 193 lib + 18 end-to-end, db-truncater
  30 lib — all green.

## Remaining (A3 R3c-6)

- Teach `db-analyser` to load the persisted `LedgerStore`
  snapshot (`<db>/ledger/`) as the apply-loop's starting state —
  the snapshot R705 writes still has no consumer.
