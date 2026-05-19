# Round 515 — A3 R3c-1b: synthesizer initial forge state

**Date:** 2026-05-19
**Area:** sister-tools / `crates/tools/db-synthesizer`
**Upstream reference:** the ledger-seeding half of
`Cardano.Tools.DBSynthesizer.Run.synthesize` — the `pInfoInitLedger`
(initial `ExtLedgerState`) the forge loop runs on.

## Summary

R3c-1b — the second half of A3 R3c-1 — wires db-synthesizer to build
its initial `(LedgerState, NonceEvolutionState)` via the shared
`build_base_ledger_state` extracted in R3c-1a. **A3 R3c-1 is complete:**
the synthesizer can now construct a byte-identical-to-the-node initial
ledger state plus the genesis-seeded Praos nonce-evolution state.

## Changes

- `Cargo.toml` — `yggdrasil-consensus` workspace path-dependency added
  (for `NonceEvolutionState`).
- `run.rs`:
  - new `InitialForgeState { ledger_state, nonce_evolution }`.
  - `build_initial_forge_state(&GenesisBundle)` — sources the ten
    `BaseLedgerStateInputs` fields from the R3b-1 `GenesisBundle` (Byron
    entries directly; the Shelley bootstrap / protocol params / enact
    state via the `build_shelley_genesis_bootstrap` /
    `build_protocol_parameters` / `build_genesis_enact_state`
    genesis-crate helpers; `f` and `k` from the Shelley genesis) and
    folds them through the shared `build_base_ledger_state`; seeds
    `NonceEvolutionState` from the genesis Praos nonce.
  - `load_initial_forge_state(config_path)` — the public entry point
    (resolve config stub → load the genesis bundle → build).
  - 1 new test (`load_initial_forge_state` builds a Byron-era seeded
    state with a concrete-hash nonce).
- `AGENTS.md` — functional surface refreshed.

Two synthesizer-side design choices, both documented in code:

- `expected_network_id` is derived from the *optional* Shelley-genesis
  `networkMagic` (the node uses the mandatory
  `NodeConfigFile::network_magic`) — a minor divergence; `networkMagic`
  is present in every vendored mainnet/preprod/preview genesis.
- The Byron→Shelley boundary scalars default to `(None, None, 21_600)`
  — they are yggdrasil-internal node-config keys, absent from every
  genesis file, and the synthesizer forges a single-era Shelley-stamped
  chain, so the defaults are exact.

## Verification

- Focused (`yggdrasil-db-synthesizer`): `cargo test` — 89 lib tests
  (+1 new) + 7 integration tests pass; the new test confirms
  `build_shelley_genesis_bootstrap` / `build_protocol_parameters` /
  `build_genesis_enact_state` all succeed on the multi-era fixture.
- Full workspace: `cargo fmt --all -- --check`, `cargo check-all`,
  `cargo lint` all green; `cargo test --workspace --all-features
  --no-fail-fast` — **6,540 passing, 0 failing** (+1 over the R3c-1a
  baseline of 6,539 — exactly the new test).

## Remaining (A3 R3c)

R3c-1 is complete. Remaining:

- **R3c-2** — bulk credentials → `Vec<BlockProducerCredentials>`.
- **R3c-3** — thread `InitialForgeState` through `run_forge`'s
  `ForgeState`, applying `LedgerState` + `NonceEvolutionState` per block
  (blocks stay structural — a four-gates-green intermediate).
- **R3c-4** — real Praos forge (`check_should_forge` + `forge_block`).
- **R3c-5** — epoch-boundary stake-distribution rebuild.
- **R3c-6** — `FileImmutable` → `ChainDb` migration (the `db-analyser`
  exit gate).
