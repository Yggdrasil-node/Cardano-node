---
title: Completion Roadmap
layout: default
parent: Reference
nav_order: 6
---

# Completion Roadmap

**Created:** 2026-05-17 · **Status reference:** R503 + post-R503 commit-level work

## Purpose

This is the forward-looking backlog of work remaining to take Yggdrasil from
its current state — a verified, code-level parity-closed core node — to *full*
completion of the project's stated goal: 100% protocol, naming, functionality,
and filename parity with upstream `cardano-node`, including the full
sister-tool suite.

It complements, and does not replace:

- [`PARITY_SUMMARY.md`](PARITY_SUMMARY.md) / [`PARITY_PROOF.md`](PARITY_PROOF.md) / [`UPSTREAM_PARITY.md`](UPSTREAM_PARITY.md) — what is *already* done and proven.
- [`TECH-DEBT.md`](TECH-DEBT.md) — narrow, named consolidation-debt items.
- [`parity-matrix.json`](parity-matrix.json) — the machine-readable per-feature inventory.

**Honest framing.** Categories B and C below *cannot be closed in a normal
development environment.* They require a long-running operator soak, or a
running upstream Haskell `cardano-node` for byte-level wire comparison. They
are listed so the full path is visible — not because they are executable in
one session.

## Status snapshot

- **Build health baseline (verified 2026-05-17):** all four cargo gates green on Rust 1.95.0 — `cargo fmt`,
  `cargo check-all`, `cargo lint`, `cargo test-all` (**6,519 tests passing,
  0 failing, 3 ignored**). `check-strict-mirror.py` + `check-fixture-manifest.py`
  clean.
- **Core node:** crypto, ledger, storage, consensus, mempool, network, plutus,
  and the `crates/node/*` runtime crates are feature-complete for syncing and
  validating the official networks (code-level parity closure, v0.2.0).
- **Parity matrix (updated through R532):** 22 tracked entries — 2 `verified_11_0_1`,
  12 `implemented_needs_11_0_1_evidence`, 8 `partial`.
- **Sister tools (13):** 1 verified (`bech32`); 4 functional-pending-soak
  (`cardano-cli` with 40 operational subcommands, `cardano-submit-api`,
  `db-truncater`, `db-synthesizer`); 2 functional-partial (`cardano-tracer`,
  `db-analyser`); 6 skeleton (`kes-agent`, `kes-agent-control`,
  `snapshot-converter`, `tx-generator`, `dmq-node`, `cardano-testnet`).
- **Reference parity tag:** `11.0.1`.

---

## Category A — executable now (no external dependency)

Needs only the workspace + the vendored reference tree. Each item is a
standard R-arc round series: one bounded slice per round, four cargo gates
green, one `docs/operational-runs/` doc, one commit. Use the
`continuous-agent-loop` + `round-extraction` skills; author a `parity-plan`
first for any slice touching protocol/CBOR/crypto behavior.

### A1 — Feature-flag gating  (`TECH-DEBT.md` §"Wave 3 / Wave 5 feature flags")
Status verified 2026-05-17: `forge` and `yggdrasil-plutus/{secp256k1,
bls12-381}` are already wired (real `#[cfg]` sites; `--no-default-features`
builds and `cargo lint-no-default` clean). The genuinely-inert flags that
remain are `yggdrasil-ledger/plutus` and `yggdrasil-network/{ntn,ntc}`
(0 `#[cfg]` sites). `ntc` is the cleanly wireable one — a relay/producer with
the node-to-client local socket excluded is still a valid node — but it is a
multi-crate round (`yggdrasil-network` NtC modules + the
`yggdrasil-node-ntc-server` crate + the binary's `query`/`submit-tx`
subcommands). `plutus` gates the Alonzo+ phase-2 witness paths across ~8
per-era ledger apply-rule files and needs a slim-build soundness decision
(a node without it skips phase-2 validation). `ntn` is required by every
node — a candidate for removal rather than wiring. **Exit:** the chosen flag
conditionally compiles the code it names; `cargo lint-no-default` stays green.

### A2 — cardano-cli subcommand migration — ✅ COMPLETE (verified 2026-05-20)
The cardano-cli C-arc closed at R515 and the R527-R529 stale-placement
cleanup moved the remaining query plans into the shared CLI crate.
`crates/tools/cardano-cli/src/command.rs` carries all **40 `Command`
variants**, `run.rs` dispatches them, and the crate has passing focused
coverage for the post-R529 LSQ additions. The standalone
`yggdrasil-cardano-cli` binary covers the
offline operator toolkit (keys / addresses / txid / sign / build / build-raw /
view), the full 27-query LocalStateQuery surface, and `transaction submit`.
Older central docs undercounted the migrated subcommands; the current surface
is the 40-command / 27-query split above, matching
`crates/tools/cardano-cli/AGENTS.md`. The only outstanding item is
byte-equivalence evidence against a real upstream `cardano-cli 11.0` binary —
Category-B operator-soak work, tracked by the `parity-matrix.json`
`sister-tool.cardano-cli` entry (`implemented_needs_11_0_1_evidence`).

### A3 — db-synthesizer Phase 4 R3 (R1 ✅, R2 ✅ done 2026-05-17)
`crates/tools/db-synthesizer` shipped the forge-loop slice (Phase 4 R1) and the
R2 genesis-loading slice (round 504, commit `a46bae1`). **Remaining — R3 is a
multi-slice arc.** The earlier "1 round" estimate was wrong: grounding (2026-05-18)
against upstream `runForge` showed the synthesizer's flat `FileImmutable` loop
carries none of the ledger state / `ChainDepState` / consensus config that real
Praos forging needs. Verified decomposition:

- **R3a — credential loading.** Wire the operator `cert`/`vrf`/`kes`/`bulk`
  paths into validated forger credentials. Credential model verified via the
  `haskell-reference-auditor` (2026-05-18, `Cardano.Node.Protocol.Shelley`
  `readLeaderCredentialsSingleton`/`Bulk`): the cold issuer VKey is the trailing
  32-byte field of the `NodeOperationalCertificate` text-envelope — there is no
  separate cold-vkey artifact or CLI flag upstream. Prep findings in
  `crates/node/block-producer`: `decode_opcert_cbor` already parses that cold
  vkey but discards it; `load_block_producer_credentials` takes a divergent
  separate `issuer_vkey_path`; the singleton path is missing upstream's
  `MismatchedKesKey` check (KES key ↔ opcert hot-vkey).
  - ✅ **Slice 1** (round 505, commit `55ee243`) — opcert loader carries the
    cold vkey: `decode_opcert_cbor` returns `(OpCert, Option<VerificationKey>)`
    and the new `load_operational_certificate_with_issuer` surfaces it. Carried
    *alongside* `OpCert` in a tuple — the 45-site `yggdrasil_consensus::OpCert`
    type is untouched (zero blast radius).
  - ✅ **Slice 2** (round 506, commit `91e1ee3`) — `MismatchedKesKey` check:
    `load_block_producer_credentials` now rejects a (KES key, opcert) pair
    where `derive_sum_kes_vk(kes_key) != operational_cert.hot_vkey`, mirroring
    upstream `opCertKesKeyCheck` (`Cardano.Node.Protocol.Shelley`). Additive —
    function signature unchanged, zero caller blast radius.
  - ✅ **Slice 3** (round 507, commit `0089c6a`) — `issuer_vkey_path`
    removal: `load_block_producer_credentials` drops the param and sources
    the issuer cold vkey from `load_operational_certificate_with_issuer`
    (a bare-`OCert` envelope → `OpCertMissingIssuerKey`). The
    upstream-divergent `--shelley-operational-certificate-issuer-vkey` CLI
    flag and `NodeConfigFile` field are removed end-to-end (cli / main / run /
    configuration / validate_config + the 3 config presets); the
    credential-policy set is now 3 fields, not 4. **A3 R3a is complete.**
    Follow-up (round 508): swept the removed flag from the producer scripts
    and the operator manual / runbooks.
- **R3b — consensus config.** Port `Run.initProtocol` /
  `mkConsensusProtocolCardano` — parse every era genesis file + the hard-fork
  config into the protocol params the leader check + forge need. Grounding
  (2026-05-18) verified R3b is a **3-slice arc and overwhelmingly wiring** of
  existing yggdrasil machinery: `crates/node/genesis` already parses every era
  genesis, verifies hashes, and exposes `shelley_genesis_hash_to_praos_nonce`;
  R3a supplies credential loading. New code is aggregator structs + JSON
  parsers + one orchestration fn — no new crypto / ledger / consensus algorithm.
  - ✅ **R3b-1** (round 510, commit `73ffcb4`) — multi-era genesis bundle:
    `load_genesis_bundle` reads every era's genesis (Byron / Shelley /
    Alonzo / Conway) into a typed `GenesisBundle` plus the initial Praos
    nonce, wiring the existing `genesis`-crate loaders;
    `synthesize_from_config` builds it. Hash verification deferred to
    R3b-3 (needs the config's `*GenesisHash` fields); Dijkstra omitted
    (era not yet activated — no `load_dijkstra_genesis`).
  - ✅ **R3b-2** (round 511, commit `f560692`) — per-era protocol-config
    types: `types.rs` declares `NodeByronProtocolConfiguration` (9 fields),
    `NodeHardForkProtocolConfiguration` (8 fields), and the four
    `Node{Shelley,Alonzo,Conway,Dijkstra}ProtocolConfiguration` records,
    mirroring db-synthesizer's vendored
    `unstable-cardano-tools/Cardano/Node/Types.hs`. Byron + HardFork carry
    `#[derive(Deserialize)]` (`#[serde(rename/default)]` mirroring
    `Orphans.hs`'s `FromJSON`); the 4 era structs need no deserializer.
    `RequiresNetworkMagic` reused from `yggdrasil-node-config`.
  - ✅ **R3b-3** (round 512, commit `a0e7b1b`) — `CardanoProtocolParams`
    aggregator: `run::load_consensus_protocol` / `mk_consensus_protocol_cardano`
    fold R3b-1's `GenesisBundle` + R3b-2's per-era configs into a
    synthesizer-scoped 6-field `CardanoProtocolParams` (upstream field
    *names*, simplified types — faithful mirrors of the hard-fork-combinator
    `NP` / `TransitionConfig` machinery are not on the db-synthesizer arc and
    a single-era forge consumes none of it). Hard-fork triggers case-mapped
    from `Test*HardForkAtEpoch`; `ProtVer` `(11,0)`/`(10,7)` on the dev flag.
    Genesis-hash verification: upstream `initProtocol` passes `Nothing` for
    the Shelley-family hashes — nothing to fold in. **A3 R3b is complete.**
  Scope boundary: R3b stops at the config bundle. Building the initial
  `ExtLedgerState` the synthesizer forges on (`pInfoInitLedger`) is R3c.
- **R3c — Praos forge loop.** Re-architect `run_forge` to forge Praos-valid
  blocks. Grounding (2026-05-18) verified R3c is a **6-slice arc** — the
  roadmap's "hard part". The forge *cryptography* is ~100% reuse
  (`crates/node/block-producer`, R3a-complete — `check_should_forge` /
  `forge_block` callable as-is; zero new VRF/KES/OpCert/CBOR code). What is
  genuinely new is the **offline state-evolution orchestration** — the
  synthesizer is the first yggdrasil forge path with no network and no wall
  clock, so it must own the ledger / nonce / stake evolution the runtime gets
  from the sync pipeline and upstream `runForge` gets from the ChainDB.
  Verified decomposition:
  - ✅ **R3c-1 — initial ledger + nonce state.** Construct the
    `pInfoInitLedger` analog. Grounding (2026-05-19) found the faithful
    genesis→`LedgerState` build is the ~115-line `strict_base_ledger_state`
    (`crates/node/cardano-node/src/startup.rs`) — UTxO + stake + delegs + protocol
    params + epoch config — which lives in the `yggdrasil-node` *binary*
    crate, tied to `NodeConfigFile`. The synthesizer needs the identical
    state; duplicating 115 drift-prone lines is wrong. Two sub-slices:
    - ✅ **R3c-1a** (round 514, commit `c19b8f8`) — extracted the shared
      `build_base_ledger_state` + `BaseLedgerStateInputs` into
      `yggdrasil-node-genesis`; `startup.rs::strict_base_ledger_state`
      refactored to load-pieces + call the shared builder. Behavior-
      preserving — node unchanged, four gates green (6,539 tests, the
      baseline, 0 fail).
    - ✅ **R3c-1b** (round 515, commit `c902285`) — db-synthesizer's
      `load_initial_forge_state` builds the genesis-seeded initial
      `LedgerState` (via the shared `build_base_ledger_state`) +
      `NonceEvolutionState`, returned as `InitialForgeState`. **A3 R3c-1
      is complete.**
  - ✅ **R3c-2 — bulk credentials + multi-forger** (round 516, commit
    `305e0b0`). `load_bulk_block_producer_credentials` — a
    `yggdrasil-node-block-producer` port of `readLeaderCredentialsBulk` —
    parses the inline `[cert,vrf,kes]` text-envelope triples;
    `run::read_leader_credentials` returns the singleton ∪ bulk
    `Vec<BlockProducerCredentials>` forger set. The per-slot loop picking
    the first leader is R3c-4.
  - ✅ **R3c-3 — thread evolving state** (round 530). The forge loop now threads
    `LedgerState` + `NonceEvolutionState` through `ForgeState`; append-mode
    runs replay the existing ChainDB prefix into the genesis-seeded state
    before forging more blocks; each new structural block applies to cloned
    ledger/nonce state before append. Blocks stay structural here — a
    four-gates-green intermediate.
  - ✅ **R3c-4 — real Praos forge** (round 531). The production path now
    consumes `BlockProducerCredentials`, runs the shared
    `check_should_forge` leader check (skipping `NotLeader` slots), calls
    `forge_block`, persists raw Conway block CBOR via
    `forged_block_to_storage_block`, replays raw-CBOR VRF nonce inputs in
    append mode, and returns before ChainDB open when no forgers are
    supplied, matching upstream `Run.hs`.
  - ✅ **R3c-5 — epoch-boundary stake rebuild** (round 532). The
    production path now computes each forger's leader-check `sigma`
    from `StakeSnapshots.set`, seeds the initial forecast snapshot from
    Shelley genesis `staking.pools` / `staking.stake` / `initialFunds`,
    activates genesis pools on the first Shelley-family block, and uses
    `apply_epoch_boundary` to rotate snapshots as the synthetic chain
    advances across epochs.
  - 🟡 **R3c-6 — `FileImmutable` → `ChainDb` migration.** Persist a
    `LedgerStore` snapshot so `db-analyser` can validate the synthesized chain.

**Each slice is its own protocol-critical round** — author a `parity-plan`
first; R3a/R3c touch the consensus `OpCert` / forge surface. **Exit (R3c):**
synthesizer produces a Praos-valid on-disk ChainDB that `db-analyser` validates.

### A4 — Sister-tool build-out
Six tools remain implementation arcs:
`kes-agent`, `kes-agent-control`, `snapshot-converter`, `tx-generator`,
`dmq-node`, `cardano-testnet`. One is still pre-gated: `kes-agent` on a
socket-protocol byte-equivalence fixture capture (highest-stakes — key
custody). `tx-generator` is no longer blocked on the cardano-cli C-arc;
that prerequisite closed at R515/R529, so its remaining blocker is its
own parser / generator / submission implementation plus upstream
  comparison evidence. R533 shipped its upstream `Command.hs` parser
  mirror, R534 shipped `Setup/TestnetDiscovery.hs`, R535 shipped
  `Setup/NixService.hs` high-level option parsing/projections, R536
  shipped `Compiler.hs` script generation plus the `Script/Types.hs` IR,
  R537 shipped `Script/Aeson.hs` low-level script JSON parsing, and R538
  shipped `Script/Env.hs` plus `Script/Action.hs` state-only action
  execution. R539 moved the Core-owned state helpers into a strict
  `Script/Core.hs` mirror. The remaining tx-generator blocker is
  Script/Core protocol/query behavior plus GeneratorTx / Submission
  implementation and upstream comparison evidence.
**Scope:** ~5–8 rounds per tool. **Exit:** each
reaches `implemented_needs_11_0_1_evidence` in `parity-matrix.json`.

### A5 — cardano-submit-api structured rejection enum  (`TECH-DEBT.md` §"validation error")
Phase 1 (raw-bytes carrier) landed; Phase 2 — the structured per-era
`ApplyTxError` enum + per-era CBOR decoders + `Display` impl — is not built
(~400 lines). **Scope:** 1 focused arc. **Exit:** operators can pattern-match
typed rejection variants without a CBOR re-walk.

### A6 — Workspace + documentation hygiene
Post-reorganization cleanup guardrails:
- **Workspace members:** closed. The root `Cargo.toml` now explicitly lists
  the shipped `crates/node/cardano-node` binary crate plus all 9 Wave 5
  `crates/node/*` support crates (`block-producer`, `config`, `genesis`,
  `ntc-server`, `ntn-server`, `plutus-eval`, `runtime`, `sync`, `tracer`),
  so `cargo check-all`/`lint`/`test-all` can cover their own targets.
  Current `cargo metadata --no-deps` evidence: 34 workspace packages total,
  including 10 `crates/node/*` packages and 13 `crates/tools/*` packages.
  Keep future extracted crates explicit in `[workspace].members`.
- **`.rs` comment sweep:** live placement comments were normalized after the
  Wave 5 crate split; keep future comments on `crates/node/<sub-crate>/...`
  paths and leave upstream Haskell URLs unchanged.
- **Parity-data files:** `docs/parity-matrix.json` now uses
  `scripts/...` for operator scripts; keep
  `check-parity-matrix.py` green after future path edits.
- **Historical-doc paths:** round-stamped historical narratives now use the
  post-split `crates/node/<sub-crate>/...` paths where they mention local Rust
  files; keep upstream Haskell `cardano-node/...` URLs unchanged.
- **Filetree descriptions:** `.claude/filetree/manifest.json` was bootstrapped
  2026-05-17 with auto-derived descriptions; refine the weak ones incrementally
  via the `cardano-filetree-maintainer` skill / `filetree-reviewer` agent.

---

## Category B — operator-soak gated

Code may be complete; closure needs a long-running rehearsal an automated
environment cannot perform.

### B1 — cardano-tracer full Network.Mux semantics + conformance soak
11 of 12 sub-items shipped (`TECH-DEBT.md` §"cardano-tracer Mux Layer 2/3").
Remaining: full per-mini-protocol queue limits + scheduler fairness, then a
24h+ soak forwarding live traces to a real `cardano-tracer` endpoint.
**Closes with:** the soak harness + a clean 24h run.

### B2 — cardano-submit-api integration soak (R345)
Functional binary exists; needs a drop-in byte-equivalence soak vs the
upstream `cardano-submit-api`. **Closes with:** operator soak →
`verified_11_0_1`.

### B3 — db-truncater integration soak (R351)
Functional; needs integration verification vs upstream `db-truncater`.
**Closes with:** operator soak → `verified_11_0_1`.

### B4 — EKG-parity metrics consolidation  (`TECH-DEBT.md` §"EKG-parity metrics")
`install_prometheus_exporter` has no live consumer; best wired when a sister
tool drives it. **Closes with:** an integration driver + the ~30 `NodeMetrics`
update sites bridged.

### B5 — Production-readiness operator gates
`MANUAL_TEST_RUNBOOK.md` §2–9 mainnet endurance rehearsal (24h+) and the §6.5
parallel-fetch sign-off (`scripts/parallel_blockfetch_soak.sh`)
before flipping the default `max_concurrent_block_fetch_peers` from 1 to 2.

---

## Category C — running-Haskell-node gated

These require a running upstream Haskell `cardano-node` (or pre-captured wire
fixtures) for byte-level forensic comparison. Blocked in any environment
without `.reference-haskell-cardano-node/install/`. Author a `parity-plan`
and delegate to the `haskell-reference-auditor` agent before any fix.

### C1 — Gap BO: preprod TPraos VRF parity (slot ~429,460)
VRF check fails in the Shelley `d=1` federation period. Candidates: overlay
slot mis-classification, active-genesis-delegate VRF check, or TPraos nonce
drift. **Closes with:** per-block VRF input/seed/key diff vs upstream
`classifyOverlaySlot` + `pbftVrfChecks`.

### C2 — Gap BP: preview Plutus V2 cost-budget overrun (slot ~1,462,057)
CEK overruns the CPU budget by ≈0.0185% on a real V2 script. Workaround:
`YGG_SKIP_PHASE2=1` (sync-only; never on a block producer). **Closes with:**
per-builtin step-cost trace diff vs upstream `Cek/Internal.hs::stepAndMaybeSpend`.

### C3 — R178-followup: Conway HFC LSQ response envelope
cardano-cli's HFC decoder expects a different Conway-era LSQ response envelope
than yggdrasil's current `[1, body]` shape. **Closes with:** captured upstream
Conway-era wire fixtures + aligned `encode_query_if_current_match`.

### C4 — Performance: 2× Haskell sync throughput
Yggdrasil ~2,321 slot/s vs Haskell ~5,296 slot/s (0.44×). Needs governor
warm/hot promotion of snapshot peers for multi-peer BlockFetch, batched
Ed25519 verify, pipelined CBOR decode, allocator tuning. **Closes with:** a
side-by-side preview soak vs the Haskell node.

---

## Verification matrix

| Item | Closes when | External dependency |
|---|---|---|
| A1 feature flags | flags conditionally compile; `lint-no-default` green | none |
| A2 cardano-cli ports | each subcommand byte-equivalent to upstream | none (vendored reference) |
| A3 db-synthesizer R2–R3 | Praos forge path plus stake-equivalent synthesized ChainDB | none |
| A4 skeleton tools | each → `implemented_needs_11_0_1_evidence` | none for tx-generator; kes-agent still needs socket-protocol fixtures |
| A5 submit-api errors | typed rejection variants | none |
| A6 hygiene | `cargo metadata --no-deps` lists 34 packages, including all 10 node packages; gates green | none |
| B1 tracer Mux | 24h+ trace-forward soak clean | operator soak |
| B2 submit-api soak | byte-equivalent vs upstream | operator soak |
| B3 db-truncater soak | integration verified vs upstream | operator soak |
| B4 EKG metrics | metrics flow through the global facade | a sister-tool driver |
| B5 operator gates | runbook §2–9 + §6.5 sign-off | 24h+ mainnet rehearsal |
| C1 Gap BO | VRF diff resolved | running Haskell node |
| C2 Gap BP | per-builtin cost diff resolved | running Haskell node |
| C3 R178-followup | envelope aligned to upstream | Conway wire fixtures |
| C4 perf 2× | preview soak ≥ 2× Haskell | side-by-side Haskell node |

## How to use this doc

Work proceeds in the project's R-arc rhythm: one bounded slice per round, the
four cargo gates green between rounds, one `docs/operational-runs/` doc per
round, and the "proceed" human-in-the-loop checkpoint (see the
`continuous-agent-loop` skill). Prefer Category A items first — they are fully
executable and unblock parts of B/C. Update this file whenever an item moves:
closed items graduate to `PARITY_SUMMARY.md` / `PARITY_PROOF.md` and their
`parity-matrix.json` entry.
