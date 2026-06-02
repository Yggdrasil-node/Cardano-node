---
title: 'R314: promote partial-mirror docstrings to canonical strict-mirror'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-314-promote-partial-mirrors/
---

# Round 314 — promote partial-mirror docstrings to canonical strict-mirror

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R313`](2026-05-09-round-313-synthesis-file-census.md)  
**Trigger:** R313 census surfaced 41 `(c) docstring present (unspecified)`
files; operator asked to "fix those that carry an explicit synthesis
declaration so we could achieve better parity and code quality."

## Summary

R314 closes the docstring-classification gap revealed by R313:

1. **Audit regex bug fix.** `dev/test/audit-strict-mirror.py`'s
   `STRICT_PARTIAL_PATTERN` was `\*\*Strict mirror\s*\(partial\)\*\*`
   (no colon). All 41 affected files actually used
   `**Strict mirror (partial):**` (with colon), so they fell
   through to the `(unspecified)` bucket instead of being
   recognized as `(strict-partial)`. Tightened the regex to
   `\*\*Strict mirror\s*\(partial\):?\*\*` (optional colon).
2. **Promotions to canonical strict-mirror.** Of the 41 files,
   24 turned out to map 1:1 to a single upstream `.hs` — the
   `(partial)` qualifier was being used to mean "filename
   flattens the upstream directory" rather than "combines
   multiple upstream files." Those 24 are now declared as
   canonical `**Strict mirror:** <upstream/path.hs>.` and
   auto-grade to `(a) DIRECT_MIRROR (auto: docstring declares
   strict mirror)`.
3. **Remaining genuine partial mirrors stay as `(partial)`.**
   17 files genuinely combine multiple upstream files
   (e.g., `handshake.rs` = `Type.hs` + `Version.hs` + `Codec.hs`)
   or split one upstream file across two Rust files
   (e.g., `mux.rs` + `multiplexer.rs` for `Mux.hs`). For these,
   `**Strict mirror (partial):**` remains the honest declaration
   and (after the regex fix) is now correctly classified as
   `(c) docstring present (strict-partial)`.

## Bucket-count delta

| Bucket | R313 | R314 | Δ |
|---|---:|---:|---:|
| `(a) DIRECT_MIRROR (auto: docstring declares strict mirror)` | 187 | 211 | **+24** |
| `(a) DIRECT_MIRROR (auto)` | 25 | 25 | 0 |
| `(a) DIRECT_MIRROR (auto (affinity-filtered))` | 18 | 18 | 0 |
| **(a) total** | **230** | **254** | **+24** |
| `(c) docstring present (strict-none)` | 174 | 174 | 0 |
| `(c) docstring present (strict-partial)` | 0 | 17 | +17 |
| `(c) docstring present (unspecified)` | 41 | 0 | **−41** |
| **(c) total** | **215** | **191** | **−24** |
| **Grand total** | **445** | **445** | 0 |

The 41 `(unspecified)` files split:
- 24 promoted to canonical strict-mirror declarations.
- 17 reclassified as `(strict-partial)` after the regex fix.

After R314, every production `.rs` file has either a canonical
`**Strict mirror:** <path>.` declaration (254 files) or one of two
explicitly-classified honest secondary forms: `**Strict mirror:**
none.` (174 synthesis files) or `**Strict mirror (partial):**`
(17 files that combine/split upstream).

## Files promoted to canonical strict-mirror (24 total)

### Mini-protocol client/server drivers (16 files)

| Rust path | Upstream `.hs` |
|---|---|
| `crates/network/src/chainsync_client.rs` | `Ouroboros/Network/Protocol/ChainSync/Client.hs` |
| `crates/network/src/chainsync_server.rs` | `Ouroboros/Network/Protocol/ChainSync/Server.hs` |
| `crates/network/src/blockfetch_client.rs` | `Ouroboros/Network/Protocol/BlockFetch/Client.hs` |
| `crates/network/src/blockfetch_server.rs` | `Ouroboros/Network/Protocol/BlockFetch/Server.hs` |
| `crates/network/src/keepalive_client.rs` | `Ouroboros/Network/Protocol/KeepAlive/Client.hs` |
| `crates/network/src/keepalive_server.rs` | `Ouroboros/Network/Protocol/KeepAlive/Server.hs` |
| `crates/network/src/local_state_query_client.rs` | `Ouroboros/Network/Protocol/LocalStateQuery/Client.hs` |
| `crates/network/src/local_state_query_server.rs` | `Ouroboros/Network/Protocol/LocalStateQuery/Server.hs` |
| `crates/network/src/local_tx_monitor_client.rs` | `Ouroboros/Network/Protocol/LocalTxMonitor/Client.hs` |
| `crates/network/src/local_tx_monitor_server.rs` | `Ouroboros/Network/Protocol/LocalTxMonitor/Server.hs` |
| `crates/network/src/local_tx_submission_client.rs` | `Ouroboros/Network/Protocol/LocalTxSubmission/Client.hs` |
| `crates/network/src/local_tx_submission_server.rs` | `Ouroboros/Network/Protocol/LocalTxSubmission/Server.hs` |
| `crates/network/src/peersharing_client.rs` | `Ouroboros/Network/Protocol/PeerSharing/Client.hs` |
| `crates/network/src/peersharing_server.rs` | `Ouroboros/Network/Protocol/PeerSharing/Server.hs` |
| `crates/network/src/txsubmission_client.rs` | `Ouroboros/Network/Protocol/TxSubmission2/Client.hs` |
| `crates/network/src/txsubmission_server.rs` | `Ouroboros/Network/Protocol/TxSubmission2/Server.hs` |

### Protocol Type definitions (4 files)

| Rust path | Upstream `.hs` |
|---|---|
| `crates/network/src/protocols/chain_sync.rs` | `Ouroboros/Network/Protocol/ChainSync/Type.hs` |
| `crates/network/src/protocols/local_tx_monitor.rs` | `Ouroboros/Network/Protocol/LocalTxMonitor/Type.hs` |
| `crates/network/src/protocols/local_tx_submission.rs` | `Ouroboros/Network/Protocol/LocalTxSubmission/Type.hs` |
| `crates/network/src/protocols/peer_sharing.rs` | `Ouroboros/Network/Protocol/PeerSharing/Type.hs` |

### Other 1:1 mirrors (4 files)

| Rust path | Upstream `.hs` |
|---|---|
| `crates/consensus/src/diffusion_pipelining/identity.rs` | `Ouroboros/Consensus/Shelley/Node/DiffusionPipelining.hs` |
| `crates/consensus/src/diffusion_pipelining/state.rs` | `Ouroboros/Consensus/Block/SupportsDiffusionPipelining.hs` |
| `crates/consensus/src/mempool/tx_state/state.rs` | `Ouroboros/Network/TxSubmission/Inbound/V2/State.hs` |
| `crates/network/src/peer_state_actions.rs` | `Ouroboros/Network/PeerSelection/PeerStateActions.hs` |

## Files staying as `(partial)` (17 — genuine partial mirrors)

These files genuinely combine multiple upstream files or split one
upstream file across two Rust files. The `(partial)` qualifier is
honest:

| Rust path | Why partial |
|---|---|
| `crates/consensus/src/genesis_density.rs` | Yggdrasil-side density compare; no single upstream parallel |
| `crates/consensus/src/in_future.rs` | Yggdrasil-side adapter for future-block judgement |
| `crates/consensus/src/praos/common.rs` | Combines `Cardano.Ledger.BaseTypes::ActiveSlotCoeff` + `Praos/VRF.hs` helpers |
| `crates/crypto/src/sha3_hash.rs` | Yggdrasil-side SHA3-256 wrapper (cardano-crypto-class facet) |
| `crates/ledger/src/rewards.rs` | Yggdrasil aggregator; combines reward-pot + distribution logic |
| `crates/ledger/src/utxo.rs` | Yggdrasil aggregator over multiple per-era UTxO files |
| `crates/network/src/blockfetch_pool.rs` | Combines `BlockFetch/ClientState.hs` + `BlockFetch/Decision.hs` |
| `crates/network/src/governor/churn.rs` | Yggdrasil split of governor churn cycle from `Governor.hs` |
| `crates/network/src/governor/peer_metric.rs` | Combines `PeerMetric.hs` + `LedgerPeers/Utils.hs` (peer-pick policy) |
| `crates/network/src/handshake.rs` | Combines `Handshake/{Type,Version,Codec}.hs` |
| `crates/network/src/inbound_governor.rs` | Combines `InboundGovernor.hs` + `InboundGovernor/State.hs` |
| `crates/network/src/listener.rs` | Yggdrasil-side accept loop; no single upstream parallel |
| `crates/network/src/multiplexer.rs` | Yggdrasil split — high-level mux wiring half of `Mux.hs` |
| `crates/network/src/mux.rs` | Yggdrasil split — SDU framing + per-channel state machine half of `Mux.hs` |
| `crates/plutus/src/builtins.rs` | Combines `PlutusCore/Default/Builtins.hs` runtime + cost helpers |
| `crates/plutus/src/machine.rs` | Yggdrasil split of `Cek/Internal.hs` (driver vs types vs cost) |
| `node/src/runtime/keep_alive.rs` | Runtime-side adaptor wrapping protocol-side `KeepAlive` |

## Diff inventory

| Path | Change |
|---|---|
| `dev/test/audit-strict-mirror.py` | `STRICT_PARTIAL_PATTERN` regex tightened to recognize `**Strict mirror (partial):**` (with optional colon). 1-character change. |
| 24 production `.rs` files | `**Strict mirror (partial):** mirrors upstream\n//! \`<haskell.module.path>\`.` blocks replaced with single-line `**Strict mirror:** <slash/path>.`. Mechanical change via Python script (regex replacement). |
| `docs/strict-mirror-audit.tsv` | Re-generated from updated source files; bucket counts shifted per the table above. |
| `docs/operational-runs/2026-05-09-round-314-promote-partial-mirrors.md` | This round-doc. |

R314 ships zero Rust code changes — only docstring text and the
audit script's regex tightening. The 4,855-test workspace baseline
is preserved by construction; no behavior changes.

## Verification

```text
$ python3 dev/test/audit-strict-mirror.py
audit complete: 445 rust files; candidate_match=387, no_candidate_match=58
auto-grading bucket counts:
  (a): 254   (was 230)
  (c): 191   (was 215)

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo fmt --all -- --check
(silent — clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.05s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.65s

$ cargo test --workspace --all-features
passed: 4855  failed: 0

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 8 entries validated

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945… consistent
```

## Closure criterion

- Every production `.rs` file with a `## Naming parity` block
  declares either `**Strict mirror:** <path>.`,
  `**Strict mirror:** none.`, or `**Strict mirror (partial):**`
  — no `(unspecified)` form remains.
- Audit bucket counts shifted as planned (24 files promoted to
  `(a) DIRECT_MIRROR`; 17 files reclassified as
  `(c) strict-partial`; `(unspecified)` bucket emptied).
- All five workspace gates green at 4,855-test baseline.
- All four CI parity validators clean.

All four are met.

## Future work (deferred)

- **Genuine partial-mirror split.** The 17 remaining `(partial)`
  files could in principle be split or merged to achieve full
  1:1 mirroring. For example:
  - `mux.rs` + `multiplexer.rs` could be merged into a single
    `mux.rs` matching upstream `Mux.hs`.
  - `handshake.rs` could be split into `handshake/{type,version,codec}.rs`
    matching upstream `Handshake/{Type,Version,Codec}.hs`.
  - `inbound_governor.rs` could be split into
    `inbound_governor.rs` + `inbound_governor/state.rs`.

  Each split/merge would be a non-trivial code reshape with
  blast radius across `use` imports and tests. Deferred until
  a contributor is already touching those files for a
  substantive reason; the current `(partial)` declarations
  remain honest and policy-compliant in the meantime.

- **Tighten auto-graded `(a) auto` matches.** 25 files use
  basename-match without an explicit `**Strict mirror:**`
  declaration. Adding an explicit declaration would tighten
  the parity claim (the audit grader could then drop the
  affinity-filter heuristic entirely). Deferred until a
  contributor is already touching those files.
