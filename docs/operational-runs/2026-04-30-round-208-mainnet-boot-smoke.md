## Round 208 — Mainnet boot smoke test

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Quick (2-minute) smoke test of `--network mainnet` boot path
to validate the cardano-cli LSQ + NtC + storage layers work on
mainnet codepath. **Not** a full Phase E.2 mainnet rehearsal;
intended to surface any obvious mainnet-specific gaps quickly.

### Setup

```
$ rm -rf /tmp/ygg-r208-mainnet-db /tmp/ygg-r208-mainnet.sock
$ /workspaces/Cardano-node/target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r208-mainnet-db \
    --socket-path /tmp/ygg-r208-mainnet.sock \
    --metrics-port 12408 &
```

### What worked

- **Process boots cleanly** — no panic, no startup error.
- **NtC server starts** — `/tmp/ygg-r208-mainnet.sock` socket
  binds successfully.
- **Peer connection establishes** — `Net.ConnectionManager.Remote
  verified sync session established peer=18.221.168.221:3001
  reconnectCount=0`.
- **`cardano-cli query tip --mainnet`** returns valid JSON:

  ```json
  {
      "epoch": 0,
      "era": "Byron",
      "slotInEpoch": 0,
      "slotsToEpochEnd": 21600,
      "syncProgress": "0.00"
  }
  ```

### What didn't work

- **Block fetch + apply does NOT advance past Origin** in the
  2-minute window.
- `volatile/` directory is **0 bytes** — no blocks ever
  written to disk.
- Log shows repeated `Node.Recovery.Checkpoint ... action=
  cleared-origin` events, suggesting the verified-sync session
  is repeatedly resetting to Origin without making progress.
- Sidecars (`nonce_state.cbor`, `ocert_counters.cbor`,
  `stake_snapshots.cbor`) are absent (no checkpoint ever
  landed since no blocks applied).

### Hypothesis

This is a real operational gap distinct from the testnet
parity surface. Likely culprits:

1. **Byron-era ChainSync/BlockFetch shape mismatch** —
   preview's `Test*HardForkAtEpoch=0` config skips Byron
   entirely; preprod has only ~80 K Byron blocks before
   Shelley starts at slot 86 400; mainnet's first ~17 M
   blocks are Byron.  Yggdrasil's Byron handling is
   exercised on preprod but mainnet's ancient first blocks
   may have shape variations (e.g. genesis-delegation
   bootstrap blocks that differ from preprod).
2. **Bootstrap peer behavior** — the configured peer
   `18.221.168.221:3001` accepts the verified-sync session
   but might not be serving blocks in the expected shape.
3. **Block apply pipeline stall** — verified sync may be
   fetching blocks but the apply path may be rejecting them
   silently (no `Error` lines in log other than warm-peer
   keepalive failures from secondary peers).

### Status

**Phase E.2 mainnet rehearsal partial**: confirmed boot path
works on mainnet, but full sync gap diagnosis is **deferred
to a follow-up round** that does deeper diagnostic capture
(BlockFetch wire bytes, apply-path tracing, comparison
against upstream cardano-node 10.7.x on the same bootstrap
peer).

The yggdrasil binary, NtC dispatcher, sidecar persistence,
and LSQ surface are **fully verified on testnets** (preview
R205 + preprod R207).  Mainnet sync at the block-pipeline
layer needs separate investigation.

### Verification gates (no code change)

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

1. **R209+ Phase E.2 diagnosis** — capture mainnet BlockFetch
   wire bytes via socat, identify why blocks aren't fetched
   from the established session, compare against upstream
   cardano-node 10.7.x on the same bootstrap peer.
2. Phase A.6 — `GetGenesisConfig` ShelleyGenesis serialiser.
3. Phase C.2 — pipelined fetch+apply.
4. Phase D.1/D.2 — deep rollback + multi-session peer
   accounting.
5. Phase E.1 cardano-base — coordinated fixture refresh.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status:
  [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §8b
  (mainnet boot smoke test).
- Captures: `/tmp/ygg-r208-mainnet.log`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-207-... TODO`](2026-04-30-round-205-comprehensive-verification.md)
  (R207's preprod verification was documented in PARITY_PROOF
  §8a; R208 is the first standalone operational-run doc since
  R205).
