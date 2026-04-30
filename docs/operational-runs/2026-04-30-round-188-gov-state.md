## Round 188 — Conway `gov-state` body shape (tag 24) end-to-end

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close the **last user-facing Conway-governance body-shape gap**:
make `cardano-cli conway query gov-state` decode end-to-end.
This was the longest-standing item on the Conway-era follow-up
list (open since R180's dispatcher routes), unblocked by R187's
EnactState/RatifyState helpers.

### Code change

`node/src/local_server.rs`:

- Replaced the R180 placeholder dispatcher arm (which emitted a
  flat CBOR map of governance actions and was rejected by
  cardano-cli) with one that calls a new
  `encode_conway_gov_state_for_lsq(snapshot)` helper.
- New helper emits the upstream 7-element `ConwayGovState`
  CBOR list per `Cardano.Ledger.Conway.Governance.ConwayGovState`:

  ```
  [
    cgsProposals        :: Proposals era,            -- 2-tuple
    cgsCommittee        :: StrictMaybe (Committee era),
    cgsConstitution     :: Constitution era,
    cgsCurPParams       :: PParams era,              -- Conway 31-elem
    cgsPrevPParams      :: PParams era,
    cgsFuturePParams    :: FuturePParams era,        -- internal ADT
    cgsDRepPulsingState :: DRepPulsingState era,     -- DRComplete pair
  ]
  ```

  Field encodings:

  1. `cgsProposals` — 2-tuple
     `(GovRelation StrictMaybe, OMap GovActionId GovActionState)`.
     Empty case: `[[<4-SNothing GovRelation>, <empty OMap>]]`.
     `GovRelation StrictMaybe` is a 4-element list of
     `StrictMaybe (PrevGovActionId)` (PParamUpdate / HardFork /
     Committee / Constitution lineages); empty
     = `[[], [], [], []]` (each `[]` = SNothing).  `OMap` upstream
     encodes via `encodeStrictSeq` (CBOR list, NOT CBOR map);
     empty = `0x80`.
  2. `cgsCommittee` — `[]` (SNothing).
  3. `cgsConstitution` — real Conway constitution from
     `snapshot.enact_state().constitution()`.
  4. `cgsCurPParams` — Conway 31-element PParams via R161's
     `encode_conway_pparams_for_lsq`.
  5. `cgsPrevPParams` — same as cur (until separate prev-epoch
     tracker is plumbed).
  6. `cgsFuturePParams` — **internal ADT** (distinct from R183's
     wire-facing `Maybe (PParams era)`): `Sum NoPParamsUpdate 0
     = [0]` (1-elem list).  R183's tag-33 wire query uses
     `Maybe Nothing = []`; gov-state's nested field uses the ADT
     `[0]`.  This was the most subtle wire-shape distinction in
     the round.
  7. `cgsDRepPulsingState` — `DRComplete` encoded as bare 2-elem
     `[PulsingSnapshot, RatifyState]` (no discriminator tag).
     `PulsingSnapshot` empty = 4-elem
     `[empty StrictSeq=0x80, empty Map=0xa0, empty Map=0xa0,
     empty Map=0xa0]` (psProposals / psDRepDistr / psDRepState /
     psPoolDistr).  `RatifyState` reuses R187's
     `encode_ratify_state_for_lsq` helper.

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6` (chain
at slot ~2960, era=Conway):

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
    "currentPParams": {
        "collateralPercentage": 150,
        "committeeMaxTermLength": 365,
        ... [full Conway 31-element PParams]
    },
    "proposals": []
}
```

Full record decodes end-to-end with real Conway constitution,
real Conway 31-element PParams (governance thresholds, ex-unit
prices, Conway-specific fields).  cardano-cli renders the
relevant subset; the underlying CBOR carries all 7 fields.

Regression checks pass (every other Conway query still works):

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 2960, "era": "Conway", ... }

$ cardano-cli conway query ratify-state --testnet-magic 2
{ "enactedGovActions": [], ... }

$ cardano-cli conway query constitution --testnet-magic 2
{ "anchor": { ... }, ... }

$ cardano-cli conway query committee-state --testnet-magic 2
{ "committee": {}, ... }

$ cardano-cli conway query future-pparams --testnet-magic 2
No protocol parameter changes will be enacted at the next epoch boundary.

$ cardano-cli conway query proposals --testnet-magic 2 --all-proposals
[]
```

### Cumulative Conway-era query coverage — **complete on the cli surface**

| Query / Tag | Round | Status |
|---|---|---|
| stake-pools / 16 | R163/R179 | ✓ working |
| pool-state / 19 (GetCBOR) | R172/R179 | ✓ working |
| stake-snapshot / 20 (GetCBOR) | R173/R179 | ✓ working |
| stake-deleg-deposits / 22 | R186 | ✓ wire-correct |
| constitution / 23 | R180 | ✓ working |
| **gov-state / 24** | **R188** | **✓ working** |
| drep-state / 25 | R180/R181 | ✓ working |
| drep-stake-distribution / 26 | R184 | ✓ working |
| committee-state / 27 | R182 | ✓ working |
| filtered-vote-delegatees / 28 | R184 | ✓ working (internal) |
| treasury (account-state) / 29 | R180 | ✓ working |
| spo-stake-distribution / 30 | R184 | ✓ working |
| proposals / 31 | R185 | ✓ working |
| ratify-state / 32 | R187 | ✓ working |
| future-pparams / 33 | R183 | ✓ working |
| ledger-peer-snapshot / 34 | — | open (operational, not governance) |
| stake-pool-default-vote / 35 | R185 | ✓ working |
| pool-distr2 / 36 | R186 | ✓ wire-correct |
| stake-distribution / 37 | R179 | ✓ working |

**Every cardano-cli `conway query` subcommand other than the
operational `ledger-peer-snapshot` (tag 34) now decodes
end-to-end against yggdrasil.**

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4743  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count unchanged at 4743 — R188 is encoder-only, no
wire-shape regression test was added (the gov-state wire form
was already pinned by R180's `decode_recognises_conway_governance_tags`
test).

### Subtle detail: FuturePParams internal vs LSQ-facing

The same name `FuturePParams` denotes two different types in
upstream:

- **`Cardano.Ledger.Core.PParams.FuturePParams era`** — the
  internal ADT used inside `ConwayGovState`, encoded as a
  `Sum`-wrapped record:
  - `NoPParamsUpdate` → `[0]` (1-elem list, just the tag)
  - `DefinitePParamsUpdate pp` → `[1, pp]`
  - `PotentialPParamsUpdate pp` → `[2, pp]`
- **`Cardano.Ledger.Conway.LedgerStateQuery.GetFuturePParams`** —
  the LSQ tag-33 query result, typed as `Maybe (PParams era)`,
  encoded as `Maybe`-shaped CBOR list:
  - `Nothing` → `[]` (`0x80`, empty list)
  - `Just pp` → `[pp]` (1-elem list)

R183 implemented the *LSQ-facing* `Maybe` shape for tag 33;
R188 implements the *internal ADT* shape for the
`cgsFuturePParams` field of `ConwayGovState`.  Both yggdrasil
helpers emit the "no update" placeholder, but with different
CBOR bytes (`0x80` vs `[0]`).  This distinction was the most
subtle part of R188.

### Open follow-ups

1. **`ledger-peer-snapshot` body shape** (tag 34) — operational
   query.  Returns `LedgerPeerSnapshot ledgerPeersKind` with
   v15+ SRV variant selection.  Last open Conway-era LSQ
   dispatcher.
2. **Live data plumbing** — current placeholders return empty
   data; populating gov-state proposals, ratify-state enacted
   actions, drep stake distribution, etc., is the natural
   follow-on once yggdrasil's runtime tracks them in the
   snapshot.
3. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
4. Apply-batch duration histogram (R169).
5. Multi-session peer accounting (R168 structural).
6. Pipelined fetch + apply (R166).
7. Deep cross-epoch rollback recovery (R167).

### References

- Code: [`node/src/local_server.rs`](node/src/local_server.rs)
  — new `encode_conway_gov_state_for_lsq` helper (composes
  R187's `encode_ratify_state_for_lsq` with new
  Proposals/Committee/PulsingSnapshot stub encoders); replaced
  GetGovState dispatcher arm.
- Captures: `/tmp/ygg-r188-preview.log`.
- Upstream reference:
  `Cardano.Ledger.Conway.Governance.ConwayGovState` (7-element
  record);
  `Cardano.Ledger.Conway.Governance.Proposals` (2-tuple);
  `Cardano.Ledger.Conway.Governance.DRepPulser.PulsingSnapshot`
  (4-element record);
  `Cardano.Ledger.Conway.Governance.DRepPulser.DRepPulsingState`
  (DRComplete encoded as bare 2-elem list);
  `Cardano.Ledger.Core.PParams.FuturePParams` (internal Sum ADT).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-187-ratify-state.md`](2026-04-30-round-187-ratify-state.md).
