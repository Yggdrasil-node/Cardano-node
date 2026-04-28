## Round 168 — Bootstrap-peer registry promotion fixes `/metrics` peer counts

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Fix the metric anomaly visible across Rounds 165–167 where
`/metrics` reported
`yggdrasil_active_peers 0`,
`yggdrasil_known_peers 0`,
`yggdrasil_established_peers 0` while sync was demonstrably
running (blocks_synced advancing, current_slot advancing, no
reconnects).  The numbers should at minimum reflect the single
upstream peer the bootstrap path is actively driving sync from.

### Root cause

`bootstrap_with_attempt_state` opens a direct outbound connection
to the configured upstream peer (or a topology fallback) and
hands the resulting `PeerSession` to the sync loop.  This bypasses
the governor's normal warm→hot promotion flow — which is the only
code path that calls `PeerRegistry::set_status` to mark a peer
`PeerHot`.

`PeerSelectionCounters::from_registry` (called from the
governor's per-tick metrics update) then iterates the
`PeerRegistry` and counts entries by status.  The bootstrap peer
appears in the registry (it was inserted at `seed_peer_registry`
startup time with `PeerSourceBootstrap`) but its status remained
`PeerCold` — so it never contributed to
`active`/`established`/`known` counters.  The metric reported 0
while the chain syncing peer was, in fact, actively serving
ChainSync + BlockFetch.

### Fix

`node/src/runtime.rs`: two new helpers (`registry_mark_bootstrap_hot`
and `registry_mark_bootstrap_cooling`) wrap the
`PeerRegistry::insert_source` + `set_status` calls behind an
`Option<&Arc<RwLock<PeerRegistry>>>` so call sites without a
registry (the legacy `run_reconnecting_verified_sync_service_with_tracer`
path that takes no `peer_registry` field) become no-ops.

The hot-mark is invoked alongside `pool_register_peer` at session
establishment in both production sync paths
(`run_reconnecting_verified_sync_service_chaindb_inner` and
`run_reconnecting_verified_sync_service_shared_chaindb_inner`),
mirroring the existing BlockFetch-pool registration pattern.

The cooling-mark is invoked alongside `pool_unregister_peer` at
session teardown — both at the `synchronize_chain_sync_to_point`
intersect-failure path and at the reconnect-batch error
disposition path.  The `BatchErrorDisposition::ReconnectAndPunish`
branch's existing `set_status(addr, PeerCold)` continues to
override `Cooling → Cold` for offending peers, matching upstream's
`InvalidBlockPunishment` semantics.

The entry stays in the registry (with `PeerSourceBootstrap`)
across cooling so the next reconnect attempt can resume from the
same status row — matching upstream's `cooldownPeerInfo`
post-session bookkeeping.

### Code change

`node/src/runtime.rs`:

```rust
fn registry_mark_bootstrap_hot(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    peer_addr: SocketAddr,
) {
    if let Some(reg) = peer_registry {
        if let Ok(mut guard) = reg.write() {
            guard.insert_source(peer_addr, PeerSource::PeerSourceBootstrap);
            guard.set_status(peer_addr, PeerStatus::PeerHot);
        }
    }
}

fn registry_mark_bootstrap_cooling(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    peer_addr: SocketAddr,
) {
    if let Some(reg) = peer_registry {
        if let Ok(mut guard) = reg.write() {
            guard.set_status(peer_addr, PeerStatus::PeerCooling);
        }
    }
}
```

Five call sites updated:
- 2× session establishment alongside `pool_register_peer`
- 2× synchronize-failure mux abort path
- 2× reconnect-batch error disposition path

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), `/metrics` now reports the active sync peer:

```
$ curl -s :12368/metrics | grep -E 'yggdrasil_(active|known|established)_peers '
yggdrasil_known_peers 1
yggdrasil_established_peers 1
yggdrasil_active_peers 1
```

Compare with prior rounds (R165, R166, R167) which reported `0`
for all three counters during identical sync conditions.  Sync
itself proceeds unchanged — `query tip` returns the expected
`epoch=0, era=Alonzo` shape and `query tx-mempool info` reports
the latest slot.

### Verification gates

```
cargo fmt --all -- --check       # clean (one auto-format applied)
cargo lint                       # clean
cargo test-all                   # passed: 4710  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count unchanged — the metric anomaly only manifests in the
production runtime's session-establishment path, which is not
covered by the registry / governor unit tests.  The fix is
exercised end-to-end by every fresh sync.

### Open follow-ups

1. **Multi-session peer accounting** — once
   `max_concurrent_block_fetch_peers > 1` activates parallel
   fetches across multiple peers, the registry promotion will need
   to fan out per peer.  Currently the production runtime is
   single-session so a single Hot entry suffices.
2. Carry-over from R167: deep cross-epoch rollback recovery
   (snapshot redo).
3. Carry-over from R166: pipelined fetch + apply.
4. Carry-over from R163: live stake-distribution computation and
   `GetGenesisConfig` ShelleyGenesis serialisation.
5. Carry-over from R161: Babbage TxOut datum_inline / script_ref
   operational verification once preview crosses Alonzo.

### References

- Captures: `/tmp/ygg-r168-preview.log` (post-fix preview sync,
  `/metrics` shows active_peers=1).
- Code: [`node/src/runtime.rs`](node/src/runtime.rs)
  `registry_mark_bootstrap_hot` / `registry_mark_bootstrap_cooling`.
- Upstream reference:
  `Ouroboros.Network.PeerSelection.Governor` —
  `peerSelectionStateToView` / `KnownPeerInfo.peerStatus`;
  `Cardano.Diffusion.NodeToNode.outbound-governor` for the
  warm→hot session lifecycle the registry tracks.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-167-mid-sync-rollback-epoch-fixup.md`](2026-04-28-round-167-mid-sync-rollback-epoch-fixup.md).
