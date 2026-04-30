## Round 203 — `stake_snapshots.cbor` sidecar persist + load (Phase A.7 final)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Phase A.7 final slice — wire the sync runtime to persist
`StakeSnapshots` (mark/set/go) to a `stake_snapshots.cbor`
sidecar at every checkpoint landing, and extend the LSQ
acquire-time loader to attach the persisted value to the
snapshot via R202's `with_stake_snapshots`.

This completes the read+write path so `cardano-cli conway
query stake-snapshot` will surface live per-pool stake totals
once the chain crosses an epoch boundary (when snapshot
rotation fires).

### Code change

`crates/storage/src/ocert_sidecar.rs`:

- New `STAKE_SNAPSHOTS_FILENAME = "stake_snapshots.cbor"`
  constant.
- New `stake_snapshots_sidecar_path(dir)` private helper.
- New `save_stake_snapshots(dir, encoded)` /
  `load_stake_snapshots(dir)` helpers, mirroring the
  existing OCert + nonce sidecar atomic-write contract.

`crates/storage/src/lib.rs`:

- Re-exports `STAKE_SNAPSHOTS_FILENAME`,
  `save_stake_snapshots`, `load_stake_snapshots`.

`node/src/sync.rs`:

- `update_ledger_checkpoint_after_progress` — at the same
  conditional block that persists the OCert sidecar, also
  persist `tracking.stake_snapshots` if present.  Uses
  `StakeSnapshots::encode_cbor` (already in `crates/ledger`).

`node/src/local_server.rs`:

- `attach_chain_dep_state_from_sidecar` extended:
  - Refactored to mutate `snap` through three independent
    sidecar reads (OCert, Nonce, StakeSnapshots).
  - When `stake_snapshots.cbor` decodes successfully, calls
    `snap.with_stake_snapshots(snapshots)` (R202 builder).
  - Each sidecar is still independently optional; missing
    or undecodeable files are silently skipped.

### Operational verification

After ~30 s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ ls -la /tmp/ygg-r203-preview-db/*.cbor
-rw-r--r-- 1 vscode vscode 114 nonce_state.cbor
-rw-r--r-- 1 vscode vscode   1 ocert_counters.cbor
-rw-r--r-- 1 vscode vscode  18 stake_snapshots.cbor   ← NEW

$ cardano-cli conway query stake-snapshot --testnet-magic 2 --all-stake-pools
{
    "pools": {
        "38f4a58aaf3fec84f3410520c70ad75321fb651ada7ca026373ce486": {
            "stakeGo": 0, "stakeMark": 0, "stakeSet": 0
        },
        ...
    },
    "total": { "stakeGo": 1, "stakeMark": 1, "stakeSet": 1 }
}
```

`stake_snapshots.cbor` is now persisted (18 bytes — three
empty `StakeSnapshot` records + zero `fee_pot` per
`encode_cbor` 4-element envelope).  The encoder picks it up
via `snapshot.stake_snapshots()` and uses the real-data path
(R202).  Per-pool totals are 0 because preview's chain at
slot ~5K hasn't crossed an epoch boundary yet (snapshot
rotation fires on epoch transition), so mark/set/go are all
empty `StakeSnapshot::empty()`.  Total fields show
1-lovelace (the `NonZero Coin` placeholder) because the real
`total_for_snap` returned 0.

When preview crosses its first epoch boundary (slot 86 400 →
epoch 1), `tracking.stake_snapshots` will rotate and the
persisted sidecar will contain real per-credential stake.
The same encoder path will then surface real totals
without further code changes.

Regression checks pass for all other LSQ queries.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Phase A.7 closed

All three consensus-side sidecars now persist + load + attach
end-to-end:

| Sidecar | R-round | Filename | Surfaces in |
|---------|---------|----------|-------------|
| OCert counters | R196/R198 | `ocert_counters.cbor` | `query protocol-state` |
| Nonce evolution | R197/R198 | `nonce_state.cbor` | `query protocol-state` |
| Stake snapshots | R202/R203 | `stake_snapshots.cbor` | `query stake-snapshot` |

All three sidecars persist atomically alongside the ledger
checkpoint and survive node restarts.

### Open follow-ups

1. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis serialiser.
2. **Phase A.3 OMap proposals** — gov-state proposal entries.
3. **Phase C.2** — pipelined fetch+apply.
4. **Phase D.1/D.2** — deep cross-epoch rollback + multi-session
   peer accounting.
5. **Phase E.1 cardano-base** — coordinated fixture refresh.
6. **Phase E.2/E.3** — mainnet rehearsal + parity proof.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`crates/storage/src/ocert_sidecar.rs`](crates/storage/src/ocert_sidecar.rs)
  — new save/load helpers + filename constant;
  [`crates/storage/src/lib.rs`](crates/storage/src/lib.rs) —
  re-exports;
  [`node/src/sync.rs`](node/src/sync.rs) — persist alongside
  OCert sidecar at checkpoint;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  loader refactor + attach via `with_stake_snapshots`.
- Yggdrasil reference:
  `crates/ledger/src/stake.rs::StakeSnapshots` (already had
  CborEncode/CborDecode).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-202-stake-snapshots-infra.md`](2026-04-30-round-202-stake-snapshots-infra.md).
