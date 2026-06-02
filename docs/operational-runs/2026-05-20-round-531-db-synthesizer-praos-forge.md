# R531 db-synthesizer Praos Forge

Date: 2026-05-20

## Goal

Close db-synthesizer Phase 4 R3c-4 by replacing the production structural
forge path with the shared Praos leader-check and KES-signed block-forging
surface, while keeping the remaining stake-distribution rebuild explicit.

## Upstream references

- `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Forging.hs`
  - `goSlot`, `checkShouldForge`, first-leader selection, `forgeBlock`, and
    adoption after state transition.
- `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Run.hs`
  - no-forgers early return, ChainDB open, tip resume, and `runForge`.
- `crates/node/block-producer/src/lib.rs`
  - `check_should_forge`, `forge_block`, and
    `forged_block_to_storage_block`.

## Rust scope

- `crates/tools/db-synthesizer/src/forging.rs`
- `crates/tools/db-synthesizer/src/run.rs`
- `crates/tools/db-synthesizer/src/lib.rs`
- `crates/tools/db-synthesizer/src/status.rs`
- `crates/tools/db-synthesizer/tests/integration.rs`
- `crates/tools/db-synthesizer/AGENTS.md`
- `docs/COMPLETION_ROADMAP.md`

## Result

`run::synthesize_from_config` now loads the full consensus protocol, builds
the genesis-seeded `ForgeState`, reads singleton plus bulk leader credentials,
and calls `forging::run_forge`. If the forger set is empty, the run returns
`ForgeResult 0` before opening or creating the ChainDB, matching upstream.

`forging::run_forge` now replays existing blocks into ledger / nonce state,
uses the current epoch nonce for `check_should_forge`, skips non-leader slots,
forges via `forge_block`, persists the raw Conway CBOR with
`forged_block_to_storage_block`, and commits the ledger / nonce transition only
after the append succeeds. Append replay extracts the Praos VRF output from raw
Conway block CBOR when available, falling back to structural hashes only for
older structural test blocks.

R3c-5 remains open: the leader-check stake fraction is still a documented
temporary full-stake synthesizer placeholder until the forecast-ledger-view
stake-distribution rebuild is ported.

## Verification

- `cargo fmt --all -- --check`
- `cargo check -p yggdrasil-db-synthesizer`
- `cargo test -p yggdrasil-db-synthesizer`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python dev/test/check-parity-matrix.py`
- `python dev/test/check-stale-placement.py`
- `python dev/test/check-fixture-manifest.py`
- `python dev/test/check-strict-mirror.py --fail-on-violation`
- `python dev/test/filetree.py check`
- `git diff --check`
