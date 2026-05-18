# Round 510 — db-synthesizer A3 R3b-1: multi-era genesis bundle

**Date:** 2026-05-18
**Area:** sister-tools / `crates/tools/db-synthesizer`
**Upstream reference:** IntersectMBO/cardano-node 11.0.1 —
`Cardano.Node.Protocol.Cardano` (`mkConsensusProtocolCardano`
genesis-reading), `Cardano.Node.Protocol.Shelley`
(`genesisHashToPraosNonce`), `Cardano.Tools.DBSynthesizer.Run`
(`initialize`).

## Summary

db-synthesizer's R2 slice loaded only the Shelley genesis (for the
`epochLength`). This round — A3 R3b-1, the first slice of the R3b
consensus-config arc — lifts that to a typed `GenesisBundle` covering
every era (Byron / Shelley / Alonzo / Conway) plus the initial Praos
nonce. It is the genesis-reading half of upstream
`mkConsensusProtocolCardano`.

The slice is pure wiring of existing yggdrasil machinery:
`crates/node/genesis` already parses every era's genesis and exposes
`shelley_genesis_hash_to_praos_nonce`. New code is one aggregator struct
and one loader fn — no new crypto / ledger / consensus algorithm.

## Parity basis

- Upstream `mkConsensusProtocolCardano` (`Cardano.Node.Protocol.Cardano`)
  reads each era's genesis file inline while building
  `CardanoProtocolParams`. `GenesisBundle` has no upstream type — it is
  the yggdrasil-side collection of exactly that genesis-reading step,
  surfaced as a typed value for the R3b-3 orchestration to fold.
- `genesisHashToPraosNonce` (`Cardano.Node.Protocol.Shelley`): the
  initial Praos nonce is the Blake2b-256 hash of the Shelley genesis
  file's raw bytes — mirrored by `genesis::shelley_genesis_hash_to_praos_nonce`.

## Changes

- `run.rs`:
  - new `GenesisBundle { byron, shelley, alonzo, conway, praos_nonce }`.
  - new `load_genesis_bundle(config_path)` — reads every era's genesis
    via the existing `yggdrasil-node-genesis` loaders and derives the
    initial Praos nonce from the Shelley genesis file hash.
  - `resolve_node_config_stub` extracted — the config-read +
    config-dir-relative path resolution now shared by
    `resolve_epoch_size_from_config` and `load_genesis_bundle`.
  - `synthesize_from_config` builds the bundle and forges with
    `bundle.shelley.epoch_length`.
  - `RunError::GenesisLoad` message generalized (Shelley → any era).
  - the `write_config` test helper writes every era's genesis (Byron
    `{}`, a minimal `AlonzoGenesis`-parseable fixture, Conway `{}`);
    2 new tests (`load_genesis_bundle` loads every era + derives a
    concrete nonce; a missing era genesis errors with `GenesisLoad`).
- `tests/integration.rs` — `args_for` extended to write every era's
  genesis (`run` now loads the full bundle).
- `AGENTS.md` — functional surface refreshed for R3b-1.

Scope: R3b-1 stops at the genesis bundle. The per-era protocol configs
+ hard-fork triggers (R3b-2), the `CardanoProtocolParams` aggregator and
`mk_consensus_protocol_cardano` orchestration (R3b-3), and genesis-hash
verification (folded into R3b-3, where the config's `*GenesisHash`
fields are parsed) remain. Dijkstra is omitted — that era is not yet
activated in yggdrasil (no `load_dijkstra_genesis`). The synthesis era
stays `SYNTH_ERA` (structural Shelley stamp) until R3c.

## Verification

- Focused (`yggdrasil-db-synthesizer`): `cargo test` — 80 lib tests
  (+2 new) + 7 integration tests pass; `synthesize_from_config_creates_chain_db`
  and the integration tests exercise the multi-era fixtures.
- Full workspace: `cargo fmt --all -- --check`, `cargo check-all`,
  `cargo lint` all green; `cargo test --workspace --all-features
  --no-fail-fast` — **6,531 passing, 0 failing** (+2 over the R508
  baseline of 6,529 — exactly the new tests).
- A plain `cargo test-all` aborts early on the documented timing-flaky
  cardano-tracer test (cargo's default fail-fast); `--no-fail-fast` ran
  every crate clean.

## Remaining (A3 R3b)

- **R3b-2** — six `Node{Byron,Shelley,Alonzo,Conway,Dijkstra}ProtocolConfiguration`
  serde parsers + `NodeHardForkProtocolConfiguration` (hard-fork
  triggers), off the stashed `NodeConfigStub` JSON value.
- **R3b-3** — the `CardanoProtocolParams` 6-field aggregator mirror +
  `mk_consensus_protocol_cardano` folding R3b-1 + R3b-2 + R3a
  credentials; also absorbs genesis-hash verification.

Then R3c — the Praos forge loop.
