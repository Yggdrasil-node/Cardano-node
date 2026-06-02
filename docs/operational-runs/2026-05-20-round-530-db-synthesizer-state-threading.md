# R530 db-synthesizer State Threading

Date: 2026-05-20

## Goal

Close db-synthesizer Phase 4 R3c-3 by threading the genesis-seeded ledger and
nonce state through the structural forge loop. This moves the tool one slice
closer to upstream `runForge`, where ChainDB adoption advances the ledger and
chain-dependency state on every accepted block.

## Upstream references

- `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Forging.hs`
  - `runForge`, `ForgeState`, `goSlot`, `applyChainTick`, `tickChainDepState`,
    and `forgeBlock`.
- `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Run.hs`
  - `synthesize`, `pInfoInitLedger`, ChainDB open, tip resume, and `runForge`.

## Rust scope

- `crates/tools/db-synthesizer/src/forging.rs`
- `crates/tools/db-synthesizer/src/run.rs`
- `crates/tools/db-synthesizer/src/status.rs`
- `crates/tools/db-synthesizer/AGENTS.md`
- `docs/COMPLETION_ROADMAP.md`

## Result

`forging::ForgeState` now carries `LedgerState` and `NonceEvolutionState`.
`run_forge_with_state` replays any existing ChainDB prefix into that state
before append-mode forging, then applies every newly synthesized structural
block to cloned ledger/nonce state before appending it. `synthesize_from_config`
now passes the genesis-seeded `InitialForgeState` and a Shelley-genesis-derived
`NonceEvolutionConfig` into the forge loop.

The block producer is still structural and non-Praos. R3c-4 remains the slice
that replaces `synth_structural_block` with `checkShouldForge` plus
`forgeBlock`.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-db-synthesizer`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python dev/test/check-stale-placement.py`
- `python dev/test/check-parity-matrix.py`
- `python dev/test/check-strict-mirror.py --fail-on-violation`
- `python dev/test/filetree.py check`
- `git diff --check`
