## Round 159 — Alonzo PParams shape (24-element list) for preview's `query protocol-parameters`

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Preview reports `era=Alonzo` (era_index=4) at slot ~3000.  Round
156 only handled era_index 1..=3 (Shelley-family 17-element PP
shape), so `cardano-cli query protocol-parameters` against
preview returned `null` and cardano-cli failed with
`DeserialiseFailure 2 "expected list len"`.  Implement the
upstream Alonzo PP shape so this query works on preview.

### Implementation

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `encode_alonzo_pparams_for_lsq(params)` emits the 24-element
  list per `Cardano.Ledger.Alonzo.PParams.encCBOR`:
  - 16 inherited Shelley-family fields (with `minPoolCost` at slot 16,
    replacing Shelley's `minUTxOValue` slot)
  - `coinsPerUtxoWord` (= `coins_per_utxo_byte * 8`)
  - `costModels` (CBOR map of language → array of i64 ops)
  - `prices` ([priceMem, priceSteps] UnitInterval pair)
  - `maxTxExUnits` ([mem, steps])
  - `maxBlockExUnits` ([mem, steps])
  - `maxValSize`, `collateralPercentage`, `maxCollateralInputs`
- Helpers: `encode_alonzo_cost_models`, `encode_ex_unit_prices`,
  `encode_ex_units`.

`node/src/local_server.rs::dispatch_upstream_query`:

- `GetCurrentPParams` arm now branches on `era_index`:
  - 1..=3 → `encode_shelley_pparams_for_lsq`
  - 4 → `encode_alonzo_pparams_for_lsq`
  - 5+ → null (Babbage/Conway PP shapes are Phase-3 follow-ups)

### Regression test

`alonzo_pparams_emit_24_element_list` — pins the
`0x98 0x18` array-len-24 prefix and the minFeeA/minFeeB shared
prefix bytes.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4701  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4700 (Round 158) → 4701.

### Operational verification — preview at Alonzo era

```json
$ cardano-cli query protocol-parameters --testnet-magic 2
{
    "collateralPercentage": 150,
    "costModels": {},
    "decentralization": 1,
    "executionUnitPrices": {
        "priceMemory": 5.77e-2,
        "priceSteps": 7.21e-5
    },
    "extraPraosEntropy": null,
    "maxBlockBodySize": 65536,
    "maxBlockExecutionUnits": {
        "memory": 50000000,
        "steps": 40000000000
    },
    "maxBlockHeaderSize": 1100,
    "maxCollateralInputs": 3,
    "maxTxExecutionUnits": {
        "memory": 10000000,
        "steps": 10000000000
    },
    "maxTxSize": 16384,
    "maxValueSize": 5000,
    "minPoolCost": 340000000,
    "monetaryExpansion": 3.0e-3,
    "poolPledgeInfluence": 0.3,
    "poolRetireMaxEpoch": 18,
    "protocolVersion": { "major": 6, "minor": 0 },
    "stakeAddressDeposit": 2000000,
    "stakePoolDeposit": 500000000,
    "stakePoolTargetNum": 150,
    "treasuryCut": 0.2,
    "txFeeFixed": 155381,
    "txFeePerByte": 44,
    "utxoCostPerByte": 34480
}
```

Every Alonzo-specific field renders correctly.

Preview also picks up Alonzo `query utxo --whole-utxo` with
`datum`/`datumhash` TxOut fields (already supported by
`encode_txout_era_specific` from Round 157):

```json
{
    "e3ca57e8…07b6#0": {
        "address": "addr_test1qp8cprhse9pnnv7f4l3n6pj0afq2hjm6f7r2205dz0583ed6zj0zugmep9lxtuxq8unn85csx9g70ugq6dklmvq6pv3qa0n8cl",
        "datum": null,
        "datumhash": null,
        "value": { "lovelace": 100000000000000 }
    },
    ...
}
```

### Preprod regression check

`query tip` and `query protocol-parameters` (Shelley shape) on
preprod still work — the era-aware dispatcher branches preserve
Shelley-shape output for era_index=1.

### Survey of remaining era-blocked queries

cardano-cli rejects these client-side at era ≤ Alonzo:

- `query stake-pools`
- `query stake-distribution`
- `query protocol-state`
- `query ledger-state`
- `query ledger-peer-snapshot`
- `query stake-address-info`

Error: `"This query is not supported in the era: Alonzo. Please
use a different query or switch to a compatible era."`

Unblocking these requires Babbage+ snapshot, achievable via:
1. **Babbage PP encoder + era classification fix**: classify
   Alonzo blocks with PV major ≥ 7 as Babbage era so
   `snapshot.current_era` reports Babbage even when wire-tagged
   Alonzo (matches upstream's HFC era-tracking semantics).
2. **Sync to real Babbage blocks**: yggdrasil's preview snapshot
   would naturally advance to era_index=5 when the chain
   transitions wire-format from Alonzo to Babbage.

### Cumulative cardano-cli parity

| Command | Preprod (Shelley) | Preview (Alonzo) |
|---|---|---|
| `query tip` | ✓ | ✓ |
| `query protocol-parameters` | ✓ Shelley shape | ✓ Alonzo shape (R159) |
| `query utxo --whole-utxo` | ✓ Shelley TxOuts | ✓ Alonzo TxOuts (datum/datumhash) |
| `query utxo --address X` | ✓ | ✓ |
| `query utxo --tx-in T#i` | ✓ | ✓ |
| `query era-history` | ✓ | ✓ |
| `query tx-mempool info` | ✓ | ✓ |
| `query tx-mempool next-tx` | ✓ | ✓ |
| `query tx-mempool tx-exists` | ✓ | ✓ |
| `submit-tx` | ✓ | ✓ |
| `query stake-pools` | client-blocked | client-blocked |
| `query stake-distribution` | client-blocked | client-blocked |
| `query protocol-state` | client-blocked | client-blocked |
| `query ledger-state` | client-blocked | client-blocked |
| `query ledger-peer-snapshot` | client-blocked | client-blocked |
| `query stake-address-info` | client-blocked | client-blocked |

### Open follow-ups

1. Babbage PP shape (drops `d`/`extraEntropy`, adds
   `coinsPerUtxoByte` rename of `coinsPerUtxoWord`/8).
2. Conway PP shape (adds DRep/governance/committee fields and
   tiered ref-script fees).
3. Era classification fix — advance `snapshot.current_era` when
   block PV major ≥ next-era threshold so preview's PV=(7,2)
   snapshot reports Babbage and client-side era checks unblock.

### References

- `Cardano.Ledger.Alonzo.PParams.encCBOR` (24-element list shape).
- Previous round: `docs/operational-runs/2026-04-28-round-158-tx-mempool-parity.md`.
- Code: `crates/network/src/protocols/local_state_query_upstream.rs`,
  `node/src/local_server.rs`.
