---
title: Parity Proof Report (Round 239)
layout: default
parent: Reference
nav_order: 1
---

# Yggdrasil Parity Proof Report

**Document round**: R239 refresh (2026-05-01)
**Cumulative arc**: R1 → R239
**Build**: `target/release/yggdrasil-node` (Cargo `release` profile, Rust 1.95.0)
**Workspace tests**: 4.7K+ passing, 0 failing at the R239 slice boundary

This report documents yggdrasil's parity status against upstream
IntersectMBO Cardano node / cardano-cli behavior. It is the
canonical reference for "what works end-to-end" today and what
remains. Each claim cites the round that closed it and the
operational evidence captured under `docs/operational-runs/`.

---

## 1. cardano-cli LSQ surface — 25/25 subcommands working

R205 verified all 25 `cardano-cli conway query` subcommands decode
end-to-end against yggdrasil's NtC socket on preview with
`YGG_LSQ_ERA_FLOOR=6`:

### Always-available (no era gating)

| Subcommand | Round | Wire encoder | Live data |
|---|---|---|---|
| `query tip` | R155 | `BlockQuery (QueryHardFork ...)` | ✅ live |
| `query protocol-parameters` | R156/R159/R160/R161 | Conway 31-elem PParams | ✅ live |
| `query era-history` | R153/R162 | `Interpreter` with bignum relativeTime | ✅ live |
| `query slot-number` | R162 | era-history coverage to slot 2^48 | ✅ live |
| `query utxo --whole-utxo`/`--address`/`--tx-in` | R157 | era-specific TxOut shapes | ✅ live |
| `query tx-mempool info`/`next-tx`/`tx-exists` | R158 | LocalTxMonitor v23+ | ✅ live |

### Era-gated (require `YGG_LSQ_ERA_FLOOR=6`)

| Subcommand | Round | Wire encoder | Live data |
|---|---|---|---|
| `query stake-pools` | R163/R179 | tag 16 `Set PoolKeyHash` | ✅ live (3 pools on preview) |
| `query stake-distribution` | R179 | tag 37 `[map, NonZero Coin]` | ✅ live |
| `query stake-snapshot` | R173/R179/R202/R203 | tag 20 `GetCBOR` wrap | ✅ live (per-pool from sidecar) |
| `query pool-state` | R172/R179 | tag 19 `GetCBOR` wrap | ✅ live |
| `query stake-address-info` | R177 | tag 10 `GetFilteredDelegationsAndRewardAccounts` | ✅ live |
| `query ref-script-size` | R163 | era-specific TxOut script-ref | ✅ live |

### Conway governance (R180–R188 + R193–R204)

| Subcommand | Round | Wire encoder | Live data |
|---|---|---|---|
| `query constitution` | R180 | tag 23 `Constitution` 2-elem | ✅ live (real anchor + script hash on preview) |
| `query gov-state` | R188/R193/R204 | tag 24 `ConwayGovState` 7-field | ✅ live (`GovRelation` from `EnactState`; OMap proposals shape adapter) |
| `query drep-state` | R180/R181 | tag 25 `Map DRep DRepState` | ✅ live |
| `query drep-stake-distribution` | R184/R194 | tag 26 `Map DRep Coin` | ✅ live |
| `query committee-state` | R182 | tag 27 `CommitteeMembersState` 3-elem | ✅ live |
| `query treasury` | R180 | tag 29 `[treasury, reserves]` | ✅ live |
| `query spo-stake-distribution` | R184/R194 | tag 30 `Map (KeyHash 'StakePool) Coin` | ✅ live (3 preview pools surfaced) |
| `query proposals` | R185 | tag 31 `Seq GovActionState` | ✅ live (empty on preview) |
| `query ratify-state` | R187 | tag 32 `RatifyState` 4-field record | ✅ live (real EnactState rendered) |
| `query future-pparams` | R183 | tag 33 `Maybe (PParams era)` | ✅ live |
| `query stake-pool-default-vote` | R185 | tag 35 `DefaultVote` enum | ✅ live |
| `query ledger-peer-snapshot` | R189/R195 | tag 34 V2 `[1, [WithOrigin SlotNo, indef pools]]` | ✅ live (3 preview pools with relays) |

### Operational (R190)

| Subcommand | Round | Wire encoder | Live data |
|---|---|---|---|
| `query ledger-state` | R190 | tag 12 `DebugNewEpochState` (null acceptable per cli convention) | ✅ |
| `query protocol-state` | R190/R191/R196/R197/R198 | tag 13 `Versioned 0` PraosState 8-field | ✅ live (real nonces + OCert counters) |

### CLI-side validation (not yggdrasil bugs)

These three subcommands were initially flagged in R190 audit but
turned out to be client-side argument validation issues, not
yggdrasil bugs. They work given correct CLI inputs:

- `query kes-period-info` — needs valid `--op-cert-file`
- `query leadership-schedule` — needs `--genesis FILEPATH` +
  `--stake-pool-verification-key STRING`
- `query stake-address-info` — needs Bech32-valid stake address
  (verified working via `cardano-cli conway stake-address build`-
  generated address in R190)

---

## 2. Consensus-side state persistence

R238 makes slot-indexed ChainDepState bundles the authoritative
nonce/OpCert state source. They persist atomically at ledger-checkpoint
cadence, restore on restart, restore on rollback, and feed LSQ
`protocol-state` for exact acquired points. `stake_snapshots.cbor`
remains the separate stake-snapshot mirror for stake-query surfaces:

| Sidecar | Round | Filename | Surfaces in | Restart resilient |
|---|---|---|---|---|
| ChainDepState nonce + OpCert bundle | R238 | `chain_dep_state/<slot-hex>.cbor` | `query protocol-state` nonces + `oCertCounters` | ✅ exact restart/rollback path |
| Stake snapshots | R202/R203 | `stake_snapshots.cbor` | `query stake-snapshot` per-pool totals | ✅ |

Current canonical verification is R238: exact ChainDepState sidecars are
saved under `chain_dep_state/`, rollback restores the newest bundle at or
before the target point, and LSQ `protocol-state` ignores any stale
root-level nonce/OpCert mirror files.

R205 verified the live-nonce restart path before R238 replaced the
root-level nonce/OpCert mirrors with slot-indexed ChainDepState bundles.
Those filenames below are retained only as historical evidence for that
round:

```
Pre-restart at slot ~10K:
$ ls /tmp/ygg-r205-preview-db/*.cbor
nonce_state.cbor       (114 B)
ocert_counters.cbor    (218 B)
stake_snapshots.cbor   ( 18 B)

Restart log:
[Node.Recovery] recovered ledger state from coordinated storage
  checkpointSlot=9960
  point=BlockPoint(SlotNo(10960), HeaderHash(c6dfa20907819b0c...))
  replayedVolatileBlocks=50

Post-restart:
$ cardano-cli conway query protocol-state --testnet-magic 2
{
    "candidateNonce": "509aed8ad40c83c7201fd99c84501c698137a7152127e2ebe1bb9fe70a39077c",
    "evolvingNonce":  "509aed8ad40c83c7201fd99c84501c698137a7152127e2ebe1bb9fe70a39077c",
    "labNonce":       "0e45467482b969fd4a2f50031bda686935efad281be1b714e408afe7f3eb523a",
    "lastSlot": 11940,
    ...
}
```

The current sidecars are persisted via:
- `crates/storage/src/ocert_sidecar.rs` — atomic-write helpers for
  `chain_dep_state/<slot-hex>.cbor` and `stake_snapshots.cbor`
- `node/src/sync.rs::update_ledger_checkpoint_after_progress` —
  persists ChainDepState only at checkpoint landing after nonce/OpCert
  updates
- `node/src/local_server.rs::attach_chain_dep_state_from_sidecar` —
  loads exact point sidecars at LSQ acquire time and attaches to
  `LedgerStateSnapshot` via the R192 `with_chain_dep_state` and R202
  `with_stake_snapshots` builders

---

## 3. Sync robustness — Phase B verified

R199 verified Phase B (R91 multi-peer storage livelock) is fully
resolved:

```
Setup: --max-concurrent-block-fetch-peers 4 for 2 minutes
Result: 22 K blocks synced (slot 21960 reached)
        667 immutable files written
        volatile=963 KB, immutable=1.5 MB, ledger=22 KB

Restart: recovered ledger state from coordinated storage
         checkpointSlot=21960
         replayedVolatileBlocks=100
         tip resumes from slot 23960 → advances to 25940
```

Multi-peer dispatch correctly persists to all three storage tiers
and recovery from checkpoint resumes sync without re-fetching from
origin. R91 was likely closed by R196's checkpoint persistence wiring
in the chaindb apply path; R199 explicitly verified the resolution.

---

## 4. Observability — Phase C.1 baseline (+R217 + R218 sync-rate quantification)

R200 added `yggdrasil_apply_batch_duration_seconds` Prometheus
histogram (10 cumulative buckets `[0.001, 0.005, 0.01, 0.05, 0.1,
0.5, 1.0, 5.0, 10.0, +Inf]` + `_sum` + `_count`):

```
$ curl -s http://127.0.0.1:12400/metrics | grep apply_batch
yggdrasil_apply_batch_duration_seconds_bucket{le="0.5"} 2
yggdrasil_apply_batch_duration_seconds_bucket{le="1"} 2
yggdrasil_apply_batch_duration_seconds_bucket{le="+Inf"} 2
yggdrasil_apply_batch_duration_seconds_sum 0.412206
yggdrasil_apply_batch_duration_seconds_count 2
```

Preview baseline: ~206 ms/batch.

**R217 added the companion `yggdrasil_fetch_batch_duration_seconds`
histogram** (same bucket boundaries; covers ChainSync `RequestNext` +
BlockFetch `RequestRange` round-trip + body-hash + KES verification).
Mainnet baseline (60 s, 4 batches, single-peer):

```
yggdrasil_fetch_batch_duration_seconds_sum 51.384605
yggdrasil_fetch_batch_duration_seconds_count 4
yggdrasil_apply_batch_duration_seconds_sum 0.871671
yggdrasil_apply_batch_duration_seconds_count 4
```

→ fetch avg = 12.85 s/batch, apply avg = 0.22 s/batch.  **Fetch is
~59× more expensive than apply** on mainnet — pipelined fetch+apply
(Phase C.2) saves at most ~1.7% throughput.

**R218 operationally validated multi-peer dispatch as the actual
sync-rate lever**.  `--max-concurrent-block-fetch-peers 4` on mainnet
(2 active warm peers) produces:

| Configuration                                           | fetch avg / batch | apply avg / batch | throughput |
| ------------------------------------------------------- | ----------------: | ----------------: | ---------: |
| Single-peer (R217)                                      |          12.85 s  |           0.22 s  | 3.33 blk/s |
| Multi-peer, 2 workers (R218)                            |           8.56 s  |           0.23 s  | 5.55 blk/s |
| **Δ**                                                   |    **−33%**       |   **flat (noise)**| **+67%**   |

Apply is unchanged — confirms multi-peer dispatch isolates fetch
parallelism without touching the apply path.  Each additional warm
peer that registers as a worker subtracts ≈ `(fetch_avg / N)` from
the per-batch fetch time, so operators can recover sync rate by
adding topology peers.

---

## 4b. Phase D.2 — Multi-session peer accounting (5 lifetime counters)

R222 + R223 + R224 + R226 deliver the major Phase D.2 scope: a
parallel-tracking shadow data structure (`PeerLifetimeStats`) on
`GovernorState` that accumulates monotonically across reconnects,
distinct from the session-keyed `failures` map (which decays /
resets on `record_success`).  Five Prometheus counters / gauges
expose the aggregate state:

| Metric                                    | Type    | Source                              | Round  |
| ----------------------------------------- | ------- | ----------------------------------- | ------ |
| `peer_lifetime_sessions_total`            | counter | `promote_to_warm` Ok branch         | R223   |
| `peer_lifetime_failures_total`            | counter | `promote_to_warm` Err branch        | R223   |
| `peer_lifetime_bytes_in_total`            | counter | BlockFetch `bytes_delivered` mirror | R224   |
| `peer_lifetime_unique_peers`              | gauge   | `lifetime_stats` map cardinality    | R226   |
| `peer_lifetime_handshakes_total`          | counter | `successful_handshakes` aggregate   | R226   |

**Mainnet operator-derived signals** (verified R226, 60 s knob=4):

```
yggdrasil_peer_lifetime_sessions_total 2
yggdrasil_peer_lifetime_failures_total 0
yggdrasil_peer_lifetime_bytes_in_total 1548246
yggdrasil_peer_lifetime_unique_peers 3
yggdrasil_peer_lifetime_handshakes_total 2
```

Operators compute:

```promql
# Reliability ratio
yggdrasil_peer_lifetime_failures_total
  / yggdrasil_peer_lifetime_sessions_total

# Avg bytes per session
yggdrasil_peer_lifetime_bytes_in_total
  / yggdrasil_peer_lifetime_sessions_total

# Registry-leakage indicator (peers tracked but never promoted)
1 - (yggdrasil_peer_lifetime_sessions_total
     / yggdrasil_peer_lifetime_unique_peers)

# Real peer churn rate (cumulative reconnects)
rate(yggdrasil_peer_lifetime_sessions_total[5m])
```

R234, R235, and R237 complete the aggregate server egress path:
BlockFetch, ChainSync, KeepAlive, TxSubmission2, and PeerSharing
bytes-out counters are recorded without high-cardinality Prometheus
labels, and per-peer egress totals are folded into lifetime stats.

---

## 4c. Phase D.1 — Rollback recovery and sidecars (R225+R237+R238)

R225 adds `yggdrasil_rollback_depth_blocks` Prometheus histogram
classifying actual rollback depths. Bucket boundaries
`[1, 2, 5, 50, 2160 (k), 10_000, +Inf]` span shallow chain
reorgs through the stability window edge to cross-epoch and
full-resync shapes.

Operators alert on rare deep rollbacks via:

```promql
histogram_quantile(0.99,
    rate(yggdrasil_rollback_depth_blocks_bucket[1h]))
```

R237 adds epoch-boundary-aware checkpoint replay when stake
snapshots are enabled. R238 completes the code-level nonce/OpCert
sidecar hardening: storage keeps opaque slot-indexed
`chain_dep_state/<slot-hex>.cbor` bundles, verified sync writes
them only at ledger-checkpoint cadence after nonce/OpCert updates,
`RollBackward` terminates the current batch, recovery restores the
newest bundle at or before the rollback point, verifies the bundled
point against the selected chain prefix, and replays stored raw
blocks to the rollback target. LSQ `protocol-state` prefers exact
point sidecars and does not read nonce/OpCert latest mirrors.

---

## 5. Upstream alignment — Phase E.1 closed

R201 advanced the first documentary upstream pins to live HEAD, R216
refreshed the pins that drifted again, and R239 completed the
coordinated `cardano-base` fixture refresh. All six canonical
IntersectMBO pins now match live HEAD and `cardano-base` still keeps
the test-vector directory name, crypto test constant, and node pin in
lockstep:

| Repository | Pinned (post-R239) | Status |
|---|---|---|
| `cardano-base` | `7a8a991945d4…` (R239 fixture refresh) | **in-sync** |
| `cardano-ledger` | `42d088ed84b7…` | **in-sync** |
| `ouroboros-consensus` | `b047aca4a731…` (R216 advance) | **in-sync** |
| `ouroboros-network` | `0e84bced45c7…` | **in-sync** |
| `plutus` | `4cd40a14e364…` (R216 advance) | **in-sync** |
| `cardano-node` | `799325937a45…` | **in-sync** |

Drift detector (`bash node/scripts/check_upstream_drift.sh`) reports
`drifted=0 unreachable=0 total=6`. Three drift-guard tests pass
(format, cardinality, vendored-directory match). R201 → R216 → R239
demonstrates the audit baseline is actively maintained against
upstream while preserving SHA-anchored vendored fixture provenance.

---

## 6. Cumulative phase status

| Phase | Item | Status | Round(s) |
|---|---|---|---|
| **A.1** | `ChainDepStateContext` infrastructure | ✅ closed | R192 |
| **A.2** | Live PraosState (OCert + nonces) | ✅ closed | R196+R197+R198 |
| **A.3** | Live `GovRelation` + gov-state OMap shape | ✅ closed | R193+R204 |
| **A.4** | Live DRep/SPO stake + deleg deposits | ✅ closed | R194 |
| **A.5** | Live ledger-peer-snapshot pools | ✅ closed | R195 |
| **A.6** | `GetGenesisConfig` ShelleyGenesis serialiser | ✅ closed | R214 |
| **A.7** | Live stake-snapshots | ✅ closed | R202+R203 |
| **B** | R91 multi-peer dispatch livelock | ✅ verified resolved | R199 |
| **B (mainnet)** | Mainnet sync unblocked (Byron EBB hash + same-slot tolerance + mux egress) | ✅ closed | R211+R213 |
| **B (P2P)** | Bidirectional P2P parity (server ChainSync `Tip` envelope) | ✅ closed | R220+R221 |
| **C.1** | Apply-batch duration histogram | ✅ wired | R200 |
| **C.1+** | Fetch-batch duration histogram + multi-peer quantification | ✅ wired | R217+R218 |
| **C.2** | Pipelined fetch+apply | 🚫 de-prioritised | R217 measurement showed ~1.7% gain — multi-peer dispatch is the actual sync-rate lever |
| **D.1** | Deep rollback recovery and chain-dep sidecars | ✅ closed code-level slice | R225+R237+R238 |
| **D.2** | Multi-session peer accounting + aggregate bytes-out | ✅ shipped | R222+R223+R224+R226+R234+R235+R237 |
| **E.1** | Audit baseline pin refresh + `cardano-base` fixture refresh | ✅ closed, 6/6 pins in-sync | R201+R216+R239 |
| **E.2** | Mainnet rehearsal (24h+) | ⏳ deferred | (long-running observation) |
| **E.3** | Parity proof report | ✅ this document (R206) | — |

The remaining gates are no longer known code-level parity blockers.
They require sustained operator time: the 24h+ mainnet rehearsal and
the runbook §6.5 sign-off before changing the default
`max_concurrent_block_fetch_peers`.

---

## 7. Remaining gates

### Phase E.2 — Mainnet rehearsal

24+ hour continuous mainnet sync from genesis with metrics capture.
Validates the already-working R211/R213 mainnet path at operator
duration, including restart cycles, hash comparison against the
Haskell node, and R238 rollback sidecar behavior under real chain
conditions.

### Parallel BlockFetch default flip

The multi-peer dispatch path is implemented and observable, but
`max_concurrent_block_fetch_peers` defaults to `1` until
[`MANUAL_TEST_RUNBOOK.md`](MANUAL_TEST_RUNBOOK.md) §6.5 signs off
2- and 4-peer rehearsals with restart-resilience evidence.

---

## 8. Verification commands

To reproduce R205's comprehensive verification:

```bash
# Build
cargo build --release -p yggdrasil-node

# Boot
rm -rf /tmp/ygg-verify-db /tmp/ygg-verify.sock
YGG_LSQ_ERA_FLOOR=6 target/release/yggdrasil-node run \
    --network preview \
    --database-path /tmp/ygg-verify-db \
    --socket-path /tmp/ygg-verify.sock \
    --metrics-port 12400 &

sleep 30  # let sync establish

# Sweep all 25 cardano-cli subcommands
export CARDANO_NODE_SOCKET_PATH=/tmp/ygg-verify.sock
cardano-cli query tip --testnet-magic 2
cardano-cli conway query protocol-state --testnet-magic 2
cardano-cli conway query gov-state --testnet-magic 2
# ... (full list in R205 operational-run doc)

# Verify sidecars persist
find /tmp/ygg-verify-db -maxdepth 2 -name '*.cbor' -print
# Expected: stake_snapshots.cbor plus chain_dep_state/<slot-hex>.cbor

# Apply-batch histogram
curl -s http://127.0.0.1:12400/metrics | grep apply_batch
# Expected: 10 bucket lines + _sum + _count

# Drift detector
bash node/scripts/check_upstream_drift.sh
# Expected: drifted=0 unreachable=0 total=6

# Workspace gates
cargo fmt --all -- --check
cargo lint
cargo test-all
```

---

## 8b. Mainnet boot smoke test (R208, 2026-04-30)

This section is retained as historical diagnostic evidence. The
failure was resolved by R211/R213 and then verified through the
cardano-cli wire stack in R212.

R208 ran a 2-minute mainnet boot smoke test to validate the
`--network mainnet` codepath:

```
$ /workspaces/Cardano-node/target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r208-mainnet-db \
    --socket-path /tmp/ygg-r208-mainnet.sock \
    --metrics-port 12408 &

# After 2 minutes:
$ cardano-cli query tip --mainnet
{
    "epoch": 0,
    "era": "Byron",
    "slotInEpoch": 0,
    "slotsToEpochEnd": 21600,
    "syncProgress": "0.00"
}

# Storage:
volatile/  (0 bytes — no blocks fetched)
ledger/    (no checkpoints persisted)
immutable/ (empty)

# Log shows repeated "ledger checkpoints cleared at origin"
# and verified-sync session establishes but doesn't advance.
```

**Historical result**: yggdrasil's `--network mainnet` flag was
recognised and booted cleanly, but block fetch/apply did not
advance past Origin in this 2-minute R208 window.

**Resolution**: R211 fixed the Byron EBB hash prefix and same-slot
consensus tolerance, R213 fixed the mux egress limit for large
single LSQ payloads, and R212 verified mainnet `cardano-cli`
queries against an actively syncing node. Phase E.2 now means
long-duration operator rehearsal, not diagnosing this R208 stall.

---

## 8e. Mainnet operational verification with cardano-cli (R212, 2026-04-30)

R212 validates R211's mainnet sync fix through the full LSQ wire
stack — cardano-cli queries decode end-to-end against an actively
syncing mainnet yggdrasil node.

**Setup**:
```
$ ./target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r212-mainnet-db \
    --socket-path /tmp/ygg-r212-mainnet.sock \
    --peer 3.135.125.51:3001 \
    --metrics-port 12412 &
$ sleep 45
```

**Sync after 45 s**:
```
volatile/  1 455 234 bytes
ledger/    1 363 702 bytes
checkpoint persisted slot=47 retainedSnapshots=1
checkpoint skipped   slot=97 / 147 (interval=2160)
```

**cardano-cli query results** (all against the active mainnet
sync):

| Query                        | Result                                               |
| ---------------------------- | ---------------------------------------------------- |
| `query tip`                  | `{block: 197 → 397, era: "Shelley", hash: cf29…/a15b…}` |
| `query era-history`          | indef-length 2-era CBOR summary (Byron + Shelley)    |
| `query slot-number 2024-06-01T00:00:00Z` | `125712000`                              |
| `query protocol-parameters`  | 17-element Shelley shape, full PP JSON               |
| `query tx-mempool info`      | `{capacity: 0, count: 0, size: 0, slot: 397}`        |

**Sidecars** (post-test mainnet `<storage_dir>/`, historical R212
pre-R238 filenames):
```
nonce_state.cbor      12 B
ocert_counters.cbor    1 B
stake_snapshots.cbor  14 B
```

Smaller than testnets because mainnet at slot 397 is pre-Shelley
(post-Byron consensus state mostly empty — same shape as the
pre-Shelley testnet behaviour observed in R207).

Post-R238 runs should instead show ChainDepState point bundles under
`<storage_dir>/chain_dep_state/` plus the separate
`stake_snapshots.cbor` mirror:

```
$ find <storage_dir>/chain_dep_state -type f -name '*.cbor' | sort
<storage_dir>/chain_dep_state/000000000000002f.cbor
...
```

**Multi-network parity matrix** (closed by R212):

| Network          | Operational verification | LSQ subcommands              | Sidecars | Round    |
| ---------------- | ------------------------ | ---------------------------- | -------- | -------- |
| Preview          | ✅ (Conway era)           | 25/25 with `YGG_LSQ_ERA_FLOOR=6` | ✅       | R205     |
| Preprod          | ✅ (Allegra era)          | 6/6 baseline                 | ✅       | R207     |
| Mainnet          | ✅ (Byron at slot 397)    | 5/5 baseline (utxo TBD)      | ✅       | **R212** |

**Known limitation** (closed in R213, 2026-04-30): `query utxo
--whole-utxo --mainnet` initially failed with `BearerClosed`.  Root
cause was a 10-line semantic miscoding in the mux back-pressure
check (`current + len > limit` rejected single large payloads even
with empty buffer; should be `current > limit` per upstream
`network-mux::egressSoftBufferLimit`).  After R213's fix the query
returns the **full mainnet AVVM bootstrap UTxO**: 14 505 entries
totaling 31 112 484 745 ADA — matching `byron-genesis.json::avvmDistr`
exactly.  See R213 in `docs/operational-runs/`.

---

## 8d. Mainnet sync unblocked — Byron EBB hash + same-slot tolerance (R211, 2026-04-30)

R211 closed the Phase E.2 critical path with a two-bug cascade fix:

**Bug 1 — wrong hash prefix for Byron EBB headers**.  yggdrasil's
[`node/src/sync.rs::point_from_raw_header`](../node/src/sync.rs)
helper used `byron_main_header_hash` (prefix `[0x82, 0x01]`) for
EBB-shape headers.  Byron EBBs require `[0x82, 0x00]` per
`Cardano.Chain.Block.Header.boundaryHeaderHashAnnotated`.  Wrong
prefix → wrong hash → upstream BlockFetch can't resolve the
upper-bound point → IOG peer closes mux mid-request.

**Bug 2 — strict slot-monotonicity rejects Byron EBB→main_block at
same slot**.  Consensus `ChainState::roll_forward` rejected the
legitimate Byron transition where the genesis EBB at slot 0 is
followed by the first main block of epoch 0 also at slot 0 (Byron
EBBs are virtual epoch-boundary markers).  The ledger-side check
already had Byron exemption; consensus-side was missing it.

**Verification — mainnet now syncs**:

```
$ rm -rf /tmp/ygg-r211e-mainnet-db
$ YGG_SYNC_DEBUG=1 timeout 60 ./target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r211e-mainnet-db \
    --peer 3.135.125.51:3001 \
    --max-concurrent-block-fetch-peers 1

[YGG_SYNC_DEBUG] shared applied
    stable_block_count=0 epoch_events=0 rolled_back_tx_ids=0
    tracking.tip=BlockPoint(SlotNo(197), HeaderHash(cf298afbb9eae55d…))

volatile/  1 532 832 bytes  ← non-zero
ledger/    1 363 702 bytes  ← checkpoint snapshots accumulating
```

Comparison R210 → R211:

| Signal                         |   R210  |   R211e |
| ------------------------------ | ------- | ------- |
| `[YGG_SYNC_DEBUG] applied`     |     0   |     6   |
| `volatile/` size               |   0 B   | 1.5 MB  |
| `ledger/` size                 |   0 B   | 1.4 MB  |
| Final tip                      | Origin  | slot 197|
| `cleared-origin` recoveries    |    12   |     0   |

**Code changes** (4 files):
- `node/src/sync.rs` — new `byron_ebb_header_hash` helper;
  `decode_point_from_byron_raw_header` returns `Some(Point)` for
  EBBs (slot from inner `epoch * BYRON_SLOTS_PER_EPOCH`, hash via
  EBB prefix).
- `crates/consensus/src/chain_state.rs` — slot check relaxed from
  `<=` to `<`.  Block-number contiguity check above catches
  re-application; Praos guarantees ≤ 1 block/slot post-Byron.
- `node/src/runtime.rs` — R210's `YGG_SYNC_DEBUG=1` trace mirrored
  to shared-chaindb apply call site (the production NtN+NtC path).
- Test updates: `roll_forward_accepts_same_slot_byron_ebb_main_pair`,
  `point_from_raw_header_decodes_observed_byron_serialised_header_envelope`
  updated to expect EBB hash + slot=0 from inner header (the
  original test pinned the wrong slot 83 from outer envelope + main
  hash, masking the bug for ~200 rounds).

**Strategic significance**: yggdrasil now syncs mainnet end-to-end
(subject to long-running stability + performance, separately
tracked).  The two-step diagnosis (R210 narrows to BlockFetch wire
layer → R211 source-level diff identifies the encoding bug) is the
canonical pattern for operational-parity work.

---

## 8c. Mainnet stall narrowed to BlockFetch wire layer (R210, 2026-04-30)

R210 added an opt-in `YGG_SYNC_DEBUG=1` apply-side trace at the
`apply_verified_progress_to_chaindb` call site in
[`node/src/runtime.rs`](../node/src/runtime.rs) (~line 5008) to
answer R208's open question: is the stall at BlockFetch (zero
blocks fetched per batch) or at apply (blocks fetched but
silently rejected)?

90 s mainnet run findings:

```
YGG_SYNC_DEBUG=1 timeout 90 ./target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r210-mainnet-db \
    --peer 3.135.125.51:3001 \
    --max-concurrent-block-fetch-peers 1

| Signal                                     |   Count |
| ------------------------------------------ | ------- |
| [YGG_SYNC_DEBUG] apply_verified_progress   |     0   |
| [ygg-sync-debug] blockfetch-range          |   634   |
| [ygg-sync-debug] demux-exit                |     2   |
| Node.Recovery.Checkpoint cleared-origin    |    12   |
| volatile/, immutable/, ledger/             |  0 B ea |
```

ChainSync header decodes cleanly (`header_point_decoded=true
raw_header_len=94`) for the first Byron-era range
`Origin → SlotNo(648087)`, but the IOG backbone peer **closes the
mux during the BlockFetch request**, so `apply_verified_progress`
is never invoked and no checkpoint, sidecar, volatile, or
immutable file lands.

**Conclusion**: the R208 mainnet sync gap is at the
**BlockFetch wire layer**, NOT at apply / ledger / storage.
Every R208 hypothesis pointing at apply-path silent rejection
or storage hand-off is now ruled out.

**Narrowed root-cause candidates**:
1. Byron BlockFetch `MsgRequestRange` CBOR shape divergence on
   the request side (most likely).
2. NtN handshake version negotiation rejecting BlockFetch but
   accepting ChainSync.
3. Byron EBB hash indirection upstream expects in the upper
   bound.

**R211+ follow-up scope**: capture `MsgRequestRange` bytes via
`tcpdump`/socat-relay against the same peer; run upstream
`cardano-node 10.7.x` for byte-comparison; fix in
[`crates/network/src/protocols/blockfetch_pool.rs`](../crates/network/src/protocols/blockfetch_pool.rs)
or the `MsgRequestRange` encoder.

The R210 instrumentation is permanent in the runtime, env-gated,
zero-overhead when unset, and ready for use during the wire-byte
diagnosis follow-up.

---

## 8a. Multi-network verification (R207, 2026-04-30)

R207 verified the same gates work on preprod (Shelley-era):

```
$ /workspaces/Cardano-node/target/release/yggdrasil-node run \
    --network preprod \
    --database-path /tmp/ygg-r207-preprod-db \
    --socket-path /tmp/ygg-r207-preprod.sock \
    --metrics-port 12407 &
sleep 35

$ ls -la /tmp/ygg-r207-preprod-db/*.cbor
114 nonce_state.cbor
  1 ocert_counters.cbor
 18 stake_snapshots.cbor
# Historical pre-R238 mirror files; current runs use chain_dep_state/*.cbor
# for nonce/OpCert and stake_snapshots.cbor for stake-query snapshots.

$ cardano-cli query tip --testnet-magic 1
{
    "block": 87440,
    "epoch": 4,
    "era": "Allegra",
    "slot": 87440,
    "syncProgress": "1.40"
}
# 87K blocks synced in 35s

$ Sweep baseline cardano-cli queries:
OK: tip / protocol-parameters / era-history / slot-number /
    utxo --whole-utxo / tx-mempool info
=== preprod: pass=6 fail=0 ===
```

R190 already verified the full era-gated + Conway suite on preview;
R207 confirms the always-available baseline subcommands work on
preprod (Shelley-era chain) without requiring `YGG_LSQ_ERA_FLOOR`.
**Both networks** demonstrate consistent yggdrasil parity end-to-end.

---

## 9. References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md)
- Operational runs:
  [`docs/operational-runs/`](operational-runs/)
- Cumulative parity matrix:
  [`docs/UPSTREAM_PARITY.md`](UPSTREAM_PARITY.md)
- Per-round summaries:
  [`docs/PARITY_SUMMARY.md`](PARITY_SUMMARY.md)
- Roadmap:
  [`docs/PARITY_PLAN.md`](PARITY_PLAN.md)
- Workspace journal:
  [`AGENTS.md`](../AGENTS.md)

---

**End of parity proof report (R206 / 2026-04-30).**
