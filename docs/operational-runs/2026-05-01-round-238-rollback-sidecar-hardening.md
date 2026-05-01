# Round 238 - Rollback ChainDepState Sidecar Parity Hardening

Date: 2026-05-01

## Summary

R238 closes the code-level Phase D.1 rollback sidecar hardening gap. The node now keeps slot-indexed chain-dependency state snapshots under `chain_dep_state/<slot-hex>.cbor`; this bundle is the canonical durable nonce/OpCert ChainDepState source for rollback, restart, and LSQ attachment.

The sidecar bundle is node-owned deterministic CBOR:

```text
[version=1, point, nonce_state|null, ocert_counters|null]
```

Verified sync writes the bundle only when a ledger checkpoint lands, after nonce evolution and OpCert counter updates for the same accepted blocks. On rollback, recovery loads the newest sidecar at or before the rollback point, verifies that the bundled point is on the selected chain prefix, restores nonce and OpCert state, and replays stored raw blocks from that sidecar point to the rollback target. Persistent non-origin rollback fails closed when exact ChainDepState history is unavailable; non-persistent in-memory configurations can only reset to the origin/no-state baseline.

## Upstream alignment

- [Ouroboros Consensus LedgerDB/openDB restore model](https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/ouroboros-consensus/Ouroboros-Consensus-Storage-LedgerDB.html): restore a persisted snapshot and replay blocks to the requested point.
- [Caught-up node storage model](https://ouroboros-consensus.cardano.intersectmbo.org/docs/explanations/node_tasks/): LedgerDB holds ledger states for volatile-chain points and validates forks against those points.
- [UTxO-HD rollback/snapshot design](https://ouroboros-consensus.cardano.intersectmbo.org/docs/references/miscellaneous/utxo-hd/utxo-hd_in_depth/): persisted anchors and volatile differences are replayed to construct the needed state without changing ledger snapshot CBOR.

R238 follows that shape without modifying yggdrasil's ledger checkpoint format: the chain-dependency state is stored in a separate opaque sidecar history and replayed in lockstep with the block store.

## Code changes

- `crates/storage/src/ocert_sidecar.rs`
  - Added slot-indexed sidecar helpers:
    - `save_chain_dep_state_snapshot`
    - `load_latest_chain_dep_state_snapshot_before_or_at`
    - `truncate_chain_dep_state_snapshots_after`
    - `retain_latest_chain_dep_state_snapshots`
  - Kept storage byte helpers opaque, with slot-indexed ChainDepState snapshots as the canonical nonce/OpCert history.

- `node/src/sync.rs`
  - Added private typed encode/decode helpers for the R238 bundle.
  - Persisted ChainDepState bundles at the same checkpoint cadence as ledger snapshots after nonce/OpCert updates.
  - Made verified sync terminate the current batch immediately on `RollBackward`.
  - Added recovery that restores nonce/OpCert from the sidecar point and replays stored blocks to the rollback target.
  - Kept conservative reset for in-memory or no-sidecar configurations.

- `node/src/local_server.rs`
  - Updated LSQ protocol-state attachment to use only an exact sidecar for the acquired point or tip.

- `node/src/runtime.rs`
  - Wired ChainDepState sidecar restore-and-replay through runtime reconnect and shared ChainDb paths so restart, rollback, and LSQ share the same authoritative state source.

## Verification

Commands run at the R238 slice boundary:

```bash
cargo fmt --all -- --check
cargo test -p yggdrasil-storage sidecar
cargo test -p yggdrasil-node rollback
cargo test -p yggdrasil-node protocol_state_uses_exact_chain_dep_sidecar_and_ignores_latest_mirrors
cargo check-all
cargo test-all
cargo lint
git diff --check
```

Follow-up full-gate rerun after the small rollback helper adjustment:

```bash
cargo fmt --all -- --check
cargo test -p yggdrasil-node rollback
cargo lint
cargo test-all
git diff --check
cargo check-all
```

All commands passed.

## Documentation updates

The living status docs now treat Phase D.1 code-level rollback sidecar hardening as closed:

- `README.md`
- `AGENTS.md`
- `CLAUDE.md`
- `.github/CLAUDE.md`
- `docs/ARCHITECTURE.md`
- `docs/UPSTREAM_PARITY.md`
- `docs/PARITY_PLAN.md`
- `docs/PARITY_SUMMARY.md`
- `docs/PARITY_PROOF.md`
- `docs/MANUAL_TEST_RUNBOOK.md`

Older `docs/operational-runs/*.md` files remain historical evidence snapshots, so earlier "open follow-up" wording is intentionally left intact in those dated records.
