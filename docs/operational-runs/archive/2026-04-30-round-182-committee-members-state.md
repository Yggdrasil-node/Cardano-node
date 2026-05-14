## Round 182 — Conway `committee-state` LSQ dispatcher (tag 27)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Add `GetCommitteeMembersState` (tag 27) so
`cardano-cli conway query committee-state` decodes
end-to-end against yggdrasil.  Builds on R180/R181's
constitution / drep-state / treasury / account-state
dispatchers.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::GetCommitteeMembersState
  { cold_creds_cbor, hot_creds_cbor, statuses_cbor }` variant
  carrying the three filter-set parameters.
- `decode_query_if_current` recognises `(4, 27)` and slices
  the three filter-set CBOR items separately (the query is
  `[27, cold_set, hot_set, status_set]` — a 4-element list
  including the tag).

`node/src/local_server.rs`:

- New helper `encode_committee_members_state_for_lsq(snapshot)`
  emitting the upstream 3-element `CommitteeMembersState`
  record `[csCommittee_map, csThreshold, csEpochNo]`.  Threshold
  encoded as `StrictMaybe Nothing` (`0x80`, zero-element list)
  since yggdrasil's `CommitteeState` doesn't track the threshold
  separately.
- `GetCommitteeMembersState` dispatcher arm wraps the encoded
  body in the standard HFC envelope.  Filter-set parameters
  accepted but not applied — cardano-cli filters client-side.

### Operational verification

After 15s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query committee-state --testnet-magic 2
{
    "committee": {},
    "epoch": 0,
    "threshold": null
}
```

Empty committee, epoch 0, no threshold — correct empty state
for preview's chain at slot ~5K (no committee yet established).
Decoded end-to-end through cardano-cli 10.16.

Regression checks (all R180/R181 governance queries still
work):
- `conway query constitution` → real Conway constitution data
- `conway query drep-state --all-dreps` → `[]`
- `conway query treasury` → `0`

### Updated cumulative Conway-era query coverage

| Query | Tag | Round | Status |
|---|---|---|---|
| `conway query constitution` | 23 | R180 | ✓ working |
| `conway query gov-state` | 24 | R180 dispatcher | body shape pending |
| `conway query drep-state --all-dreps` | 25 | R180/R181 | ✓ working |
| `conway query treasury` | 29 | R180 | ✓ working |
| `conway query stake-pools` | 16 | R163/R179 | ✓ working |
| `conway query stake-distribution` | 37 | R179 | ✓ working |
| `conway query pool-state --all-stake-pools` | 19 (GetCBOR) | R172/R179 | ✓ working |
| `conway query stake-snapshot --all-stake-pools` | 20 (GetCBOR) | R173/R179 | ✓ working |
| **`conway query committee-state`** | **27** | **R182** | **✓ working** |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4738  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4737 → **4738** (added
`decode_recognises_committee_members_state_tag_27` covering
the 4-element wire form).

### Open follow-ups

1. **`gov-state` body shape** — upstream `ConwayGovState`
   7-element record; substantial work due to `Proposals` tree
   + `DRepPulsingState`.
2. Live stake-snapshot plumbing (R163/R173).
3. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
4. Apply-batch duration histogram (R169).
5. Multi-session peer accounting (R168).
6. Pipelined fetch + apply (R166).
7. Deep cross-epoch rollback recovery (R167).
8. Tag 21 `GetPoolDistr` / 22 `GetStakeDelegDeposits` /
   26 `GetDRepStakeDistr` / 30 `GetSPOStakeDistr` /
   31 `GetProposals` / 32 `GetRatifyState` /
   33 `GetFuturePParams` — additional dispatchers for
   completeness.

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — `EraSpecificQuery::GetCommitteeMembersState` variant +
  decoder branch + regression test;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  `encode_committee_members_state_for_lsq` helper + dispatcher
  arm.
- Captures: `/tmp/ygg-r182-preview.log`
  (`cardano-cli conway query committee-state` returns the
  3-element `{committee: {}, epoch: 0, threshold: null}`
  envelope end-to-end).
- Upstream reference:
  `Cardano.Ledger.Conway.LedgerStateQuery.GetCommitteeMembersState`;
  `Cardano.Ledger.Conway.Governance.CommitteeMembersState`
  (3-element record `[csCommittee, csThreshold, csEpochNo]`).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-181-drep-state-map-shape.md`](2026-04-30-round-181-drep-state-map-shape.md).
