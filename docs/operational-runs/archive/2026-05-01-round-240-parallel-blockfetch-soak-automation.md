## Round 240 - Parallel BlockFetch soak automation

Date: 2026-05-01
Phase: E.2 operator evidence hardening
Scope: operator automation and living-doc correction, no consensus/runtime behavior change

### Summary

R240 makes the remaining runbook §6.5 multi-peer BlockFetch sign-off
repeatable. The new `node/scripts/parallel_blockfetch_soak.sh` harness:

- starts `yggdrasil-node run` with `--max-concurrent-block-fetch-peers`;
- accepts `CONFIG` / `TOPOLOGY` overrides for real operator topologies;
- captures Prometheus snapshots under `$METRICS_DIR`;
- asserts `yggdrasil_blockfetch_workers_registered` reaches the expected
  worker count;
- asserts `yggdrasil_blockfetch_workers_migrated_total` reaches the same
  count;
- optionally runs `compare_tip_to_haskell.sh` against `HASKELL_SOCK` at
  `COMPARE_INTERVAL_S` cadence;
- scans logs for the known worker failure traces
  `fetch worker channel closed` and `fetch worker dropped response`;
- writes `$LOG_DIR/summary.txt` with blocks, slot, reconnect, worker, and
  fetch/apply batch-duration evidence.
- is pinned by node smoke tests for help output and fail-closed rejection
  of the legacy single-peer knob.

This does not change the sync path. It converts an operator-time parity
gate into a deterministic evidence collection procedure before flipping
the default `max_concurrent_block_fetch_peers` from `1`.

### Documentation corrections

The same slice refreshes living docs that were stale relative to already
implemented behavior:

- Runbook §6.5 now points to `parallel_blockfetch_soak.sh` as the
  preferred harness.
- Runbook §6.5b now documents the actual env-var interface for
  `node/scripts/compare_tip_to_haskell.sh`.
- Runbook §8 no longer claims `cardano-cli conway query gov-state` is an
  open gap; R188/R193/R204 closed tag 24 `ConwayGovState` shape parity.
- Release docs include `parallel_blockfetch_soak.sh` in the bundled script
  list.

### Verification

Commands run during this slice:

```sh
bash -n node/scripts/parallel_blockfetch_soak.sh
node/scripts/parallel_blockfetch_soak.sh --help
cargo test -p yggdrasil-node parallel_blockfetch_soak_script
cargo fmt --all -- --check
cargo check-all
cargo test-all
cargo lint
git diff --check
git diff --cached --check
```

### Status impact

- No new code-level parity blockers.
- The remaining BlockFetch concurrency default flip is still gated by
  operator-time evidence, but the evidence now has a first-class harness.
- The stale `gov-state` runbook caveat is removed from current operator
  guidance; future failures of that query should be treated as regressions.
