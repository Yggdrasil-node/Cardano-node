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

- Rust toolchain pinned to `1.85.0` (per `rust-toolchain.toml`); confirm with `cargo --version`.
- Build the binary in release mode for any rehearsal that runs longer than ~5 minutes:
  ```sh
  cargo build --release -p yggdrasil-node
  export YGG_BIN="$PWD/target/release/yggdrasil-node"
  ```
  (debug build works for short smoke tests but is much slower at block validation.)
- Optional but recommended for hash-comparison: an upstream `cardano-cli` binary on `$PATH` and (separately) a running `cardano-node` syncing the same network with a known `--socket-path`.
- Optional for producer mode: a real pool's `kes.skey`, `vrf.skey`, `node.cert`, and `cold.vkey` (issuer vkey).
- Vendored network configs are already present at:
  - `node/configuration/mainnet/`
  - `node/configuration/preprod/`
  - `node/configuration/preview/`

  Each directory contains the JSON config plus the four genesis files (Byron, Shelley, Alonzo, Conway). No download step is required.

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

### 6.5a Two-peer parity check (preprod)

Edit the preprod config to set:

```json
{
  "max_concurrent_block_fetch_peers": 2,
  ...
}
```

Or pass via env-overridden CLI:

```sh
NODE_CONFIG_OVERRIDE_max_concurrent_block_fetch_peers=2 \
  scripts/run_preprod_real_pool_producer.sh
```

Watch the tracer for the activation event:

```
[Net.BlockFetch.Worker] Info — BlockFetch migrated to per-peer worker
  peer=<addr> maxConcurrent=2
```

This event must fire once per warm peer at promote time. If the event never appears, the migration path is not active — investigate `Net.Governor` warning lines first.

### 6.5b Hash-compare under parallel fetch

Run the existing tip-comparison harness from §5 against a Haskell node that's also fully synced on preprod:

```sh
scripts/compare_tip_to_haskell.sh \
  --rust-socket /tmp/ygg.sock \
  --haskell-socket /tmp/cardano.sock \
  --watch 15
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
