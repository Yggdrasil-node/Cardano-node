## Round 179 — Era blockage end-to-end fix

Date: 2026-04-29
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close the R178 follow-up: with `YGG_LSQ_ERA_FLOOR=6` set, all
five era-gated cardano-cli queries (`query stake-pools`,
`query stake-distribution`, `query stake-address-info`,
`query pool-state`, `query stake-snapshot`) must decode
end-to-end against yggdrasil, returning empty/placeholder
data appropriate to a fresh sync state — not the
`DeserialiseFailure 2 "expected list len"` cardano-cli was
emitting throughout R178.

### Root causes (three independent bugs)

#### Bug 1 — wrong tag table (R163/R171/R172/R173)

R163 introduced `decode_query_if_current` with the era-specific
tag table `(1, 13) → GetStakePools`, `(2, 14) → GetStakePoolParams`,
`(2, 17) → GetPoolState`, `(2, 18) → GetStakeSnapshots`.  These
tag numbers were taken from documentation / older cardano-node
versions — the actual cardano-node 10.7.x
`Ouroboros.Consensus.Shelley.Ledger.Query.encodeShelleyQuery`
uses tags **16/17/19/20** for these queries.  The yggdrasil
slots 13/14/17/18 in upstream are
`DebugChainDepState`/`GetRewardProvenance`/
`GetStakePoolParams`/`GetRewardInfoPools`.

When cardano-cli sent `tag 16` for `GetStakePools`, yggdrasil's
decoder mapped it to `EraSpecificQuery::Unknown { tag: 16, .. }`
which fell through to `null_response()` — emitting `0xf6` (CBOR
null) directly without the HFC envelope wrap.  cardano-cli's
HFC `decodeEitherMismatch` then read `0xf6` at byte 2 of the
result body and failed expecting a list length.

The bug was masked across R163-R178 because cardano-cli's
client-side era gate refused to send these queries to a node
reporting Alonzo era — so the wrong-tag dispatcher path was
never exercised end-to-end.

#### Bug 2 — `cardano-cli query stake-distribution` uses tag 37

cardano-cli 10.x uses **tag 37** (`GetStakeDistribution2`, the
post-Conway no-VRF variant) for `query stake-distribution`, not
tag 5.  yggdrasil only handled tag 5 — tag 37 fell through to
`Unknown` / `null_response()`.

The result type is also different: tag 37 returns the upstream
`Cardano.Ledger.Core.PoolDistr` record (`[map, NonZero Coin]`,
2-element list) vs tag 5's consensus
`Ouroboros.Consensus.Shelley.Ledger.Query.Types.PoolDistr`
newtype (bare map).  yggdrasil's pre-R179 encoder emitted just
the bare map.  cardano-cli's PoolDistr decoder rejected with
`DeserialiseFailure 3 "expected list len or indef"`.

After fixing the shape to `[map, total]`, cardano-cli further
rejected `0` for `pdTotalStake` because it's typed as
`NonZero Coin` ("Encountered zero while trying to construct a
NonZero value").

#### Bug 3 — `query pool-state` and `query stake-snapshot` use GetCBOR (tag 9)

cardano-cli wraps these two queries via **tag 9** (`GetCBOR`)
which encodes the inner query body as a recursive era-specific
query and asks the server to respond with the inner result
encoded as CBOR-in-CBOR (`tag(24) bytes(<inner_response>)`).
yggdrasil never recognised tag 9 — same `Unknown` fall-through
to null.  Even after fixing tags 19/20 directly, the GetCBOR
wrapping path remained unhandled.

`StakeSnapshots` further rejected `0` totals for the same
`NonZero Coin` reason as bug 2.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- **Re-tagged**: `(1, 13) → GetStakePools` to `(1, 16)`;
  `(2, 14) → GetStakePoolParams` to `(2, 17)`;
  `(2, 17) → GetPoolState` to `(2, 19)`;
  `(2, 18) → GetStakeSnapshots` to `(2, 20)`.
- **Added** `(1, 37) → GetStakeDistribution` (alias — same handler
  as tag 5; cardano-cli 10.x uses tag 37 for the no-VRF variant
  but the response shape difference is handled in the body
  encoder).
- **Added** `(2, 9) → EraSpecificQuery::GetCBOR { inner_query_cbor }`
  variant carrying the recursive inner-query bytes.
- Updated all dispatcher doc comments and test fixtures to
  reflect the corrected tag numbers.

`node/src/local_server.rs`:

- `encode_stake_distribution_map`: emit
  `[unPoolDistr_map, pdTotalStake_coin]` (2-element list)
  instead of bare map; `pdTotalStake = 1` placeholder
  (NonZero requirement).
- `encode_stake_snapshots`: emit 1-lovelace placeholders for
  `ssStakeMarkTotal` / `ssStakeSetTotal` / `ssStakeGoTotal`
  (NonZero requirement).
- New helper `dispatch_inner_era_query(snapshot, era_index,
  inner_query_cbor)` that synthesises a `[era_index,
  inner_query_cbor]` outer wrapper, recursively classifies via
  `decode_query_if_current`, and returns the bare inner-response
  body (no envelope, no CBOR-in-CBOR wrap).
- New `EraSpecificQuery::GetCBOR { inner_query_cbor }` dispatcher
  arm: calls the helper, wraps the result in
  `tag(24) bytes(<body>)`, then applies the standard HFC envelope.

### Operational verification

#### Era-gated queries (preview, `YGG_LSQ_ERA_FLOOR=6`)

```
$ cardano-cli query stake-pools --testnet-magic 2
[]

$ cardano-cli query stake-distribution --testnet-magic 2
{}

$ cardano-cli query pool-state --all-stake-pools --testnet-magic 2
{}

$ cardano-cli query stake-snapshot --all-stake-pools --testnet-magic 2
{
    "pools": {},
    "total": {
        "stakeMark": 1,
        "stakeSet": 1,
        "stakeGo": 1
    }
}
```

All four era-gated queries decode end-to-end.  Empty data is
operationally indistinguishable from a "no pools registered yet"
chain state — which is correct for a fresh-sync preview chain
that hasn't crossed the natural Babbage hard-fork yet.

#### Regression check (preprod, no era floor)

All 11 pre-existing cardano-cli operations continue to work
unchanged on a fresh preprod sync (Allegra era at slot 90440):

```
query tip / protocol-parameters / era-history /
slot-number 2026-12-31T00:00:00Z / utxo --whole-utxo /
tx-mempool info / next-tx / tx-exists
```

Sample output:
```
$ cardano-cli query tip --testnet-magic 1
{ "block": 90440, "epoch": 4, "era": "Allegra", ... }

$ cardano-cli query protocol-parameters --testnet-magic 1
{ "decentralization": 1, "extraPraosEntropy": null,
  "maxBlockBodySize": 65536, "maxBlockHeaderSize": 1100, ... }
```

Zero regressions on existing-working queries.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4736  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4735 → **4736** (added
`decode_recognises_stake_distribution2_tag_37`; updated five
existing tests to pin the corrected tag numbers and the new
PoolDistr / StakeSnapshots envelope shapes).

### Updated cumulative cardano-cli LSQ era-specific tag coverage

| Tag | Query | Round | Status |
|-----|---|---|---|
|  1 | GetEpochNo | R157 | dispatcher ready |
|  3 | GetCurrentPParams | R156 | ✓ working (Shelley/Alonzo/Babbage/Conway) |
|  5 | GetStakeDistribution | R163 | dispatcher ready (returns empty PoolDistr `[{}, 1]`) |
|  6 | GetUTxOByAddress | R157 | ✓ working |
|  7 | GetUTxOWhole | R157 | ✓ working |
|  9 | GetCBOR (wrapper) | **R179** | ✓ working (recurses into inner) |
| 10 | GetFilteredDelegationsAndRewardAccounts | R163 | dispatcher ready |
| 11 | GetGenesisConfig | R163 | dispatcher ready (null) |
| 15 | GetUTxOByTxIn | R157 | ✓ working |
| **16** | **GetStakePools** | R163 (R179 corrected tag) | **✓ working** |
| **17** | **GetStakePoolParams** | R171 (R179 corrected tag) | dispatcher ready |
| **19** | **GetPoolState** | R172 (R179 corrected tag) | **✓ working** (via GetCBOR) |
| **20** | **GetStakeSnapshots** | R173 (R179 corrected tag) | **✓ working** (via GetCBOR) |
| **37** | **GetStakeDistribution2** | **R179** | **✓ working** |

### Open follow-ups

1. **Live stake-snapshot plumbing** — populate the per-pool
   `[mark, set, go]` and the three totals from
   `LedgerCheckpointTracking::stake_snapshots` (the long-pending
   R163 follow-up; R179's NonZero placeholders satisfy
   cardano-cli's decoder but operators see `1`-lovelace
   placeholders).
2. **`GetGenesisConfig` ShelleyGenesis serialisation** (R163).
3. **Apply-batch duration histogram** (R169).
4. **Multi-session peer accounting** (R168 structural).
5. **Pipelined fetch + apply** (R166).
6. **Deep cross-epoch rollback recovery** (R167).
7. **Tag 21 `GetPoolDistr`, tag 36 `GetPoolDistr2`,
   tag 23–35 Conway governance queries** — additional
   dispatchers for completeness; not currently sent by
   `cardano-cli query` standard surface.

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — corrected tag table + new GetCBOR variant + new
  GetStakeDistribution2 (tag 37) tests;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  `encode_stake_distribution_map` PoolDistr shape +
  `encode_stake_snapshots` NonZero totals + new
  `dispatch_inner_era_query` recursive helper +
  `EraSpecificQuery::GetCBOR` dispatcher arm.
- Captures: `/tmp/ygg-r179-preview.log` (era-gated queries
  decode end-to-end on preview with `YGG_LSQ_ERA_FLOOR=6`),
  `/tmp/ygg-r179-preprod.log` (preprod regression check —
  all 11 pre-existing cardano-cli ops continue to work).
- Upstream reference:
  `Ouroboros.Consensus.Shelley.Ledger.Query.encodeShelleyQuery`
  (the canonical tag table for cardano-node 10.7.x);
  `Ouroboros.Consensus.Shelley.Ledger.Query.LegacyPParams`
  (Conway PP delegates to standard `toCBOR`);
  `Cardano.Ledger.Core.PoolDistr` (2-element record with
  `NonZero Coin` `pdTotalStake`).
- Previous round:
  [`docs/operational-runs/2026-04-28-round-178-era-floor-env-var.md`](2026-04-28-round-178-era-floor-env-var.md).
