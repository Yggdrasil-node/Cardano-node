# Round 514 — A3 R3c-1a: shared `build_base_ledger_state` extraction

**Date:** 2026-05-19
**Area:** node sub-crates / `crates/node/{genesis,yggdrasil-node}`
**Upstream reference:** the node's `strict_base_ledger_state` mirrors
upstream's initial-ledger seeding (`Cardano.Node.Configuration.POM`
validation + `Ouroboros.Consensus.Node.Genesis` seeding).

## Summary

R3c-1a — the first sub-slice of A3 R3c-1 — extracts the
genesis→`LedgerState` construction out of the node's
`strict_base_ledger_state` (which lives in the `yggdrasil-node` *binary*
crate, tied to `NodeConfigFile`) into a shared `build_base_ledger_state`
in the `yggdrasil-node-genesis` *library* crate. This lets the
db-synthesizer (R3c-1b) seed a byte-identical initial ledger state
without duplicating ~115 drift-prone lines — a parity guarantee that
duplication would not give.

This is a behavior-preserving refactor: the node's
`strict_base_ledger_state` keeps its exact signature and behavior.

## Changes

- `genesis/src/lib.rs`:
  - new `BaseLedgerStateInputs` — a 10-field struct of the pre-loaded
    genesis pieces (Byron UTxO entries, Shelley bootstrap, protocol
    params, enact state) + the scalar runtime config (network id, the
    Byron→Shelley boundary, epoch length, `f`, `k`).
  - new `build_base_ledger_state(inputs) -> LedgerState` — the
    pure-construction half of `strict_base_ledger_state`: Byron
    genesis-UTxO seeding, Shelley UTxO / stake / delegation staging,
    `reserves` from `max_lovelace_supply - circulation`, epoch / slot /
    active-slot-coeff config, the Byron→Shelley transition, the
    `3k/f` stability window, protocol params, and enact state. Returns
    `LedgerState` directly — every step is infallible; all genesis I/O
    and hash verification stay in the caller. Carries a `## Naming
    parity` / `**Strict mirror:** none.` stanza.
- `startup.rs`:
  - `strict_base_ledger_state` shrinks from ~115 lines to ~35: verify
    genesis hashes → load the four genesis pieces (each `wrap_err`-ed)
    → pack `BaseLedgerStateInputs` → `build_base_ledger_state`. Its
    `pub fn` signature is unchanged. The now-construction-only
    `GenesisDelegationState` / `StakeCredential` imports are trimmed
    (`Era` retained — `best_effort_base_ledger_state` still uses it).
- `genesis/AGENTS.md` — the new shared builder noted.

## Verification

Four gates green: `cargo fmt --all -- --check`, `cargo check-all`,
`cargo lint`; `cargo test --workspace --all-features --no-fail-fast` —
**6,539 passing, 0 failing — unchanged from the R3b-3 baseline** (a
behavior-preserving refactor adds no tests, removes none). The existing
`strict_base_ledger_state_seeds_preview_reserves_from_genesis_supply`
test (`yggdrasil-node/src/main_tests.rs`) exercises the full
Byron+Shelley seeding path end-to-end through the new builder — its
passing is the behavior-preservation proof.

## Remaining (A3 R3c)

- **R3c-1b** — db-synthesizer builds its initial `LedgerState` from the
  R3b-1 `GenesisBundle` via `build_base_ledger_state`, plus
  `NonceEvolutionState::new(praos_nonce)`.
- Then **R3c-2…R3c-6** — bulk credentials, threading the evolving
  ledger + nonce state through `run_forge`, the real Praos forge,
  the epoch-boundary stake rebuild, and the `FileImmutable` → `ChainDb`
  migration.
