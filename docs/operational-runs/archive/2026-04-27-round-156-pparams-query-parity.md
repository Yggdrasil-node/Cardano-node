## Round 156 — `cardano-cli query protocol-parameters` end-to-end

Date: 2026-04-27
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Continue the operational-parity push: make the second-most-used
cardano-cli operation (after `query tip`) work end-to-end against
yggdrasil's NtC socket.  Wallets and tx-builders all start with
`query protocol-parameters` to get the active fee/limit/cost-model
parameters.

### Pre-fix symptom

```
$ cardano-cli query protocol-parameters --testnet-magic 1
Command failed: query protocol-parameters
Error: "DecoderFailure ... DeserialiseFailure 2 \"expected list len\""
```

Yggdrasil's `dispatch_upstream_query` returned `null` for any
`BlockQuery (QueryIfCurrent ...)` query, regardless of the
era-specific tag inside.

### Diagnostic path

1. socat -x -v capture between cardano-cli and yggdrasil revealed
   the wire query: `82 03 82 00 82 00 82 01 81 03` =
   `MsgQuery [BlockQuery [QueryIfCurrent [era_index=1,
   [GetCurrentPParams=3]]]]`.
2. Initial implementation wrapped result as `[1, pp_cbor]`
   (2-element list with discriminator).  cardano-cli reported
   `DeserialiseFailure 3` — wire offset 3 in the wrapped result
   was `0x01`, but the decoder expected a list (the 2nd element
   of the `Left/Mismatch` form).
3. Fetched upstream `encodeEitherMismatch` source via WebFetch
   from `Ouroboros.Consensus.HardFork.Combinator.Serialisation.Common`:
   ```haskell
   (HardForkNodeToClientEnabled{}, Right a) ->
     mconcat [ Enc.encodeListLen 1 , enc a ]
   (HardForkNodeToClientEnabled{}, Left (MismatchEraInfo err)) ->
     mconcat [ Enc.encodeListLen 2 , encodeNS ... era1
             , encodeNS ... era2 ]
   ```
   Confirmed the load-bearing fact: **HFC uses list-length
   discrimination between Right and Left, no leading variant tag**.

### Fix

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery` enum + `decode_query_if_current(inner_cbor)`
  parses the `[era_index, era_specific_query]` payload and
  classifies the era-specific tag.
- New `encode_query_if_current_match(result_cbor)` emits the
  upstream `Right a` shape: 1-element list `[encoded_a]`.
- New `encode_query_if_current_mismatch(ledger_era_idx,
  query_era_idx)` emits the `Left mismatch` shape: 2-element list
  of NS-encoded era names.
- New `encode_shelley_pparams_for_lsq(params)` emits the upstream
  `Cardano.Ledger.Shelley.PParams.encCBOR` 17-element list shape.

`node/src/local_server.rs::dispatch_upstream_query`:

- Added `HardForkBlockQuery::QueryIfCurrent` arm that calls
  `decode_query_if_current` and dispatches `GetCurrentPParams`
  for era_index in 1..=3 (Shelley/Allegra/Mary share PP shape).
- Other era_indexes return null; mismatched era_index returns the
  proper Left/EraMismatch envelope.

### Regression tests

`crates/network/src/protocols/local_state_query_upstream.rs`:

- `decode_real_cardano_cli_get_current_pparams_payload` — pins
  the captured cardano-cli wire payload.
- `encode_query_if_current_match_is_one_element_list_no_tag` —
  pins the 1-element Right form, including a "MUST NOT be
  2-element" assertion guarding against regression.
- `encode_query_if_current_mismatch_is_two_element_ns_list` —
  pins the 2-element Left form with NS-encoded era indices.
- `shelley_pparams_emit_17_element_list_with_preprod_values` —
  pins the 17-element list with preprod minFeeA/minFeeB prefix
  bytes (`0x91 0x18 0x2c 0x1a 0x00 0x02 0x5e 0xf5`).

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4693  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4689 (Round 155) → 4693.

### Operational verification

```
$ CARDANO_NODE_SOCKET_PATH=/tmp/ygg-verify-multi.sock \
  cardano-cli query protocol-parameters --testnet-magic 1
{
    "decentralization": 1,
    "extraPraosEntropy": null,
    "maxBlockBodySize": 65536,
    "maxBlockHeaderSize": 1100,
    "maxTxSize": 16384,
    "minPoolCost": 340000000,
    "minUTxOValue": 1000000,
    "monetaryExpansion": 3.0e-3,
    "poolPledgeInfluence": 0.3,
    "poolRetireMaxEpoch": 18,
    "protocolVersion": {
        "major": 2,
        "minor": 0
    },
    "stakeAddressDeposit": 2000000,
    "stakePoolDeposit": 500000000,
    "stakePoolTargetNum": 150,
    "treasuryCut": 0.2,
    "txFeeFixed": 155381,
    "txFeePerByte": 44
}
```

Every field correctly populated with preprod-genesis Shelley
parameters.

### Open follow-ups

1. **Alonzo/Babbage/Conway PParams shape encoders** — each era
   has additional fields beyond Shelley's 17-element list: cost
   models (key 18), ex_unit_prices (key 19), max_tx_ex_units
   (key 20), max_block_ex_units (key 21), max_val_size (key 22),
   collateral_percentage (key 23), max_collateral_inputs (key
   24), coins_per_utxo_byte (Babbage rename of Alonzo's key 17);
   Conway adds DRep/governance fields and tiered ref-script fees.
   Needed once yggdrasil syncs past Mary on preprod or operates
   against Alonzo+ chains.
2. **Other era-specific queries** — `GetUTxOByAddress` /
   `GetUTxOByTxIn` (essential for wallets), `GetEpochNo`,
   `GetCurrentEpochState`, `GetGenesisConfig`,
   `GetStakeDistribution`, `GetPoolState`, `GetGovState` (Conway).
3. **EraMismatch refinement** — currently the era-name strings
   in the mismatch payload are derived from era_ordinal_to_name;
   upstream uses `SingleEraInfo`/`LedgerEraInfo` text strings
   that may differ slightly.

### References

- `Cardano.Consensus.HardFork.Combinator.Ledger.Query` (HFC dispatch)
- `Cardano.Consensus.HardFork.Combinator.Serialisation.Common.encodeEitherMismatch`
- `Cardano.Ledger.Shelley.PParams.encCBOR` (17-element list shape)
- Previous round: `docs/operational-runs/2026-04-27-round-155-tx-size-fee-parity.md`
- Code: `crates/network/src/protocols/local_state_query_upstream.rs`,
  `node/src/local_server.rs`
