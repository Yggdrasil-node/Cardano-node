# Guidance for the pure-Rust port of upstream `db-synthesizer`.

**Status:** `functional` (Phase 4 R1/R2/R3b/R3c-1..R3c-5 shipped —
see below). The Praos VRF/KES/OpCert forge path is live and leader
sigma is derived from ledger-view stake snapshots. Remaining gate:
upstream ChainDB byte-equivalence soak. Scope band: **MEDIUM**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/` (6 `.hs` files).

## Mini-arc scope

Synthetic chain generator for stress tests. Phase C.1 mini-arc R408-R415 (8 rounds, MEDIUM). R411 leverages `crates/node/block-producer/src/lib.rs` forging logic.

## Current functional surface (post Phase 4 R3c-5)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Typed `parser::Args` dispatch — forge-limit (slot/block/epoch) +
  open-mode (create/create-force/append) parsed + validated.
- ✅ Forge loop (Phase 4 R1) — `forging.rs` ports `Forging.hs`'s
  `runForge` control loop (`ForgeState`, `forgingDone`,
  `nextForgeState`); `run.rs` ports `Run.hs`'s `preOpenChainDB` +
  `synthesize`. `lib::run` synthesizes a real on-disk ChainDB when
  forgers are supplied and returns before opening the DB when the
  forger set is empty, matching upstream `Run.hs`.
- ✅ Genesis loading (Phase 4 R2) — `run::resolve_epoch_size_from_config`
  reads `--config`, parses the `NodeConfigStub`, resolves the genesis
  paths relative to the config directory, and loads the real
  Shelley-genesis `epochLength`. (The synthesis era stays a structural
  `Shelley` stamp until the R3 hard-fork plan.)
- ✅ Multi-era genesis bundle (Phase 4 R3b-1) — `run::load_genesis_bundle`
  reads every era's genesis (Byron / Shelley / Alonzo / Conway) into a
  typed `GenesisBundle` and derives the initial Praos nonce
  (`genesisHashToPraosNonce`); `run::synthesize_from_config` is the
  production entry point.
- ✅ Per-era protocol-config types (Phase 4 R3b-2) — `types.rs` declares
  `NodeByronProtocolConfiguration` (9 fields),
  `NodeHardForkProtocolConfiguration` (8 fields), and the four
  `Node{Shelley,Alonzo,Conway,Dijkstra}ProtocolConfiguration` records,
  mirroring `unstable-cardano-tools/Cardano/Node/Types.hs`. Byron +
  HardFork derive `Deserialize` (the `Orphans.hs` `FromJSON` carve-out).
- ✅ Consensus protocol params (Phase 4 R3b-3 — **R3b complete**) —
  `run::load_consensus_protocol` / `run::mk_consensus_protocol_cardano`
  fold the genesis bundle (R3b-1) + per-era configs (R3b-2) into
  `CardanoProtocolParams` — a synthesizer-scoped 6-field mirror of
  upstream `Ouroboros.Consensus.Cardano.Node` (`CardanoHardForkTriggers`
  case-mapped from the hard-fork config; `(major, minor)` protocol
  version). R3c-4 consumes this in the production Praos forge path.
- ✅ Initial forge state (Phase 4 R3c-1a/1b) — `run::load_initial_forge_state`
  builds the genesis-seeded initial `LedgerState` (via the shared
  `yggdrasil-node-genesis::build_base_ledger_state`) plus the Praos
  `NonceEvolutionState`, returned as `InitialForgeState`.
- ✅ Leader credentials (Phase 4 R3c-2) — `run::read_leader_credentials`
  builds the synthesizer's `Vec<BlockProducerCredentials>` forger set:
  the union of the singleton CLI cert/vrf/kes triple
  (`load_block_producer_credentials`) and the inline-triple bulk file
  (`load_bulk_block_producer_credentials` — a new
  `yggdrasil-node-block-producer` port of `readLeaderCredentialsBulk`).
  The forger set is consumed by the R3c-4 leader-check slice.
- ✅ Evolving forge state (Phase 4 R3c-3) — `forging::ForgeState`
  now carries `LedgerState` + `NonceEvolutionState`;
  `run::synthesize_from_config` passes the genesis-seeded
  `InitialForgeState` into `run_forge`; append-mode runs
  replay the existing ChainDB prefix into the supplied initial state
  before forging more blocks.
- ✅ Praos forge path (Phase 4 R3c-4) — the production path uses the
  shared `crates/node/block-producer` `checkShouldForge` /
  `forgeBlock` equivalents: VRF/KES/OpCert leader checking, KES-signed
  headers, raw Conway block CBOR persistence, and no-forgers early
  return.
- ✅ Stake-distribution rebuild (Phase 4 R3c-5) — the production
  forge path derives per-forger Praos sigma from `StakeSnapshots.set`,
  seeds the initial forecast snapshot from Shelley genesis
  `staking.pools` / `staking.stake` / `initialFunds`, activates
  genesis pools on the first Shelley-family block, and runs epoch
  boundaries through the shared ledger `apply_epoch_boundary` path.
- ❌ Byte-equivalence soak vs the upstream binary's ChainDB chunk
  format — deferred to the integration round.

## Closeout inventory (post Phase 4 R3c-5)

The Phase 4 R1 forge-loop slice (`forging.rs` + `run.rs`), the R2
genesis-loading slice (`run::synthesize_from_config`), the R3b
consensus-protocol slice, and R3c-1..R3c-5 state / credential /
Praos-forge / stake-distribution slices are shipped.
`crates/tools/db-synthesizer/src/status.rs` ships `forge_loop_status()`
returning a `ForgeLoopStatus` descriptor of the remaining closeout gate:

| Carve-out         | Slice | Deferral rationale (one-liner)                                            |
|-------------------|-------|---------------------------------------------------------------------------|
| ChainDB byte-equivalence soak | closeout | Compare yggdrasil output against upstream `db-synthesizer` chunk output with matching staked genesis + credentials before operator swap-in. |

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-db-synthesizer

# Run via the universal launcher (recommended).
scripts/run-tools.sh db-synthesizer --help
scripts/run-tools.sh db-synthesizer --version

# Or invoke the binary directly:
target/release/db-synthesizer --help
```

The binary is named `db-synthesizer` (matching upstream exactly).
Operator swap-in remains gated on the upstream ChainDB
byte-equivalence soak.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `db-synthesizer` is the
  acceptance gate for any concrete implementation.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new release with different help output, refresh the
  fixtures + bump the relevant SHA pin in
  `crates/node/config/src/upstream_pins.rs` as a coordinated round.

## Round roadmap

Per the R326-R459 plan plus the R3c closeout slices, this crate's full
implementation lands across the named mini-arc rounds:

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at R335-R336).
- ✅ R1/R2/R3b/R3c-1..R3c-5 shipped through the stake-based Praos
  forge path.
- 🟡 Closeout — when all subcommands are functional, parity-matrix
  entry advances `partial → verified_11_0_1`. Operators can then
  swap upstream binary for the yggdrasil binary without script
  changes.

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-db-synthesizer

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/db-synthesizer --help) \
     <(target/debug/db-synthesizer --help)
diff <(.reference-haskell-cardano-node/install/bin/db-synthesizer --version) \
     <(target/debug/db-synthesizer --version)
# (empty diffs expected — byte-equivalent)
```

## Maintenance Guidance

- Update this AGENTS.md when concrete subcommand implementations
  land (replace `❌ not yet implemented` rows with `✅ shipped` +
  round number).
- Keep the per-tool migration round numbers in sync with the
  authoritative plan file at `/home/daniel/.claude/plans/playful-tickling-plum.md`.
- If upstream ships a new release: refresh the help/version
  fixtures, advance the relevant SHA pin in `upstream_pins.rs`,
  re-run the full cargo gate.
