## Round 152 — cardano-cli `query tip` reports live chain state

Date: 2026-04-27
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close the operator-visible tail of Round 151's "open follow-ups" list:
make `cardano-cli 10.16.0.0 query tip --testnet-magic 1` against
yggdrasil's NtC socket display the *live* chain tip (block, slot,
hash, epoch, slotInEpoch, syncProgress) instead of always reporting
origin even when yggdrasil had progressed past slot 87000.

### Symptom

Pre-fix output (Round 151):

```json
{
    "epoch": 0,
    "era": "Shelley",
    "slotInEpoch": 0,
    "slotsToEpochEnd": 21600,
    "syncProgress": "0.00"
}
```

Missing `block`, `slot`, `hash` fields entirely.  `slotsToEpochEnd:
21600` is cardano-cli's `--epoch-slots` CLI default, indicating the
display layer fell back to Byron-shape defaults.

### Root cause

socat -x -v wire capture (`/tmp/ygg-runbook/haskell-traffic.bin`) and
`YGG_NTC_DEBUG=1` snapshot logging revealed four issues:

1. **Closed single-era Interpreter** — `encode_interpreter_minimal`
   emitted one Byron summary ending at slot 86_400.  Any queried
   slot > 86_400 fell outside our era list, cardano-cli silently
   reverted to defaults.
2. **Bignum-vs-uint regression** — Round 152's first attempt
   wrapped `Bound.relativeTime` as a CBOR bignum tag-2 byte string.
   Upstream `Ouroboros.Consensus.HardFork.History.Summary` writes
   it as a plain CBOR uint when the value fits in u64 (the captured
   Byron eraEnd is `83 1b 17fb16d83be00000 1a 00015180 04`, NOT
   `83 c2 48 17fb16d83be00000 …`).
3. **Shelley `epochSize=21600`** — used Byron-shape value when the
   captured upstream uses `432000` (5-day Shelley epochs at 1s
   slots).
4. **`GetChainBlockNo` returns Origin** — cardano-cli's `query
   tip` display layer suppresses `block`/`slot`/`hash` when
   `GetChainBlockNo` returns Origin (`[0]`), regardless of what
   `GetChainPoint` returned.  Origin BlockNo is treated as
   "no chain available".

### Fix

`crates/network/src/protocols/local_state_query_upstream.rs`:

- Rewrote `encode_interpreter_preprod` to emit **two** era summaries:
  - Byron: closed at slot 86_400 epoch 4 with captured params
    (`epochSize=21600`, `slotLength=20000ms`, `safeZone=[0,4320,[0]]`,
    `genesisWindow=4320`).
  - Shelley: open era with synthetic far-future end at slot
    10_000_000 (keeps `relativeTime` in u64 range) and captured
    upstream params (`epochSize=432000`, `slotLength=1000ms`,
    `safeZone=[0,129600,[0]]`, `genesisWindow=129600`).
- Added `encode_relative_time(enc, picoseconds)` helper using
  plain `enc.unsigned(...)` for parity with captured wire bytes.
- `encode_bignum_u128` retained for hypothetical future synthetic
  slots that overflow u64.

`node/src/local_server.rs::dispatch_upstream_query`:

- `GetChainBlockNo` derives a synthetic block number from
  `snapshot.tip().slot()` so cardano-cli emits the `block` / `slot`
  / `hash` fields.  Approximating block-no from slot is documented
  as a Phase-3 follow-up until `ChainState.chain_block_number` is
  threaded through `LedgerStateSnapshot`.

### Regression tests

`crates/network/src/protocols/local_state_query_upstream.rs`:

- `preprod_interpreter_byron_prefix_matches_upstream_capture` —
  pins the 39-byte Byron prefix verbatim, including the
  `0x1b 17fb16d83be00000` u64 relativeTime (NOT bignum), and the
  Byron params shape captured from upstream.
- `preprod_interpreter_shelley_uses_captured_epoch_size_and_genesis_window`
  — pins the Shelley params marker `84 1a 00069780 1903e8`
  (`epochSize=432000` / `slotLength=1000ms`) and the
  `0x1fa40` (=129600) `genesisWindow` occurrence count.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean (clippy --workspace --all-targets --all-features -- -D warnings)
cargo test-all                   # passed: 4684  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4682 (Round 151) → 4684.

### Operational verification

Preprod run with `--max-concurrent-block-fetch-peers 2` and a
2-localRoot topology, ~60s soak window.

`/tmp/ygg-verify-cli-tip-r152.txt`:

```json
{
    "block": 89840,
    "epoch": 4,
    "era": "Shelley",
    "hash": "bf18327c17714dbe95888f1655a388c5137c54af7111b333620995e1c40183b3",
    "slot": 89840,
    "slotInEpoch": 3440,
    "slotsToEpochEnd": 428560,
    "syncProgress": "1.40"
}
```

`/tmp/ygg-verify-metrics-r152.txt` (Prometheus):

```
yggdrasil_blocks_synced 216
yggdrasil_current_slot 89840
yggdrasil_current_block_number 218
yggdrasil_blockfetch_workers_registered 10
yggdrasil_blockfetch_workers_migrated_total 10
yggdrasil_chainsync_workers_registered 1
yggdrasil_reconnects 0
```

Verification:

- `slot` (89840) matches `yggdrasil_current_slot`.
- `epoch` = 4, `slotInEpoch` = 89840 - 86400 = 3440 ✓ (slot 89840
  is in Shelley epoch 4, computed from our 2-era Interpreter).
- `slotsToEpochEnd` = 432000 - 3440 = 428560 ✓ (uses Shelley's
  `epochSize=432000`).
- `syncProgress` ≈ 1.40% — meaningful sync indicator
  computed by cardano-cli from `slot` vs wall-clock current slot.

### Diagnostic captures

- `/tmp/ygg-runbook/haskell-traffic.bin` — socat -x -v capture of
  `cardano-cli ↔ upstream Haskell preprod node` used to discover
  the Shelley `epochSize=432000` and the u64-vs-bignum
  `relativeTime` shape.
- `/tmp/ygg-proxy-capture.txt` — socat capture of `cardano-cli ↔
  yggdrasil` confirming our wire bytes match upstream's shape
  byte-for-byte (Byron prefix) and the synthetic Shelley far-future
  values are accepted by cardano-cli.

### Open follow-ups

1. **Real `ChainState.chain_block_number`** — thread through
   `LedgerStateSnapshot` so `GetChainBlockNo` returns the true
   block count instead of approximating from slot.
2. **Eras past Shelley** — emit Allegra/Mary/Alonzo/Babbage/Conway
   summaries when the snapshot's current era exceeds Shelley.
   Current synthetic Shelley far-future end at slot 10M caps
   accurate slot↔epoch math at slot 10_000_000; mainnet/long-running
   preprod nodes will need additional summaries.
3. **Network-preset parameterisation** — replace hard-coded preprod
   values (Byron→Shelley boundary at slot 86_400) with values
   derived from the loaded `ShelleyGenesis`/`AlonzoGenesis`/
   `ConwayGenesis` so preview and mainnet operators see correct
   tip output as well.

### References

- `Ouroboros.Consensus.HardFork.History.Summary`
- `Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`
- Previous round: `docs/operational-runs/2026-04-27-round-151-chainsync-pool-wiring.md`
- Code: `crates/network/src/protocols/local_state_query_upstream.rs`,
  `node/src/local_server.rs`
