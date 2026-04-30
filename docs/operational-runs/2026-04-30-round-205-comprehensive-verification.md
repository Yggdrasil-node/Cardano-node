## Round 205 — Comprehensive end-to-end verification (post-Phase A)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Operational verification round (no code changes) — exercise
the cumulative Phase A data-plumbing arc end-to-end on a fresh
preview sync, confirming:

1. All 25 cardano-cli `conway query` subcommands decode
   end-to-end
2. All 3 consensus-side sidecars persist atomically
3. Live nonces (R196/R197/R198) survive node restart
4. Sync resumes from checkpoint (R196 + R203 verified)

### Setup

Fresh preview sync from origin with `YGG_LSQ_ERA_FLOOR=6`,
default config + R201 documentary pins.

### Result 1 — All 25 cardano-cli queries pass

```
OK: tip                       OK: pool-state
OK: protocol-parameters       OK: ref-script-size
OK: era-history               OK: ledger-state
OK: slot-number               OK: protocol-state
OK: utxo-whole                OK: default-vote
OK: tx-mempool-info           OK: stake-snapshot
OK: constitution              OK: stake-distribution
OK: drep-state                OK: stake-pools
OK: drep-stake                OK: ledger-peer-snapshot
OK: committee-state           OK: gov-state
OK: treasury                  OK: future-pparams
OK: spo-stake                 OK: ratify-state
OK: proposals
=== Summary: pass=25 fail=0 ===
```

Includes:
- 6 baseline always-available queries
- 12 Conway-governance queries
- 5 era-gated queries (stake-pools/distribution/snapshot,
  pool-state, ref-script-size)
- 2 operational queries (ledger-state, protocol-state)

### Result 2 — All 3 sidecars persist

After ~30s of sync at slot ~5K:

```
$ ls -la /tmp/ygg-r205-preview-db/*.cbor
114 nonce_state.cbor          (R197/R198)
218 ocert_counters.cbor       (R196/R198)
 18 stake_snapshots.cbor      (R202/R203)
```

After ~60s of sync at slot ~10K:

```
immutable_files=117
ledger_snapshots=4
```

### Result 3 — Live nonces survive restart

Stop node at slot ~10K, restart with same DB:

```
[Node.Recovery]
  recovered ledger state from coordinated storage
  checkpointSlot=9960
  point=BlockPoint(SlotNo(10960), HeaderHash(c6dfa20907819b0c...))
  replayedVolatileBlocks=50
```

After restart, `cardano-cli conway query protocol-state`:

```
{
    "candidateNonce": "509aed8ad40c83c7201fd99c84501c698137a7152127e2ebe1bb9fe70a39077c",
    "epochNonce": null,
    "evolvingNonce": "509aed8ad40c83c7201fd99c84501c698137a7152127e2ebe1bb9fe70a39077c",
    "labNonce": "0e45467482b969fd4a2f50031bda686935efad281be1b714e408afe7f3eb523a",
    "lastEpochBlockNonce": null,
    "lastSlot": 11940,
    "oCertCounters": {}
}
```

**Live nonces (`candidateNonce`, `evolvingNonce`, `labNonce`)
persist across restart** via the `nonce_state.cbor` sidecar.
`epochNonce` and `lastEpochBlockNonce` remain `null` because
preview is still in epoch 0 (no epoch transition fired);
correct live behavior.

Tip advances naturally from slot 10960 to 11940 after restart
— sync resumes from checkpoint, not origin.

### Cumulative Phase A status

| Phase | Round(s) | Status |
|-------|---------|--------|
| A.1 (`ChainDepStateContext` infra) | R192 | ✅ closed |
| A.2 (live PraosState) | R196+R197+R198 | ✅ closed |
| A.3 (live `GovRelation` + OMap shape) | R193+R204 | ✅ closed |
| A.4 (DRep/SPO stake + deposits) | R194 | ✅ closed |
| A.5 (ledger-peer-snapshot pools) | R195 | ✅ closed |
| A.7 (live stake-snapshots) | R202+R203 | ✅ closed |

**Six of seven Phase A items closed.**  Only A.6
(GetGenesisConfig ShelleyGenesis serialiser) remains, and it
has no direct cardano-cli subcommand consumer (the LSQ
dispatcher returns `null_response` placeholder; cardano-cli's
`leadership-schedule` and `kes-period-info` use it internally
but fail at client-side arg validation per R190).

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

1. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis
   serialiser (16-field upstream record; deferred — no
   user-facing cardano-cli subcommand currently exercises
   it).
2. **Phase C.2** — pipelined fetch+apply.
3. **Phase D.1** — deep cross-epoch rollback.
4. **Phase D.2** — multi-session peer accounting.
5. **Phase E.1 cardano-base** — coordinated fixture refresh.
6. **Phase E.2** — mainnet rehearsal (24h+).
7. **Phase E.3** — parity proof report.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Captures: `/tmp/ygg-r205-preview.log`,
  `/tmp/ygg-r205-restart.log`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-204-gov-action-state-shape-adapter.md`](2026-04-30-round-204-gov-action-state-shape-adapter.md).
