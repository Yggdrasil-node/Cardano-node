---
title: 'R328: parity infrastructure expansion for sister-tools port arc'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-328-parity-infrastructure-expansion/
---

# Round 328 — parity infrastructure expansion

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R327b`](2026-05-09-round-327-twelve-skeleton-crates.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), prep block.

## Summary

R328 extends the parity-tracking infrastructure to cover the 12 new
sister-tool crates landed at R327, plus the 3 newly-vendored upstream
repos at R326b. Concrete deliverables:

1. **`docs/parity-matrix.json`**: 12 new entries added (one per
   sister tool) with `status: "absent"` and per-tool
   `next_milestone` pointing at the tool's skeleton round (R331
   bech32 through R450 dmq-node). Total entries: 8 → 20.

2. **`dev/test/check-parity-matrix.py`**: allowlist extended:
   - `ALLOWED_AREAS` gains `"sister-tools"` (the new area for all
     12 sister-tool entries).
   - `ALLOWED_MILESTONES` gains 12 new round IDs (R331, R335,
     R344, R355, R360, R386, R391, R401, R408, R416, R434, R450).

3. **`node/src/upstream_pins.rs`**: 3 new SHA pins added
   alongside the existing 6:
   - `UPSTREAM_BECH32_COMMIT = "4624d3a84606615c1ca1410d6dd3fd9213211215"`
   - `UPSTREAM_KES_AGENT_COMMIT = "6d54ac2ee325aadeeb3659cfefcd58035f69acd9"`
   - `UPSTREAM_DMQ_NODE_COMMIT = "bd5fbf69fcdeaa9d8b4a3d2b4554016d546b17ea"`
   `UPSTREAM_PINS` slice extended from 6 to 9 entries. The
   cardinality drift-guard test (`upstream_pins_cover_all_six_canonical_repos`)
   renamed to `upstream_pins_cover_all_nine_canonical_repos` and
   the expected-repo list extended.

4. **`node/dev/scripts/check_upstream_drift.sh`**: extended to handle
   cross-org URLs (kes-agent lives under `input-output-hk`, the
   other 8 under `IntersectMBO`). Replaced the hardcoded
   `https://github.com/IntersectMBO/${repo}.git` URL prefix with a
   per-repo `repo_url` associative array. Iteration order pinned
   via a new `PIN_ORDER` array (matches `UPSTREAM_PINS` order in
   the Rust file). Total count is now `${#PIN_ORDER[@]}` (9, was
   hardcoded 6 in the previous version).

## Diff inventory

| Path | Change |
|---|---|
| `docs/parity-matrix.json` | 12 new entries appended (sister-tool.{bech32, cardano-submit-api, kes-agent, kes-agent-control, cardano-tracer, db-truncater, db-analyser, snapshot-converter, db-synthesizer, cardano-testnet, tx-generator, dmq-node}). All entries `status: "absent"`. Total entries: 8 → 20. |
| `dev/test/check-parity-matrix.py` | `ALLOWED_AREAS` gains `"sister-tools"`. `ALLOWED_MILESTONES` extended with 12 new round IDs. |
| `node/src/upstream_pins.rs` | 3 new const declarations + extended `UPSTREAM_PINS` slice (6 → 9). Cardinality test renamed + extended. Module docstring updated to mention the 9 repos. |
| `node/dev/scripts/check_upstream_drift.sh` | Per-repo URL table replacing hardcoded org prefix; `PIN_ORDER` array for canonical iteration; total count derived from array length. |
| `docs/operational-runs/2026-05-09-round-328-parity-infrastructure-expansion.md` | This round-doc. |

## Verification

```text
$ cargo fmt --all -- --check
(silent — clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.84s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.90s

$ cargo test --workspace --all-features
passed: 4856  failed: 0

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 20 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c
    consistent across pin source, fixture tree, and docs;
    2 corpora validated.
```

The renamed cardinality test (`upstream_pins_cover_all_nine_canonical_repos`)
runs as part of `cargo test-all` and passes — the 9-entry order
(6 cardano-node support repos + 3 sister-tool repos) is pinned.

## Closure criterion

- 12 sister-tool entries in `docs/parity-matrix.json` with `status:
  "absent"` and `next_milestone` pointing at the tool's skeleton
  round.
- `dev/test/check-parity-matrix.py` allowlist extended for the new
  area + milestones; validator green.
- `node/src/upstream_pins.rs` carries 3 new SHAs; `UPSTREAM_PINS`
  cardinality test pins 9; cargo test-all green.
- `node/dev/scripts/check_upstream_drift.sh` handles cross-org URLs
  and iterates 9 repos.
- All 5 cargo gates + 3 CI parity validators clean.

All five are met.

## Out of scope (R329+ next steps)

- **R329** — `node/dev/scripts/run-tools.sh` launcher (mirror of
  `install/run-node.sh`) routing `$1 <tool> <args>` to each
  yggdrasil sister-tool binary. Plus `node/configuration/preprod/checkpoints.json`
  (the only operator-config gap identified in R326's audit).
- **R330** — Pure-Rust ecosystem dependency audit: survey
  `bech32`, `axum`, `tracing-appender` against the workspace's
  license + transitive-dep policy. Add to root `Cargo.toml
  [workspace.dependencies]`.
- **Authorization checkpoint after R330** — operator approves
  Phase A entry. Then R331 = bech32 file-mirror skeleton round.
