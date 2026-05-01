---
title: Manual Test Runbook
layout: default
parent: Reference
nav_order: 8
---

# Manual Test Runbook — Yggdrasil Real-Life Operations

**Purpose**: ordered checklist for the operator running `yggdrasil-node` against real Cardano networks (preprod first, then mainnet) with the supporting scripts created in Slices L–N. This is the document referenced as the bring-up runbook in `docs/AUDIT_VERIFICATION_2026Q2.md`.

**Audience**: someone with shell access to a host that can reach the public Cardano relay set, optionally with a real pool's KES/VRF/OpCert/issuer-vkey credentials, and (optionally) a running upstream Haskell `cardano-node` for hash comparison.

**When to use this**: after the audit/bring-up plan slices have landed (or when validating the as-is 99% codebase). Designed to be exercised end-to-end at least once before declaring the node "manually verified".

---

## 1. Prerequisites

- Rust toolchain pinned to `1.95.0` (per `rust-toolchain.toml`); confirm with `cargo --version`.
- Build the binary in release mode for any rehearsal that runs longer than ~5 minutes:
  ```sh
  cargo build --release -p yggdrasil-node
  export YGG_BIN="$PWD/target/release/yggdrasil-node"
  ```
  (debug build works for short smoke tests but is much slower at block validation.)
- Optional but recommended for hash-comparison: an upstream `cardano-cli` binary on `$PATH` and (separately) a running `cardano-node` syncing the same network with a known `--socket-path`.
- Optional for producer mode: a real pool's `kes.skey`, `vrf.skey`, `node.cert`, and `cold.vkey` (issuer vkey).
- Optional for upstream E2E parity (§8.5): Docker or Podman for the
  official `cardano-node-tests` `runner/runc.sh` wrapper. The
  repository devcontainer provisions Docker CLI access through the
  devcontainers `docker-outside-of-docker` feature, plus `gh`,
  `actionlint`, and `shellcheck` for manual workflow inspection and
  harness validation.
- Vendored network configs are already present at:
  - `node/configuration/mainnet/`
  - `node/configuration/preprod/`
  - `node/configuration/preview/`

  Each directory contains the JSON config plus the four genesis files (Byron, Shelley, Alonzo, Conway). No download step is required.

### 1a. Preview producer harness (fast local gate)

Use preview when you need the fastest public-network startup/sync loop and do not yet have real preprod/mainnet pool credentials. The harness uses upstream `cardano-cli` to generate text-envelope cold, VRF, KES, and operational-certificate files, then writes self-contained preview relay and producer configs under `tmp/preview-producer/`.

```sh
cargo build --release -p yggdrasil-node
export YGG_BIN="$PWD/target/release/yggdrasil-node"

FORCE=1 node/scripts/preview_producer_harness.sh generate
node/scripts/preview_producer_harness.sh wallet
node/scripts/preview_producer_harness.sh certs
node/scripts/preview_producer_harness.sh validate
RUN_SECONDS=60 node/scripts/preview_producer_harness.sh smoke-relay
RUN_SECONDS=60 node/scripts/preview_producer_harness.sh smoke-producer
RUN_SECONDS=300 MIN_SLOT_ADVANCE=1000 node/scripts/preview_producer_harness.sh endurance-producer
```

Pass conditions:
- `wallet` prints a preview payment address and stake address under `tmp/preview-producer/wallet/`.
- `certs` emits stake registration, stake delegation, pool registration, and pool-id files under `tmp/preview-producer/certs/`.
- `validate` reports relay credentials absent and producer credentials complete.
- `smoke-relay` observes preview bootstrap connection, metrics, and synced blocks.
- `smoke-producer` observes `Startup.BlockProducer`, `block producer loop started`, preview bootstrap connection, metrics, and synced blocks.
- `endurance-producer` runs for the full duration and proves the preview sync point advances by at least `MIN_SLOT_ADVANCE` slots.
- No `invalid VRF proof` line appears.

This does **not** prove real block adoption until the generated pool is registered and delegated on-chain. With the default zero pledge, fund `tmp/preview-producer/wallet/payment.addr` with at least the preview stake-key deposit plus pool deposit and transaction fees (currently 502 tADA plus fees from the vendored preview genesis), submit the generated certificates, then wait for active stake before expecting leader election.

When constructing the registration transaction manually, keep certificate order deterministic: stake-address registration first, pool registration second, stake delegation third. Submitting delegation before the pool-registration certificate is processed is rejected by the preview ledger as `DelegateeStakePoolNotRegisteredDELEG`.

---

## 2. Preprod smoke test (start here)

The preprod network is a stable, pool-registration-friendly testnet. Use it to validate startup, peer discovery, sync, and (optionally) block production before pointing at mainnet.

### 2a. Sync-only smoke (no credentials needed)

```sh
RUN_SECONDS=120 \
EXPECT_FORGE_EVENTS=0 \
EXPECT_ADOPTED_EVENTS=0 \
node/scripts/run_preprod_real_pool_producer.sh
```

The script aborts immediately if `KES_SKEY_PATH` etc. are missing — but you can run a relay-only sync by invoking the binary directly:

```sh
$YGG_BIN run --network preprod \
  --database-path /tmp/ygg-preprod-db \
  --socket-path /tmp/ygg-preprod.sock \
  --metrics-port 9001
```

Verify the node reaches `bootstrap peer connected` in the log and exposes `yggdrasil_current_slot` on `http://127.0.0.1:9001/metrics`.

### 2b. Producer mode (requires real preprod pool credentials)

```sh
KES_SKEY_PATH=/secure/preprod/kes.skey \
VRF_SKEY_PATH=/secure/preprod/vrf.skey \
OPCERT_PATH=/secure/preprod/node.cert \
ISSUER_VKEY_PATH=/secure/preprod/cold.vkey \
RUN_SECONDS=600 \
EXPECT_FORGE_EVENTS=1 \
node/scripts/run_preprod_real_pool_producer.sh
```

Pass condition: script exits 0, log contains `Startup.BlockProducer`, `block producer loop started`, and at least one `elected as slot leader` / `forged local block` / `adopted forged block` line.

Failure to observe forge events on a 10-minute window is normal if the pool has low active stake; bump `RUN_SECONDS` or rerun against a session where the pool is expected to be elected.

---

## 3. Mainnet rehearsal — relay-only first

**Critical**: do NOT skip the relay-only step. Validating sync against mainnet without credentials is the safest first contact with the live network.

```sh
RELAY_ONLY=1 \
RUN_SECONDS=600 \
node/scripts/run_mainnet_real_pool_producer.sh
```

Pass conditions (asserted by the script):
- `bootstrap peer connected` observed in the log.
- `yggdrasil_active_peers >= EXPECT_HOT_PEERS` (default 2) at midpoint and at end.
- No `invalid VRF proof` lines.
- Process did not exit before `RUN_SECONDS` elapsed.

For longer settling, bump `RUN_SECONDS=3600` (1 hour) and tail the log to observe `chain_tip_slot` advancing on the metrics endpoint.

---

## 4. Mainnet rehearsal — block production (after credentials supplied)

**User decision point**: only run this once you have real mainnet pool credentials and have validated the pool registration is intact.

```sh
KES_SKEY_PATH=/secure/mainnet/kes.skey \
VRF_SKEY_PATH=/secure/mainnet/vrf.skey \
OPCERT_PATH=/secure/mainnet/node.cert \
ISSUER_VKEY_PATH=/secure/mainnet/cold.vkey \
RUN_SECONDS=3600 \
EXPECT_FORGE_EVENTS=0 \
node/scripts/run_mainnet_real_pool_producer.sh
```

`EXPECT_FORGE_EVENTS=0` is appropriate for a one-hour window — mainnet pool slots can be hours apart depending on stake. For a longer rehearsal that should observe a forge, set `EXPECT_FORGE_EVENTS=1` and `RUN_SECONDS` to at least 3× the pool's expected slot interval.

---

## 5. Hash comparison vs. Haskell node

Sample the chain tip on both nodes simultaneously to confirm they agree on `{slot, hash, block, epoch}`. Designed for the 15min / 60min / 6h checkpoints per `docs/PARITY_SUMMARY.md` Next Steps item 2.

### 5a. Single-shot comparison

Both nodes must already be running and synced to roughly the same point.

```sh
YGG_SOCK=/tmp/ygg-mainnet.sock \
HASKELL_SOCK=/run/cardano-node/socket \
NETWORK_MAGIC=764824073 \
node/scripts/compare_tip_to_haskell.sh
```

Exit codes:
- `0` — tips match. Record the timestamp and continue.
- `1` — divergence detected. Snapshot dir saved at `$SNAPSHOT_DIR/<ts>/` with both raw JSONs.
- `2` — one or both nodes unreachable.

### 5b. Watching loop (every 15 minutes)

```sh
watch -n 900 'YGG_SOCK=/tmp/ygg.sock HASKELL_SOCK=/run/cardano.sock \
  NETWORK_MAGIC=764824073 node/scripts/compare_tip_to_haskell.sh'
```

### 5c. Decision tree on divergence

The script prints this on every divergence; reproduced here for clarity:

1. **Slot differs by >1**: one node is behind. Wait 30s and rerun. Likely transient catch-up.
2. **Slot equal, hash differs**: likely a fork at the current slot. Wait 30s and rerun — one node may converge to the other's chain.
3. **Divergence persists across 3 consecutive samples**: real parity bug. Capture all snapshot dirs, the `chain_tip_slot` history from `/metrics` of both nodes, and report against the parity-audit cadence.

---

## 6. Restart resilience

Kill/restart cycles validate the WAL + dirty-flag recovery (storage Round 83) over a long window.

### 6a. 1-hour preprod restart cycle (recommended first)

```sh
NETWORK=preprod \
CYCLES=12 \
INTERVAL_BASE_S=300 \
node/scripts/restart_resilience.sh
```

Pass condition: 12 cycles complete with monotonic tip progression at each settle window plus a final post-cycle recovery probe. The script asserts `tip[N+1] >= tip[N]` after every restart.

### 6b. Mainnet restart cycle (after relay-only sync confirmed)

```sh
NETWORK=mainnet \
CYCLES=12 \
INTERVAL_BASE_S=300 \
node/scripts/restart_resilience.sh
```

Logs land at `$LOG_ROOT/cycle-NN.log` (default `/tmp/ygg-restart/`). Preserve them for forensic diff if any cycle reports non-monotonic regression (exit code 1).

---

## 6.5 Parallel BlockFetch soak (multi-peer dispatch)

Yggdrasil's runtime supports the upstream-faithful per-peer BlockFetch worker architecture (mirrors `Ouroboros.Network.BlockFetch.ClientRegistry`). When `max_concurrent_block_fetch_peers > 1`, the governor migrates each warm peer's `BlockFetchClient` into a per-peer worker task and the sync loop dispatches fetch ranges in parallel via the shared `FetchWorkerPool`. **Default ships at `1`** to keep the legacy single-peer path active until this rehearsal is complete.

Preferred automation for §6.5 is the repository harness:

```sh
NETWORK=preprod \
MAX_CONCURRENT_BLOCK_FETCH_PEERS=2 \
RUN_SECONDS=21600 \
HASKELL_SOCK=/tmp/cardano.sock \
node/scripts/parallel_blockfetch_soak.sh
```

The harness starts `yggdrasil-node`, captures Prometheus snapshots,
asserts worker registration/migration, optionally runs
`compare_tip_to_haskell.sh` at `COMPARE_INTERVAL_S` cadence, scans logs
for worker-channel failures, and writes a concise summary under
`$LOG_DIR/summary.txt`. Use `RUN_SECONDS=86400` for the 24-hour soak
steps and set `TOPOLOGY=/path/to/topology.json` when rehearsing with
a custom multi-relay topology.

### 6.5a Two-peer parity check (preprod)

Two prerequisites combine to activate the multi-peer BlockFetch dispatch:

1. **Set the knob** to ≥ 2.  Either edit the config, or pass the CLI
   override (preferred for one-off rehearsals — no config-file edit
   needed):

   ```sh
   $YGG_BIN run --network preprod \
     --max-concurrent-block-fetch-peers 2 \
     --database-path /tmp/ygg-preprod-db \
     --socket-path /tmp/ygg-preprod.sock \
     --metrics-port 9201
   ```

2. **Make sure the governor has at least 2 peers to promote**, otherwise
   the legacy single-peer ChainSync path stays active and the worker
   pool stays empty.  The vendored preprod `topology.json` ships with a
   single `bootstrapPeer`, and ledger-derived peers only kick in once
   the chain crosses `useLedgerAfterSlot=112406400`.  Until then, edit
   `node/configuration/preprod/topology.json` to add at least one
   `localRoot`:

   ```json
   {
     "bootstrapPeers": [{ "address": "preprod-node.play.dev.cardano.org", "port": 3001 }],
     "localRoots": [
       {
         "accessPoints": [
           { "address": "preprod-node.play.dev.cardano.org", "port": 3001 },
           { "address": "<second-preprod-relay>", "port": 3001 }
         ],
         "advertise": false,
         "trustable": false,
         "valency": 2
       }
     ]
   }
   ```

   (Sources for additional preprod relays: the [Cardano Operations
   Book — env-preprod](https://book.world.dev.cardano.org/env-preprod.html)
   page, or any preprod stake-pool's published relay.)

**Activation criteria** — both must hold:

- The Prometheus gauge `yggdrasil_blockfetch_workers_registered` must
  rise from `0` to the warm-peer count (one worker per peer).  Capture
  with: `curl -sS http://127.0.0.1:9201/metrics | grep
  ^yggdrasil_blockfetch_workers_registered`.
- `yggdrasil_blockfetch_workers_migrated_total` must be ≥ 1 (at least
  one promote-time migration completed).

Watch the tracer for the activation event:

```
[Net.BlockFetch.Worker] Info — BlockFetch migrated to per-peer worker
  peer=<addr> maxConcurrent=2
```

This event must fire once per warm peer at promote time. **If
`yggdrasil_blockfetch_workers_registered` stays at 0 even after a
multi-minute sync and the activation event never appears, the
migration path is not active** — the rehearsal is testing the legacy
single-peer path and not the multi-peer one.  Investigate
`Net.Governor` warning lines first; common causes are (a) only one
peer in the registry, (b) `useLedgerAfterSlot` not yet crossed and no
`localRoots` configured.

> **Known issues (2026-04-27 operator rehearsal)**:
>
> - **Round 90 Gap BM (CLOSED)** — the previous `RollbackPointNotFound`
>   crash at session-handoff is fixed.  Look for
>   `Net.PeerSelection: realigning from_point to volatile storage tip
>   before reconnect` in the trace as confirmation that the runtime
>   resync logic is engaging cleanly.
> - **Round 91 Gap BN (CLOSED — Round 144)** — the from-genesis
>   livelock under `max_concurrent_block_fetch_peers > 1` is fixed.
>   Root cause: `partition_fetch_range_across_peers` was producing
>   multi-chunk plans whose intermediate boundaries carried the
>   all-zeros `HeaderHash` placeholder synthesised by `split_range`
>   for boundaries the runtime cannot anchor (it has no candidate
>   fragment to resolve them).  Each batch dispatched
>   `MsgRequestRange { lower: BlockPoint(N, real), upper: BlockPoint(M, [0;32]) }`
>   on the wire — peers respond with `NoBlocks` for unknown-hash
>   bounds, every batch returned zero blocks, volatile storage stayed
>   empty, and the node livelocked re-syncing from Origin on every
>   handoff.  In-session debug capture `[ygg-sync-debug]
>   blockfetch-request-cbor=83008218535820152bf9...821904635820 0000…`
>   confirmed the placeholder upper-hash being sent.  Fix in two
>   layers: (1) a placeholder-hash guard in
>   `partition_fetch_range_across_peers` collapses any multi-chunk
>   plan containing a synthesised boundary to a single-chunk plan
>   targeting `peers[0]` with the original `(lower, upper)` preserved
>   exactly; (2) the multi-peer dispatch branch in
>   `sync_batch_verified_with_tentative` performs the same
>   `lower_hash` dedup as the legacy single-peer branch (the
>   BlockFetch wire returns the closed interval `[lower, upper]`, and
>   the consensus `track_chain_state_entries` block-number contiguity
>   check rejected the duplicate front entry as `expected N, got
>   N-1`).  Verification: with the knob set to 2 and a 2-localRoot
>   topology, `find $YGG_DB -type f | wc -l` climbs steadily past
>   zero, `yggdrasil_blocks_synced` advances monotonically, and 0
>   reconnects / 0 consensus errors are observed.  Throughput delta
>   knob=2 vs knob=1 is currently ~0.54× because the placeholder
>   collapse forces single-chunk dispatch even when N peers are
>   migrated; the path is correctness-only at this stage and
>   throughput parity tracks the multi-peer ChainSync candidate
>   fragments work needed to remove the collapse.

### 6.5b Hash-compare under parallel fetch

Run the existing tip-comparison harness from §5 against a Haskell node that's also fully synced on preprod:

```sh
YGG_SOCK=/tmp/ygg.sock \
HASKELL_SOCK=/tmp/cardano.sock \
NETWORK_MAGIC=1 \
node/scripts/compare_tip_to_haskell.sh

# Watch loop, every 15 minutes:
watch -n 900 'YGG_SOCK=/tmp/ygg.sock HASKELL_SOCK=/tmp/cardano.sock NETWORK_MAGIC=1 node/scripts/compare_tip_to_haskell.sh'
```

**Pass criterion:** the Yggdrasil tip `{slot, hash, block, epoch}` must match the Haskell tip at every check for at least 6 hours after the multi-peer mode is engaged. Any divergence under parallel fetch indicates a bug in the dispatch / reorder / tentative-header path that does not surface in the single-peer path.

### 6.5c Sustained-rate measurement

Compare slot-rate metrics between knob=1 and knob=2 over the same wall-clock window:

```sh
# Knob=1 baseline (record before opting in)
curl -fsS http://127.0.0.1:9001/metrics \
  | grep '^yggdrasil_blocks_synced\|^yggdrasil_current_slot'
sleep 600
curl -fsS http://127.0.0.1:9001/metrics \
  | grep '^yggdrasil_blocks_synced\|^yggdrasil_current_slot'

# Restart with knob=2, repeat the same 10-minute window
```

Expected: knob=2 throughput ≥ knob=1 throughput. Upstream typically observes a 1.5–2× speedup on bulk-sync periods.

**Operator-quantified empirical numbers (Round 217 + 218, 2026-04-30 mainnet)**:

The R217 fetch-batch histogram + R218 multi-peer rehearsal give concrete per-batch numbers from the IOG backbone peer:

| Configuration                         | fetch avg/batch | apply avg/batch | throughput |
| ------------------------------------- | --------------: | --------------: | ---------: |
| `--max-concurrent-block-fetch-peers 1`|         12.85 s |          0.22 s | 3.33 blk/s |
| `--max-concurrent-block-fetch-peers 4`<br>(2 active workers) |          8.56 s |          0.23 s | 5.55 blk/s |

The fetch path dominates (~59× more expensive than apply) so multi-peer dispatch is the real lever — each additional warm peer that migrates to a worker subtracts ≈ `(fetch_avg / N)` from the per-batch fetch time.  Apply rate is unchanged across knob values.

Use the R217 histograms to verify your topology is healthy:

```sh
curl -fsS http://127.0.0.1:9001/metrics \
  | grep -E "yggdrasil_(fetch|apply)_batch_duration_seconds_(sum|count)"
```

A healthy multi-peer mainnet sync should show `fetch_avg/batch ≈ baseline / N` where N is the active worker count (`yggdrasil_blockfetch_workers_registered`).

### 6.5d Knob=4 stress test

After 6.5a–6.5c pass, repeat with `max_concurrent_block_fetch_peers=4` for at least 24 hours of preprod soak. Watch:

- `yggdrasil_active_peers` should reach 4 once the governor has promoted enough peers.
- 4 distinct `Net.BlockFetch.Worker` migration events (one per warm peer).
- No tracer lines containing `fetch worker channel closed` or `fetch worker dropped response` — these indicate worker task crashes.
- `yggdrasil_reconnects` rate within the same band as the knob=1 baseline.

### 6.5e Mainnet rehearsal at knob=2

Only after preprod 6.5a–6.5d are clean: repeat 6.5a + 6.5b on mainnet relay-only mode for 24 hours.

### 6.5f Sign-off

Record in §9:

- Preprod knob=2 6h hash compare: PASS / FAIL
- Preprod knob=4 24h soak: PASS / FAIL
- Mainnet knob=2 24h hash compare: PASS / FAIL
- Throughput delta knob=2 vs knob=1 (target: ≥ 1.0×, expected: 1.5–2×)

If all sign-offs pass, the team can flip the default in `node/src/config.rs::default_max_concurrent_block_fetch_peers` from `1` to `2` (matching upstream `bfcMaxConcurrencyBulkSync = 2`). Update preset constructors in lockstep — there's a drift-guard test (`preset_configs_share_canonical_max_concurrent_block_fetch_peers`) that pins all three presets to the same value, so changing the default in one place fails CI until all are updated.

---

## 7. Metrics snapshot collection

At each checkpoint of the long-running rehearsal (T+15min, T+60min, T+6h), capture a Prometheus snapshot for later trend analysis:

```sh
mkdir -p /tmp/ygg-metrics-snapshots
curl -fsS http://127.0.0.1:9001/metrics > "/tmp/ygg-metrics-snapshots/snapshot-$(date -u +%Y%m%dT%H%M%SZ).txt"
```

Key metrics to track:
- `yggdrasil_current_slot` — must advance monotonically.
- `yggdrasil_active_peers` — typically `>= 2` once settled.
- `yggdrasil_blocks_synced` — strictly non-decreasing.
- `yggdrasil_mempool_tx_count` — varies; useful for relay-mode validation.
- `yggdrasil_reconnects` — should stay low (<10/hour) on a stable peer set.

For Phase 6 parallel-fetch validation (§6.5):
- `yggdrasil_blockfetch_workers_registered` — current pool size. `0` in legacy single-peer mode (knob = 1); equal to the number of warm peers when knob > 1 and the governor has migrated their `BlockFetchClient`s. Watching this gauge climb to the configured knob value is the operator's primary signal that multi-peer dispatch has activated.
- `yggdrasil_blockfetch_workers_migrated_total` — lifetime count of promote-time migrations. Should monotonically increase as warm peers are promoted; flat-lining while warm peers are being promoted indicates the migration call path is broken (check `Net.BlockFetch.Worker` tracer events).
- `yggdrasil_fetch_batch_duration_seconds` — R217 fetch+verify histogram (per-batch).  Compare against `yggdrasil_apply_batch_duration_seconds` (R200) to size sync-rate bottlenecks.  On mainnet from the IOG backbone peer the R217 baseline is ~12.85 s/batch single-peer; multi-peer with 2 active workers brings it to ~8.56 s/batch (R218).  Operator action: if `fetch_avg/batch` is much higher than expected for your `blockfetch_workers_registered` count, the topology peers may be routing-distant / unhealthy — increase peer diversity.
- `yggdrasil_apply_batch_duration_seconds` — R200 ledger-apply histogram (per-batch).  Mainnet baseline ~0.22 s/batch (≈4 ms/block); essentially independent of multi-peer knob.  Stable apply time confirms the multi-peer dispatch path doesn't distort apply behaviour.

For Phase D.2 multi-session peer accounting (R222–R226) — five lifetime peer-stats counters monotonic across reconnects, distinct from the live `known/active/established_peers` gauges:
- `yggdrasil_peer_lifetime_sessions_total` — cumulative warm-peer establishments. `rate(...)` over a 5-minute window gives real peer-churn rate independent of current state.
- `yggdrasil_peer_lifetime_failures_total` — cumulative session failures. Pair with `sessions_total` to compute peer reliability ratio (`failures/sessions`).
- `yggdrasil_peer_lifetime_bytes_in_total` — cumulative bytes received from peers (sourced from BlockFetch `bytes_delivered`).  Lower bound for total ingress — does not include ChainSync header bytes or TxSubmission2 traffic.  Pair with `sessions_total` for average bytes/session throughput.
- `yggdrasil_peer_lifetime_unique_peers` — distinct peer addresses ever observed during this process lifetime.  When `unique_peers > sessions_total`, some peer entries exist in the registry but never promoted to warm — useful registry-leakage signal.
- `yggdrasil_peer_lifetime_handshakes_total` — cumulative successful NtN handshakes.  When `handshakes > sessions`, sessions are completing handshake but disconnecting before mini-protocol traffic.

Operator-derived signals via PromQL:
```
# Peer reliability ratio (failures per session)
yggdrasil_peer_lifetime_failures_total / yggdrasil_peer_lifetime_sessions_total

# Average bytes received per session
yggdrasil_peer_lifetime_bytes_in_total / yggdrasil_peer_lifetime_sessions_total

# Registry-leakage indicator (peers tracked but never promoted)
1 - (yggdrasil_peer_lifetime_sessions_total / yggdrasil_peer_lifetime_unique_peers)

# Real peer churn rate (cumulative reconnects, distinct from current peer-count gauges)
rate(yggdrasil_peer_lifetime_sessions_total[5m])
```

For Phase D.1 rollback observability and R238 sidecar recovery:
- `yggdrasil_rollback_depth_blocks` — histogram (7 buckets `[1, 2, 5, 50, 2160, 10_000, +Inf]`) of rollback depths in rolled-back transactions. Recorded at every batch with `rollback_count > 0`; depth=0 captures session-start `RollBackward(Origin)` confirms. Operators alert on rare deep cross-epoch rollbacks via `histogram_quantile(0.99, rate(yggdrasil_rollback_depth_blocks_bucket[1h]))`. This metric is now the operator signal for validating rollback distribution and restart-resilience behavior after R238's exact nonce/OpCert sidecar restore-and-replay implementation.

Quick health JSON:

```sh
curl -fsS http://127.0.0.1:9001/health | head -20
```

---

## 8. Local query / submit smoke

Exercises the NtC Unix-socket server and confirms LocalStateQuery / LocalTxSubmission round-trip.

### 8a. Tip query

```sh
$YGG_BIN cardano-cli query-tip \
  --socket-path /tmp/ygg-preprod.sock \
  --network-magic 1
```

### 8b. UTxO-by-address query

```sh
$YGG_BIN query \
  --socket-path /tmp/ygg-preprod.sock \
  --network-magic 1 \
  utxo-by-address addr_test1...
```

### 8c. Submit a pre-built transaction

```sh
$YGG_BIN submit-tx \
  --socket-path /tmp/ygg-preprod.sock \
  --network-magic 1 \
  --tx-file /tmp/signed-tx.cbor
```

(Or `--tx-hex 0x...` if you have the hex-encoded body.)

### 8d. Upstream `cardano-cli` parity sweep (R164 / R179 / R180–R182)

Yggdrasil's NtC socket speaks the canonical
`Ouroboros.Network.NodeToClient` wire protocol so the upstream
`cardano-cli` binary can drive it directly.  Required: a recent
`cardano-cli` (≥ 10.16) on `$PATH` and yggdrasil's socket from
§3 (`/tmp/ygg-preprod.sock`).  Set
`CARDANO_NODE_SOCKET_PATH=/tmp/ygg-preprod.sock` once and run:

```sh
# Always-available Shelley-and-later queries (R157–R164):
cardano-cli query tip --testnet-magic 1
cardano-cli query protocol-parameters --testnet-magic 1
cardano-cli query era-history --testnet-magic 1
cardano-cli query slot-number --testnet-magic 1 2030-01-01T00:00:00Z
cardano-cli query utxo --whole-utxo --testnet-magic 1
cardano-cli query utxo --address addr_test1... --testnet-magic 1
cardano-cli query utxo --tx-in <txid>#<ix> --testnet-magic 1
cardano-cli query tx-mempool info --testnet-magic 1
cardano-cli query tx-mempool next-tx --testnet-magic 1
cardano-cli query tx-mempool tx-exists <hex64> --testnet-magic 1
```

All ten must return well-formed JSON.

### 8e. Era-gated queries via `YGG_LSQ_ERA_FLOOR` (R178 / R179 / R180–R182)

cardano-cli 10.16 client-side gates several queries at Babbage+
era; on a fresh-sync chain stuck at PV=(6,0) Alonzo, set the
opt-in env var to bypass the gate **on the yggdrasil node**
before starting it (not on cardano-cli):

```sh
YGG_LSQ_ERA_FLOOR=6 \
  $YGG_BIN run --network preprod \
    --database-path /tmp/ygg-preprod-db \
    --socket-path /tmp/ygg-preprod.sock \
    --metrics-port 12345
```

Then exercise the previously-gated queries via the
`cardano-cli conway query` subcommand path:

```sh
cardano-cli conway query stake-pools --testnet-magic 1
cardano-cli conway query stake-distribution --testnet-magic 1
cardano-cli conway query pool-state --all-stake-pools --testnet-magic 1
cardano-cli conway query stake-snapshot --all-stake-pools --testnet-magic 1
cardano-cli conway query constitution --testnet-magic 1
cardano-cli conway query gov-state --testnet-magic 1
cardano-cli conway query drep-state --all-dreps --testnet-magic 1
cardano-cli conway query treasury --testnet-magic 1
cardano-cli conway query committee-state --testnet-magic 1
```

Expected outputs on a fresh-sync chain (no pools / DReps yet
registered): `[]`, `{}`, `{}`, `{ "pools": {}, "total": ... }`,
real Conway constitution data, a Conway governance-state object with
empty proposals unless governance traffic has been observed, `[]`, `0`, and
`{ "committee": {}, "epoch": 0, "threshold": null }`
respectively.

> **Note**: the env var floors what the LSQ dispatcher reports
> as the active era — it does **not** affect block production
> or ledger validation.  Set it only when you intentionally need
> to exercise the era-gated query surface against a partial sync.
> Default behaviour (env var unset) is unchanged.

`cardano-cli conway query gov-state` is no longer a known gap:
R188/R193/R204 aligned tag 24's 7-field `ConwayGovState` shape,
the `GovRelation` encoding, and the OMap proposal adapter used by
cardano-cli 10.16. If this query fails during a rehearsal, capture
the raw LSQ request/response CBOR plus the active-era floor used for
that run; treat it as a regression, not as expected incompleteness.

---

## 8.5 Upstream `cardano-node-tests` E2E harness

Use the official IntersectMBO [`cardano-node-tests`](https://github.com/IntersectMBO/cardano-node-tests) suite as an external parity harness, not as a default `cargo test` dependency. The upstream documentation at <https://tests.cardano.intersectmbo.org/> describes the suite as system/E2E coverage for `cardano-node`, with `runner/runc.sh` for containerized runs and a `.bin` custom-binary path for alternate `cardano-node` / `cardano-cli` executables.

The upstream process index at <https://tests.cardano.intersectmbo.org/process.html>
is the selector for Yggdrasil external parity runs. Apply it as a coverage
taxonomy, not as permission to promote the full upstream suite into required
CI before wrapper behavior is deterministic:

| Upstream process area | Yggdrasil parity use |
|---|---|
| Node CLI E2E and local-cluster tests | First external gate. Run unchanged against upstream Haskell `cardano-node` first, then run the same pytest expression through Yggdrasil wrappers. |
| Node sync tests | Use for operator evidence after local wrapper slices pass. Keep sync speed, RAM, CPU, and disk observations with the §2-§9 endurance evidence. |
| Tag-testing regression combinations | Select the lowest-risk combinations that match implemented surfaces: local cluster, P2P or legacy topology as configured, current transaction era, startup, local query, submit-tx, relay sync, and producer preflight. |
| Upgrade, rollback, mixed-topology, and block-production checks | Treat as manual sign-off gates until the corresponding Yggdrasil runtime path has already passed native runbook checks. Do not make these required CI solely because they exist upstream. |
| Submit-API, DB Sync, explorer, Plutus, governance, and UAT coverage | Classify unsupported surfaces as explicit parity gaps. Use upstream binaries or skip only when documenting that the missing component is outside the selected Yggdrasil slice. |
| Negative and error-path tests | Preserve upstream expectations. A missing command or incompatible error shape is either a Yggdrasil parity gap or wrapper debt; do not rewrite upstream tests to hide it. |

When selecting tests, prefer upstream markers or `-k` expressions that map to a
single Yggdrasil surface and record the exact expression in the pass/fail
summary. If the upstream tag-testing checklist adds a new class for the target
node tag, either add a matching row to this runbook or record why that class is
not yet a Yggdrasil gate.

### GitHub Actions path

For an Actions-hosted run, use either a fork of the upstream test repository or Yggdrasil's manual-only `.github/workflows/upstream-cardano-node-tests.yml` workflow. Do not wire this directly into Yggdrasil's required CI until the wrapper layer and selected pytest expression are deterministic.

1. Fork `IntersectMBO/cardano-node-tests`.
2. Enable Actions in the fork: `Settings` -> `Actions` -> `General` -> `Actions permissions` -> `Allow all actions and reusable workflows`.
3. Build and publish a Yggdrasil release artifact, or make a branch in the fork that downloads the exact Yggdrasil binary from the commit under test and installs wrappers into `.bin/`.
4. In the fork's `Actions` tab, manually dispatch one of the upstream workflows:
   - `01 Regression tests`
   - `02 Regression tests with db-sync`
   - `03 Upgrade tests`
5. For the Yggdrasil workflow, dispatch `Upstream cardano-node-tests`, select the upstream test ref (`master` by default), runner script, pytest expression, cluster count, and `cardano-cli` mode. If `cardano-cli` mode is `upstream`, provide `cardano_cli_bin` through a workflow customization that installs the upstream binary first.
6. Record the upstream workflow URL, Yggdrasil commit, selected test expression, and whether the run used upstream `cardano-cli` or the Yggdrasil `cardano-cli` shim.

Treat this as external evidence. A failing upstream workflow caused by an unsupported Yggdrasil CLI command is a parity gap to classify, not a reason to edit upstream tests. A failing workflow caused by the wrapper layer is harness debt and must be fixed before the selected slice can become a CI gate.

Recommended flow:

1. Run the selected pytest slice unchanged against upstream Haskell `cardano-node` and record the baseline.
2. Add Yggdrasil wrapper binaries under the test repo's `.bin/` directory:
   - `cardano-node` wrapper -> `target/release/yggdrasil-node run ...`
   - `cardano-cli` wrapper -> `target/release/yggdrasil-node cardano-cli ...` or the upstream `cardano-cli` when the test requires a command Yggdrasil has not implemented yet.
3. Start with role/protocol slices that match implemented surfaces: startup, topology, local query, submit-tx, relay sync, producer credential preflight, KES/OpCert startup failure cases.
4. Mark failures caused by unsupported `cardano-cli` commands as explicit parity gaps. Do not rewrite upstream tests to hide missing behavior.
5. Promote stable Yggdrasil-compatible selections into a separate optional CI job once the wrapper layer is deterministic.

Example targeted invocation from a sibling checkout:

Prerequisites:

- A working Docker or Podman runtime visible from the current shell.
  GitHub-hosted runners satisfy this directly.  In a devcontainer that
  uses Docker-outside-of-Docker, the upstream checkout and `.bin/`
  wrapper files must live on a path the host Docker daemon can bind
  mount; files created only inside the devcontainer overlay can appear
  missing inside `runner/runc.sh`.
- A container-compatible Yggdrasil binary.  Upstream `runner/runc.sh`
  validates `.bin/` before starting and rejects dynamically linked
  non-`/nix` binaries.  Use a MUSL/static build for local container
  runs, or a Nix-store binary when running from Nix.

```sh
cd ../cardano-node-tests
mkdir -p .bin
rm -rf .yggdrasil-configuration
cp -R /workspaces/Cardano-node/node/configuration .yggdrasil-configuration
cp /workspaces/Cardano-node/target/x86_64-unknown-linux-musl/release/yggdrasil-node .bin/yggdrasil-node
cat > .bin/cardano-node <<'SH'
#!/usr/bin/env sh
set -eu
bin_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$bin_dir/yggdrasil-node" "$@"
SH
cat > .bin/cardano-cli <<'SH'
#!/usr/bin/env sh
set -eu
bin_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
config_root=${YGGDRASIL_CONFIG_ROOT:-"$bin_dir/../.yggdrasil-configuration"}
case "${1:-}" in
  --version|-V)
    shift
    exec "$bin_dir/yggdrasil-node" cardano-cli --upstream-config-root "$config_root" version "$@"
    ;;
  *)
    exec "$bin_dir/yggdrasil-node" cardano-cli --upstream-config-root "$config_root" "$@"
    ;;
esac
SH
chmod +x .bin/yggdrasil-node .bin/cardano-node .bin/cardano-cli

./runner/runc.sh -- \
  TEST_THREADS=0 \
  CLUSTERS_COUNT=1 \
  PYTEST_ARGS="-k 'test_cli or test_local_state_query'" \
  ./runner/regression.sh
```

The wrapper command above is intentionally minimal. Tests that require exact upstream CLI argument compatibility should get a purpose-built wrapper rather than changing Yggdrasil's production CLI only for test harness convenience.

---

## 9. Pass / fail summary template

At the end of a successful rehearsal session, record (e.g. into a session log):

```
[date]   yyyy-mm-dd HH:MM Z
[network]  preprod | mainnet
[mode]   relay-only | producer
[duration]  Nh
[checkpoints]
  T+15m   compare_tip_to_haskell -> 0 (match) | 1 (divergence: <hash diff>)
  T+60m   compare_tip_to_haskell -> 0
  T+6h    compare_tip_to_haskell -> 0
[restart-resilience]
  CYCLES=12 result=PASS final_tip=<slot>
[parallel-blockfetch]   (only fill when knob > 1 was exercised)
  preprod  knob=2  6h hash-compare       result=PASS|FAIL
  preprod  knob=4  24h soak                result=PASS|FAIL
  mainnet  knob=2  24h hash-compare       result=PASS|FAIL
  throughput-delta knob=2/knob=1 = <N.NN>x  (target >= 1.0x)
[cardano-node-tests]    selected pytest expression=<expr> result=PASS|FAIL|N/A
  workflow=<url-or-local> yggdrasil_commit=<sha> cli=<upstream|yggdrasil-shim>
[metrics-snapshots]
  /tmp/ygg-metrics-snapshots/*.txt  N captured
[evidence-summary]
  leaders=<N>  forged=<N>  adopted=<N>  notAdopted=<N>
```

---

## 10. Where to look on failure

| Symptom | First place to look |
|---|---|
| Process exits at startup | log file (last 60 lines via `tail -n 60`) — usually a config / genesis path resolution error |
| Stuck at `bootstrap peer connected` with no further sync | `/metrics` `yggdrasil_active_peers` (if 0, peer governor never promoted; check topology) |
| `invalid VRF proof` | header verification mismatch; capture log + a `cardano-cli query tip` snapshot from a known-good Haskell node and compare slot/hash |
| Non-monotonic tip during restart | `restart_resilience.sh` exit 1; logs preserved in `$LOG_ROOT`. Likely a storage WAL recovery bug — file a parity-audit issue and attach all `cycle-NN.log` files |
| Hash divergence persists | run `compare_tip_to_haskell.sh` 3× over 90s; if all three diverge with `slot equal, hash different`, capture both snapshot JSONs and report against `docs/PARITY_PLAN.md` |

---

## References

- `docs/AUDIT_VERIFICATION_2026Q2.md` — gap status that this runbook closes
- `docs/PARITY_SUMMARY.md` lines 303–307 — "Next Steps" defining the rehearsal cadence
- `node/scripts/run_preprod_real_pool_producer.sh` — preprod rehearsal template
- `node/scripts/run_mainnet_real_pool_producer.sh` — mainnet rehearsal (Slice L)
- `node/scripts/compare_tip_to_haskell.sh` — hash-comparison harness (Slice M)
- `node/scripts/restart_resilience.sh` — restart-resilience automation (Slice N)
- `node/scripts/parallel_blockfetch_soak.sh` — §6.5 multi-peer BlockFetch soak automation
- `IntersectMBO/cardano-node-tests` — upstream system/E2E parity harness and process taxonomy: <https://github.com/IntersectMBO/cardano-node-tests>, <https://tests.cardano.intersectmbo.org/>, <https://tests.cardano.intersectmbo.org/process.html>
