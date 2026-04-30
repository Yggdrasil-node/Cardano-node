## Round 185 — Conway `proposals` + `stake-pool-default-vote` LSQ dispatchers (tags 31, 35)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Add two more Conway-era LSQ dispatchers so
`cardano-cli conway query proposals --all-proposals` and
`cardano-cli conway query stake-pool-default-vote
--spo-key-hash <hash>` decode end-to-end.  Continues the
Conway-governance dispatcher series after R180/R181/R182/R183/R184.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::GetProposals { gov_action_id_set_cbor }`
  (tag 31).  Result type per upstream: `Seq (GovActionState era)`
  (a CBOR list).  Wire form: `[31, gov_action_id_set]`.
- New `EraSpecificQuery::QueryStakePoolDefaultVote
  { pool_key_hash_cbor }` (tag 35).  Result type per upstream:
  `DefaultVote = DefaultNo (0) | DefaultAbstain (1) |
  DefaultNoConfidence (2)` (single CBOR uint).  Wire form:
  `[35, pool_key_hash]`.
- Decoder branches `(2, 31)` and `(2, 35)` extract the filter
  parameter.
- Regression test `decode_recognises_proposals_and_default_vote_tags`
  pinning both wire forms.

`node/src/local_server.rs`:

- `GetProposals` dispatcher emits empty CBOR list (`0x80`)
  — no pending proposals on a fresh-sync chain.
- `QueryStakePoolDefaultVote` dispatcher emits `DefaultNo (0)`
  as a single CBOR uint.  Until yggdrasil tracks per-pool
  default-vote registrations, this is the safest placeholder
  (matches upstream behaviour for un-registered SPOs).
- Filter parameters carried for protocol compatibility but
  not applied — cardano-cli filters/contextualises client-side.

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6` (chain
at slot ~7K, era=Conway):

```
$ cardano-cli conway query proposals --testnet-magic 2 --all-proposals
[]

$ cardano-cli conway query stake-pool-default-vote \
      --testnet-magic 2 --spo-key-hash 00000000000000000000000000000000000000000000000000000000
"DefaultNo"
```

Both decode end-to-end through cardano-cli 10.16.

Regression checks (R180/R181/R182/R183/R184 queries still work):

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 6960, "epoch": 0, "era": "Conway", ... }

$ cardano-cli conway query drep-stake-distribution --testnet-magic 2 --all-dreps
{}

$ cardano-cli conway query spo-stake-distribution --testnet-magic 2 --all-spos
[]

$ cardano-cli conway query future-pparams --testnet-magic 2
No protocol parameter changes will be enacted at the next epoch boundary.

$ cardano-cli conway query treasury --testnet-magic 2
0
```

### Updated cumulative Conway-era query coverage

| Query | Tag | Round | Status |
|---|---|---|---|
| constitution | 23 | R180 | ✓ working |
| gov-state | 24 | R180 dispatcher | body shape pending |
| drep-state | 25 | R180/R181 | ✓ working |
| drep-stake-distribution | 26 | R184 | ✓ working |
| committee-state | 27 | R182 | ✓ working |
| filtered-vote-delegatees | 28 | R184 | ✓ working (internal) |
| treasury (account-state) | 29 | R180 | ✓ working |
| spo-stake-distribution | 30 | R184 | ✓ working |
| **proposals** | **31** | **R185** | **✓ working** |
| ratify-state | 32 | — | open (substantial — 4-field record incl. EnactState) |
| future-pparams | 33 | R183 | ✓ working |
| **stake-pool-default-vote** | **35** | **R185** | **✓ working** |
| stake-pools | 16 | R163/R179 | ✓ working |
| stake-distribution | 37 | R179 | ✓ working |
| pool-state | 19 (GetCBOR) | R172/R179 | ✓ working |
| stake-snapshot | 20 (GetCBOR) | R173/R179 | ✓ working |

### Verification gates

```
cargo fmt --all -- --check       # clean (one auto-fmt of the
                                 # rust 2-line struct pattern in
                                 # local_server.rs)
cargo lint                       # clean
cargo test-all                   # passed: 4741  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4740 → **4741** (one new regression
test added).

### Open follow-ups

1. **`gov-state` body shape** — substantial 7-element
   `ConwayGovState` record with `Proposals` tree +
   `DRepPulsingState` cache.
2. Tag 32 `GetRatifyState` — substantial 4-field record
   `[EnactState era, Seq GovActionState, Set GovActionId,
   Bool]`; emitting a default value requires a working
   EnactState encoder matching upstream's wire shape.
3. Tag 36 `GetPoolDistr2` — additional Conway-era PoolDistr
   variant with explicit pool-id filter (similar shape to
   tag 37 `GetStakeDistribution2`).
4. Tag 22 `GetStakeDelegDeposits` — `Map (Credential 'Staking)
   Coin` per-credential delegation deposits.
5. Live stake-distribution plumbing (R163/R173/R184 follow-up).
6. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
7. Apply-batch duration histogram (R169).
8. Multi-session peer accounting (R168 structural).
9. Pipelined fetch + apply (R166).
10. Deep cross-epoch rollback recovery (R167).

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — two new `EraSpecificQuery` variants + decoder branches +
  one regression test;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  two new dispatcher arms emitting empty list / DefaultNo
  placeholders.
- Captures: `/tmp/ygg-r185-preview.log`.
- Upstream reference:
  `Cardano.Ledger.Conway.LedgerStateQuery.GetProposals`
  (`Set GovActionId → Seq (GovActionState era)`);
  `Cardano.Ledger.Conway.LedgerStateQuery.QueryStakePoolDefaultVote`
  (returns `DefaultVote`);
  `Cardano.Ledger.Conway.Governance.DefaultVote` (3-variant
  enum encoded as Word8: 0=DefaultNo, 1=DefaultAbstain,
  2=DefaultNoConfidence).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-184-drep-spo-stake-distr.md`](2026-04-30-round-184-drep-spo-stake-distr.md).
