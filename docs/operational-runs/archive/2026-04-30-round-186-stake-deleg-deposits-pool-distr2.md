## Round 186 — Conway `GetStakeDelegDeposits` + `GetPoolDistr2` LSQ dispatchers (tags 22, 36)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close out the simpler remaining Conway-era LSQ dispatcher
gaps so the codec layer recognises every documented Conway
era-specific query tag — leaving only `gov-state` (tag 24)
and `ratify-state` (tag 32) as open *body shape* gaps
(both substantial records).

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::GetStakeDelegDeposits
  { stake_cred_set_cbor }` (tag 22).  Result type per upstream:
  `Map (Credential 'Staking) Coin` (per-credential delegation
  deposits).  Wire form: `[22, stake_cred_set]`.
- New `EraSpecificQuery::GetPoolDistr2
  { maybe_pool_hash_set_cbor }` (tag 36).  Result type per
  upstream: `PoolDistr` (2-element record `[map, NonZero
  Coin]`).  Same shape as `GetStakeDistribution2` (tag 37,
  R179) but with a `Maybe (Set PoolKeyHash)` filter
  parameter.  Wire form: `[36, maybe_pool_set]`.
- Decoder branches `(2, 22)` and `(2, 36)`.
- Regression test
  `decode_recognises_stake_deleg_deposits_and_pool_distr2_tags`
  pinning both wire forms.

`node/src/local_server.rs`:

- `GetStakeDelegDeposits` dispatcher emits empty CBOR map
  (`0xa0`).  Until yggdrasil tracks per-credential delegation
  deposits, this is the correct empty placeholder.
- `GetPoolDistr2` dispatcher emits `[map, 1]` (empty
  individual-stake map + 1-lovelace `pdTotalStake` placeholder
  to satisfy upstream's `NonZero Coin` requirement).
- Filter parameters carried for protocol compatibility but
  not applied — yggdrasil's response is the same regardless.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4742  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4741 → **4742** (one new regression
test added).

### Updated cumulative Conway-era query coverage

| Query / Tag | Round | Status |
|---|---|---|
| **stake-deleg-deposits / 22** | **R186** | **✓ wire-correct (empty placeholder)** |
| constitution / 23 | R180 | ✓ working |
| gov-state / 24 | R180 dispatcher | body shape pending (substantial) |
| drep-state / 25 | R180/R181 | ✓ working |
| drep-stake-distribution / 26 | R184 | ✓ working |
| committee-state / 27 | R182 | ✓ working |
| filtered-vote-delegatees / 28 | R184 | ✓ working (internal) |
| treasury (account-state) / 29 | R180 | ✓ working |
| spo-stake-distribution / 30 | R184 | ✓ working |
| proposals / 31 | R185 | ✓ working |
| ratify-state / 32 | — | open (substantial — 4-field record incl. EnactState) |
| future-pparams / 33 | R183 | ✓ working |
| ledger-peer-snapshot / 34 | — | open (LedgerPeerSnapshot with v15 SRV variant) |
| stake-pool-default-vote / 35 | R185 | ✓ working |
| **pool-distr2 / 36** | **R186** | **✓ wire-correct (empty placeholder)** |
| stake-pools / 16 | R163/R179 | ✓ working |
| stake-distribution / 37 | R179 | ✓ working |
| pool-state / 19 (GetCBOR) | R172/R179 | ✓ working |
| stake-snapshot / 20 (GetCBOR) | R173/R179 | ✓ working |

### Operational note

Tags 22 and 36 don't have direct `cardano-cli conway query`
subcommands — they're invoked internally by other queries
or by external tooling that talks the LSQ protocol directly.
The dispatchers are added as part of the Conway-era
completeness arc so any client sending these queries gets a
wire-valid response (empty placeholder) instead of the
fall-through `null` from `Unknown`.

### Open follow-ups

1. **`gov-state` body shape** (tag 24) — substantial 7-element
   `ConwayGovState` record per upstream (`Proposals` 2-tuple,
   `StrictMaybe Committee`, `Constitution`, current `PParams`,
   previous `PParams`, `FuturePParams` ADT, `DRepPulsingState`
   2-element `[PulsingSnapshot, RatifyState]`).  Each nested
   field has substantial encoding work; tackle as a dedicated
   round.
2. **`ratify-state` body shape** (tag 32) — 4-field record
   `[EnactState era, Seq GovActionState, Set GovActionId,
   Bool]`.  EnactState alone is substantial (committee,
   constitution, PParams, governance lineage).  Tackle as a
   dedicated round; will share encoder helpers with `gov-state`.
3. **`ledger-peer-snapshot` body shape** (tag 34) — operational
   query, returns `LedgerPeerSnapshot` with v15+ SRV variant
   selection.  Lower-priority for cardano-cli parity but useful
   for downstream peer-discovery tooling.
4. Live stake-distribution plumbing (R163/R173/R184 follow-up).
5. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
6. Apply-batch duration histogram (R169).
7. Multi-session peer accounting (R168 structural).
8. Pipelined fetch + apply (R166).
9. Deep cross-epoch rollback recovery (R167).

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — two new `EraSpecificQuery` variants + decoder branches +
  one regression test;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  two new dispatcher arms emitting `Map = 0xa0` and
  `[map, NonZero=1]` placeholders.
- Upstream reference:
  `Cardano.Ledger.Conway.LedgerStateQuery.GetStakeDelegDeposits`
  (`Set (Credential 'Staking) → Map (Credential 'Staking) Coin`);
  `Cardano.Ledger.Conway.LedgerStateQuery.GetPoolDistr2`
  (`Maybe (Set PoolKeyHash) → PoolDistr`).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-185-proposals-default-vote.md`](2026-04-30-round-185-proposals-default-vote.md).
