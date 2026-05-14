# Round 286 — `_runstate_impl_marker` deletion + unused `mk_txout` deletion

**Date:** 2026-05-09
**Phase:** C (tech-debt purge)
**Predecessor:** R285 (`docs/operational-runs/2026-05-09-round-285-phase-6-allow-cleanup.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Resolve the last 2 production `#[allow(dead_code)]` sites:

1. `node/src/runtime/reconnecting.rs:379` — `mod _runstate_impl_marker`
   (visibility marker between two halves of `impl ReconnectingRunState`).
2. `crates/ledger/src/eras/shelley.rs:1887` — `mk_txout` test helper
   (Shelley-era variant; an `mk_txout` exists for Allegra and is used
   there).

After R286, **zero `#[allow(dead_code)]` remain in production code**.

## Resolution

### `_runstate_impl_marker` — replace with comment line

The marker module exists purely as a visual seam between two halves
of `impl ReconnectingRunState` to discourage accidental insertion of
unrelated items at the boundary. A comment line carries the same
visual effect without the dead-code allow.

Replaced:
```rust
#[allow(dead_code)]
mod _runstate_impl_marker {
    // Marker module — keeps the split impl-block boundary visible and
    // prevents accidental insertion of unrelated items between the two
    // halves of `impl ReconnectingRunState`.
}
```

With a documenting comment block + the second `impl ReconnectingRunState`
header:
```rust
// === Second `impl ReconnectingRunState` block — progress tracking + ===
// === reconnect-cycle counters + sync-step trace surfaces.            ===
//
// Splitting the impl into two blocks (constructor / lifecycle, then
// progress tracking) keeps each block focused. The previous
// `_runstate_impl_marker` module served the same purpose; replaced
// with a comment line in R286 since a marker module has the same
// visual effect without carrying a `dead_code` allow.
impl ReconnectingRunState {
    pub(super) fn record_progress(&mut self, progress: &MultiEraSyncProgress) {
        ...
    }
}
```

### `shelley.rs::mk_txout` — delete

`grep "mk_txout"` showed:
- `crates/ledger/src/eras/shelley.rs:1888` — defined but never called.
- `crates/ledger/src/eras/allegra.rs:524` — defined and called from 5 sites in Allegra-era tests.

The Shelley variant has no test consumers. Per the plan's verdict
("If the helper genuinely has no test home, delete it."), removed
the unused function entirely.

## Production `#[allow(dead_code)]` site count

| Site | Pre-R286 | Post-R286 |
|---|---|---|
| `reconnecting.rs::_runstate_impl_marker` | 1 | 0 ✅ |
| `shelley.rs::mk_txout` test helper | 1 | 0 ✅ |
| **TOTAL production** | 2 | **0** |

R286 brings production-side `#[allow(dead_code)]` count to zero. All
9 starting sites (block_producer, sync, local_server, peer_management
× 5, reconnecting, shelley) closed across R282–R286.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 6.03s)
cargo lint                          clean (Finished `dev` profile in 14.03s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
```

```text
$ grep -rn "#\[allow(dead_code)\]" --include="*.rs" crates/ node/src/ | grep -v "/tests/" | grep -v "cfg(test)"
(empty)
```

## Diff stat

```text
node/src/runtime/reconnecting.rs       -7 lines (delete marker module,
                                                  add comment block to second impl)
crates/ledger/src/eras/shelley.rs      -7 lines (delete unused mk_txout)
docs/operational-runs/2026-05-09-round-286-... (new)
```

## Stop point — Phase C closing

| Round | Site | Status |
|---|---|---|
| R282 | `block_producer.rs::description` | ✅ closed |
| R283 | `sync.rs era_tag` + `local_server.rs lsq_era_index` | ✅ closed |
| R284 | `local_server.rs:713` LSQ TODO | ✅ closed |
| R285 | `peer_management.rs` Phase-6 allows | ✅ closed |
| **R286** | `reconnecting.rs` marker + `shelley.rs` test helper | ✅ closed |
| R287 | `code-audit.md` + `REFACTOR_BLUEPRINT.md` re-grade | next |

R287 is the last Phase C round (doc re-grade only, no code changes).

## Phase C summary

| Round | Sites resolved |
|---|---|
| R282 | 1 (block_producer description) |
| R283 | 1 (sync.rs era_tag) + lsq_era_index added |
| R284 | 1 (local_server.rs LSQ TODO) |
| R285 | 5 (peer_management.rs Phase-6 helpers) |
| R286 | 2 (reconnecting marker + shelley test helper) |
| **TOTAL** | **9 production tech-debt sites + 1 TODO + magic-number wiring** |

After R286, zero production `#[allow(dead_code)]` and zero production
TODO/FIXME annotations remain.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R285 (`docs/operational-runs/2026-05-09-round-285-phase-6-allow-cleanup.md`)
