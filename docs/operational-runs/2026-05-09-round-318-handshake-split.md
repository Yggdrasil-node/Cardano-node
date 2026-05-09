---
title: 'R318: split handshake.rs into 3 leaves matching upstream Type/Version/Codec'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-318-handshake-split/
---

# Round 318 — split `handshake.rs` into 3 leaves matching upstream

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R317`](2026-05-09-round-317-merge-multiplexer-into-mux.md)

## Goal

Promote 3 new leaves to `(a) DIRECT_MIRROR` of upstream
`Ouroboros.Network.Protocol.Handshake.{Type,Version,Codec}.hs`
by splitting Yggdrasil's previous monolithic `handshake.rs`
(694 lines, `(c) strict-partial`) along the same axes upstream
uses.

## File reshape

```
Before                              After
======                              =====
handshake.rs  694 lines  (c)        handshake.rs        ~30 lines (c) parent shell
                                    handshake/
                                      type.rs    ~205 lines (a)
                                      version.rs  ~62 lines (a)
                                      codec.rs   ~330 lines (a)
```

### `handshake/type.rs` (mirrors `Type.hs`)

State-machine + protocol types:
- `HandshakeMessage` enum (4 variants: ProposeVersions / AcceptVersion / Refuse / QueryReply)
- `RefuseReason` enum (3 variants) + `Display` impl
- `HandshakeState` enum (StPropose / StConfirm / StDone)
- `HandshakeRequest` legacy convenience wrapper
- `HandshakeTransitionError` enum
- `impl HandshakeState::transition` (state machine driver)
- `impl HandshakeMessage::tag_name` / `wire_tag` (metadata helpers)
- 4 RefuseReason `Display` tests

### `handshake/version.rs` (mirrors `Version.hs`)

Version-number type and per-version negotiation data:
- `HandshakeVersion(pub u16)` newtype + `V13` / `V14` / `V15` constants
- `NodeToNodeVersionData` struct (4 fields: network_magic, initiator_only_diffusion_mode, peer_sharing, query)
- 1 sequential-constants drift-guard test

### `handshake/codec.rs` (mirrors `Codec.hs`)

CBOR encode/decode (split-impl on `HandshakeMessage`):
- `encode_version_data` / `decode_version_data` (v7-v15 backward-compat decoder)
- `encode_version_table` / `decode_version_table`
- `impl HandshakeMessage::to_cbor` / `from_cbor`
- 3 codec drift-guard tests (version-data shape, message tag/arity, RefuseReason inner-tag/arity)

### `handshake.rs` (parent shell, `(c) strict-none`)

Re-export aggregator preserving the existing flat
`crate::handshake::Foo` API for callers in the workspace. Declares
synthesis explicitly: upstream's `Ouroboros.Network.Protocol.Handshake`
umbrella additionally carries `runHandshakeClient` / `runHandshakeServer`
runtime drivers, but in Yggdrasil that runtime surface is folded
into `peer.rs`, so this parent file is a pure re-export shell
without a runtime API to mirror.

## Bucket-count delta

| Bucket | R317 | R318 | Δ |
|---|---:|---:|---:|
| Total production `.rs` files | 444 | 447 | **+3** (3 leaves added; old monolith is now parent shell) |
| `(a) DIRECT_MIRROR (auto: docstring declares strict mirror)` | 212 | 215 | **+3** |
| `(a) DIRECT_MIRROR (auto)` | 25 | 25 | 0 |
| `(a) DIRECT_MIRROR (auto (affinity-filtered))` | 18 | 18 | 0 |
| **(a) total** | **255** | **258** | **+3** |
| `(c) docstring present (strict-none)` | 185 | 186 | **+1** (parent shell becomes strict-none) |
| `(c) docstring present (strict-partial)` | 4 | 3 | **−1** |
| **(c) total** | **189** | **189** | 0 |

## Remaining 3 strict-partial files (after R318)

- `crates/network/src/inbound_governor.rs` (1478) — R319 candidate (split into 2)
- `crates/plutus/src/builtins.rs` (1496) — R320 candidate (declare strict mirror with sibling-file rationale)
- `crates/plutus/src/machine.rs` (1471) — R320 candidate

## Verification

```text
$ python3 scripts/audit-strict-mirror.py
audit complete: 447 rust files; candidate_match=390, no_candidate_match=57
auto-grading bucket counts:
  (a): 258
  (c): 189

$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo fmt --all -- --check
(silent — clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.74s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 20.61s

$ cargo test --workspace --all-features
passed: 4855  failed: 0
```

The 8 tests originally in `handshake.rs::tests` are now distributed
across the 3 leaves (4 in type.rs, 1 in version.rs, 3 in codec.rs)
and continue to pass — no test count delta.

## Closure criterion

- 3 leaves declare canonical strict-mirror to upstream `Type.hs` /
  `Version.hs` / `Codec.hs`.
- Parent shell preserves the flat `crate::handshake::Foo` API via
  `pub use` re-exports; no caller in the workspace needs an import
  path update.
- All 5 workspace gates green at 4,855-test baseline.
- All 8 original tests preserved across the 3 leaves.

All four are met.
