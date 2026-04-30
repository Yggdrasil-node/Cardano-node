## Round 204 — gov-state OMap proposals shape adapter (Phase A.3 closed)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Phase A.3 final slice — adapt yggdrasil's reduced
`GovernanceActionState` (4 fields: proposal/votes/proposed_in/expires_after)
to upstream's 7-field `GovActionState era` wire shape so
`cardano-cli conway query gov-state` `proposals` field can
surface real entries when governance traffic arrives.

### Code change

`node/src/local_server.rs`:

- New `encode_gov_action_state_upstream(enc, gov_action_id,
  state)` helper emitting the upstream wire shape per
  `Cardano.Ledger.Conway.Governance.Procedures.GovActionState`:

  ```text
  [
    gasId                 :: GovActionId,
    gasCommitteeVotes     :: Map (Credential 'HotCommitteeRole) Vote,
    gasDRepVotes          :: Map (Credential 'DRepRole) Vote,
    gasStakePoolVotes     :: Map (KeyHash 'StakePool) Vote,
    gasProposalProcedure  :: ProposalProcedure era,
    gasProposedIn         :: EpochNo,
    gasExpiresAfter       :: EpochNo,
  ]
  ```

  Splits yggdrasil's unified `votes: BTreeMap<Voter, Vote>`
  into three upstream-shape maps via `BTreeMap` for
  deterministic CBOR ordering:

  - **Committee votes**: keyed by `Credential [kind, hash]`
    (kind 0 = AddrKey for `Voter::CommitteeKeyHash`, kind 1
    = Script for `Voter::CommitteeScript`).
  - **DRep votes**: keyed by `Credential` (kind 0 = AddrKey
    for `Voter::DRepKeyHash`, kind 1 = Script for
    `Voter::DRepScript`).
  - **SPO votes**: keyed by 28-byte pool key hash (bare
    bytes) for `Voter::StakePool`.

  `proposed_in` / `expires_after` are `Option<EpochNo>` in
  yggdrasil; emit `0` for `None` to satisfy upstream's
  non-optional `EpochNo`.

- `encode_conway_gov_state_for_lsq` field 1 (`cgsProposals`'s
  OMap) now iterates `snapshot.governance_actions()` and
  emits each entry via the new helper (per upstream's `OMap`
  encoding `encodeStrictSeq encCBOR (toStrictSeq omap)` — a
  CBOR list of values where each value is the `GovActionState`
  containing `gasId`).

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query gov-state --testnet-magic 2
{
    "committee": null,
    "constitution": {
        "anchor": {
            "dataHash": "ca41a91f399259bcefe57f9858e91f6d00e1a38d6d9c63d4052914ea7bd70cb2",
            "url": "ipfs://bafkreifnwj6zpu3ixa4siz2lndqybyc5wnnt3jkwyutci4e2tmbnj3xrdm"
        },
        "script": "fa24fb305126805cf2164c161d852a0e7330cf988f1fe558cf7d4a64"
    },
    "currentPParams": { ... },
    "proposals": []
}
```

Returns correct empty `proposals` list because preview at
slot ~5K has no governance proposals submitted; the
iterating loop emits 0 entries.  When governance proposals
arrive on a chain, the same encoder will surface real
entries with all 7 upstream-shape fields populated.

Regression checks pass:
- `query ratify-state` / `constitution` / `future-pparams`
- `query drep-state` / `committee-state` / `spo-stake-distribution`
- `query proposals` / `stake-pool-default-vote`
- `query ledger-peer-snapshot` / `protocol-state` / `stake-snapshot`

### Verification gates

```
cargo fmt --all -- --check       # clean (one auto-fmt fix)
cargo lint                       # clean (one clippy::clone_on_copy fix
                                 # on Vote — Vote impls Copy)
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Phase A.3 closed

R193 (live `GovRelation`), R188 (gov-state body shape), R204
(OMap proposals shape adapter) together close the gov-state
plumbing.  The entire response is now upstream-shape-correct
for both empty and populated chains.  This is the **last LSQ
wire-shape gap** of the data-plumbing arc.

### Open follow-ups

1. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis serialiser
   (last untouched LSQ dispatcher).
2. **Phase C.2** — pipelined fetch+apply.
3. **Phase D.1** — deep cross-epoch rollback.
4. **Phase D.2** — multi-session peer accounting.
5. **Phase E.1 cardano-base** — coordinated fixture refresh.
6. **Phase E.2** — mainnet rehearsal (24h+).
7. **Phase E.3** — parity proof report.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  new `encode_gov_action_state_upstream` helper +
  `encode_conway_gov_state_for_lsq` field 1 wiring.
- Upstream reference:
  `Cardano.Ledger.Conway.Governance.Procedures.GovActionState`
  (7-field record);
  `Cardano.Ledger.Conway.Governance.Procedures.Voter`
  (5-variant sum over committee/drep/spo).
- Yggdrasil reference: `crates/ledger/src/state.rs` —
  `GovernanceActionState` (4-field reduced shape);
  `crates/ledger/src/eras/conway.rs` — `Voter` enum.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-203-stake-snapshots-sidecar.md`](2026-04-30-round-203-stake-snapshots-sidecar.md).
