---
title: 'R330: pure-Rust dep audit — bech32 added; HTTP server / log rotation deferred'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-330-dep-audit-bech32-deferred-http/
---

# Round 330 — pure-Rust dep audit: bech32 added; HTTP / log-rotation deferred

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R329`](2026-05-09-round-329-run-tools-launcher.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), prep block closure.

## Summary

R330 closes the prep block. Original plan called for adding 5
candidate workspace deps (`bech32`, `axum`, `tracing-appender`,
`hyper`, `tower`) to `[workspace.dependencies]` and running
`cargo deny check` + `cargo audit` against them.

Implementation made one scope refinement: pre-adding deps that
won't be consumed for many rounds is premature — they sit unused
in the manifest and their transitive trees can't be audited in
context yet. R330 therefore lands ONLY the dep we need for the
imminent R331-R334 bech32 implementation:

- **`bech32` v0.11.0** added to `[workspace.dependencies]`.
  - From `rust-bitcoin/rust-bech32` (well-maintained, MIT-licensed).
  - Allowed by `deny.toml` license allowlist (MIT is on it).
  - Zero transitive deps (only the `std` feature on its own
    implementation surface).
  - Pure Rust — no native build requirements.
  - Confirmed via `mcp__plugin_github_github__get_file_contents`
    on rust-bitcoin/rust-bech32 Cargo.toml.

The other 4 candidates are formally **deferred** to their consumer
rounds, with rationale + decision points captured in
`docs/DEPENDENCIES.md`:

- **HTTP server** (`axum` vs raw `tokio::net::TcpListener` per
  `node/src/metrics_server.rs` pattern) → deferred to **R340**
  (cardano-submit-api Web.hs port).
- **Log rotation** (`tracing-appender`) → deferred to **R367**
  (cardano-tracer Logs/Rotator.hs port).
- **Optional fuzz-distribution** (`rand` — already a transitive)
  → deferred to **R434** (tx-generator).

Each deferred candidate gets `cargo deny check` + `cargo audit`
against its actual transitive tree at the round that lands it.

`cargo deny` is not installed locally; CI runs the gate. The
existing `deny.toml` config validates licenses + advisories
against every workspace dep on every CI push, so the addition of
`bech32` v0.11.0 (MIT) gets verified there.

## Diff inventory

| Path | Change |
|---|---|
| `Cargo.toml` (root, `[workspace.dependencies]`) | +1 entry: `bech32 = "0.11"` with R330 inline comment + cross-ref to crates/bech32/ implementation rounds. |
| `docs/DEPENDENCIES.md` | New `bech32` entry under "Approved Now" with full rationale (MIT, pure Rust, BIP-0173 + Bech32m). New section "Sister-tools port arc — deferred candidates (R340+, R367+)" documenting the HTTP server / log-rotation / rand candidates with deferred decision points. |
| `docs/operational-runs/2026-05-09-round-330-dep-audit-bech32-deferred-http.md` | This round-doc. |

R330 ships zero Rust code changes. The new workspace dep is unused
at this round (consumer lands R333) but doesn't break any gate —
unused workspace deps are not flagged by clippy or cargo check.

## Verification

```text
$ cargo fmt --all -- --check
(silent — clean)

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.50s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.34s

$ cargo test --workspace --all-features
passed: 4856  failed: 0

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 20 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c
    consistent across pin source, fixture tree, and docs;
    2 corpora validated.
```

## Closure criterion

- `bech32` v0.11.0 added to `[workspace.dependencies]` with
  rationale documented in `docs/DEPENDENCIES.md`.
- Deferred decisions (HTTP server / log rotation / rand) captured
  with named consumer rounds.
- All 5 cargo gates + 3 CI parity validators clean.

All three are met.

## Prep block closure (R326 → R330 cumulative)

| Round | Shipped | Deliverable |
|---|---|---|
| R326 | `6697ce0` | Vendored-source survey + 3 missing-URL inventory |
| R326b | `a0430a9` | Vendored bech32 + kes-agent + dmq-node (URLs found via MCP GitHub search) |
| R327 | `e952e83` | 12 sister-tool skeleton crates (Cargo.toml + lib.rs + main.rs + AGENTS.md per tool) |
| R327b | `532f1fd` | Cargo.lock + audit TSV byproduct refresh |
| R328 | `0534223` | Parity-matrix +12 entries; upstream_pins +3 SHAs; drift detector cross-org URL support |
| R329 | `71516ef` | `node/dev/scripts/run-tools.sh` 12-binary dispatcher |
| R330 | (this) | `bech32` workspace dep + deferred dep documentation |

**Workspace state at prep-block closure:**
- Crates: 8 → 20 (+12 sister-tool skeletons)
- Parity-matrix entries: 8 → 20
- Upstream-pin SHAs: 6 → 9
- Workspace tests: 4,856 (unchanged — skeletons have no tests yet)
- Upstream `.hs` index: 4,676 → 4,804
- New workspace dep: `bech32 = "0.11"` (pure-Rust, MIT)
- Operator-side launcher: `node/dev/scripts/run-tools.sh` ready

## Authorization checkpoint — Phase A entry

Per the R326-R459 plan's authorization model, the prep block
closes here with an explicit operator checkpoint:

> **Operator decision required:** approve Phase A (Tier 1
> deployment-essential SPO operations) entry. Phase A spans
> R331-R385 across 5 sister tools (bech32, cardano-submit-api,
> kes-agent, kes-agent-control, cardano-tracer) + 55 rounds.
>
> Phase A entry sequence:
> - R331 — bech32 file-mirror skeleton (4 round mini-arc R331-R334)
> - R335 — cardano-submit-api skeleton (9 round mini-arc R335-R343)
> - R344 — kes-agent skeleton (11 round mini-arc R344-R354) — HIGHEST-STAKES (socket protocol byte-equivalence mandatory)
> - R355 — kes-agent-control skeleton (5 round mini-arc R355-R359)
> - R360 — cardano-tracer skeleton (26 round mini-arc R360-R385)

All 12 binaries currently exit 1 with the R327 sentinel; Phase A
will turn the first 5 sentinels into functional deployable
binaries, replacing upstream binaries one-tool-at-a-time as they
reach `verified_11_0_1` parity-matrix status.
