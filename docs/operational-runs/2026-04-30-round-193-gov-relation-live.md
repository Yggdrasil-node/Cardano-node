## Round 193 — Live `GovRelation` from `EnactState` (Phase A.3)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Phase A.3 first slice — wire live governance-action lineage
from yggdrasil's `EnactState` into the LSQ `gov-state` and
`ratify-state` responses, replacing the static 4-SNothing
placeholder for `GovRelation StrictMaybe`.  Yggdrasil already
tracks the prev-action IDs (R67's `enact_gov_action`); only
the LSQ encoder was emitting placeholders.

### Code change

`node/src/local_server.rs`:

- New `encode_strict_maybe_gov_action_id` helper emitting
  upstream `Cardano.Ledger.Conway.Governance.GovRelation` field
  shape: `SNothing → []` (empty list), `SJust id → [id_cbor]`
  (1-element list with `GovActionId`'s native CBOR encoding).
- `encode_enact_state_for_lsq` field 7 (`ensPrevGovActionIds`)
  now reads `EnactState::prev_pparams_update`,
  `prev_hard_fork`, `prev_committee`, `prev_constitution`
  (all already public fields per R67) and emits each via the
  new helper.
- `encode_conway_gov_state_for_lsq` field 1 (`cgsProposals`'s
  `GovRelation`) updated identically — same lineage data.
- OMap of proposals in `cgsProposals` remains empty pending a
  separate slice that adapts yggdrasil's reduced
  `GovernanceActionState` to upstream's 7-field
  `GovActionState era` shape.

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query gov-state --testnet-magic 2
{
    "committee": null,
    "constitution": {
        "anchor": { ... },
        "script": "..."
    },
    "currentPParams": { ... 31-element Conway PParams ... },
    "proposals": []
}

$ cardano-cli conway query ratify-state --testnet-magic 2
{
    "enactedGovActions": [],
    "expiredGovActions": [],
    "nextEnactState": {
        "committee": null,
        "constitution": { ... },
        ...
    }
}
```

Both queries decode end-to-end.  Preview's chain at slot ~3960
has no governance actions enacted, so all four prev-action IDs
are `SNothing` — this is **correct live data**, not a
placeholder: cardano-cli renders the lineage fields as
absent, matching upstream behaviour for chains without
governance traffic.

Once governance actions are enacted (mainnet, late-stage
preview, or test scenarios with submitted proposals), the
same encoders will surface the real lineage IDs without any
further code changes.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count stable at 4744.

### Open follow-ups

Continuing the data-plumbing arc from `/home/vscode/.claude/plans/clever-shimmying-quokka.md`:

1. **Phase A.2 — runtime nonce attach** (deferred): plumb
   live `NonceEvolutionState` + `OcertCounters` from sync
   layer into `LedgerStateSnapshot` via the R192
   `ChainDepStateContext` channel.
2. **Phase A.3 — gov-state OMap proposals** (next): adapt
   yggdrasil's reduced `GovernanceActionState` to upstream's
   7-field `GovActionState era` wire shape.
3. **Phase A.4 — drep+spo stake distributions**: wire from
   `DrepState::stake_distribution()` + pool-stake snapshots.
4. **Phase A.5 — ledger-peer-snapshot pool list**: wire from
   peer governor's big-ledger ranking.
5. **Phase A.6 — `GetGenesisConfig`**: ShelleyGenesis
   serialiser.
6. **Phase B** — R91 multi-peer dispatch storage livelock.
7. **Phase C/D/E** — sync perf, deep rollback, mainnet
   rehearsal.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  new `encode_strict_maybe_gov_action_id` helper +
  `encode_enact_state_for_lsq` field 7 +
  `encode_conway_gov_state_for_lsq` field 1 GovRelation.
- Upstream reference:
  `Cardano.Ledger.Conway.Governance.GovRelation` (4-tuple of
  `StrictMaybe (PrevGovActionId)`);
  `Cardano.Ledger.Conway.Governance.Internal.EnactState.ensPrevGovActionIds`.
- Yggdrasil reference: `EnactState::prev_pparams_update` /
  `prev_hard_fork` / `prev_committee` / `prev_constitution`
  (public fields, R67 lineage tracking).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-192-chain-dep-state-context.md`](2026-04-30-round-192-chain-dep-state-context.md).
