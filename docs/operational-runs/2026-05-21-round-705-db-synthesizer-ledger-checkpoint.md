---
title: "Round 705 db-synthesizer persists a ledger checkpoint (A3 R3c-6 slice 1)"
parent: Reference
---

# Round 705 db-synthesizer persists a ledger checkpoint (A3 R3c-6 slice 1)

Date: 2026-05-21

## Scope

First slice of A3 R3c-6: the db-synthesizer Praos forge path now
persists the forged tip's ledger-state checkpoint, so the
synthesized chain carries a `LedgerStore` snapshot alongside its
immutable blocks.

## Upstream references

- `Cardano.Tools.DBSynthesizer.Run` / `Ouroboros.Consensus.Storage.ChainDB`
  — a real ChainDb carries both the immutable blocks and a
  `LedgerDB` snapshot. The db-synthesizer's flat `FileImmutable`
  output carried only blocks.

## Changes

- `run::synthesize_with_forge_state` — after `run_forge`, builds a
  `LedgerStateCheckpoint` from the final `ForgeState.ledger_state`
  and persists it via `FileLedgerStore::open(db_dir/"ledger")` +
  `save_snapshot(tip_slot, …)`. Reuses the node's existing
  `LedgerStateCheckpoint` CBOR codec — no new wire format.
  Skips the write when the forge tip is still `Origin` (no blocks
  forged or replayed — no meaningful post-genesis state).
- The immutable blocks stay flat in `db_dir` (where `db-analyser`
  opens them via `FileImmutable::open`), so `db-analyser` is
  unaffected; the `ledger/` subdir is additive and consistent
  with `pre_open_chain_db`'s ChainDb-subdir heuristic.
- Test-hygiene fix: `tests/integration.rs::write_bulk_credentials`
  now `chmod`s the secret-key fixture to `0o400` (restoring write
  access first for the tests that build `args_for` twice). The
  credential loader rejects group/world-readable secret-key
  files, so the integration suite was previously failing on any
  host with a `0o022` umask — a pre-existing umask-dependent
  fragility, now deterministic.

1 new integration test:
- `praos_synthesis_persists_a_ledger_checkpoint` — runs the
  bulk-credentials Praos forge end-to-end, asserts a checkpoint
  file appears under `<db>/ledger/`, and round-trips it through
  `LedgerStateCheckpoint::from_cbor_bytes` (restored tip slot
  matches the snapshot key).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-db-synthesizer` (96 lib + 2 doctests
  + 8 integration — all green; the 7 pre-existing integration
  tests were failing on this host before the umask fix)

## Remaining (A3 R3c-6)

- Move the immutable blocks under a canonical `immutable/`
  subdir so the synthesizer output is a standard ChainDb layout.
- Teach `db-analyser` to consume the persisted `LedgerStore`
  snapshot as its apply-loop starting state — today `db-analyser`
  reads only the immutable blocks; the snapshot this slice writes
  has no consumer yet (it is a prerequisite for the apply-loop
  arc).
