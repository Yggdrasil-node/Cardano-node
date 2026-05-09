---
title: 'R317: merge multiplexer.rs into mux.rs (1:1 with upstream Mux.hs)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-317-merge-multiplexer-into-mux/
---

# Round 317 — merge `multiplexer.rs` into `mux.rs` (1:1 with upstream `Mux.hs`)

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R316`](2026-05-09-round-316-reclassify-three-more-synthesis.md)

## Goal

Promote `crates/network/src/mux.rs` from `(c) strict-partial` to
`(a) DIRECT_MIRROR` of upstream `Ouroboros/Network/Mux.hs` by
merging the previously-separate `multiplexer.rs` types module
back in. Upstream `Mux.hs` carries SDU framing types + per-channel
state machine + multiplexer/demultiplexer runtime in a single
file; Yggdrasil's earlier split into `multiplexer.rs` (types) +
`mux.rs` (runtime) was a code-organization choice with no upstream
basis.

## Diff inventory

| Path | Change |
|---|---|
| `crates/network/src/multiplexer.rs` | **Deleted.** 276 lines (SDU types: `MiniProtocolNum`, `MiniProtocolDir`, `SduHeader`, `SduDecodeError`, `MuxChannel` + tests). |
| `crates/network/src/mux.rs` | **+265 lines.** Multiplexer body inlined where the previous `use crate::multiplexer::...` line stood, preserving doc-comments + the wire-ID-pin tests. Docstring updated from `**Strict mirror (partial):**` to `**Strict mirror:** Network/Mux.hs.`. Final size: 1392 lines (vs upstream `Mux.hs` ~ matching scale; see verification below). |
| `crates/network/src/lib.rs` | Removed `pub mod multiplexer;` and consolidated the multiplexer-side re-exports into the existing `pub use mux::{...}` block. |
| 9 importing files | Bulk-replaced `crate::multiplexer::*` → `crate::mux::*` and `yggdrasil_network::multiplexer::*` → `yggdrasil_network::mux::*`. Affected: `bearer.rs`, `chainsync_client.rs`, `diffusion.rs`, `governor.rs`, `governor/peer_metric.rs`, `mux.rs` itself, `ntc_peer.rs`, `peer.rs`, plus 1 in `node/`. |

## Bucket-count delta

| Bucket | R316 | R317 | Δ |
|---|---:|---:|---:|
| Total production `.rs` files | 445 | 444 | **−1** (multiplexer.rs deleted) |
| `(a) DIRECT_MIRROR (auto: docstring declares strict mirror)` | 211 | 212 | **+1** (mux.rs declares strict mirror) |
| `(a) DIRECT_MIRROR (auto)` | 25 | 25 | 0 |
| `(a) DIRECT_MIRROR (auto (affinity-filtered))` | 18 | 18 | 0 |
| **(a) total** | **254** | **255** | **+1** |
| `(c) docstring present (strict-none)` | 185 | 185 | 0 |
| `(c) docstring present (strict-partial)` | 6 | 4 | **−2** (mux.rs promoted; multiplexer.rs deleted) |
| **(c) total** | **191** | **189** | **−2** |

## Verification

```text
$ python3 scripts/audit-strict-mirror.py
audit complete: 444 rust files; candidate_match=387, no_candidate_match=57
auto-grading bucket counts:
  (a): 255
  (c): 189

$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo fmt --all -- --check
(silent — clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.33s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.32s

$ cargo test --workspace --all-features
passed: 4855  failed: 0
```

The 9 wire-ID-pin tests from `multiplexer.rs::tests` are now part
of `mux.rs::tests` and continue to pass — no test count delta.

## Remaining 4 strict-partial files (after R317)

| Rust path | Refactor track |
|---|---|
| `crates/network/src/handshake.rs` | R318: split into `handshake/{type,version,codec}.rs` matching upstream `Handshake/{Type,Version,Codec}.hs` |
| `crates/network/src/inbound_governor.rs` | R319: split into `inbound_governor.rs` + `inbound_governor/state.rs` matching upstream `InboundGovernor.hs` + `InboundGovernor/State.hs` |
| `crates/plutus/src/builtins.rs` | Stay partial — intentional Yggdrasil split along runtime/cost/types axes |
| `crates/plutus/src/machine.rs` | Stay partial — intentional Yggdrasil split along driver/types/cost axes |

After R318+R319, only the 2 plutus partials remain — both
documented as intentional Yggdrasil idiom.

## Closure criterion

- `mux.rs` declares canonical `**Strict mirror:** Network/Mux.hs.`
- `multiplexer.rs` deleted; no dangling references.
- All 9 importing files updated to use `crate::mux` instead of
  `crate::multiplexer`.
- All five workspace gates green at 4,855-test baseline.
- Wire-ID-pin tests from the merged module continue to pass.

All five are met.
