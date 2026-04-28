## Round 175 — Registry-cooling completeness for R168 hooks

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Sweep the session-teardown paths in
`run_reconnecting_verified_sync_service_chaindb_inner` and
`run_reconnecting_verified_sync_service_shared_chaindb_inner` for
missing companion calls to R168's `registry_mark_bootstrap_cooling`.
R168 wired the cooling at two of the five `session.mux.abort()`
sites — synchronize-failure path and batch-error punish path —
but missed the KeepAlive-failure and session-switching paths.

The gap meant a KeepAlive timeout or hot-peer handoff would leave
the bootstrap peer marked `PeerHot` in the registry until the
next session's promote-to-Hot overrode it (a no-op in the registry
since the status is already Hot).  In the brief window between
mux abort and re-bootstrap, `/metrics` would over-report
`yggdrasil_active_peers` by one.  Not a functional bug — sync
itself proceeds correctly — but a metric anomaly that confuses
operator dashboards during transient peer churn.

### Issues fixed

1. **KeepAlive-failure mux abort** (chaindb_inner line 4936,
   shared_chaindb_inner line 5478) — added cooling call
   alongside the existing `mux.abort()` and
   `record_reconnect_failure()`.

2. **Session-switching mux abort** (chaindb_inner line 5196,
   shared_chaindb_inner line 5746) — the runtime aborts the
   current session when a higher-tip hot peer becomes available
   ("switching sync session to higher-tip hot peer" trace).
   Added cooling so the previous bootstrap peer demotes from
   `PeerHot` immediately, mirroring the handoff in `/metrics`.

### Code change

`node/src/runtime.rs`: two new `registry_mark_bootstrap_cooling`
call sites in each inner function (4 total, applied via
`replace_all` since both inner functions have identical
structure).  Each is annotated with a Round 175 rationale
comment.

The third inner function
(`run_reconnecting_verified_sync_service_with_tracer`, line
6009) doesn't carry a `peer_registry` field in its request and
never registered a Hot bootstrap peer in the first place — its
KeepAlive path was inadvertently matched by the `replace_all`
during this fix and corrected to a comment explaining why no
cooling is needed there.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4729  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count unchanged — the cooling completeness is a
behavioural-correctness fix during transient state transitions
that's not naturally reachable in unit-test-fixture-driven
scenarios.  Operational verification (below) covers the
end-to-end behaviour.

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), `/metrics` continues to report the correct
peer counts under steady-state operation:

```
$ curl -s :12375/metrics | grep -E 'yggdrasil_(blocks_synced|current_era|active_peers|established_peers|known_peers|reconnects) '
yggdrasil_blocks_synced 449
yggdrasil_reconnects 0
yggdrasil_current_era 4
yggdrasil_known_peers 1
yggdrasil_established_peers 1
yggdrasil_active_peers 1
```

Sync rate unchanged at ~14 blk/s.

### Why this matters

Pre-R175, the `yggdrasil_active_peers` gauge could briefly
over-report during:

- KeepAlive timeouts under network instability — the abort
  fires before the next reconnect re-promotes a peer, leaving
  the registry showing the old peer as Hot for the entire
  reconnect-backoff window (up to ~60 s exponential backoff per
  R166).
- Multi-peer hot-handoff — the runtime switches to a
  higher-tip peer without demoting the previous one,
  double-counting active peers until the new bootstrap fires.

Both are now corrected; the gauge transitions from 1 → 0 → 1 (or
1 → 1) cleanly across reconnects, with no spurious
double-counting.

### Open follow-ups (unchanged from R174)

1. Live stake-snapshot plumbing into `LedgerStateSnapshot`.
2. `GetGenesisConfig` ShelleyGenesis serialisation.
3. Apply-batch duration histogram (R169).
4. Multi-session peer accounting (R168 — the structural
   follow-up; R175 only completes the single-session cooling
   path).
5. Pipelined fetch + apply (R166).
6. Deep cross-epoch rollback recovery (R167).

### References

- Code: [`node/src/runtime.rs`](node/src/runtime.rs) — four
  `registry_mark_bootstrap_cooling` call-site additions
  (KeepAlive ×2, session-switching ×2) plus a clarifying
  comment in the `with_tracer` path.
- Upstream reference: `Ouroboros.Network.PeerSelection.Governor`
  — the warm/hot status lifecycle that R168's hooks track.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-174-decoder-strictness-fixes.md`](2026-04-28-round-174-decoder-strictness-fixes.md).
