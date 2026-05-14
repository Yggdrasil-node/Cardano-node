## Round 202 — `stake_snapshots` snapshot infrastructure (Phase A.7 partial)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Phase A.7 first slice — extend `LedgerStateSnapshot` with an
optional `stake_snapshots: Option<StakeSnapshots>` companion
field (mirroring R192's `chain_dep_state` pattern) and update
`encode_stake_snapshots` to use real per-pool stake totals
when the runtime attaches the active mark/set/go rotation.

Same R196/R197 read-first-write-later pattern: ship the
read-side now (encoder reads from snapshot if present); the
runtime-attach call site is deferred to a follow-up round.

### Code change

`crates/ledger/src/state.rs`:

- New optional `stake_snapshots: Option<StakeSnapshots>` field
  on `LedgerStateSnapshot`.
- New `with_stake_snapshots(snapshots)` builder method
  mirroring `with_chain_dep_state`.
- New `stake_snapshots()` accessor returning
  `Option<&StakeSnapshots>`.
- `LedgerState::snapshot()` defaults the field to `None`; the
  runtime opts in via builder.

`node/src/local_server.rs`:

- `encode_stake_snapshots` now branches on
  `snapshot.stake_snapshots()`:
  - **When attached**: emits real per-pool [mark, set, go]
    totals computed by summing each credential's stake into
    the credential's `delegated_pool` per snapshot
    generation, plus accurate `ssStake{Mark,Set,Go}Total`
    (saturating-add over `IndividualStake::iter()`).
  - **When absent**: falls back to R163/R179 placeholder
    behavior (zero per-pool, 1-lovelace `NonZero Coin` totals
    per cardano-cli's decoder requirement).

### Operational verification

After ~30 s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query stake-snapshot --testnet-magic 2 --all-stake-pools
{
    "pools": {
        "38f4a58aaf3fec84f3410520c70ad75321fb651ada7ca026373ce486": {
            "stakeGo": 0, "stakeMark": 0, "stakeSet": 0
        },
        "40d806d73c8d2a0c8d9b1e95ccb9f380e40cb4d4b23ff6e403ae1456": {
            "stakeGo": 0, "stakeMark": 0, "stakeSet": 0
        },
        "d5cfc42cf67f6b637688d19fa50a4342658f63370b9e2c9e3eaf4dfe": {
            "stakeGo": 0, "stakeMark": 0, "stakeSet": 0
        }
    },
    "total": { "stakeGo": 1, "stakeMark": 1, "stakeSet": 1 }
}
```

Output matches the previous R163/R179 behavior because no
runtime has yet attached real `StakeSnapshots` to the snapshot
(the `stake_snapshots()` accessor returns `None` and the
encoder falls back to placeholders).  Once the runtime is
wired (Phase A.7 follow-up at `update_ledger_checkpoint_after_progress`
where `stake_snapshots` is already tracked in
`LedgerCheckpointTracking`), the same encoder will surface
real per-pool stake totals automatically.

Regression checks pass for all other LSQ queries.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

1. **Phase A.7 next** — runtime attach: at the same checkpoint
   landing site that persists `nonce_state.cbor` and
   `ocert_counters.cbor`, also call
   `snapshot = snapshot.with_stake_snapshots(...)` from
   `tracking.stake_snapshots.clone()` if present.
2. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis
   serialiser.
3. **Phase A.3 OMap proposals** — gov-state proposal entries.
4. **Phase C.2** — pipelined fetch+apply.
5. **Phase D.1/D.2** — deep rollback + multi-session peer
   accounting.
6. **Phase E.1 cardano-base** — coordinated fixture refresh.
7. **Phase E.2/E.3** — mainnet rehearsal + parity proof.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`crates/ledger/src/state.rs`](crates/ledger/src/state.rs)
  — new `stake_snapshots` field, builder, accessor;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  `encode_stake_snapshots` extended with real-data path.
- Upstream reference:
  `Cardano.Ledger.Shelley.LedgerStateQuery.GetStakeSnapshots`;
  `Cardano.Ledger.Shelley.LedgerState.SnapShots`.
- Yggdrasil reference: `crates/ledger/src/stake.rs` —
  `StakeSnapshots` (mark/set/go), `StakeSnapshot`
  (stake/delegations/pool_params).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-201-pin-refresh.md`](2026-04-30-round-201-pin-refresh.md).
