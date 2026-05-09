---
title: 'R319: split inbound_governor.rs to mirror upstream State.hs separation'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-319-inbound-governor-split/
---

# Round 319 — split `inbound_governor.rs` to mirror upstream `State.hs` separation

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R318`](2026-05-09-round-318-handshake-split.md)

## Goal

Promote 2 files to `(a) DIRECT_MIRROR` of upstream
`Ouroboros.Network.InboundGovernor.hs` + `InboundGovernor/State.hs`
by splitting Yggdrasil's previous monolithic `inbound_governor.rs`
(1478 lines, `(c) strict-partial`) along the same axis upstream
uses (data definitions vs runtime decision engine).

## File reshape

```
Before                                 After
======                                 =====
inbound_governor.rs  1478 (c)          inbound_governor.rs       ~1361 (a)
                                       inbound_governor/
                                         state.rs                  ~127 (a)
```

### `inbound_governor/state.rs` (mirrors `InboundGovernor/State.hs`)

Pure data + constructors + simple accessors:
- `InboundConnectionEntry` struct (per-connection IG state)
- `InboundGovernorState` struct (full IG state — connections map, mature/fresh duplex peer maps, counters, idle-timeout)
- `new()` / `with_idle_timeout()` constructors
- Pure accessors: `connection_count()`, `remote_state()`, `mature_duplex_peer_set()`
- `Default` impl

### `inbound_governor.rs` (mirrors `InboundGovernor.hs`)

Runtime decision engine (split-impl on `InboundGovernorState`):
- `InboundGovernorAction` enum (4 variants: PromotedToWarmRemote / DemotedToColdRemote / ReleaseInboundConnection / UnregisterConnection)
- `recompute_counters()` private impl
- `mature_peers()`, `step()`, all 9 `handle_*` event handlers
- `apply_commit_result()`, `expired_idle_connections()`, `update_responder_counters()`, `set_responder_counters()`
- `verify_remote_transition()` predicate
- All tests (data + behavior)

The runtime methods are added via `impl InboundGovernorState { ... }` in `inbound_governor.rs` — the split-impl pattern (same as R318 codec.rs adding methods to handshake/type.rs's HandshakeMessage).

## Bucket-count delta

| Bucket | R318 | R319 | Δ |
|---|---:|---:|---:|
| Total production `.rs` files | 447 | 448 | **+1** (state.rs added; old monolith retained as the InboundGovernor.hs mirror) |
| `(a) DIRECT_MIRROR (auto: docstring declares strict mirror)` | 215 | 217 | **+2** |
| `(a) DIRECT_MIRROR (auto)` | 25 | 25 | 0 |
| `(a) DIRECT_MIRROR (auto (affinity-filtered))` | 18 | 18 | 0 |
| **(a) total** | **258** | **260** | **+2** |
| `(c) docstring present (strict-none)` | 186 | 186 | 0 |
| `(c) docstring present (strict-partial)` | 3 | 2 | **−1** |
| **(c) total** | **189** | **188** | **−1** |

## Remaining 2 strict-partial files (after R319)

Only the 2 intentional Yggdrasil-axis plutus splits remain:

- `crates/plutus/src/builtins.rs` (1496) — Yggdrasil splits upstream `Default/Builtins.hs` along runtime/cost/types axes (siblings: `cost_model/*.rs`, `types/*.rs`).
- `crates/plutus/src/machine.rs` (1471) — Yggdrasil splits upstream `Cek/Internal.hs` along driver/types/cost axes (siblings: `types/cek_internal.rs`, `cost_model/step.rs`).

R320 will tighten these docstrings to declare strict mirror of their primary upstream `.hs` (with the sibling-file rationale documented).

## Verification

```text
$ python3 scripts/audit-strict-mirror.py
audit complete: 448 rust files; candidate_match=391, no_candidate_match=57
auto-grading bucket counts:
  (a): 260
  (c): 188

$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo fmt --all -- --check
(silent — clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.55s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.32s

$ cargo test --workspace --all-features
passed: 4856  failed: 0
```

Test count moved +1 (4,855 → 4,856) — a doc-test in the new
`state.rs` module became visible to the test runner. The 22 inline
`#[test]` items in `inbound_governor.rs::tests` continue to pass
unchanged.

## Closure criterion

- `inbound_governor.rs` declares canonical strict-mirror to upstream
  `InboundGovernor.hs`.
- `inbound_governor/state.rs` declares canonical strict-mirror to
  upstream `InboundGovernor/State.hs`.
- All 22 original tests preserved + 1 doc-test added.
- All 5 workspace gates green at 4,856-test baseline.
- No public API path changes — `InboundConnectionEntry` and
  `InboundGovernorState` remain reachable via
  `crate::inbound_governor::{InboundConnectionEntry, InboundGovernorState}`
  (re-exported through the parent shell).

All five are met.
