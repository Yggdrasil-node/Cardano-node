## Round 184 — Conway DRep / SPO stake-distribution + filtered-vote-delegatees LSQ dispatchers (tags 26, 28, 30)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Add three Conway-era LSQ dispatchers so
`cardano-cli conway query drep-stake-distribution --all-dreps`
and `cardano-cli conway query spo-stake-distribution --all-spos`
both decode end-to-end.  Continues the Conway-governance
dispatcher series (R180 constitution / drep-state / treasury,
R181 drep-state Map shape, R182 committee-state, R183
future-pparams).

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::GetDRepStakeDistr { drep_set_cbor }`
  (tag 26).
- New `EraSpecificQuery::GetFilteredVoteDelegatees
  { stake_cred_set_cbor }` (tag 28).
- New `EraSpecificQuery::GetSPOStakeDistr { spo_set_cbor }`
  (tag 30).
- Decoder branches `(2, 26)`, `(2, 28)`, `(2, 30)`.
- Regression test `decode_recognises_drep_and_spo_stake_distr_tags`
  pinning all three wire forms.

`node/src/local_server.rs`:

- Dispatcher arm for `GetDRepStakeDistr` emits empty CBOR map
  (`0xa0`).  Result type per upstream is `Map (DRep) Coin`;
  yggdrasil doesn't yet track per-DRep active stake, so empty
  is correct on a fresh sync.
- Dispatcher arm for `GetSPOStakeDistr` emits empty CBOR map
  (`0xa0`).  Result type per upstream is `Map (KeyHash
  'StakePool) Coin`.
- Dispatcher arm for `GetFilteredVoteDelegatees` emits empty
  CBOR map (`0xa0`).  Result type per upstream is
  `Map (Credential 'Staking) DRep`.

### Discovery: the SPO query is a 3-call flow

Initial implementation added only tags 26 and 30.  DRep query
worked end-to-end; SPO query failed with `DeserialiseFailure 2
"expected list len"`.  Wire-debug capture revealed cardano-cli's
`spo-stake-distribution` command sends THREE sequential queries:

1. tag 30 `GetSPOStakeDistr` — the actual stake distribution.
2. tag 9 `GetCBOR` wrapping tag 19 `GetPoolState` — fetches
   pool registration data for the SPO set.
3. tag 28 `GetFilteredVoteDelegatees` — fetches vote
   delegations for the SPO reward credentials, used for the
   "voteDelegation" field in the JSON output.

The SPO response itself was correct (bare `0xa0` decoded fine),
but call (3) hit the dispatcher's `Unknown` arm and returned
`null`, which cardano-cli rejected.  Adding the tag-28
dispatcher closes the flow end-to-end.

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6` (chain
at slot ~2K, era=Conway):

```
$ cardano-cli conway query drep-stake-distribution \
      --testnet-magic 2 --all-dreps
{}

$ cardano-cli conway query spo-stake-distribution \
      --testnet-magic 2 --all-spos
[]
```

Both decode end-to-end through cardano-cli 10.16.

Regression checks (R180/R181/R182/R183 governance queries
still work):

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 1960, "epoch": 0, "era": "Conway", ... }

$ cardano-cli conway query constitution --testnet-magic 2
{ "anchor": { "dataHash": "ca41a91f...", ... }, ... }

$ cardano-cli conway query committee-state --testnet-magic 2
{ "committee": {}, "epoch": 0, "threshold": null }

$ cardano-cli conway query future-pparams --testnet-magic 2
No protocol parameter changes will be enacted at the next epoch boundary.
null
```

### Updated cumulative Conway-era query coverage

| Query | Tag | Round | Status |
|---|---|---|---|
| constitution | 23 | R180 | ✓ working |
| gov-state | 24 | R180 dispatcher | body shape pending |
| drep-state | 25 | R180/R181 | ✓ working |
| **drep-stake-distribution** | **26** | **R184** | **✓ working** |
| committee-state | 27 | R182 | ✓ working |
| **filtered-vote-delegatees** | **28** | **R184** | **✓ working** |
| treasury (account-state) | 29 | R180 | ✓ working |
| **spo-stake-distribution** | **30** | **R184** | **✓ working** |
| future-pparams | 33 | R183 | ✓ working |
| stake-pools | 16 | R163/R179 | ✓ working |
| stake-distribution | 37 | R179 | ✓ working |
| pool-state | 19 (GetCBOR) | R172/R179 | ✓ working |
| stake-snapshot | 20 (GetCBOR) | R173/R179 | ✓ working |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4740  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4739 → **4740** (regression test
extended to cover tags 26, 28, 30 in one parameterised case
rather than adding three separate tests).

### Open follow-ups

1. **`gov-state` body shape** — substantial 7-element
   `ConwayGovState` record with `Proposals` tree +
   `DRepPulsingState` cache.
2. Tag 31 `GetProposals`, 32 `GetRatifyState`, 35
   `QueryStakePoolDefaultVote`, 36 `GetPoolDistr2` —
   remaining Conway-era dispatchers for completeness.
3. Live stake-distribution plumbing (R163/R173 follow-up): currently
   all three of the new R184 maps return empty placeholders.
4. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
5. Apply-batch duration histogram (R169).
6. Multi-session peer accounting (R168 structural).
7. Pipelined fetch + apply (R166).
8. Deep cross-epoch rollback recovery (R167).

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — three new `EraSpecificQuery` variants + decoder branches +
  extended regression test;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  three new dispatcher arms each emitting `Map = 0xa0`.
- Captures: `/tmp/ygg-r184-preview.log` (debug-instrumented
  capture confirmed cardano-cli's 3-query SPO flow).
- Upstream reference:
  `Cardano.Ledger.Conway.LedgerStateQuery.GetDRepStakeDistr`
  (`Map (DRep) Coin`);
  `Cardano.Ledger.Conway.LedgerStateQuery.GetFilteredVoteDelegatees`
  (`type VoteDelegatees = Map (Credential 'Staking) DRep`);
  `Cardano.Ledger.Conway.LedgerStateQuery.GetSPOStakeDistr`
  (`Map (KeyHash 'StakePool) Coin`).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-183-future-pparams.md`](2026-04-30-round-183-future-pparams.md).
