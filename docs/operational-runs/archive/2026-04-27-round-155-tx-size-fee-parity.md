## Round 155 — Alonzo+ tx-size for fee/max excludes `is_valid` byte (preview unblocked)

Date: 2026-04-27
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close Round 154's open follow-up #1: fix yggdrasil's tx-size
computation to match upstream's Mary-era-compatible `sizeAlonzoTxF`
so preview's bootstrap chain syncs past the
`fee too small: minimum 237_837 lovelace, declared 237_793`
rejection.

### Root cause

Upstream `Cardano.Ledger.Alonzo.Tx.toCBORForSizeComputation`:

```haskell
toCBORForSizeComputation AlonzoTx {atBody, atWits, atAuxData} =
  encodeListLen 3
    <> encCBOR atBody
    <> encCBOR atWits
    <> encodeNullStrictMaybe encCBOR atAuxData
```

Note the **3-element list** with `is_valid` deliberately **excluded**.
The doc-string in upstream's source explains: *"Notably, IsValid is
excluded from size computation for Mary-era compatibility."*  This
keeps the linear fee formula `min_fee = a × txSize + b` consistent
across the Mary→Alonzo hard-fork: a tx that's valid in both eras
computes the same fee in both.

A separate upstream encoder `toCBORForMempoolSubmission` uses the
**4-element** form `[body, wits, isValid, aux]` for the wire format
of submitted txs.  But fee math always uses the 3-element form via
`sizeAlonzoTxF`.

### Pre-fix bug

Yggdrasil's `Tx::serialized_size` was matching the 4-element wire
form, including the `is_valid` byte in tx_size:

```rust
let is_valid_size: usize = if alonzo_plus { 1 } else { 0 };
header_size + body_size + witness_size + is_valid_size + aux_data_size
```

For every Alonzo+ tx, this produces a `tx_size` 1 byte too large.
At `minFeeA=44`, `min_fee` is 44 lovelace too high → real Alonzo+
blocks fail `validateFeeTooSmallUTxO`.

### Diagnostic path

1. Round 154 surfaced the symptom on preview: `fee too small`
   rejection at `currentPoint=Origin`.
2. Round 155 added `YGG_FEE_DEBUG=1` instrumentation to dump
   `tx_size`, `body.fee`, `witness_bytes_len`, `aux_data_len`,
   `is_valid`, and `reencoded_body_len` for failing Alonzo+ block
   txs.
3. Captured: `tx_size=1874, witness_bytes_len=710, aux_data_len=None,
   is_valid=Some(true), reencoded_body_len=1161`.
4. Reverse-computed expected upstream `tx_size = (237_793-155_381)/44
   = 1873` — exactly 1 byte less than ours.
5. Fetched upstream's `toCBORForSizeComputation` source directly,
   which confirmed the Mary-era-compat `is_valid` exclusion.

### Fix

`crates/ledger/src/tx.rs`:

- `Tx::serialized_size()` now uses the 3-element form regardless of
  era: `1 (header) + body + wits + aux_or_null`.
- New `AlonzoCompatibleSubmittedTx::size_for_fee_and_max()` returns
  `raw_cbor.len() - 1` for submitted-tx fee/max-tx-size validation.

`crates/ledger/src/state.rs`:

- Three submitted-tx Alonzo+ call sites updated from
  `tx.raw_cbor.len()` to `tx.size_for_fee_and_max()` (Alonzo,
  Babbage, Conway arms of `MultiEraSubmittedTx::*`).
- Block-apply paths (using `tx.serialized_size()`) inherit the fix
  automatically.

### Regression tests

`crates/ledger/src/tx.rs`:

- `serialized_size_alonzo_plus_excludes_is_valid` — pins the
  3-element form returning 10 bytes for a 5/1/3-byte body/wits/aux
  Alonzo+ tx (pre-fix returned 11).
- `serialized_size_invariant_across_eras_for_fee_math` — pins that
  pre-Alonzo and Alonzo+ produce identical fee/size for the same
  content.
- `serialized_size_larger_than_body_only` — updated expected value
  from 12 to 11 to match the new 3-element semantics.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4689  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4688 (Round 154) → 4689.

### Operational verification

**Preview — now syncs end-to-end**

Pre-Round-155: `node run failed error=ledger decode error: fee too
small` at slot 0.

Post-Round-155 preview knob=2 ~30s soak:

```
yggdrasil_blocks_synced 1988
yggdrasil_current_slot 39740
yggdrasil_reconnects 1
yggdrasil_blockfetch_workers_registered 12
yggdrasil_blockfetch_workers_migrated_total 12
yggdrasil_chainsync_workers_registered 2
```

cardano-cli output:

```json
{
    "block": 39740,
    "epoch": 0,
    "era": "Alonzo",
    "hash": "c6d9124b1baf2ece003530a6602b52b325c58db59bc00e20bdf4edfb26c43385",
    "slot": 39740,
    "slotInEpoch": 39740,
    "slotsToEpochEnd": 46660,
    "syncProgress": "0.04"
}
```

Every JSON field correctly populated for preview's 86_400-slot
epochs and Alonzo-era genesis.

**Bonus parity win**: preview's larger peer count exercises
multi-peer ChainSync — `yggdrasil_chainsync_workers_registered=2`,
the first time we've observed >1 in the field.  Previously preprod's
reader-side path only registered 1 worker; preview's bootstrap chain
generates enough RollForward observations from multiple peers to
populate per-peer candidate fragments.

**Preprod — no regression**

Post-Round-155 preprod knob=2 ~30s soak:

```json
{
    "block": 86840,
    "epoch": 4,
    "era": "Shelley",
    "hash": "7dab2681a5e4e42831f05e7d171df361ec0ee4d113b5d13a66af1ca6dedbdcc6",
    "slot": 86840,
    "slotInEpoch": 440,
    "slotsToEpochEnd": 431560,
    "syncProgress": "1.40"
}
```

Same shape as Round 154 baseline.

### Open follow-ups

1. **Live `chain_block_number`** — Round 152 follow-up still open.
   `GetChainBlockNo` currently approximates block-no from slot;
   threading the consensus chain-tracker into `LedgerStateSnapshot`
   would give correct values.
2. **Allegra+ era summaries** — Round 153 follow-up still open.
   Current single Shelley summary covers ~10M slots; mainnet at
   slot 75M+ would need explicit Allegra/Mary/Alonzo/Babbage/Conway
   summaries.
3. **Babbage→Conway / Conway-internal transition signals** — as
   preview progresses past Babbage, the era-PV admission table
   from Round 154 will need extension if upstream signals more
   Conway-internal hard forks via PV bumps.

### Diagnostic captures

- `/tmp/ygg-preview-r155.log` — preview run log showing
  end-to-end sync.
- `/tmp/ygg-preview-cli-tip-r155.txt` — cardano-cli output
  against yggdrasil's preview NtC socket.
- `/tmp/ygg-preview-metrics-r155.txt` — Prometheus snapshot.
- `/tmp/ygg-verify-cli-tip-r155.txt` — preprod regression baseline.

### References

- `Cardano.Ledger.Alonzo.Tx.toCBORForSizeComputation` (upstream)
- `Cardano.Ledger.Shelley.Rules.Utxo.validateMaxTxSizeUTxO`
- `Cardano.Ledger.Shelley.Rules.Utxo.validateFeeTooSmallUTxO`
- Previous round: `docs/operational-runs/2026-04-27-round-154-era-pv-transition-signal.md`
- Code: `crates/ledger/src/tx.rs::Tx::serialized_size`,
  `crates/ledger/src/tx.rs::AlonzoCompatibleSubmittedTx::size_for_fee_and_max`,
  `crates/ledger/src/state.rs` (three Alonzo+ submitted-tx call sites)
