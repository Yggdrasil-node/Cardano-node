## Round 237 — `GetPoolDistr2`, low-volume egress counters, and rollback replay slice

Date: 2026-05-01
Branch: main
Phase: LSQ data plumbing / Phase D.2 observability / Phase D.1 rollback recovery

### Goal

Close the remaining implementation items from the revised parity closure plan that
are feasible in code without a long-running operator rehearsal:

- Replace the `GetPoolDistr2` `[empty_map, 1]` placeholder with live `PoolDistr`.
- Finish the current `LedgerPeerSnapshotV2` live stake CDF behavior.
- Add lower-volume server egress counters for KeepAlive, TxSubmission2, and PeerSharing.
- Fold per-peer server bytes-out into `PeerLifetimeStats` without exposing peer labels.
- Replace rollback recovery's bare ledger replay with epoch-boundary-aware replay when
  stake-snapshot tracking is active.

### Implementation

#### LSQ `GetPoolDistr2`

`node/src/local_server.rs` now routes `EraSpecificQuery::GetPoolDistr2` through
the same live `PoolDistr` encoder used by `GetStakeDistribution2`.

Behavior:

- Source data: `snapshot.stake_snapshots().set.pool_stake_distribution()`.
- Filter: `maybe_pool_hash_set_cbor` is decoded and applied when present.
- Shape: `[Map KeyHash IndividualPoolStake, NonZero Coin]`.
- Denominator: `pdTotalActiveStake` remains the full active stake of the
  unfiltered source distribution, so filtered and unfiltered responses remain
  wire-compatible with upstream `PoolDistr`.
- Fallback: empty snapshots still emit `[empty_map, 1]` to preserve `NonZero`
  decoding behavior.

`LedgerPeerSnapshotV2` keeps the R237 dirty-state work: registered pools with
live `set` stake are sorted by descending stake with hash tie-breaks, and the
response emits cumulative `AccPoolStake` plus per-pool `PoolStake` rationals.

#### Server Egress

`NodeMetrics` now tracks aggregate counters for all server-side data
mini-protocol egress:

- `yggdrasil_blockfetch_server_bytes_served_total`
- `yggdrasil_chainsync_server_bytes_served_total`
- `yggdrasil_keepalive_server_bytes_served_total`
- `yggdrasil_txsubmission_server_bytes_served_total`
- `yggdrasil_peersharing_server_bytes_served_total`

Responder loops now receive the remote peer address. Internally, bytes served
are accumulated by peer in `NodeMetrics::peer_egress_bytes_by_peer`; the
runtime governor tick folds those totals into `GovernorState::lifetime_stats`
via `set_lifetime_bytes_out`. Prometheus remains aggregate-only to avoid
high-cardinality peer labels.

#### Rollback Recovery

`update_ledger_checkpoint_after_progress` now derives the actual rollback point
from the `MultiEraSyncStep::RollBackward` step and truncates ledger checkpoints
after that point rather than after the final batch point. When
`LedgerCheckpointTracking` has stake snapshots plus an epoch schedule, recovery
uses `recover_ledger_state_chaindb_with_epoch_boundary`:

1. Restore the latest checkpoint at or before the rollback point.
2. Replay immutable suffix and volatile suffix through
   `advance_ledger_with_epoch_boundary`.
3. Rebuild `StakeSnapshots` from the restored ledger state and replayed epoch
   transitions.
4. Preserve current-epoch pool block counts by seeding from the restored
   `LedgerState::blocks_made()` and replaying forward.

Fallback recovery still uses the older replay path when snapshot tracking is
not enabled.

### Verification

Focused regression checks:

```text
cargo check -p yggdrasil-node                                            PASS
cargo test -p yggdrasil-node get_pool_distr2 --lib                       PASS
cargo test -p yggdrasil-node get_ledger_peer_snapshot_with_live_stake_emits_cdf_rationals --lib
                                                                          PASS
cargo test -p yggdrasil-node node_metrics_tracks_phase_d2_lifetime_peer_stats --lib
                                                                          PASS
cargo test -p yggdrasil-node inbound_accept_loop_records_responder_egress_metrics --lib
                                                                          PASS
cargo test -p yggdrasil-node update_ledger_checkpoint_after_progress_clears_ocert_counters_on_rollback --lib
                                                                          PASS
cargo test -p yggdrasil-network lifetime_stats_accumulate_across_simulated_reconnects --lib
                                                                          PASS
cargo fmt --all -- --check                                               PASS
cargo check-all                                                          PASS
cargo lint                                                               PASS
cargo test-all                                                           PASS
```

The inbound responder metric test requires localhost TCP bind permissions. The
first sandboxed run failed with `Operation not permitted`; rerun outside the
filesystem/network sandbox passed.

### Deferred Gates

Not completed in this code slice:

- `cardano-base` upstream vector refresh; this remains network/corpus-refresh
  work.
- Manual §6.5 parallel BlockFetch hash comparison and soak.
- §2-9 24h+ mainnet endurance sequence.
- Default `max_concurrent_block_fetch_peers` remains `1` until the operator
  sign-off evidence passes.
- Exact rollback of nonce and OpCert sidecars to the restore point still needs
  historical sidecar/checkpoint coordination. R237 improves ledger/stake replay
  and keeps existing OpCert reset behavior as the safe fallback.

### References

- Upstream: `Cardano.Ledger.Core.PoolDistr`
- Upstream: `Cardano.Protocol.TPraos.API.IndividualPoolStake`
- Upstream: `Ouroboros.Consensus.Storage.ChainDB`
- Upstream: `Ouroboros.Network.Protocol.{KeepAlive,TxSubmission,PeerSharing}`
- yggdrasil:
  - `node/src/local_server.rs::encode_pool_distr_for_lsq`
  - `node/src/local_server.rs::encode_ledger_peer_snapshot_v2_for_lsq`
  - `node/src/server.rs::{run_keepalive_server,run_txsubmission_server,run_peersharing_server}`
  - `node/src/tracer.rs::NodeMetrics`
  - `node/src/sync.rs::recover_ledger_state_chaindb_with_epoch_boundary`
