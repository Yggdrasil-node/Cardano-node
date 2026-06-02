---
title: 'R327: workspace skeleton stubs for 12 sister-tool crates'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-327-twelve-skeleton-crates/
---

# Round 327 — workspace skeleton stubs for 12 sister-tool crates

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R326b`](2026-05-09-round-326b-vendor-bech32-kes-agent-dmq-node.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), prep block.

## Summary

R327 creates 12 new workspace crates — one per sister tool — as
deployable Rust skeletons. Each crate exposes `[lib]` (for naming-
parity tests) + `[[bin]]` (deployable binary matching the upstream
binary's name) and ships:

- `Cargo.toml` — package + lib + bin entries, workspace-inherited
  metadata.
- `src/lib.rs` — placeholder `run()` returning "not yet implemented"
  with `**Strict mirror:** none.` synthesis declaration.
- `src/main.rs` — minimal binary entry calling `<crate>::run()`.
- `AGENTS.md` — operational guide stub with strict 1:1 file-mirror
  policy + per-tool round roadmap.

The 12 binaries match upstream's deployable surface exactly:
`bech32`, `cardano-submit-api`, `cardano-testnet`, `cardano-tracer`,
`db-analyser`, `db-synthesizer`, `db-truncater`, `dmq-node`,
`kes-agent`, `kes-agent-control`, `snapshot-converter`, `tx-generator`.
Each binary's `[[bin]]` `name` field is the upstream-compatible name
(no `yggdrasil-` prefix on the binary surface) so `cargo build` +
`run-tools.sh` (R329) produces drop-in replacements.

The Rust crate names use the `yggdrasil-` prefix for workspace
hygiene (`yggdrasil-bech32` etc.) — same convention as the existing
`yggdrasil-{crypto,ledger,storage,consensus,network,plutus,
cardano-cli,node}` crates.

## Diff inventory

| Path | Change |
|---|---|
| `Cargo.toml` (root) | `[workspace.members]` extended with 12 new entries (sister-tool crates), grouped under a comment-block separator from the existing 8 crates. |
| `crates/bech32/{Cargo.toml, src/lib.rs, src/main.rs, AGENTS.md}` | new (skeleton). |
| `crates/cardano-submit-api/...` | new (skeleton). |
| `crates/cardano-testnet/...` | new (skeleton). |
| `crates/cardano-tracer/...` | new (skeleton). |
| `crates/db-analyser/...` | new (skeleton). |
| `crates/db-synthesizer/...` | new (skeleton). |
| `crates/db-truncater/...` | new (skeleton). |
| `crates/dmq-node/...` | new (skeleton). |
| `crates/kes-agent/...` | new (skeleton). |
| `crates/kes-agent-control/...` | new (skeleton). |
| `crates/snapshot-converter/...` | new (skeleton). |
| `crates/tx-generator/...` | new (skeleton). |
| `docs/operational-runs/2026-05-09-round-327-twelve-skeleton-crates.md` | This round-doc. |

**Total new files:** 12 crates × 4 files = 48 (12 Cargo.toml + 12
lib.rs + 12 main.rs + 12 AGENTS.md).

## Verification

```text
$ cargo check --workspace --all-targets
    Checking yggdrasil-bech32 v0.2.0 ...
    Checking yggdrasil-cardano-submit-api v0.2.0 ...
    Checking yggdrasil-cardano-testnet v0.2.0 ...
    Checking yggdrasil-cardano-tracer v0.2.0 ...
    Checking yggdrasil-db-analyser v0.2.0 ...
    Checking yggdrasil-db-synthesizer v0.2.0 ...
    Checking yggdrasil-db-truncater v0.2.0 ...
    Checking yggdrasil-dmq-node v0.2.0 ...
    Checking yggdrasil-kes-agent v0.2.0 ...
    Checking yggdrasil-kes-agent-control v0.2.0 ...
    Checking yggdrasil-snapshot-converter v0.2.0 ...
    Checking yggdrasil-tx-generator v0.2.0 ...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.08s

$ cargo fmt --all -- --check
(silent — clean)

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.67s

$ cargo test --workspace --all-features
passed: 4856  failed: 0

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 dev/test/audit-strict-mirror.py
audit complete: 448 rust files; candidate_match=392, no_candidate_match=56
auto-grading bucket counts:
  (a): 246
  (c): 202
```

The 24 new `lib.rs` + `main.rs` files are skipped by
`audit-strict-mirror.py` per existing convention (`lib.rs`, `main.rs`,
`mod.rs`, `build.rs` are excluded — they're crate-root wiring shells,
not strict-mirror leaves). Each skeleton file still carries
`**Strict mirror:** none.` declaration so the strict-mirror policy is
satisfied at content level. Audit-table totals remain at 246 (a) +
202 (c) = 448.

## Sister-tool crate inventory

| Crate | Package name | Binary | Upstream source |
|---|---|---|---|
| `crates/bech32/` | `yggdrasil-bech32` | `bech32` | `deps/bech32/bech32/` |
| `crates/cardano-submit-api/` | `yggdrasil-cardano-submit-api` | `cardano-submit-api` | `cardano-submit-api/` |
| `crates/cardano-testnet/` | `yggdrasil-cardano-testnet` | `cardano-testnet` | `cardano-testnet/` |
| `crates/cardano-tracer/` | `yggdrasil-cardano-tracer` | `cardano-tracer` | `cardano-tracer/` |
| `crates/db-analyser/` | `yggdrasil-db-analyser` | `db-analyser` | `unstable-cardano-tools/.../DBAnalyser/` |
| `crates/db-synthesizer/` | `yggdrasil-db-synthesizer` | `db-synthesizer` | `.../DBSynthesizer/` |
| `crates/db-truncater/` | `yggdrasil-db-truncater` | `db-truncater` | `.../DBTruncater/` |
| `crates/dmq-node/` | `yggdrasil-dmq-node` | `dmq-node` | `deps/dmq-node/dmq-node/` |
| `crates/kes-agent/` | `yggdrasil-kes-agent` | `kes-agent` | `deps/kes-agent/kes-agent/` |
| `crates/kes-agent-control/` | `yggdrasil-kes-agent-control` | `kes-agent-control` | `deps/kes-agent/kes-agent/` (control binary) |
| `crates/snapshot-converter/` | `yggdrasil-snapshot-converter` | `snapshot-converter` | `.../app/snapshot-converter.hs` |
| `crates/tx-generator/` | `yggdrasil-tx-generator` | `tx-generator` | `bench/tx-generator/` |

Each crate's stub `run()` returns:
```
yggdrasil-<tool>: not yet implemented (R327 skeleton); see
docs/operational-runs/ for the <tool> port progress.
```

Calling any binary at this stage exits 1 with that message — the
skeletons compile and bind to their CLI surface but contain no
concrete logic yet.

## Closure criterion

- 12 new workspace crates created, each with `Cargo.toml` + `lib.rs`
  + `main.rs` + `AGENTS.md`.
- Root `Cargo.toml` `[workspace.members]` updated.
- `cargo check --workspace --all-targets` succeeds.
- All 5 cargo gates clean at 4,856-test baseline.
- All 4 CI parity validators clean.
- Strict-mirror audit table unchanged (246 + 202 = 448) — skeleton
  wiring files don't enter the audit per existing convention.

All six are met.

## Out of scope (R328+ next steps)

- **R328 — Audit + parity infrastructure expansion**: parity-matrix
  entries for the 12 new tools; `node/src/upstream_pins.rs` SHA pins
  for bech32 / kes-agent / dmq-node; drift detector extension.
- **R329 — Run-tools launcher** (`node/dev/scripts/run-tools.sh`) +
  `node/configuration/preprod/checkpoints.json`.
- **R330 — Pure-Rust ecosystem dependency audit** — survey
  `bech32`, `axum`, `tracing-appender` against the workspace's
  license + transitive-dep policy.
- **Authorization checkpoint after R330** — operator approves
  Phase A (Tier 1) entry. Then R331 = bech32 skeleton round.
