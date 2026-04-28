## Round 158 — `cardano-cli query tx-mempool` end-to-end (LocalTxMonitor parity)

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Continue the operational-parity push.  Survey common cardano-cli
queries that should work in Shelley era, fix the LocalTxMonitor
wire-tag drift that was making `query tx-mempool` hang silently,
and confirm `query era-history` works (no code changes — Round
153's per-network Interpreter wiring already handled it).

### Pre-fix symptom

```
$ cardano-cli query tx-mempool info --testnet-magic 1
Terminated
```

Connection opened, handshake succeeded, then yggdrasil never
responded to `MsgAcquire`.  socat -x -v capture showed:

```
> cardano-cli sends:  81 01           = [1] = MsgAcquire (upstream)
< yggdrasil sends:   (nothing — hung waiting for more bytes)
```

### Root cause

Yggdrasil's `LocalTxMonitorMessage` codec used a non-upstream tag
scheme:

| Yggdrasil pre-fix | Upstream canonical |
|---|---|
| 0 = MsgAcquire | 0 = MsgDone |
| 1 = MsgAcquired | 1 = MsgAcquire / MsgAwaitAcquire |
| 2 = MsgNextTx | 2 = MsgAcquired |
| 3 = MsgReplyNextTx | 3 = MsgRelease |
| 4 = MsgHasTx | 5 = MsgNextTx |
| 5 = MsgReplyHasTx | 6 = MsgReplyNextTx |
| 6 = MsgGetSizes | 7 = MsgHasTx |
| 7 = MsgReplyGetSizes | 8 = MsgReplyHasTx |
| 8 = MsgRelease | 9 = MsgGetSizes |
| 9 = MsgDone | 10 = MsgReplyGetSizes |

Roundtrip tests passed (codec was internally consistent) but
cardano-cli's wire bytes didn't decode correctly.  Source:
WebFetch of upstream Haddock for
`Ouroboros.Network.Protocol.LocalTxMonitor.Codec`.

### Second bug — `MsgHasTx` era-tagged envelope

After re-tagging, `query tx-mempool tx-exists` still hung.  Wire
capture showed:

```
> 82 07 82 01 58 20 <32 bytes>
```

Decoded: `[7, [1, hash_bytes]]` — `MsgHasTx` payload is NOT a
bare hash but a `OneEraTxId xs = [era_idx, era_specific_id]`
envelope, reflecting Cardano's `HardForkBlock` parameterisation
where `TxId blk = OneEraTxId xs`.  Yggdrasil's decoder expected
bare bytes and stalled waiting for more data.

### Fix

`crates/network/src/protocols/local_tx_monitor.rs`:

- Rewrote `to_cbor` / `from_cbor` with the upstream tag table.
- `MsgHasTx` encoder emits `[7, [1, hash_bytes]]` (era_idx=1
  Shelley default).
- `MsgHasTx` decoder consumes the `[era_idx, hash_bytes]`
  envelope, preserving the `tx_id: Vec<u8>` field by discarding
  the era_idx (mempool lookup is era-independent).
- Updated rustdoc tag table on `to_cbor` to point at upstream's
  canonical mapping.

### Regression tests

- `decode_real_cardano_cli_msg_acquire_payload` — pins `81 01`
  = MsgAcquire (the symptom of the entire tag-scheme drift).
- `encode_msg_acquired_uses_tag_2` — pins `82 02 1a <slot>`
  server response.
- `decode_real_cardano_cli_has_tx_payload` — pins
  `82 07 82 01 58 20 <32 bytes>` for the OneEraTxId envelope.
- `encode_msg_has_tx_emits_one_era_tx_id_envelope` — pins the
  encoder produces the same shape.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4700  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4696 (Round 157) → 4700.

### Operational verification

All three `cardano-cli query tx-mempool` subcommands work
end-to-end against yggdrasil's preprod NtC socket:

```
$ cardano-cli query tx-mempool info --testnet-magic 1
{ "capacityInBytes": 0, "numberOfTxs": 0, "sizeInBytes": 0, "slot": 87040 }

$ cardano-cli query tx-mempool next-tx --testnet-magic 1
{ "nextTx": null, "slot": 87040 }

$ cardano-cli query tx-mempool tx-exists 0123…ef --testnet-magic 1
{ "exists": false, "slot": 87040, "txId": "0123…ef" }
```

`tx-exists` exercises the full era-tagged `MsgHasTx` →
`MsgReplyHasTx` round-trip.

### Bonus — `query era-history` works for free

```
$ cardano-cli query era-history --testnet-magic 1
{
    "type": "EraHistory",
    "description": "",
    "cborHex": "9f8383000000831b17fb16d83be000001a000151800484195460194e2083001910e081001910e083831b17fb16d83be000001a0001518004831ba18f4585293000001a00989680181a841a000697801903e883001a0001fa4081001a0001fa40ff"
}
```

The CBOR hex is yggdrasil's preprod 2-era Interpreter from Round
153 — `BlockQuery (QueryHardFork GetInterpreter)` was already
handled.  No code changes needed; just confirmed it works.

### Regression check

```
$ cardano-cli query tip                  # works
$ cardano-cli query protocol-parameters  # works
$ cardano-cli query utxo --whole-utxo    # works
```

No regression from the codec re-tagging.

### Cumulative cardano-cli parity

| Command | Status | Round |
|---|---|---|
| `query tip` | ✓ | 148-152 |
| `query protocol-parameters` | ✓ Shelley | 156 |
| `query utxo --whole-utxo` | ✓ | 157 |
| `query utxo --address X` | ✓ | 157 |
| `query utxo --tx-in T#i` | ✓ | 157 |
| `query era-history` | ✓ | (free from R153) |
| `query tx-mempool info` | ✓ | 158 |
| `query tx-mempool next-tx` | ✓ | 158 |
| `query tx-mempool tx-exists` | ✓ | 158 |
| `submit-tx` | ✓ | (LocalTxSubmission) |
| `query slot-number` | partial | era-history coverage gap past slot 10M |
| `query stake-address-info` | not yet | needs Bech32 parsing + tag 10 dispatch |
| `query stake-distribution` | client-blocked | needs Babbage+ snapshot |
| `query stake-pools` | client-blocked | needs Babbage+ snapshot |
| `query protocol-state` | client-blocked | needs Babbage+ snapshot |
| `query ledger-state` | client-blocked | needs Babbage+ snapshot |
| `query ledger-peer-snapshot` | client-blocked | needs Babbage+ snapshot |

### Open follow-ups

1. `query stake-address-info` — needs Bech32 stake-address
   parsing + GetFilteredDelegationsAndRewardAccounts (tag 10)
   dispatcher.
2. `query slot-number` past slot 10M — extend preprod
   Interpreter coverage as snapshot's current_era advances.
3. Babbage+ era-gated queries — need yggdrasil to sync past
   Mary (~slot 1.5M, ~17 hours at current sync rate).

### References

- `Ouroboros.Network.Protocol.LocalTxMonitor.Codec` (canonical
  tag table — fetched via WebFetch of upstream Haddock).
- `OneEraTxId` from `Ouroboros.Consensus.HardFork.Combinator`
  (era-tagged tx_id envelope).
- Previous round: `docs/operational-runs/2026-04-28-round-157-utxo-query-parity.md`.
- Code: `crates/network/src/protocols/local_tx_monitor.rs`.
