## Round 258 — Default `max_concurrent_block_fetch_peers` graduated 1 → 2

Date: 2026-05-06
Branch: main
Type: Operator-facing behaviour change (default config bump only)

### Goal

Graduate the multi-peer BlockFetch dispatch from "operator opt-in only"
to "default-on with upstream-aligned cap." Closes the 67% throughput gap
between Yggdrasil's shipped default and what the existing R218-verified
multi-peer code path delivers.

### Background

R218 (`docs/operational-runs/2026-04-30-round-218-mainnet-multipeer-fetch-rate.md`)
operationally verified the multi-peer BlockFetch dispatch on mainnet at
`--max-concurrent-block-fetch-peers 4`:

| Metric                              | Single-peer | Multi-peer (knob=4) | Δ        |
| ----------------------------------- | ----------: | -------------------: | -------- |
| `blockfetch_workers_registered`     |           0 |                  2  | from 0   |
| `blockfetch_workers_migrated_total` |           0 |                  2  |          |
| Throughput (blk/s)                  |        3.33 |                5.55 | **1.67×**|

The runtime path was fully wired (R166 + R199, then verified in R218);
the only remaining gap was the default config value. The original
default `1` shipped as a deliberate "groundwork-only" gate behind §6.5
operational rehearsal — that rehearsal landed at R218.

### Change

`node/src/config.rs::default_max_concurrent_block_fetch_peers()`
returns `2` (was `1`).

This matches upstream
`Ouroboros.Network.BlockFetch.Decision::bfcMaxConcurrencyBulkSync = 2`
— the canonical initial-sync concurrency cap. Operators retain full
control:

- `1` for strict single-peer (replay/audit byte-for-byte parity)
- `2` (new default) — upstream BulkSync parity
- `> 2` — push beyond the BulkSync cap (works, but not parity-aligned)

### Field rustdoc rewrite

The prior docstring claimed the runtime "is the next slice" and that
"values > 1 parse but are effectively clamped to single-peer behaviour
at runtime." Both claims were stale. Updated to cite the actual
runtime wiring (`runtime.rs::handle_cm_actions →
migrate_session_to_worker → fetch_worker_pool`) and R218's empirical
67% throughput evidence.

### Test impact

Two pinning tests updated:

- `preset_configs_share_canonical_max_concurrent_block_fetch_peers`
- `default_max_concurrent_block_fetch_peers_matches_preset_value`

Both now pin the new canonical default `2`. The integration tests in
`node/tests/{runtime,sync}.rs` that explicitly construct configs with
`max_concurrent_block_fetch_peers: 1` continue to test the legacy
single-peer path at their pinned value.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 903 passed, 0 failed
```

### Operator-facing notes

Operators upgrading to this build:

- Operators with topologies maintaining **≥ 2 warm peers** see the
  multi-peer fetch path activate by default. The knob=2 ceiling
  matches what R218 measured at knob=4 (which saturated at 2 active
  workers per the upstream `bfcMaxConcurrencyBulkSync = 2` cap), so
  R218's 67% throughput delta carries over directly to operators in
  this regime.
- Operators with single-peer topologies see **no change** — the
  runtime clamps to the available warm-peer count via
  `effective_block_fetch_concurrency`.
- Operators wanting strict single-peer (parity audit, byte-for-byte
  replay testing) should pin `MaxConcurrentBlockFetchPeers = 1` in
  their config or pass `--max-concurrent-block-fetch-peers 1` on the
  CLI.

### Stale-docs follow-up

Several pre-graduation documents reference the old default; they
remain factually correct as of their dated context but contradict the
new shipped default. Touched in this round (high-traffic):

- `docs/ARCHITECTURE.md` — Phase 6 status block, "default remains `1`
  pending §6.5 rehearsal sign-off" → updated to record graduation.
- `docs/PARITY_PROOF.md` — §5.x reference to default `1`.
- `docs/PARITY_PLAN.md` — gate item "Parallel BlockFetch default flip
  sign-off" → marked done with R258 reference.
- `docs/MANUAL_TEST_RUNBOOK.md` — §6.5 banner.
- `docs/manual/configuration.md` — config table default column.
- `docs/manual/glossary.md` — entry for `max_concurrent_block_fetch_peers`.

Older operational-runs (R218 evidence, R152, R164, R240) intentionally
preserved as historical record.

### References

- R218 evidence: [`2026-04-30-round-218-mainnet-multipeer-fetch-rate.md`](2026-04-30-round-218-mainnet-multipeer-fetch-rate.md)
- Upstream cap: `Ouroboros.Network.BlockFetch.Decision::bfcMaxConcurrencyBulkSync = 2`
- Runtime wiring: `node/src/runtime.rs::handle_cm_actions` (line ~1784)
- Soak harness: `node/scripts/parallel_blockfetch_soak.sh`
