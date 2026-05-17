# Round 504 — db-synthesizer Phase 4 R2: genesis / config loading

**Date:** 2026-05-17
**Area:** sister-tools / `crates/tools/db-synthesizer`
**Upstream reference:** IntersectMBO/cardano-node 11.0.1 —
`Cardano.Tools.DBSynthesizer.Run.initialize` (`initConf`).

## Summary

db-synthesizer's Phase 4 R1 slice forged with a hard-coded
`STUB_EPOCH_SIZE = 432_000` for every invocation, ignoring the
`--config` node config. This round (db-synthesizer Phase 4 R2) ports
the genesis-loading half of upstream `Run.initialize`: the synthesizer
now resolves the real Shelley-genesis `epochLength` from the operator's
`config.json`.

Observable difference closed: on a preview config (`epochLength =
86_400`) a `--epochs N` invocation previously over-forged by 5× against
the stubbed 432_000.

## Parity basis

Upstream `Cardano.Tools.DBSynthesizer.Run`
(`.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Run.hs`):

- `initialize` / `initConf` (Run.hs:64–141) reads `config.json`,
  parses a `NodeConfigStub`, resolves the embedded genesis paths
  relative to the config file's directory (`relativeToConfig`), and
  loads the Shelley genesis.
- `synthesize` (Run.hs:150–154) sets `epochSize = sgEpochLength
  confShelleyGenesis` — the forge-loop epoch size is the genesis
  `epochLength`, not a constant.

The protocol-building half (`initProtocol` /
`mkConsensusProtocolCardano` — the multi-era hard-fork plan) remains
the db-synthesizer R3 carve-out.

## Changes

- `Cargo.toml` — added the `yggdrasil-node-genesis` workspace
  dependency.
- `run.rs` — new `resolve_epoch_size_from_config` (mirror of
  `initConf`'s genesis-load: `config.json` → `NodeConfigStub` →
  config-dir-relative path resolution → `load_shelley_genesis` →
  `epoch_length`) and `synthesize_from_config` (production entry
  point). `STUB_EPOCH_SIZE` renamed `DEFAULT_EPOCH_SIZE` (now only the
  config-free convenience constant). 4 new `RunError` variants
  (`ConfigRead` / `ConfigParse` / `ConfigStub` / `GenesisLoad`).
- `lib.rs` — `run` dispatches through `synthesize_from_config`,
  passing `args.paths.config`.
- `status.rs` — the `ForgeLoopStatus` carve-out descriptor refreshed:
  genesis loading is no longer a deferred carve-out; only the
  Praos-forging path (R3) remains.
- `tests/integration.rs` — `args_for` now writes a real `config.json`
  + Shelley genesis (R1's fake `/unused/config.json` no longer works
  now that `run` genuinely reads the config).
- `AGENTS.md` — status line + carve-out inventory refreshed to post-R2.

## Verification

- Focused (`yggdrasil-db-synthesizer`): `cargo fmt`, `cargo check
  --all-targets`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test` all green — **87 tests pass** (78 lib + 2 + 7
  integration), including 6 new R2 tests:
  `resolve_epoch_size_reads_non_default_epoch_length`,
  `resolve_epoch_size_resolves_genesis_path_relative_to_config_dir`,
  `resolve_epoch_size_errors_on_missing_config`,
  `resolve_epoch_size_errors_on_missing_genesis`,
  `resolve_epoch_size_errors_on_non_cardano_protocol`,
  `synthesize_from_config_creates_chain_db`.
- Full workspace: `cargo fmt --all -- --check`, `cargo check-all`,
  `cargo lint` all green; `cargo test-all` — **6,525 tests passing,
  0 failing** (+6 R2 tests over the prior 6,519 baseline).

## Remaining (db-synthesizer R3)

The Praos forge path — `initProtocol` / `mkConsensusProtocolCardano`
(hard-fork era plan) + `checkShouldForge` (VRF/KES/OpCert leader
check) + KES-signed `forgeBlock`. Until R3, synthesized blocks remain
deterministic non-Praos structural blocks stamped `Era::Shelley`
(`SYNTH_ERA`).
