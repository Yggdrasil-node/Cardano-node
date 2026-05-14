## Rounds 199 + 200 — Multi-peer livelock verified resolved + apply-batch duration histogram

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Combined operational round covering two related items from the
Phase B/C plan:

1. **Phase B / R199** — Reproduce R91 multi-peer dispatch
   storage livelock and confirm fix status.
2. **Phase C.1 / R200** — Add `yggdrasil_apply_batch_duration_seconds`
   Prometheus histogram so operators can quantify per-batch apply
   latency (baseline for Phase C.2 pipelined fetch+apply).

### R199 — Phase B verification

**Setup**: started yggdrasil with `--max-concurrent-block-fetch-peers 4`,
ran 2 minutes, killed, restarted same DB, observed sync state.

**Result**: **R91 livelock no longer reproduces.**  Multi-peer
dispatch correctly persists to all three storage tiers and
recovery from checkpoint resumes sync without re-fetching from
origin.

After 2 minutes at `--max-concurrent-block-fetch-peers 4`:
```
$ ls /tmp/ygg-r91-preview-db
immutable  ledger  nonce_state.cbor  ocert_counters.cbor  volatile

$ du -sb /tmp/ygg-r91-preview-db/{volatile,immutable,ledger}
962934   /tmp/ygg-r91-preview-db/volatile
1493969  /tmp/ygg-r91-preview-db/immutable
22609    /tmp/ygg-r91-preview-db/ledger

$ ls /tmp/ygg-r91-preview-db/immutable | wc -l
667
```

22 K blocks synced in 2 min, with 667 immutable files written
and ledger checkpoints persisted at slots 960, 3960, 6960, 9960
(every 3 K slots per `checkpointIntervalSlots=2160` policy).

After restart on the same DB:
```
recovered ledger state from coordinated storage
  checkpointSlot=21960
  point=BlockPoint(SlotNo(23960), HeaderHash(086074d2c4dbf459…))
  replayedVolatileBlocks=100

# tip after restart progresses past checkpoint:
{"block": 25940, "slot": 25940, ...}
```

The R91 symptom documented in the plan ("verified-sync advances
in-memory `ChainState` but no files persist to volatile/,
immutable/, ledger/") **is not currently observable**.  This was
likely closed by an intervening round (the multi-peer dispatch
codepath has had several stability fixes since R91 was
documented) but never explicitly verified — R199 closes that
gap.

### R200 — Apply-batch duration histogram

`node/src/tracer.rs`:

- New `apply_batch_duration_buckets: [AtomicU64; 10]`,
  `apply_batch_duration_sum_micros: AtomicU64`, and
  `apply_batch_duration_count: AtomicU64` atomic fields on
  `NodeMetrics`.
- New `pub const APPLY_BATCH_BUCKETS_SECONDS: [f64; 10]` —
  bucket boundaries `[0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0,
  5.0, 10.0, +Inf]` (covers ~1 ms to ~10 s).
- New `record_apply_batch_duration(duration: Duration)`
  cumulative-bucket increment + sum-micros + count update,
  all `Ordering::Relaxed`.
- Mirrored fields on `MetricsSnapshot` plus snapshot
  construction.
- Prometheus rendering appended in `to_prometheus_text`:
  `yggdrasil_apply_batch_duration_seconds_bucket{le="X"}`,
  `_sum` (in seconds), `_count`.
- Drift-guard test
  `every_metrics_snapshot_field_is_exported_in_prometheus_text`
  extended with three accept clauses for the histogram's
  `_bucket`/`_sum`/`_count` mapping (rather than direct
  `yggdrasil_<field>` matching).

`node/src/runtime.rs`:

- Two reconnecting-runtime apply sites instrumented (chaindb
  variant + shared-chaindb variant): wrap
  `apply_verified_progress_to_chaindb` with
  `Instant::now()` / `record_apply_batch_duration`.
- Excludes block fetch (network I/O) and includes ledger
  advance, checkpoint persist, and ChainState topology
  tracking.

### Operational verification

After 30 s of preview sync:
```
$ curl -s http://127.0.0.1:12400/metrics | grep apply_batch
# HELP yggdrasil_apply_batch_duration_seconds Time spent applying a batch of fetched blocks to ledger state.
# TYPE yggdrasil_apply_batch_duration_seconds histogram
yggdrasil_apply_batch_duration_seconds_bucket{le="0.001"} 0
yggdrasil_apply_batch_duration_seconds_bucket{le="0.005"} 0
yggdrasil_apply_batch_duration_seconds_bucket{le="0.01"} 0
yggdrasil_apply_batch_duration_seconds_bucket{le="0.05"} 0
yggdrasil_apply_batch_duration_seconds_bucket{le="0.1"} 0
yggdrasil_apply_batch_duration_seconds_bucket{le="0.5"} 2
yggdrasil_apply_batch_duration_seconds_bucket{le="1"} 2
yggdrasil_apply_batch_duration_seconds_bucket{le="5"} 2
yggdrasil_apply_batch_duration_seconds_bucket{le="10"} 2
yggdrasil_apply_batch_duration_seconds_bucket{le="+Inf"} 2
yggdrasil_apply_batch_duration_seconds_sum 0.412206
yggdrasil_apply_batch_duration_seconds_count 2
```

2 batches recorded in the first 30 s of sync, total apply time
0.412 s (avg ≈ 206 ms/batch, both falling in the `[0.1, 0.5]`
bucket — i.e. `le=0.5` is the `p99`).

This baseline supports Phase C.2 (pipelined fetch + apply): a
post-pipeline rerun should preserve the per-batch p50/p99
distribution while throughput (blocks/s at the applied tip) goes
up.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

Phase B and Phase C.1 closed.  Remaining:

1. **Phase C.2** — pipelined fetch + apply.  Requires shared
   buffer between fetch worker and apply task; deadlock risk
   on rollback.  Use the R200 histogram as the regression
   baseline.
2. **Phase D.1** — deep cross-epoch rollback recovery.
3. **Phase D.2** — multi-session peer accounting.
4. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis serialiser.
5. **Phase A.7** — active stake distribution amounts.
6. **Phase A.3 OMap proposals** — gov-state proposal entries.
7. **Phase E** — pin refresh + mainnet rehearsal + parity proof.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`node/src/tracer.rs`](node/src/tracer.rs) — histogram fields,
  helper, snapshot, Prometheus render, drift-guard test
  extension;
  [`node/src/runtime.rs`](node/src/runtime.rs) — instrumented
  two apply sites.
- Captures: `/tmp/ygg-r91-preview.log`,
  `/tmp/ygg-r91-restart.log`, `/tmp/ygg-r200-preview.log`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-198-nonce-sidecar-persist.md`](2026-04-30-round-198-nonce-sidecar-persist.md).
