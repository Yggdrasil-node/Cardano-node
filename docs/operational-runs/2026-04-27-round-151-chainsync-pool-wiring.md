## Round 151 — ChainSync worker pool runtime wiring + observability

Date: 2026-04-27
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close the first follow-up flagged at the end of Round 150 — wire the
`SharedChainSyncWorkerPool` end-to-end through the production runtime so
candidate fragments populate from real preprod wire traffic, and add
operator observability via a Prometheus gauge.

### Changes

1. `VerifiedSyncServiceConfig.shared_chainsync_worker_pool:
   Option<SharedChainSyncWorkerPool>` (sync.rs).  Cloned into the
   multi-peer `MultiPeerDispatchContext` via
   `chainsync_pool: Option<&'a SharedChainSyncWorkerPool>`.
2. `RuntimeGovernorConfig.shared_chainsync_worker_pool` field +
   `with_shared_chainsync_worker_pool(...)` builder (runtime.rs).
3. `node/src/main.rs` runtime startup constructs one shared pool with
   `yggdrasil_node::new_shared_chainsync_worker_pool()` and wires it
   into both the sync-service config (reader path) and the governor
   config (metrics path).
4. `sync_batch_verified_with_tentative` calls
   `publish_announced_header(pool, peer, slot, hash)` on every
   observed RollForward header, so fragments populate from real wire
   traffic.
5. The multi-peer dispatch branch tries
   `partition_fetch_range_with_candidate_fragments` first and falls
   back to placeholder collapse when fragments don't have the
   required hashes.
6. New gauge `yggdrasil_chainsync_workers_registered` in tracer.rs;
   exported via `NodeMetrics::set_chainsync_workers_registered` and
   surfaced through `MetricsSnapshot.chainsync_workers_registered`
   and `to_prometheus_text`.
7. Governor tick reads `cs_pool.read().await.len()` alongside the
   existing BlockFetch pool size and updates the gauge.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean (clippy --workspace --all-targets --all-features -- -D warnings)
cargo test-all                   # passed: 4682  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4679 (Round 150) → 4682.

### Operational verification

Preprod run with `--max-concurrent-block-fetch-peers 2` and a 2-localRoot
topology, ~60s soak window.

`/tmp/ygg-verify-metrics-final.txt`:

```
yggdrasil_blocks_synced 556
yggdrasil_current_slot 96640
yggdrasil_current_block_number 558
yggdrasil_reconnects 0
yggdrasil_known_peers 32
yggdrasil_active_peers 5
yggdrasil_blockfetch_workers_registered 10
yggdrasil_blockfetch_workers_migrated_total 10
yggdrasil_chainsync_workers_registered 1
```

`/tmp/ygg-verify-cli-tip-final.txt` (`cardano-cli query tip --testnet-magic 1`):

```json
{
    "epoch": 0,
    "era": "Shelley",
    "slotInEpoch": 0,
    "slotsToEpochEnd": 21600,
    "syncProgress": "0.00"
}
```

### Interpretation

- `chainsync_workers_registered=1` confirms the auto-registration
  mechanism from `publish_announced_header` is firing on every
  RollForward observation in the verified-sync reader path.
- `blockfetch_workers_registered=10` reflects the knob=2 multi-peer
  path with 10 warm peers migrated (consistent with Round 144
  baseline).
- cardano-cli still returns structured JSON, confirming the new
  runtime wiring doesn't regress NtC parity from Round 148-150.
- Reported tip in cardano-cli still shows origin (epoch=0/slot=0)
  because `GetChainBlockNo` returns Origin (not yet plumbed from
  consensus state) and the `LedgerStateSnapshot` carries an
  origin-rooted Point until the chain-tracker integration ships —
  documented as a separate Phase-3 follow-up.

### Open follow-ups

1. **Per-peer ChainSync RollForward propagation** — currently only
   the reader-side peer's worker auto-registers (cap visible as
   `chainsync_workers_registered=1` even under knob=2).  Plumbing
   each upstream `ntc_chain_sync_client` task's RollForward stream
   into the pool would surface as `chainsync_workers_registered ≥ 2`
   and unlock the per-peer hash-coverage benefit of multi-peer
   partitioning.
2. **Real preprod era-history in Interpreter** — Phase-3 refinement
   of Finding E so cardano-cli computes accurate slot-to-epoch
   conversions across hard-fork boundaries.
3. **Live chain-block-no in `LedgerStateSnapshot`** — so
   `GetChainBlockNo` returns the live chain block number and
   cardano-cli's `query tip` reflects current chain progress instead
   of the snapshot-rooted origin display.

### References

- `Ouroboros.Network.ChainSync.Client`
- `Ouroboros.Network.BlockFetch.Decision.fetchDecisions`
- Previous round: `docs/operational-runs/2026-04-27-runbook-pass.md`
- Code: `node/src/chainsync_worker.rs`, `node/src/sync.rs`,
  `node/src/runtime.rs`, `node/src/main.rs`, `node/src/tracer.rs`
