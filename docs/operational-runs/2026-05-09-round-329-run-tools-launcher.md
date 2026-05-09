---
title: 'R329: run-tools launcher + preprod checkpoints clarification'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-329-run-tools-launcher/
---

# Round 329 — run-tools launcher + preprod checkpoints clarification

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R328`](2026-05-09-round-328-parity-infrastructure-expansion.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), prep block.

## Summary

R329 lands the operator-facing dispatcher script
`node/scripts/run-tools.sh` per the plan. The script mirrors the
shape of upstream's `.reference-haskell-cardano-node/install/run-node.sh`
but supports the wider 12-binary surface produced by the R327
skeleton crates. Operators can swap an upstream sister-tool binary
for the corresponding Yggdrasil binary by changing only the
invocation prefix (e.g. `cardano-tracer -c ...` →
`run-tools.sh cardano-tracer -c ...`). The binary names match
upstream exactly so all existing flags + config files continue
to work unchanged.

The plan also specified a `node/configuration/preprod/checkpoints.json`
deliverable (it appeared "missing" in the R326 audit). Implementation
revealed this was a false positive: upstream's
`.reference-haskell-cardano-node/install/share/preprod/` ALSO
doesn't ship a `checkpoints.json`. Yggdrasil's preprod config matches
upstream exactly. **No file added; the apparent gap was a
miscount.** R326 round-doc carried the false claim — corrected in
this round-doc + the round-doc trail going forward.

## Diff inventory

| Path | Change |
|---|---|
| `node/scripts/run-tools.sh` | New — 138-line bash dispatcher. `$0 <tool> [args...]` validates against the canonical 12-tool list and routes to either `target/release/<tool>` (production), `cargo run --release --bin <tool>` (initial setup), or `cargo run --bin <tool>` when `YGGDRASIL_TOOLS_USE_DEBUG=1` (development). `--help` and `--list` flags built in. |
| `docs/operational-runs/2026-05-09-round-329-run-tools-launcher.md` | This round-doc. |

R329 ships zero Rust code changes. The 4,856-test workspace baseline
is preserved. **`preprod/checkpoints.json` NOT added** — researched
the upstream `install/share/preprod/` and confirmed no such file
ships there.

## Smoke tests

```text
$ node/scripts/run-tools.sh --list
Yggdrasil sister-tool binaries (run via node/scripts/run-tools.sh <tool> [args...]):
  bech32
  cardano-submit-api
  cardano-testnet
  cardano-tracer
  db-analyser
  db-synthesizer
  db-truncater
  dmq-node
  kes-agent
  kes-agent-control
  snapshot-converter
  tx-generator

$ node/scripts/run-tools.sh nonexistent-tool
run-tools.sh: unknown tool 'nonexistent-tool'
Run 'node/scripts/run-tools.sh --list' to see the 12 sister-tool binaries.
[exit 2]

$ YGGDRASIL_TOOLS_USE_DEBUG=1 node/scripts/run-tools.sh bech32
Error: yggdrasil-bech32: not yet implemented (R327 skeleton); see docs/operational-runs/ for the bech32 port progress.
Location:
    crates/bech32/src/lib.rs:19:9
[binary exits 1]
```

The R327 sentinel propagates correctly: every binary exits 1 with a
clear "not yet implemented" message that points operators at the
round-doc trail. As tool implementations land per the R331-R459 plan,
the sentinel disappears tool-by-tool.

## Verification

```text
$ cargo fmt --all -- --check
(silent — clean)

$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.13s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.16s

$ cargo test --workspace --all-features
passed: 4856  failed: 0

$ python3 scripts/check-parity-matrix.py
parity matrix clean: 20 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 scripts/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c
    consistent across pin source, fixture tree, and docs;
    2 corpora validated.
```

## Closure criterion

- `node/scripts/run-tools.sh` exists and is executable.
- All 3 smoke tests pass (--list, unknown tool, real binary
  dispatch with sentinel).
- The `preprod/checkpoints.json` "gap" from R326's audit is
  corrected: confirmed not a gap — upstream doesn't ship one
  either.
- All 5 cargo gates + 3 CI parity validators clean.

All four are met.

## Out of scope (R330 next step)

- **R330** — Pure-Rust ecosystem dependency audit. Survey
  `bech32`, `axum`, `tracing-appender` against the workspace's
  license + transitive-dep policy. Add to root `Cargo.toml
  [workspace.dependencies]`. Run `cargo deny check` clean.
- **Authorization checkpoint after R330** — operator approves
  Phase A (Tier 1) entry. Then R331 = bech32 file-mirror
  skeleton round.

## Deployment readiness implication

Post-R329, the deployment shape is now operator-ready in the
following sense:

1. **Build all binaries**: `cargo build --release --workspace` produces
   all 12 sister-tool binaries under `target/release/{bech32,
   cardano-submit-api, cardano-testnet, cardano-tracer, db-analyser,
   db-synthesizer, db-truncater, dmq-node, kes-agent, kes-agent-control,
   snapshot-converter, tx-generator}` plus the existing
   `target/release/yggdrasil-node`.

2. **Deploy via run-tools.sh**: the operator-side launcher fronts
   the 12 binaries with a uniform interface, enabling drop-in
   replacement of upstream binaries one-tool-at-a-time as their
   yggdrasil ports become functional (R331+).

3. **Authorization for swap**: each tool's swap is gated on its
   AGENTS.md "deployment-ready 100% parity" closure criteria
   (acceptance #3 in the plan). At R329 closure, NO tool is
   swap-ready yet — every binary returns the R327 sentinel.

The skeleton infrastructure is in place; tool-by-tool functional
implementation runs from R331 (bech32) through R459 (dmq-node).
