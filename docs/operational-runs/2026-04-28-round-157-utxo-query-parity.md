## Round 157 — `cardano-cli query utxo` end-to-end (whole / address / tx-in)

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Extend Round 156's QueryIfCurrent infrastructure with the three
UTxO query variants every wallet and explorer needs:

- `query utxo --whole-utxo` — full UTxO map dump.
- `query utxo --address X` — filter by address (wallet balance scan).
- `query utxo --tx-in T#i` — resolve a specific output (tx-builder).

### Pre-fix symptom

All three variants returned the now-familiar
`DecoderFailure ... DeserialiseFailure 2 "expected list len"`
because yggdrasil's `dispatch_upstream_query` only handled
`GetCurrentPParams` (tag 3) inside `QueryIfCurrent` and returned
null for any other era-specific tag.

### Wire shape captures

socat -x -v capture between cardano-cli and yggdrasil:

| Query | Wire payload (after MsgQuery+BlockQuery+QueryIfCurrent prefix) | Era-specific tag |
|---|---|---|
| `--whole-utxo` | `82 01 81 07` (era=1, `[7]`) | 7 |
| `--address X` | `82 01 82 06 81 <addr_bytes>` | 6 |
| `--tx-in T#i` | `82 01 82 0f 81 82 58 20 <txid> <idx>` | **15** |

Initial implementation guessed tag 14 for GetUTxOByTxIn; the wire
capture confirmed tag 15.

### Implementation

`crates/network/src/protocols/local_state_query_upstream.rs`:

- Extended `EraSpecificQuery` enum with `GetEpochNo`,
  `GetWholeUTxO`, `GetUTxOByAddress { address_set_cbor }`,
  `GetUTxOByTxIn { txin_set_cbor }` variants.
- Updated `decode_query_if_current` to recognise tags 1/3/6/7/15.

`node/src/local_server.rs`:

- New `encode_utxo_map(snapshot, predicate)` helper emits a CBOR
  `Map TxIn TxOut` per upstream `EncCBOR (UTxO m) = encCBOR m`.
- New `encode_txout_era_specific` emits TxOuts in their bare
  era-specific shape (Shelley: `[address, coin]`; Mary:
  `[address, value]`; Alonzo+: more fields).  Must NOT use
  yggdrasil's internal `[era_tag, txout]` envelope — that's a
  storage-only shape.
- New `decode_address_set` / `decode_txin_set` parse the request
  payloads, tolerating CBOR tag 258 ("set" tag per CIP-21) prefix.
- Dispatcher routes:
  - `GetWholeUTxO` → unfiltered map.
  - `GetUTxOByAddress` → filter by `txout_address_bytes`.
  - `GetUTxOByTxIn` → filter by TxIn lookup.
  - `GetEpochNo` → bare CBOR uint of `snapshot.current_epoch().0`.

### Regression tests

`crates/network/src/protocols/local_state_query_upstream.rs`:

- `decode_real_cardano_cli_get_whole_utxo_payload` — pins the
  captured `82 00 82 00 82 01 81 07` payload.
- `decode_real_cardano_cli_get_utxo_by_tx_in_payload` — pins the
  tag=15 + 32-byte txid + index format (load-bearing because tag
  14 was the initial guess).
- `decode_get_utxo_by_address_recognises_tag_6` — pins the
  address-set shape.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4696  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4693 (Round 156) → 4696.

### Operational verification

**`query utxo --whole-utxo`**

```json
{
    "a00696a0c2d70c381a265a845e43c55e1d00f96b27c06defc015dc92eb206240#0": {
        "address": "addr_test1vz09v9yfxguvlp0zsnrpa3tdtm7el8xufp3m5lsm7qxzclgmzkket",
        "value": { "lovelace": 29699998493355698 }
    },
    "a3d6f2627a56fe7921eeda546abfe164321881d41549b7f2fbf09ea0b718d758#1": {
        "address": "addr_test1qz09v9yfxguvlp0zsnrpa3tdtm7el8xufp3m5lsm7qxzclvk35gzr67hz78plv88jemfs2p9e2780xm98cfrf4vvu0rq83pdz2",
        "value": { "lovelace": 100000000000000 }
    },
    ...
}
```

Three Byron-genesis bootstrap entries returned with correct
addresses and lovelace balances.

**`query utxo --address addr_test1vz09v9...`**

Returns just the single matching entry — filter works correctly.

**`query utxo --tx-in a00696a0...#0`**

Returns the entry for that specific TxIn — TxIn-set decoder
correctly reads the 32-byte txid + output index.

### Open follow-ups

1. **`query slot-number`** fails with `Past horizon` when the
   requested timestamp falls outside our 2-era preprod
   Interpreter's coverage (synthetic Shelley far-future end at
   slot 10_000_000 = epoch 26 = ~116 days post-Byron).  Extending
   coverage requires emitting additional eras as `snapshot.current_era`
   advances, or pushing the synthetic end further.
2. **Alonzo+ TxOut shapes** — current `encode_txout_era_specific`
   handles Shelley/Mary/Alonzo/Babbage variants, but TxOuts in
   blocks past Alonzo carry optional datum / script-ref fields
   that need era-aware encoding parity.  Tested only for Shelley
   shape on preprod (slot ~87000); Alonzo+ era queries remain to
   be verified.
3. **More era-specific queries** — `query stake-distribution` (tag
   5), `query stake-pools`, `query gov-state` (Conway), `query
   stake-address-info`, `query genesis`.  Each adds ~5-15 lines
   of dispatcher code once captured.

### References

- `Cardano.Ledger.Shelley.LedgerStateQuery` (era-specific query
  tag table).
- `Cardano.Ledger.Shelley.UTxO.UTxO` (`Map TxIn TxOut` encoding).
- Previous round: `docs/operational-runs/2026-04-27-round-156-pparams-query-parity.md`
- Code: `crates/network/src/protocols/local_state_query_upstream.rs`
  (decoder), `node/src/local_server.rs` (UTxO map encoder + dispatch).
