# Round 512 — db-synthesizer A3 R3b-3: CardanoProtocolParams aggregator

**Date:** 2026-05-18
**Area:** sister-tools / `crates/tools/db-synthesizer`
**Upstream reference:** IntersectMBO/cardano-node 11.0.1 —
`Ouroboros.Consensus.Cardano.Node` (`CardanoProtocolParams`),
`Cardano.Node.Protocol.Cardano` (`mkConsensusProtocolCardano`),
`Cardano.Tools.DBSynthesizer.Run` (`initProtocol`).

## Summary

R3b-3 — the final slice of the R3b consensus-config arc — adds the
`CardanoProtocolParams` aggregator and the `mk_consensus_protocol_cardano`
/ `load_consensus_protocol` orchestration that folds the R3b-1 genesis
bundle and the R3b-2 per-era protocol configs. **A3 R3b is now
complete** (R3b-1 genesis bundle, R3b-2 per-era configs, R3b-3
aggregator).

## Design — a synthesizer-scoped simplified aggregate

Grounding established that faithful mirrors of upstream's
`CardanoProtocolParams` field types would require whole new
hard-fork-combinator subsystems — `CardanoHardForkTriggers` as a typed
`NP` n-ary product, an era-crossing `TransitionConfig`,
`ProtocolParamsByron` carrying the full Byron `Genesis.Config` — none of
which exist in `crates/consensus`, and none of which a single-era
synthesizer forge consumes. So `CardanoProtocolParams` is a
**synthesizer-scoped 6-field aggregate**: it keeps the upstream field
*names* with types wrapping the already-built R3b-1 / R3b-2 artifacts.
The struct carries a `## Naming parity` → `**Strict mirror:** none.`
stanza citing the upstream record.

## Changes

- `run.rs`:
  - new `CardanoProtocolParams` — 6 fields (`byron_protocol_params`,
    `shelley_based_protocol_params`, `cardano_hard_fork_triggers`,
    `cardano_ledger_transition_config`, `cardano_checkpoints`,
    `cardano_protocol_version`) — plus supporting types `HardForkTrigger`
    (2-variant: `AtDefaultVersion` / `AtEpoch`), `CardanoHardForkTriggers`
    (7 per-era triggers), `ShelleyBasedProtocolParams`, `CheckpointsMap`.
  - `mk_consensus_protocol_cardano` — folds R3b-2's Byron + hard-fork
    configs and R3b-1's `GenesisBundle` into `CardanoProtocolParams`:
    the hard-fork triggers are case-mapped from the `Test*HardForkAtEpoch`
    fields; the protocol version is `(11, 0)` / `(10, 7)` gated on
    `test_enable_development_hard_fork_eras` (upstream `Cardano.hs`).
  - `load_consensus_protocol(config_path)` — the protocol-building half
    of upstream `initProtocol`: parses the Byron + hard-fork configs
    from `node_config`, loads the genesis bundle, folds them.
  - `load_genesis_bundle` split into a `load_genesis_bundle_from_stub`
    core, shared with `load_consensus_protocol`.
  - new `RunError::ProtocolConfigParse`; the `write_config` test helper
    gains the `LastKnownBlockVersion-{Major,Minor}` keys (required by
    the Byron config parser). 3 new tests.
- `AGENTS.md` — functional surface refreshed; R3b marked complete.

## Scope note

R3b-1 had flagged genesis-hash verification as "deferred to R3b-3".
Grounding upstream `initProtocol` showed it passes `Nothing` for the
Shelley / Alonzo / Conway / Dijkstra genesis hashes — upstream
db-synthesizer does **not** verify them. There is nothing to fold in;
R3b-3 mirrors upstream by not adding a hash-verification step.
`load_consensus_protocol` produces `CardanoProtocolParams`; wiring it
into the actual forge — and a full `initialize` returning
`(DBSynthesizerConfig, CardanoProtocolParams)` — is R3c integration.

## Verification

- Focused (`yggdrasil-db-synthesizer`): `cargo test` — 88 lib tests
  (+3 new) + 7 integration tests pass.
- Full workspace: `cargo fmt --all -- --check`, `cargo check-all`,
  `cargo lint` all green; `cargo test --workspace --all-features
  --no-fail-fast` — **6,539 passing, 0 failing** (+3 over the R3b-2
  baseline of 6,536 — exactly the new tests).
- A plain `cargo test-all` aborts early on the documented timing-flaky
  cardano-tracer test (cargo's default fail-fast); `--no-fail-fast` ran
  every crate clean.

## Remaining (A3 R3c)

The Praos forge loop — re-architect `run_forge` to thread an evolving
ledger state + `ChainDepState` per slot, run `check_should_forge` (VRF)
and `forge_block` (KES), consuming `CardanoProtocolParams` + R3a
credentials, so the synthesized chain is Praos-valid rather than the
`SYNTH_ERA` structural stamp. "The hard part" per the roadmap.
