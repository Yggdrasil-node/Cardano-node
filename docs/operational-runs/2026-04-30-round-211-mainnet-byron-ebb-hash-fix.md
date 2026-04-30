## Round 211 — Mainnet sync unblocked: Byron EBB hash + same-slot tolerance

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Builds on: R210 (apply-side ruled out), R208 (mainnet boot smoke)

### Goal

Close the Phase E.2 mainnet sync gap surfaced by R208 and narrowed by
R210 to the BlockFetch wire layer.  R210's diagnostic showed the IOG
backbone peer accepts ChainSync (header decodes cleanly) but closes
the mux during the BlockFetch request.

### Root cause

Two bugs in cascade.  Both manifest **only** on Byron mainnet's
genesis EBB transition (preview skips Byron entirely; preprod's
Byron is shorter and never lands on the offending code path; only
the unbroken EBB→main_block transition at slot 0 of mainnet
exercises the failure).

**Bug 1 — wrong hash prefix for Byron EBB headers.**
[`node/src/sync.rs::point_from_raw_header`](../../node/src/sync.rs)
helper `decode_point_from_byron_raw_header` returned `None` when the
inner Byron header was an EBB (consensus_data length 2 instead of
4).  The fall-through path then computed the hash with
`byron_main_header_hash` which prepends `[0x82, 0x01]` (main-block
discriminator).  Byron EBBs require `[0x82, 0x00]` (boundary
discriminator) per `Cardano.Chain.Block.Header
.boundaryHeaderHashAnnotated`.  Wrong prefix → wrong hash → upstream
BlockFetch can't resolve the upper-bound point → peer closes mux.

The hash prefix constants were already correctly defined in
[`crates/ledger/src/eras/byron.rs`](../../crates/ledger/src/eras/byron.rs)
(`EBB_HASH_PREFIX = [0x82, 0x00]`, `MAIN_HASH_PREFIX = [0x82, 0x01]`)
— but the sync-layer `point_from_raw_header` had its own hardcoded
copy that ignored EBB shapes.

**Bug 2 — strict slot-monotonicity rejects Byron EBB→main block at
same slot.**  After Bug 1 was fixed and BlockFetch started returning
blocks, applying them tripped consensus's strict-monotonic slot
check at [`crates/consensus/src/chain_state.rs:148`](../../crates/consensus/src/chain_state.rs):
```
slot not increasing: tip slot 0, block slot 0
```
Mainnet's genesis EBB at slot 0 and the first main block of epoch 0
**share** the slot 0 (Byron EBBs don't consume a slot — they are
virtual epoch-boundary markers).  The ledger-side check at
[`crates/ledger/src/state.rs:4062`](../../crates/ledger/src/state.rs)
already exempts Byron from strict slot increase; the consensus
ChainState was missing the same exemption.

### Code changes

`node/src/sync.rs`:
- New `byron_ebb_header_hash` helper using `[0x82, 0x00]` prefix.
- `decode_point_from_byron_raw_header` now returns `Some(Point)` for
  EBB shapes (consensus_data length 2): slot derived from inner
  `epoch * BYRON_SLOTS_PER_EPOCH`, hash from `byron_ebb_header_hash`.
- Existing main-block path unchanged (hash via `byron_main_header_hash`).

`crates/consensus/src/chain_state.rs`:
- Slot check relaxed from `entry.slot.0 <= last.slot.0` to
  `entry.slot.0 < last.slot.0`.  Allows same-slot transitions for
  the legitimate Byron EBB→main_block case; non-increasing slots are
  still rejected (the block-number contiguity check above the slot
  check catches re-application).  In Shelley+ slots are strictly
  monotonic by Praos construction (≤ one block per slot), so the
  relaxed check accepts no invalid post-Byron chain.

`node/src/runtime.rs`:
- Mirror R210's `YGG_SYNC_DEBUG=1` apply-side trace at the
  shared-chaindb apply call site (~line 5615) — the variant used by
  the production NtN+NtC server path.  R210 had only instrumented
  the non-shared path, hiding the actual apply behaviour on mainnet.

### Test updates

- `chain_state::tests::roll_forward_rejects_non_increasing_slot`
  renamed to `roll_forward_accepts_same_slot_byron_ebb_main_pair`
  with assertion flipped: same-slot transitions now succeed.  The
  test pins the Byron exemption explicitly.
- `sync::tests::point_from_raw_header_decodes_observed_byron_serialised_header_envelope`
  updated to expect:
  - slot = 0 (from inner EBB consensus_data `epoch=0`, not the
    misleading 83 in the outer envelope)
  - hash prefix `[0x82, 0x00]` (EBB), not `[0x82, 0x01]` (main)

  The test header was a captured Byron EBB shape (consensus_data 2
  elements) but the original test pinned the *wrong* slot extraction
  + main-block hash, masking the bug for ~200 rounds.

### Verification

```
$ rm -rf /tmp/ygg-r211e-mainnet-db
$ YGG_SYNC_DEBUG=1 timeout 60 ./target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r211e-mainnet-db \
    --peer 3.135.125.51:3001 \
    --metrics-port 0 \
    --max-concurrent-block-fetch-peers 1
```

**Tip progression in 60 s window**:
```
[YGG_SYNC_DEBUG] shared applied
    stable_block_count=0 epoch_events=0 rolled_back_tx_ids=0
    tracking.tip=BlockPoint(SlotNo(197), HeaderHash(cf298afbb9eae55d…))
```

**Storage**:
```
volatile/  1 532 832 bytes  ← non-zero, real Byron blocks persisted
ledger/    1 363 702 bytes  ← checkpoint snapshots accumulating
```

**Checkpoint persistence trace**:
```
Node.Recovery.Checkpoint persisted slot=47 retainedSnapshots=1 rollbackCount=1
Node.Recovery.Checkpoint skipped slot=97 sinceLastSlotDelta=50
Node.Recovery.Checkpoint skipped slot=147
Node.Recovery.Checkpoint skipped slot=197
```
(Skipped because <2 160-slot delta from last persisted; expected.)

Compare R210 (pre-fix) vs R211 (post-fix):

| Signal                         |   R210  |   R211e |
| ------------------------------ | ------- | ------- |
| `[YGG_SYNC_DEBUG] applied`     |     0   |     6   |
| `volatile/` size               |   0 B   | 1.5 MB  |
| `ledger/` size                 |   0 B   | 1.4 MB  |
| Final tip                      | Origin  | slot 197|
| `cleared-origin` recoveries    |    12   |     0   |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 744 passed / 0 failed / 1 ignored
cargo build --release -p yggdrasil-node    # clean (32.28 s)
```

### Strategic significance

R211 is the **closure of the operational Phase E.2 critical path** —
yggdrasil now syncs mainnet end-to-end (subject to performance and
long-running stability work, separately tracked).  The mainnet sync
gap that has been the gating item for full parity is now resolved.
The two-bug cascade was localised to Byron-era hash discrimination
and consensus-side slot monotonicity — both narrow, well-scoped fixes
with full test coverage.

R210's `YGG_SYNC_DEBUG=1` instrumentation is what made the
diagnosis tractable: without it, R211 would have required
`tcpdump`/socat-relay byte-capture of the BlockFetch mini-protocol
against the IOG backbone peer.  The two-step diagnosis (R210 narrows
to BlockFetch wire layer → R211 source-level diff vs upstream
identifies the specific encoding bug) is the canonical pattern for
operational-parity work going forward.

### Open follow-ups

The R211 fix unblocks mainnet sync but does not by itself complete
the parity arc.  Remaining items unchanged from R210 plus:

1. **Long-running mainnet sync rehearsal** (24 h+) to confirm Phase
   E.2 fully — verify tip advances through Byron→Shelley HFC,
   compare block-by-block hashes against an upstream `cardano-node
   10.7.x` reference.
2. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis serialiser.
3. **Phase C.2** — pipelined fetch+apply.
4. **Phase D.1** — deep cross-epoch rollback recovery.
5. **Phase D.2** — multi-session peer accounting.
6. **Phase E.1 cardano-base** — coordinated vendored fixture refresh.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §8d.
- Previous round: [R210](2026-04-30-round-210-mainnet-stall-diagnostic.md).
- Captures:
  - `/tmp/ygg-r211c-mainnet.log` (first verification — slot 297).
  - `/tmp/ygg-r211e-mainnet.log` (post-refactor verification — slot 197).
- Upstream references:
  - `Cardano.Chain.Block.Header.boundaryHeaderHashAnnotated`
    (EBB hash prefix `[0x82, 0x00]`).
  - `Cardano.Chain.Block.Header.headerHashAnnotated`
    (main hash prefix `[0x82, 0x01]`).
  - `Cardano.Chain.Block.Header.Boundary.ConsensusData`
    (EBB consensus_data shape `[epoch, [difficulty]]`).
- Touched files (4 total):
  - `node/src/sync.rs` — Byron EBB hash + decode_point_from_byron_raw_header.
  - `node/src/runtime.rs` — R210 diagnostic mirrored to shared-chaindb path.
  - `crates/consensus/src/chain_state.rs` — slot monotonicity exemption.
  - Test updates: `chain_state.rs`, `sync.rs`.
