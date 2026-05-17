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

## Status snapshot (verified 2026-05-17)

- **Build health:** all four cargo gates green on Rust 1.95.0 — `cargo fmt`,
  `cargo check-all`, `cargo lint`, `cargo test-all` (**6,519 tests passing,
  0 failing, 3 ignored**). `check-strict-mirror.py` + `check-fixture-manifest.py`
  clean.
- **Core node:** crypto, ledger, storage, consensus, mempool, network, plutus,
  and the `crates/node/*` runtime crates are feature-complete for syncing and
  validating the official networks (code-level parity closure, v0.2.0).
- **Parity matrix:** 22 tracked entries — 2 `verified_11_0_1`,
  11 `implemented_needs_11_0_1_evidence`, 9 `partial`.
- **Sister tools (13):** 1 verified (`bech32`); 2 functional-pending-soak
  (`cardano-submit-api`, `db-truncater`); 4 functional-partial (`cardano-cli`
  3/35 subcommands, `cardano-tracer`, `db-analyser`, `db-synthesizer`);
  6 skeleton (`kes-agent`, `kes-agent-control`, `snapshot-converter`,
  `tx-generator`, `dmq-node`, `cardano-testnet`).
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

### A2 — cardano-cli subcommand migration  (`TECH-DEBT.md` §"yggdrasil-cardano-cli")
`crates/tools/cardano-cli` has 3 of ~35 operator subcommands wired standalone
(`version`, `show-upstream-config`, `query-tip`). The `LsqClient` trait
abstraction (R505–R506) unblocks the socket-touching commands. **Scope:**
per-subcommand rounds, ~1–3 days each (~30 subcommands). **Exit:**
`yggdrasil-cardano-cli <subcommand>` byte-equivalent to the node binary's
`cardano-cli` group and to upstream `cardano-cli 11.0`.

### A3 — db-synthesizer Phase 4 R2–R3
`crates/tools/db-synthesizer` shipped the forge-loop slice (Phase 4 R1). R2 =
genesis/`ShelleyGenesis` config loading; R3 = the Praos forge path
(VRF/KES/OpCert leader check) so synthesized blocks are structurally Praos.
**Scope:** 2 rounds. **Exit:** synthesizer produces a Praos-valid on-disk
ChainDB; `db-analyser` validates it.

### A4 — Skeleton sister-tool build-out
Six tools are skeleton-only — each its own multi-round arc:
`kes-agent`, `kes-agent-control`, `snapshot-converter`, `tx-generator`,
`dmq-node`, `cardano-testnet`. Two are pre-gated: `kes-agent` on a
socket-protocol byte-equivalence fixture capture (highest-stakes — key
custody); `tx-generator` on the cardano-cli CLI-MVS (A2). **Scope:** ~5–8
rounds per tool. **Exit:** each reaches `implemented_needs_11_0_1_evidence`
in `parity-matrix.json`.

### A5 — cardano-submit-api structured rejection enum  (`TECH-DEBT.md` §"validation error")
Phase 1 (raw-bytes carrier) landed; Phase 2 — the structured per-era
`ApplyTxError` enum + per-era CBOR decoders + `Display` impl — is not built
(~400 lines). **Scope:** 1 focused arc. **Exit:** operators can pattern-match
typed rejection variants without a CBOR re-walk.

### A6 — Workspace + documentation hygiene (now unblocked — Rust 1.95 installed)
Toolchain-gated cleanup rounds deferred from the 2026-05-17 audit cleanup arc:
- **Workspace members:** the 9 `crates/node/*` sub-crates
  (`block-producer, config, genesis, ntc-server, ntn-server, plutus-eval,
  runtime, sync, tracer`) are not in the root `Cargo.toml` `[workspace]
  members` list, so `cargo check-all`/`lint`/`test-all` skip their own
  `--all-targets`. Add them; verify with `cargo metadata`. Treat any
  newly-surfaced standalone failure as its own round.
- **`.rs` comment sweep:** ~95 stale `node/` path strings remain in
  production `.rs` comments/docstrings; remap to `crates/node/<sub-crate>/...`
  per-symbol (not blanket-prefix — files split across sub-crates).
- **Parity-data files:** correct residual stale `node/scripts/...` strings in
  `docs/parity-matrix.json`; verify each corrected path exists on disk before
  editing; re-run `check-parity-matrix.py`.
- **Historical-doc paths:** `node/src/*.rs` references inside the round-stamped
  historical narrative of `PARITY_SUMMARY.md` / `PARITY_PROOF.md` were left
  uncorrected during the cleanup (no-rewrite-history rule). Optional low-value
  follow-up if a precise per-symbol remap is wanted.
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
parallel-fetch sign-off (`crates/node/yggdrasil-node/scripts/parallel_blockfetch_soak.sh`)
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
| A3 db-synthesizer R2–R3 | Praos-valid synthesized ChainDB | none |
| A4 skeleton tools | each → `implemented_needs_11_0_1_evidence` | none (A4/kes-agent + tx-generator pre-gated internally) |
| A5 submit-api errors | typed rejection variants | none |
| A6 hygiene | `cargo metadata` lists 24 members; gates green | none |
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
