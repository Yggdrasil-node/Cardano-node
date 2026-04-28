## Round 169 — Current-era Prometheus gauge

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Add `yggdrasil_current_era` to the `/metrics` endpoint so
operator dashboards can observe Byron→…→Conway era progression
directly without parsing `cardano-cli query tip` JSON.  Closes a
small but persistent operational-alignment gap: the metric set
already exposes slot, block number, mempool stats, peer counts,
checkpoint state — but the era was the one piece that required
out-of-band shell to read.

### Code change

`node/src/tracer.rs`:

- New `current_era: AtomicU64` field on `NodeMetrics` plus
  matching `MetricsSnapshot::current_era: u64`.
- New setter `NodeMetrics::set_current_era(u64)`.
- Prometheus exposition adds:

  ```
  # HELP yggdrasil_current_era Wire era ordinal of the latest
  # applied block (0=Byron, 1=Shelley, 2=Allegra, 3=Mary,
  # 4=Alonzo, 5=Babbage, 6=Conway).
  # TYPE yggdrasil_current_era gauge
  yggdrasil_current_era {value}
  ```

  Ordinals match `Era::era_ordinal()`.

`node/src/runtime.rs`: invoke the setter at both production
post-apply sites (`run_reconnecting_verified_sync_service_chaindb_inner`
and `run_reconnecting_verified_sync_service_shared_chaindb_inner`),
right after `apply_verified_progress_to_chaindb` returns.  The
setter reads
`tracking.ledger_state.current_era.era_ordinal()` (which
`apply_block_validated` updates per applied block) and writes it
into the gauge:

```rust
if let (Some(m), Some(tracking)) = (metrics, checkpoint_tracking.as_ref()) {
    m.set_current_era(tracking.ledger_state.current_era.era_ordinal() as u64);
}
```

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), `/metrics` reports the era directly:

```
$ curl -s :12369/metrics | grep yggdrasil_current_era
# HELP yggdrasil_current_era Wire era ordinal of the latest applied block (0=Byron, 1=Shelley, 2=Allegra, 3=Mary, 4=Alonzo, 5=Babbage, 6=Conway).
# TYPE yggdrasil_current_era gauge
yggdrasil_current_era 4
```

`4 = Alonzo`, matching preview's `Test*HardForkAtEpoch=0` shape
(blocks decode as Alonzo from genesis, R160's PV-aware era
classification reports the same to cardano-cli).

Side-by-side metric snapshot at the verification window:

```
yggdrasil_blocks_synced 99
yggdrasil_current_slot 1960
yggdrasil_current_block_number 98
yggdrasil_current_era 4              ← R169
yggdrasil_active_peers 1             ← R168
```

Note that this gauge tracks the **wire era** of the latest
applied block (i.e. the ledger's `current_era` field updated
inside `apply_block_validated`).  The PV-aware promotion that
cardano-cli sees for chain-tip queries (R160) is computed at
query-dispatch time and is intentionally not reflected here —
operators consult this gauge for raw on-disk era progression,
which is the relevant metric for sync dashboards and storage
provisioning.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean (one ref-of-ref clippy
                                 # nit fixed by destructuring as
                                 # `Some(tracking)` instead of
                                 # `Some(ref tracking)`)
cargo test-all                   # passed: 4710  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count unchanged — the gauge is exercised end-to-end by
every sync run, and unit tests for the Prometheus exposition
already cover the surrounding gauges via the
`metrics_snapshot_renders_in_prometheus_text` family.

### Open follow-ups

1. **Per-era block counters** — adding seven counters
   (`yggdrasil_blocks_byron`, `..._shelley`, …, `..._conway`)
   would let dashboards graph era split during long syncs.
   Trivially ~30 LOC; deferred until a dashboard build asks for
   it.
2. **Apply-batch duration histogram** — `_p50`, `_p90`, `_p99`
   for time spent in `apply_verified_progress_to_chaindb`, would
   let operators see whether sync is fetch-bound or apply-bound.
3. Carry-over from R168: multi-session peer accounting once
   `max_concurrent_block_fetch_peers > 1` activates.
4. Carry-over from R166: pipelined fetch + apply.
5. Carry-over from R167: deep cross-epoch rollback recovery.
6. Carry-over from R163: live stake-distribution computation +
   `GetGenesisConfig` ShelleyGenesis serialisation.

### References

- Captures: `/tmp/ygg-r169-preview.log` (post-fix preview sync,
  `/metrics` reports `yggdrasil_current_era 4`).
- Code: [`node/src/tracer.rs`](node/src/tracer.rs)
  `current_era` field + setter + Prometheus rendering;
  [`node/src/runtime.rs`](node/src/runtime.rs) post-apply
  invocations.
- Upstream reference: `Cardano.Ledger.Core.Era` ordering.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-168-bootstrap-peer-metric.md`](2026-04-28-round-168-bootstrap-peer-metric.md).
