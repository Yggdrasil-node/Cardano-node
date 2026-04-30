## Round 215 — Multi-network regression verify post-R211/R212/R213/R214

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Type: Operational verification, no code changes

### Goal

R211–R214 made substantial changes touching the consensus chain
state (slot monotonicity), Byron header decoding (EBB hash prefix),
mux egress semantics (back-pressure on accumulated bytes only), and
the LSQ dispatcher (new `genesis_config_cbor` field).  R215 confirms
none of these changes regressed the testnet operational surface
verified by R205 (preview Conway) and R207 (preprod Allegra).

### Test setup

For each network: fresh database, fresh socket, default config.
Allow 30–35 s of sync, then run a representative slice of the
cardano-cli LSQ surface.

### Preview (Alonzo / Conway with `YGG_LSQ_ERA_FLOOR=6`)

**Server-side `YGG_LSQ_ERA_FLOOR=6`** unblocks Conway-era
client-side gating.

Tip:
```json
{
  "block": 7960,
  "epoch": 0,
  "era": "Conway",  ← R160 PV-aware promotion + R178 era-floor
  "hash": "2a62c900c91292ef142256922778f78714386ba1527ae24b40eb83c50ceb00a9",
  "slot": 7960,
  "slotInEpoch": 7960,
  "slotsToEpochEnd": 78440,
  "syncProgress": "0.01"
}
```

Conway gov-state:
```json
{
  "committee": null,
  "constitution": {
    "anchor": {
      "dataHash": "ca41a91f399259bcefe57f9858e91f6d00e1a38d6d9c63d4052914ea7bd70cb2",
      "url": "ipfs://bafkreifnwj6zpu3ixa4siz2lndqybyc5wnnt3jkwyutci4e2tmbnj3xrdm"
    },
    "script": "fa24fb305126805cf2164c161d852a0e7330cf988f1fe558cf7d4a64"
  },
  ...
}
```

Conway constitution:
```json
{
  "anchor": {
    "dataHash": "ca41a91f399259bcefe57f9858e91f6d00e1a38d6d9c63d4052914ea7bd70cb2",
    "url": "ipfs://bafkreifnwj6zpu3ixa4siz2lndqybyc5wnnt3jkwyutci4e2tmbnj3xrdm"
  },
  "script": "fa24fb305126805cf2164c161d852a0e7330cf988f1fe558cf7d4a64"
}
```

R214 startup trace:
```
Net.NtC starting NtC local server
  genesisConfigCborBytes=821
  socketPath=/tmp/ygg-r215-preview2.sock
```

Sidecars (post-test):
```
nonce_state.cbor       114 B
ocert_counters.cbor    218 B
stake_snapshots.cbor    18 B
```

### Preprod (Allegra)

Tip:
```json
{
  "block": 91440,
  "epoch": 4,
  "era": "Allegra",
  "hash": "a37243cffd64c7be8e19b31ceccf26ed92c3ec802478613672b0f75f33964227",
  "slot": 91440,
  "slotInEpoch": 5040,
  "slotsToEpochEnd": 426960,
  "syncProgress": "1.40"
}
```

Protocol parameters: 17-element Shelley shape returns full JSON.

Tx-mempool info:
```json
{
  "capacityInBytes": 0,
  "numberOfTxs": 0,
  "sizeInBytes": 0,
  "slot": 91440
}
```

R214 startup trace: `genesisConfigCborBytes=821`.

Sidecars (post-test): same shape as preview (114 B + 218 B + 18 B).

### Mainnet (Byron+) — R212/R213/R214 verified

Already documented in R212/R213/R214; not re-run here.

### Cumulative multi-network parity matrix (post-R215)

| Network          | Era at slot~few-K | tip | era-hist | proto-params | utxo (whole) | tx-mempool | gov-state | sidecars | genesis-config | Round    |
| ---------------- | ----------------- | :-: | :------: | :----------: | :----------: | :--------: | :-------: | :------: | :------------: | -------- |
| Preview          | Conway*           |  ✅ |    ✅    |      ✅      |      ✅      |     ✅     |    ✅     |    ✅    |       ✅       | R205+R215 |
| Preprod          | Allegra           |  ✅ |    ✅    |      ✅      |      ✅      |     ✅     |     —     |    ✅    |       ✅       | R207+R215 |
| **Mainnet**      | Byron→Shelley*    |  ✅ |    ✅    |      ✅      |   ✅ (1.3MB) |     ✅     |     —     |    ✅    |       ✅       | R211–214 |

*Era reflects yggdrasil's PV-aware classification (R160) at the
applied tip, with optional `YGG_LSQ_ERA_FLOOR` override (R178).

### Verification gates (no code change in R215)

```
cargo fmt --all -- --check       # clean (R214 baseline preserved)
cargo lint                       # clean
cargo test-all                   # 4 745 passed / 0 failed / 1 ignored
cargo build --release            # clean
```

### Strategic significance

R215 closes the multi-network regression-confirmation gap for the
R211–R214 arc.  All three official Cardano networks demonstrate:

- Operational sync (R211 mainnet sync, testnets unaffected).
- Full LSQ surface decode end-to-end.
- All 3 consensus-side sidecars persisting atomically.
- R214's pre-encoded genesis config bytes available to the dispatcher.
- Heavyweight queries flow cleanly through the mux (R213 fix on
  mainnet; testnet UTxO sets always fit so the back-pressure
  semantic isn't exercised but the test confirms no regression).

### Open follow-ups (unchanged from R214)

1. Long-running mainnet sync rehearsal (24 h+) — verify Byron→Shelley
   HFC at slot 4 492 800.
2. Phase C.2 — pipelined fetch+apply (sync rate currently ~3.3 slot/s
   on mainnet).
3. Phase D.1 — deep cross-epoch rollback recovery.
4. Phase D.2 — multi-session peer accounting.
5. Phase E.1 cardano-base — coordinated vendored fixture refresh.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §6.
- Previous round: [R214](2026-04-30-round-214-getgenesisconfig-encoder.md).
- Captures: `/tmp/ygg-r215-{preview,preview2,preprod}.log`.
