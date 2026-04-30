## Round 191 — Live tip-slot plumbing into `protocol-state` + `ledger-peer-snapshot`

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Begin the post-R190 data-plumbing arc: replace static `Origin`
placeholders with live values from yggdrasil's snapshot.  This
round wires `LedgerStateSnapshot::tip().slot()` into the two
LSQ responses where slot information was emitted as a static
`Origin` (`[0]` CBOR singleton) regardless of chain progress.

### Code change

`node/src/local_server.rs`:

- `encode_praos_state_versioned(snapshot: &LedgerStateSnapshot)`
  now takes the snapshot and emits `praosStateLastSlot` as
  `WithOrigin SlotNo` derived from `snapshot.tip().slot()`:
  `Some(slot)` → `[1, slot]`, `None` → `[0]` (Origin only at
  pre-genesis).
- `GetLedgerPeerSnapshot` dispatcher's V2 wire-shape
  `WithOrigin SlotNo` field updated identically to use the
  live tip slot.
- Both call-sites in `dispatch_upstream_query` and
  `dispatch_inner_era_query` updated.

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6` (chain
at slot 3960):

```
$ cardano-cli query tip --testnet-magic 2 | grep slot
    "slot": 3960,
    "slotInEpoch": 3960,
    "slotsToEpochEnd": 82440,

$ cardano-cli conway query ledger-peer-snapshot --testnet-magic 2
{
    "bigLedgerPools": [],
    "slotNo": 3960,
    "version": 2
}

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

Both queries now reflect the live tip slot (3960) instead of
the static `"origin"` placeholder.  `lastSlot` and `slotNo`
update naturally as the chain advances.

Regression checks pass (every other query continues to work):

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 3960, "epoch": 0, "era": "Conway", ... }

$ cardano-cli conway query gov-state --testnet-magic 2
{ "committee": null, ... }

$ cardano-cli conway query ratify-state --testnet-magic 2
{ "enactedGovActions": [], ... }
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

The live-data plumbing arc continues.  The remaining
placeholders inside R190's PraosState response (OCert counters
+ 6 nonces) require runtime data from the consensus layer's
`NonceEvolutionState` and `OcertCounters`, which are tracked
separately from `LedgerState`.  Threading them into
`LedgerStateSnapshot` requires either:

1. Adding `nonce_state: Option<NonceEvolutionState>` and
   `opcert_counters: Option<OcertCounters>` fields to
   `LedgerStateSnapshot` plus modifying every `snapshot()`
   call site to populate them, or
2. Building a separate `ChainDepStateContext` that the
   dispatcher receives alongside the ledger snapshot.

Tracked as R192+ work (live nonces + ocert counters in
protocol-state).

Other open items (unchanged):

1. Live data plumbing — gov-state proposals, ratify-state
   enacted, drep stake distribution, spo stake distribution,
   ledger-peer-snapshot pool list.  The data tracking lives
   in yggdrasil's runtime; encoders need wiring once the
   snapshot exposes the data in a form matching upstream's
   wire shape.
2. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
3. Apply-batch duration histogram (R169).
4. Multi-session peer accounting (R168 structural).
5. Pipelined fetch + apply (R166).
6. Deep cross-epoch rollback recovery (R167).

### References

- Code:
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  `encode_praos_state_versioned` now takes `&LedgerStateSnapshot`
  and reads tip slot; `GetLedgerPeerSnapshot` dispatcher arm
  same plumbing.
- Captures: `/tmp/ygg-r191-preview.log`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-190-comprehensive-audit.md`](2026-04-30-round-190-comprehensive-audit.md).
