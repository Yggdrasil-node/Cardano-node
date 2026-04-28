## Round 170 — Per-era applied-block counters

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close the R169 follow-up #1: extend the era observability beyond
the single `yggdrasil_current_era` gauge by exposing
seven per-era applied-block counters
(`yggdrasil_blocks_byron`, `…_shelley`, `…_allegra`, `…_mary`,
`…_alonzo`, `…_babbage`, `…_conway`).  Combined with the gauge,
operator dashboards can now graph the share of blocks applied
per era during a long sync without scraping
`cardano-cli query tip` history, and the sum of the seven
counters provides a sanity-check parity row against
`yggdrasil_blocks_synced`.

### Code change

`node/src/tracer.rs`:

- New `blocks_per_era: [AtomicU64; 7]` field on `NodeMetrics`,
  indexed parallel to `Era::era_ordinal()` (`[0]=Byron … [6]=Conway`).
- Matching `MetricsSnapshot::blocks_per_era: [u64; 7]`.
- New setter `NodeMetrics::add_blocks_for_era(era_ordinal: u8, n: u64)`
  with bounds-check (out-of-range ordinals silently no-op so a
  future eighth era doesn't crash the metric path).
- Prometheus exposition adds seven `# HELP/TYPE counter` blocks
  explicitly named per era (Prometheus convention prefers
  enumerated counters over labels for low-cardinality dimensions
  with stable, well-known values).

`node/src/runtime.rs::record_verified_batch_progress`: tally
per-era block counts locally across the batch's RollForward
steps, then make one `add_blocks_for_era` call per era — keeps
the atomic write count to ≤ 7 per batch instead of one per block.

```rust
let mut tally = [0u64; 7];
for step in &progress.steps {
    if let MultiEraSyncStep::RollForward { blocks, .. } = step {
        for block in blocks {
            let ord = block.era().era_ordinal() as usize;
            if ord < tally.len() {
                tally[ord] += 1;
            }
        }
    }
}
for (ord, count) in tally.iter().enumerate() {
    if *count > 0 {
        m.add_blocks_for_era(ord as u8, *count);
    }
}
```

### Test surface fix

The existing
`every_metrics_snapshot_field_is_exported_in_prometheus_text`
test reflects over `MetricsSnapshot` JSON keys and expects each
to map to a `yggdrasil_<field_name>` Prometheus line — but
`blocks_per_era` is exploded into seven explicit counters (no
`yggdrasil_blocks_per_era` line).  Extended the test's accept
predicate to recognise the seven explicit names when checking
`blocks_per_era`, mirroring the existing exception already in
place for `uptime_ms → yggdrasil_uptime_seconds`.

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), `/metrics` reports the per-era counters:

```
$ curl -s :12370/metrics | grep -E 'yggdrasil_(blocks_synced|current_era|blocks_(byron|shelley|allegra|mary|alonzo|babbage|conway)) '
yggdrasil_blocks_synced 99
yggdrasil_current_era 4
yggdrasil_blocks_byron 0
yggdrasil_blocks_shelley 0
yggdrasil_blocks_allegra 0
yggdrasil_blocks_mary 0
yggdrasil_blocks_alonzo 99
yggdrasil_blocks_babbage 0
yggdrasil_blocks_conway 0
```

All 99 applied blocks correctly attributed to Alonzo (preview's
`Test*HardForkAtEpoch=0` shape decodes blocks as Alonzo from
genesis, R169's `current_era 4` agrees).
`sum(yggdrasil_blocks_*) = 99 = yggdrasil_blocks_synced` confirms
the per-era tally is consistent with the existing total counter.

### Verification gates

```
cargo fmt --all -- --check       # clean (one auto-format applied)
cargo lint                       # clean
cargo test-all                   # passed: 4710  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count unchanged — the new code is exercised end-to-end by
every sync run, and the reflective Prometheus-export test was
extended (not replaced) to cover the per-era expansion.

### Open follow-ups

1. **Apply-batch duration histogram** — `_p50`, `_p90`, `_p99`
   gauges (or a Prometheus histogram) for time spent in
   `apply_verified_progress_to_chaindb`.  Operators can then
   diagnose whether sync is fetch-bound or apply-bound.
   Carry-over from R169.
2. Carry-over from R168: multi-session peer accounting once
   `max_concurrent_block_fetch_peers > 1` activates.
3. Carry-over from R166: pipelined fetch + apply.
4. Carry-over from R167: deep cross-epoch rollback recovery.
5. Carry-over from R163: live stake-distribution computation +
   `GetGenesisConfig` ShelleyGenesis serialisation.

### References

- Captures: `/tmp/ygg-r170-preview.log` (post-fix preview sync,
  `/metrics` reports `yggdrasil_blocks_alonzo 99`).
- Code: [`node/src/tracer.rs`](node/src/tracer.rs)
  `blocks_per_era` field + `add_blocks_for_era` setter + seven
  Prometheus exposition lines + extended reflection test;
  [`node/src/runtime.rs`](node/src/runtime.rs)
  per-batch tally + setter call.
- Upstream reference: `Cardano.Ledger.Core.Era` ordering.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-169-current-era-metric.md`](2026-04-28-round-169-current-era-metric.md).
