## Round 192 — `ChainDepStateContext` snapshot infrastructure (Phase A.1)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Lay the foundation for live PraosState data in `query
protocol-state` (and any future consensus-side LSQ data) by
adding a companion struct that the runtime can attach to
`LedgerStateSnapshot` after construction.  This is **Phase A.1**
of the post-R191 data-plumbing arc per
`/home/vscode/.claude/plans/clever-shimmying-quokka.md`.

Subsequent rounds can plug live `NonceEvolutionState` and
`OcertCounters` from the consensus runtime into this context
without further snapshot-shape changes.

### Code change

`crates/ledger/src/state.rs`:

- New `ChainDepStateContext` struct holding:
  - 6 `Nonce` fields mirroring upstream
    `Ouroboros.Consensus.Protocol.Praos.PraosState`:
    `evolving_nonce`, `candidate_nonce`, `epoch_nonce`,
    `previous_epoch_nonce`, `lab_nonce`,
    `last_epoch_block_nonce`.
  - `opcert_counters: BTreeMap<[u8; 28], u64>` keyed by 28-byte
    cold-key hash.
- `Default` impl emits all-neutral nonces + empty counters so
  callers that don't yet populate the context get an
  upstream-correct neutral placeholder.
- New `chain_dep_state: Option<ChainDepStateContext>` field on
  `LedgerStateSnapshot`.  Defaults to `None` in
  `LedgerState::snapshot()`; the runtime opts in via
  `LedgerStateSnapshot::with_chain_dep_state(...)`.
- `LedgerStateSnapshot::chain_dep_state()` accessor returns
  `Option<&ChainDepStateContext>`.
- Re-exported `ChainDepStateContext` from `yggdrasil_ledger`
  crate root.

`node/src/local_server.rs`:

- `encode_praos_state_versioned` now branches on
  `snapshot.chain_dep_state()`:
  - When `Some(ctx)`: emit live OCert counters map + 6 nonces
    using `Nonce::Neutral → [0]`, `Nonce::Hash(h) → [1, h]`
    per upstream `Cardano.Ledger.Crypto.Nonce`.
  - When `None`: fall back to empty map + 6 NeutralNonces (the
    R190 placeholder behavior).

### Why this design

`crates/ledger` is below `crates/consensus` in the dependency
graph, so it cannot import `NonceEvolutionState` /
`OcertCounters` directly.  The mirror struct
`ChainDepStateContext` lives in `crates/ledger` so
`LedgerStateSnapshot` carries it natively; the consensus
runtime translates from its native types into this snapshot
mirror at attach time.  The `Option` wrapper keeps the change
backward-compatible — existing callers continue to work without
touching the runtime plumbing.

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query protocol-state --testnet-magic 2
{
    "candidateNonce": null,
    "epochNonce": null,
    "evolvingNonce": null,
    "labNonce": null,
    "lastEpochBlockNonce": null,
    "lastSlot": 3960,
    "oCertCounters": {}
}
```

Identical to the R191 output — confirms the neutral fallback
path is regression-free.  Once the runtime starts attaching a
populated `ChainDepStateContext`, the same query will surface
live nonces + OCert counters with no further encoder changes.

Regression checks pass for every other query:

```
$ cardano-cli conway query gov-state --testnet-magic 2
{ "committee": null, ... }

$ cardano-cli conway query ratify-state --testnet-magic 2
{ "enactedGovActions": [], ... }

$ cardano-cli conway query ledger-peer-snapshot --testnet-magic 2
{ "bigLedgerPools": [], "slotNo": 3960, ... }
```

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count stable at 4744.

### Open follow-ups

The infrastructure is in place; remaining slices of the
data-plumbing arc:

1. **Phase A.2 — Runtime attach** (next round): plumb the
   active `Arc<RwLock<NonceEvolutionState>>` and
   `Arc<RwLock<OcertCounters>>` from sync.rs/runtime.rs through
   to the dispatcher path in `local_server.rs::recover_snapshot_*`,
   translate into `ChainDepStateContext`, and call
   `snapshot.with_chain_dep_state(ctx)`.  Once landed,
   `query protocol-state` will surface live nonces.
2. **Phase A.3 — gov-state proposals + GovRelation**: wire real
   `governance_actions()` and `enact_state().enacted_root(...)`
   into `encode_conway_gov_state_for_lsq`.
3. **Phase A.4 — ratify-state enacted/expired/delayed**: pull
   from runtime ratification pipeline.
4. **Phase A.5 — drep+spo stake distributions**: wire from
   `DrepState::stake_distribution()` + pool-stake snapshots.
5. **Phase A.6 — ledger-peer-snapshot pool list**: wire from
   peer governor's big-ledger ranking.
6. **Phase A.7 — `GetGenesisConfig`**: ShelleyGenesis serialiser.
7. Phase B — R91 multi-peer dispatch storage livelock.
8. Phase C — R169 apply-batch histogram + R166 pipelined apply.
9. Phase D — R167 deep cross-epoch rollback + R168 multi-session
   peer accounting.
10. Phase E — pin refresh + mainnet rehearsal + parity proof.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`crates/ledger/src/state.rs`](crates/ledger/src/state.rs) —
  new `ChainDepStateContext` struct + optional snapshot field +
  builder/accessor methods;
  [`crates/ledger/src/lib.rs`](crates/ledger/src/lib.rs) —
  re-export;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  `encode_praos_state_versioned` branches on context presence.
- Upstream reference:
  `Ouroboros.Consensus.Protocol.Praos.PraosState`;
  `Cardano.Ledger.Crypto.Nonce`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-191-live-tip-slot-plumbing.md`](2026-04-30-round-191-live-tip-slot-plumbing.md).
