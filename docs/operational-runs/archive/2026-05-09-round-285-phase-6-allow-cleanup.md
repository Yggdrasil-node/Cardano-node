# Round 285 — Phase-6 `#[allow(dead_code)]` cleanup in peer_management.rs

**Date:** 2026-05-09
**Phase:** C (tech-debt purge)
**Predecessor:** R284 (`docs/operational-runs/2026-05-09-round-284-lsq-todo-resolution.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Resolve all 5 `#[allow(dead_code)]` annotations on
`OutboundPeerManager` methods marked `// Phase 6 scaffolding`. Phase 6
itself is already complete and default-on (`docs/ARCHITECTURE.md:72`,
"Status: complete; default-on as of R258"); the annotations are stale
leftovers from before the Phase 6 wiring fully landed.

Operator-confirmed path: "Land the wiring (Recommended)" — verified
that the 5 helpers are either already wired in production, are
truly redundant with alternative production paths, or are test-only
seams. Cleanup removes the staleness without breaking the multi-peer
sync architecture.

## Investigation

Walked each of the 5 sites against current production callers:

| Site | Production caller | Verdict |
|---|---|---|
| `migrate_session_to_worker` | `cm_actions.rs:237` (`apply_cm_actions`) | wired; allow stale |
| `unregister_worker` | (none — `demote_to_cold:383` accessed pool inline) | route `demote_to_cold` through this helper, then drop allow |
| `shared_fetch_worker_pool` getter | (none — `commands/run.rs:293` threads pool Arc directly) | redundant; mark test-only |
| `with_hot_block_fetch_clients` | (none — sync loop uses `SharedFetchWorkerPool::dispatch_plan` instead) | test-only seam |
| `hot_peer_addrs` | (none — sync loop uses `SharedFetchWorkerPool::worker_count` instead) | test-only seam |

The Phase 6 wiring landed via different helper signatures than the
ones the original scaffolding anticipated. The 5 helpers were authored
ahead of the actual dispatcher implementation; once the production
runtime took shape, alternative paths emerged that bypassed the
helpers. The 5 helpers became orphaned — used only by tests.

## Resolution

### `migrate_session_to_worker` — drop stale allow

Already called from `cm_actions.rs::apply_cm_actions` at line 237 after
successful `promote_to_warm` (in the multi-peer dispatch branch). The
`#[allow(dead_code)]` is leftover from before that caller landed; just
remove it.

### `unregister_worker` — route `demote_to_cold` through this helper

`OutboundPeerManager::demote_to_cold` previously did inline:
```rust
let _ = self.fetch_worker_pool.write().await.unregister(&peer);
```

Refactored to call the helper:
```rust
let _ = self.unregister_worker(&peer).await;
```

The behavior is identical (same write-lock, same `unregister` call).
Centralizing through the helper documents the worker-unregister
intent better and removes one duplicate code path.

### `shared_fetch_worker_pool()` getter — mark test-only

The production runtime constructs the pool once in
`commands/run.rs:293` (`yggdrasil_node::runtime::new_shared_fetch_worker_pool()`)
and threads the same `Arc<RwLock<...>>` directly into both the manager
(via `with_fetch_worker_pool(pool.clone())` in `governor_loop.rs:90`)
and the sync config (via `VerifiedSyncServiceConfig::shared_fetch_worker_pool`
field). The manager-side getter is redundant with this clone-the-Arc
pattern.

Tests, however, use the getter to inspect the manager-side pool
without poking private fields. Replace `#[allow(dead_code)]` with
`#[cfg(test)]` so the getter is compiled only in test builds.

### `with_hot_block_fetch_clients` and `hot_peer_addrs` — mark test-only

The production sync loop's multi-peer dispatcher accesses per-peer
state through `SharedFetchWorkerPool::dispatch_plan(...)` (which
operates on registered workers in the shared pool, not on warm-peer
entries in the manager) and sizes its concurrency via
`SharedFetchWorkerPool::worker_count` rather than counting hot peers
in the manager. Both helpers were authored as the original scaffolding
API but the production dispatcher took different paths.

Tests use them to exercise the borrow-checked closure pattern + hot-
peer enumeration without spinning up a full sync session. Replace
`#[allow(dead_code)]` with `#[cfg(test)]`.

The `BlockFetchClient` import (used only by `with_hot_block_fetch_clients`)
is also moved into a `#[cfg(test)]` block so production builds don't
import a now-test-only type.

## Production `#[allow(dead_code)]` site count

| Site | Pre-R285 | Post-R285 | Round |
|---|---|---|---|
| `peer_management.rs::migrate_session_to_worker` | 1 | 0 | R285 ✅ (allow removed; production caller `cm_actions.rs:237`) |
| `peer_management.rs::unregister_worker` | 1 | 0 | R285 ✅ (allow removed; routed via `demote_to_cold`) |
| `peer_management.rs::shared_fetch_worker_pool` (getter) | 1 | 0 | R285 ✅ (replaced with `#[cfg(test)]`) |
| `peer_management.rs::with_hot_block_fetch_clients` | 1 | 0 | R285 ✅ (replaced with `#[cfg(test)]`) |
| `peer_management.rs::hot_peer_addrs` | 1 | 0 | R285 ✅ (replaced with `#[cfg(test)]`) |
| `reconnecting.rs::_runstate_impl_marker` | 1 | 1 | R286 |
| `shelley.rs::mk_txout` test helper | 1 | 1 | R286 |
| **TOTAL production** | 7 | 2 | |

R285 reduces production-side `#[allow(dead_code)]` count from 7 to 2.
R286 closes the remaining two.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 2.39s)
cargo lint                          clean (Finished `dev` profile in 8.68s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
```

The Phase 6 multi-peer fetch tests in `node/src/runtime/tests.rs`
(`hot_peer_addrs_*`, `with_hot_block_fetch_clients_*`,
`migrate_session_to_worker_*`, `unregister_worker_*`,
`shared_fetch_worker_pool_*`) continue to pass after the `#[cfg(test)]`
gating since they are themselves test code.

## Diff stat

```text
node/src/runtime/peer_management.rs   +9 lines (3 allow->cfg(test); 2
                                                 doc updates; 1 inline
                                                 -> helper refactor;
                                                 BlockFetchClient import
                                                 moved into cfg(test))
docs/operational-runs/2026-05-09-round-285-... (new)
```

## Stop point — Phase C in progress

| Round | Site | Status |
|---|---|---|
| R282 | `block_producer.rs::description` | ✅ closed |
| R283 | `sync.rs era_tag` + `local_server.rs lsq_era_index` | ✅ closed |
| R284 | `local_server.rs:713` LSQ TODO | ✅ closed |
| **R285** | `peer_management.rs` Phase-6 allows | ✅ closed |
| R286 | `reconnecting.rs::_runstate_impl_marker` + `shelley.rs::mk_txout` | next |
| R287 | `code-audit.md` + `REFACTOR_BLUEPRINT.md` re-grade | pending |

R285 was originally estimated at 1.5 agent-days assuming new
production wiring needed to land. The actual scope was much smaller
(0.25 agent-days) because Phase 6 was already in production via
alternative paths; the `#[allow(dead_code)]` annotations were just
stale.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R284 (`docs/operational-runs/2026-05-09-round-284-lsq-todo-resolution.md`)
- Phase 6 status: `docs/ARCHITECTURE.md:72` ("Status: complete; default-on as of R258")
- Multi-peer dispatch verification: R199 (R91 livelock no longer
  reproduces; 22K blocks at `--max-concurrent-block-fetch-peers 4`)
