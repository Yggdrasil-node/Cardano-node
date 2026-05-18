# Guidance for the pure-Rust port of upstream `db-synthesizer`.

**Status:** `partial` (Phase 4 R1 forge-loop + R2 genesis-loading
slices shipped — see below). The Praos VRF/KES/OpCert forge path
(R3) remains. Scope band: **MEDIUM**.

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

Synthetic chain generator for stress tests. Phase C.1 mini-arc R408-R415 (8 rounds, MEDIUM). R411 leverages `node/src/block_producer.rs` Forging logic.

## Current functional surface (post Phase 4 R3b-1)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Typed `parser::Args` dispatch — forge-limit (slot/block/epoch) +
  open-mode (create/create-force/append) parsed + validated.
- ✅ Forge loop (Phase 4 R1) — `forging.rs` ports `Forging.hs`'s
  `runForge` control loop (`ForgeState`, `forgingDone`,
  `nextForgeState`); `run.rs` ports `Run.hs`'s `preOpenChainDB` +
  `synthesize`. `lib::run` synthesizes a real on-disk ChainDB; the
  `ForgeLoopDeferred` stub is retired. Blocks are deterministic,
  prev-hash-threaded, **non-Praos** structural blocks — see the
  carve-out inventory below.
- ✅ Genesis loading (Phase 4 R2) — `run::resolve_epoch_size_from_config`
  reads `--config`, parses the `NodeConfigStub`, resolves the genesis
  paths relative to the config directory, and loads the real
  Shelley-genesis `epochLength`. (The synthesis era stays a structural
  `Shelley` stamp until the R3 hard-fork plan.)
- ✅ Multi-era genesis bundle (Phase 4 R3b-1) — `run::load_genesis_bundle`
  reads every era's genesis (Byron / Shelley / Alonzo / Conway) into a
  typed `GenesisBundle` and derives the initial Praos nonce
  (`genesisHashToPraosNonce`); `run::synthesize_from_config` is the
  production entry point. The per-era protocol configs + hard-fork
  triggers (R3b-2) and the `CardanoProtocolParams` aggregator (R3b-3)
  remain.
- 🟡 Praos forge path (Phase 4 R3) — the synthesized chain is
  structurally valid but not Praos-valid until the VRF/KES/OpCert
  leader check + KES-signed `forgeBlock` land.
- ❌ Byte-equivalence soak vs the upstream binary's ChainDB chunk
  format — deferred to the integration round.

## Carve-out inventory (Phase 4 R3 deferral surface)

The Phase 4 R1 forge-loop slice (`forging.rs` + `run.rs`) and the R2
genesis-loading slice (`run::synthesize_from_config`) are shipped.
`crates/tools/db-synthesizer/src/status.rs` ships `forge_loop_status()`
returning a `ForgeLoopStatus` descriptor of the one surviving carve-out:

| Carve-out         | Slice | Deferral rationale (one-liner)                                            |
|-------------------|-------|---------------------------------------------------------------------------|
| Praos forge path  | R3    | `checkShouldForge` (VRF/KES/OpCert leader check) + KES-signed `forgeBlock`, leveraging `crates/node/block-producer`, plus `initProtocol` / `mkConsensusProtocolCardano` for the hard-fork era plan. Until then `synthesize` emits deterministic non-Praos structural blocks stamped `SYNTH_ERA`. |

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-db-synthesizer

# Run via the universal launcher (recommended).
crates/node/yggdrasil-node/scripts/run-tools.sh db-synthesizer --help
crates/node/yggdrasil-node/scripts/run-tools.sh db-synthesizer --version

# Or invoke the binary directly:
target/release/db-synthesizer --help
```

The binary is named `db-synthesizer` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
once concrete dispatch lands at `R409+`.

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
  `node/src/upstream_pins.rs` as a coordinated round.

## Round roadmap

Per the R326-R459 plan, this crate's full implementation lands across
the named mini-arc rounds:

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at R335-R336).
- 🟡 Next: **R409** — first concrete-impl round of the mini-arc.
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
