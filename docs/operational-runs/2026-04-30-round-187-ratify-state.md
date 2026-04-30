## Round 187 — Conway `ratify-state` LSQ dispatcher (tag 32) end-to-end

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close the substantial body-shape gap for `cardano-cli conway
query ratify-state` so the response decodes end-to-end.
Builds two new upstream-faithful encoders (`EnactState`,
`RatifyState`) shared with the future `gov-state` body work.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New singleton `EraSpecificQuery::GetRatifyState` (tag 32).
- Decoder branch `(1, 32)`.
- Regression test `decode_recognises_ratify_state_tag_32`
  pinning the wire form `[1, [32]]` = `0x82 0x01 0x81 0x18 0x20`.

`node/src/local_server.rs`:

- New helper `encode_enact_state_for_lsq(snapshot)` emitting
  the upstream 7-element `EnactState` CBOR list per
  `Cardano.Ledger.Conway.Governance.Internal.EnactState`:

  ```
  [
    ensCommittee         :: StrictMaybe (Committee era),
    ensConstitution      :: Constitution era,
    ensCurPParams        :: PParams era,         -- Conway 31-elem
    ensPrevPParams       :: PParams era,
    ensTreasury          :: Coin,
    ensWithdrawals       :: Map (Credential 'Staking) Coin,
    ensPrevGovActionIds  :: GovRelation StrictMaybe,  -- 4-elem
  ]
  ```

  Yggdrasil's snapshot exposes `constitution`, current `PParams`
  (via R161's Conway 31-element encoder), and `treasury` (via
  `accounting()`).  The remaining fields fall back to upstream
  defaults: `SNothing` for committee, same Conway PParams used
  for both cur/prev (until a separate prev-epoch tracker is
  plumbed), empty `Map` for withdrawals, and
  `[SNothing, SNothing, SNothing, SNothing]` for
  `GovRelation StrictMaybe`.
- New helper `encode_ratify_state_for_lsq(snapshot)` emitting
  the upstream 4-element `RatifyState` CBOR list per
  `Cardano.Ledger.Conway.Governance.Internal.RatifyState`:

  ```
  [
    rsEnactState  :: EnactState era,            -- 7-elem above
    rsEnacted     :: Seq (GovActionState era),  -- empty list
    rsExpired     :: Set GovActionId,           -- empty list
    rsDelayed     :: Bool,                       -- false
  ]
  ```

  The other three fields are empty/false placeholders until
  yggdrasil's ratify pipeline tracks pending/expired actions
  and the delayed flag.
- New dispatcher arm for `EraSpecificQuery::GetRatifyState`
  using `encode_ratify_state_for_lsq`.

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6` (chain
at slot ~2960, era=Conway):

```
$ cardano-cli conway query ratify-state --testnet-magic 2
{
    "enactedGovActions": [],
    "expiredGovActions": [],
    "nextEnactState": {
        "committee": null,
        "constitution": {
            "anchor": {
                "dataHash": "ca41a91f399259bcefe57f9858e91f6d00e1a38d6d9c63d4052914ea7bd70cb2",
                "url": "ipfs://bafkreifnwj6zpu3ixa4siz2lndqybyc5wnnt3jkwyutci4e2tmbnj3xrdm"
            },
            "script": "fa24fb305126805cf2164c161d852a0e7330cf988f1fe558cf7d4a64"
        },
        "curPParams": {
            "collateralPercentage": 150,
            "committeeMaxTermLength": 365,
            "committeeMinSize": 0,
            "costModels": {},
            "dRepActivity": 20,
            "dRepDeposit": 500000000,
            ... [full Conway 31-element PParams]
        }
    },
    "ratificationDelayed": false
}
```

Full record decodes end-to-end with real Conway constitution,
real Conway 31-element PParams, real treasury value, and
correct empty/null shapes for the remaining fields.

Regression checks pass (R180/R181/R182/R183/R184/R185/R186
queries still work):

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 2960, "era": "Conway", ... }

$ cardano-cli conway query constitution --testnet-magic 2
{ "anchor": { ... }, "script": "..." }

$ cardano-cli conway query treasury --testnet-magic 2
0

$ cardano-cli conway query future-pparams --testnet-magic 2
No protocol parameter changes will be enacted at the next epoch boundary.

$ cardano-cli conway query proposals --testnet-magic 2 --all-proposals
[]

$ cardano-cli conway query spo-stake-distribution --testnet-magic 2 --all-spos
[]
```

### Updated cumulative Conway-era query coverage

| Query / Tag | Round | Status |
|---|---|---|
| stake-deleg-deposits / 22 | R186 | ✓ wire-correct |
| constitution / 23 | R180 | ✓ working |
| gov-state / 24 | R180 dispatcher | body shape pending (substantial — uses RatifyState + Proposals) |
| drep-state / 25 | R180/R181 | ✓ working |
| drep-stake-distribution / 26 | R184 | ✓ working |
| committee-state / 27 | R182 | ✓ working |
| filtered-vote-delegatees / 28 | R184 | ✓ working (internal) |
| treasury (account-state) / 29 | R180 | ✓ working |
| spo-stake-distribution / 30 | R184 | ✓ working |
| proposals / 31 | R185 | ✓ working |
| **ratify-state / 32** | **R187** | **✓ working** |
| future-pparams / 33 | R183 | ✓ working |
| ledger-peer-snapshot / 34 | — | open (operational) |
| stake-pool-default-vote / 35 | R185 | ✓ working |
| pool-distr2 / 36 | R186 | ✓ wire-correct |
| stake-pools / 16 | R163/R179 | ✓ working |
| stake-distribution / 37 | R179 | ✓ working |
| pool-state / 19 (GetCBOR) | R172/R179 | ✓ working |
| stake-snapshot / 20 (GetCBOR) | R173/R179 | ✓ working |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4743  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4742 → **4743**.

### What's left of the Conway-governance arc

The `EnactState` encoder built in R187 is the **load-bearing
helper** for `gov-state` (tag 24) — gov-state's
`DRepPulsingState` field 7 is encoded as
`[PulsingSnapshot, RatifyState]`, both of which now have
working stubs.  The remaining gov-state work is:

1. `Proposals era` 2-tuple encoder
   `(GovRelation StrictMaybe, OMap GovActionId GovActionState)`
   — empty case is `[<4-SNothing GovRelation>, <empty OMap>]`.
2. `StrictMaybe (Committee era)` — SNothing already handled.
3. `Constitution era` — already in EnactState helper.
4. `PParams era` × 2 — already encoded via R161's helper.
5. `FuturePParams era` ADT — already R183 (Maybe shape) but
   gov-state expects the **internal** ADT shape per
   `Cardano.Ledger.Core.PParams.FuturePParams`:
   `Sum NoPParamsUpdate 0 = [0]`,
   `Sum DefinitePParamsUpdate 1 = [1, pp]`,
   `Sum PotentialPParamsUpdate 2 = [2, pp]`.  The wire-facing
   `Maybe (PParams era)` for tag 33 differs from the internal
   ADT used inside `ConwayGovState`.
6. `DRepPulsingState era` 2-element wrap — needs
   `PulsingSnapshot` empty stub (small) + R187's `RatifyState`.

So gov-state is now the smaller delta from where R187 leaves
us, and is the natural next round.

### Open follow-ups

1. **`gov-state` body shape** (tag 24) — composes R187's
   EnactState/RatifyState encoders with new Proposals,
   FuturePParams ADT, and PulsingSnapshot helpers.
2. **`ledger-peer-snapshot` body shape** (tag 34) —
   operational, lower priority for cli parity.
3. Live stake-distribution plumbing (R163/R173/R184 follow-up).
4. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
5. Apply-batch duration histogram (R169).
6. Multi-session peer accounting (R168 structural).
7. Pipelined fetch + apply (R166).
8. Deep cross-epoch rollback recovery (R167).

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — new `EraSpecificQuery::GetRatifyState` variant + decoder
  branch + regression test;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  `encode_enact_state_for_lsq` + `encode_ratify_state_for_lsq`
  helpers + dispatcher arm.
- Captures: `/tmp/ygg-r187-preview.log`.
- Upstream reference:
  `Cardano.Ledger.Conway.Governance.Internal.EnactState`
  (7-element record);
  `Cardano.Ledger.Conway.Governance.Internal.RatifyState`
  (4-element record);
  `Cardano.Ledger.Conway.LedgerStateQuery.GetRatifyState`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-186-stake-deleg-deposits-pool-distr2.md`](2026-04-30-round-186-stake-deleg-deposits-pool-distr2.md).
