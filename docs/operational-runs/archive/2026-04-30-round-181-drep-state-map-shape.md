## Round 181 — DRepState LSQ Map shape

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close the most tractable item from R180's body-shape follow-up
list: align yggdrasil's `GetDRepState` (tag 25) response with
cardano-cli's expected CBOR map shape so
`cardano-cli conway query drep-state --all-dreps` decodes
end-to-end.

### Code change

`node/src/local_server.rs`:

- New helper `encode_drep_state_for_lsq(snapshot)` that emits
  the snapshot's `DrepState` as a CBOR **map** (`encCBOR @(Map a b)`)
  instead of the storage-format array-of-pairs that
  `DrepState::encode_cbor` produces.
- `GetDRepState` dispatcher arm switched to use the new helper
  (R180 routed through `snapshot.drep_state().encode_cbor()`
  which cardano-cli rejected at depth 3 with `expected map len
  or indef`).

The credential-set filter parameter remains accepted but not
applied — cardano-cli filters client-side after decoding the
full map.

### Operational verification

After 15s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query drep-state --all-dreps --testnet-magic 2
[]
```

Empty array (preview's chain at slot ~5K has no registered
DReps yet) — correct for the snapshot state, decoded
end-to-end through cardano-cli 10.16.

### Cumulative Conway-era query coverage

| Query | Tag | Round | End-to-end status |
|---|---|---|---|
| `conway query constitution` | 23 | R180 | **✓ working** (real Conway constitution data from preview) |
| `conway query drep-state --all-dreps` | 25 | R180 + **R181 shape fix** | **✓ working** |
| `conway query treasury` (uses `GetAccountState`) | 29 | R180 | **✓ working** (returns `0`) |
| `conway query stake-pools` | 16 | R163/R179 tag fix | ✓ working (real pool set after Shelley sync) |
| `conway query stake-distribution` | 37 | R179 | ✓ working (empty `{}`) |
| `conway query pool-state --all-stake-pools` | 19 (via GetCBOR) | R172/R179 | ✓ working |
| `conway query stake-snapshot --all-stake-pools` | 20 (via GetCBOR) | R173/R179 | ✓ working (real per-pool entries) |
| `conway query gov-state` | 24 | R180 (dispatcher) | dispatcher routes; body shape pending (complex 7-element `ConwayGovState`) |
| `conway query committee-state` | 27 | not yet | dispatcher pending |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4737  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

1. **`gov-state` body shape** — upstream `ConwayGovState` is a
   7-element list `[proposals, committee, constitution,
   currentPParams, previousPParams, futurePParams,
   drepPulsingState]`; each inner record is itself complex
   (`Proposals` has its own tree structure;
   `DRepPulsingState` is a pulsed-computation cache).
   Implementing this fully is a substantial scope —
   yggdrasil's `governance_actions()` currently emits only the
   proposals as a bare CBOR map, far from the full record.
2. **`committee-state` body shape + dispatcher** (tag 27) —
   `GetCommitteeMembersState coldCreds hotCreds statuses` is
   a 4-element query taking three filter sets; result is
   `CommitteeMembersState era` (its own complex record).
3. Live stake-snapshot plumbing (R163/R173 follow-up).
4. `GetGenesisConfig` ShelleyGenesis serialisation.
5. Apply-batch duration histogram (R169).
6. Multi-session peer accounting (R168 structural).
7. Pipelined fetch + apply (R166).
8. Deep cross-epoch rollback recovery (R167).

### References

- Code: [`node/src/local_server.rs`](node/src/local_server.rs)
  — new `encode_drep_state_for_lsq` helper + dispatcher arm
  switched to use it.
- Captures: `/tmp/ygg-r181-preview.log` (drep-state returns
  `[]`, constitution returns real data, treasury returns `0`).
- Upstream reference:
  `Cardano.Ledger.Conway.LedgerStateQuery.GetDRepState`
  (result type `Map (Credential 'DRepRole) (DRepState)`).
- Previous round:
  [`docs/operational-runs/2026-04-29-round-180-conway-governance-queries.md`](2026-04-29-round-180-conway-governance-queries.md).
