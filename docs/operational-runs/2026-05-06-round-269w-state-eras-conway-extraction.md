## Round 269w — `state.rs` per-era split: twenty-third slice (Conway apply) — Phase γ per-era arc complete

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 final per-era slice)

### Slice scope

Extracted 767 source lines from `state.rs::impl LedgerState::apply_conway_block`
into `crates/ledger/src/state/eras/conway.rs` (805 lines including
imports and impl-block boilerplate). Wired
`pub(super) mod conway;` into the eras `mod.rs`.

This is the final per-era apply extraction. After R269w, every era's
block-application method lives in its own dedicated file under
`state/eras/<era>.rs`, mirroring upstream's `Cardano.Ledger.<Era>.LedgerState`
file structure.

Conway is the largest and most complex era. Compared with Babbage:

- Adds the governance pipeline: voting procedures (`apply_conway_votes`),
  vote-target validation (`validate_conway_vote_targets`), voter
  permissions (`validate_conway_voter_permissions`), proposal validation
  (`validate_conway_proposals`), unelected committee voter detection
  (`validate_unelected_committee_voters`), unregistered DRep cleanup
  (`collect_conway_unregistered_drep_voters`),
  treasury-withdrawal-vs-current-treasury (`validate_conway_current_treasury_value`),
  and withdrawal-target delegation (`validate_withdrawals_delegated`).
- Adds DRep activity: `update_dormant_drep_expiries`,
  `touch_drep_activity_for_certs`, `remove_conway_drep_votes` for
  cert-driven DRep state transitions.
- Conway-specific PV gates: `conway_post_pv10` (PV > 10 features) and
  `disjoint_ref_inputs_enforced` (Conway-only ref-input vs spending-input
  disjointness rule; Babbage allowed overlap).
- BBODY block-level reference-script size limit
  (`BodyRefScriptsSizeTooBig`) with PV-aware running-UTxO accumulation
  at PV>10 (mirrors upstream `totalRefScriptSizeInBlock`).
- `conway_governance_state_after_certificates` post-cert governance
  state simulator for treasury checks.

### Edit-tooling note: full Python-driven extraction

The 767-line function was beyond practical Edit reach. Used Python to
parse the body, detect helper-fn calls via regex, generate the import
header automatically, and write the era file in one pass:

```python
# Auto-detect phase1_validation helpers, accumulate_mir_from_certs,
# apply_certificates_and_withdrawals_with_future, and conway-specific
# free fns by scanning the body text for `name(` patterns.
```

Initial run missed five Conway helpers (`conway_post_pv10`,
`disjoint_ref_inputs_enforced`, `update_dormant_drep_expiries`,
`remove_conway_drep_votes`, `touch_drep_activity_for_certs`) because
they're called in less-conventional positions (e.g. as boolean guards
in `if`-conditions rather than direct expression statements). Added
them manually after the first compile-error report. Lesson for any
future bulk per-era / per-rule extraction: scan the entire body's
identifier set against the set of state.rs's private free fns rather
than relying on syntactic position heuristics.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 6,916 | 6,147 | −769 |
| `crates/ledger/src/state/eras/mod.rs` | 23 | 24 | +1 |
| `crates/ledger/src/state/eras/conway.rs` | (new) | 805 | +805 |

Dropped now-unused `use crate::eras::conway::ConwayTxBody;` from
state.rs (kept inside conway.rs for the qualified type annotation in
the decode loop).

### Cumulative R269 progress — Phase γ per-era arc complete

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269 – R269p | (16 sibling submodules + cbor codec) | 4,295 | 8,409 |
| R269q (Byron) | `state/eras/byron.rs` | 32 | 8,377 |
| R269r (Shelley) | `state/eras/shelley.rs` | 189 | 8,188 |
| R269s (Allegra) | `state/eras/allegra.rs` | 188 | 8,000 |
| R269t (Mary) | `state/eras/mary.rs` | 182 | 7,818 |
| R269u (Alonzo) | `state/eras/alonzo.rs` | 424 | 7,394 |
| R269v (Babbage) | `state/eras/babbage.rs` | 478 | 6,916 |
| **R269w (Conway)** | **`state/eras/conway.rs`** | **769** | **6,147** |

**Net `state.rs` reduction: 12,704 → 6,147 lines (−6,557, ~52 %)**
across 23 sibling files plus a `state/eras/` subdirectory containing
all 7 era files (one per Cardano era).

### Verification gates

```
cargo fmt --all -- --check       # clean (after rustfmt-applied tweak)
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed
```

### Stop point — Phase γ per-era arc closed; R269 series essentially done

After R269w, `state.rs` contains:

- The `LedgerState` struct definition (~140 lines).
- `impl LedgerState` accessor / builder / lifecycle methods (~95 methods).
- Cross-era helpers: `record_block_producer`, `apply_block_validated`
  (the orchestrator that dispatches to the per-era `apply_<era>_block`),
  `apply_pending_pparam_updates`, `validate_ppup_proposal`,
  `collect_pparam_proposals`, etc.
- Free helper fns at module level: `phase2_failure_reason`,
  `apply_certificates_and_withdrawals_with_future`,
  `accumulate_mir_from_certs`, `validate_conway_voters` and family,
  `apply_conway_votes`, `apply_scheduled_genesis_delegations`, etc.

These cross-cutting items are not era-specific; they could be carved
into `state/<concern>.rs` files in a future round but are not in scope
for the per-era arc. Phase γ R269 series moves to the next agenda item:
R270 (network governor split — `crates/network/src/governor.rs` →
`governor/{churn,root_peers,ledger_peers,peer_metric,public_root_peers,types}.rs`).

### Session summary across 8 rounds (R269p → R269w + R266d)

This session extracted 7 per-era apply files plus the `LedgerState`
CBOR codec, plus added Gap BP cost-model loading fixtures:

| Round | Slice | state.rs Δ | tests Δ |
|---|---|---|---|
| R269p | LedgerState CBOR codec | −307 | 0 |
| R266d | Gap BP cost-model byte-equal + variant-selection fixtures | 0 | +4 |
| R269q | Byron apply | −32 | 0 |
| R269r | Shelley apply | −189 | 0 |
| R269s | Allegra apply | −188 | 0 |
| R269t | Mary apply | −182 | 0 |
| R269u | Alonzo apply | −424 | 0 |
| R269v | Babbage apply | −478 | 0 |
| R269w | Conway apply | −769 | 0 |
| **Total** | | **−2,569** | **+4** |

**Cumulative R269 reduction: 12,704 → 6,147 lines (−52 %)** across 24
sibling files / per-era files. Tests: 4,851 → 4,855.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R269w
- Strategic envelope: `~/.claude/plans/dapper-giggling-haven.md`
  §R272 per-era ledger rules split (R269 is the in-state-scoped
  precursor to that broader rules carve)
- Upstream Conway rules:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/{Bbody,Ledger,Utxow,Utxo,Utxos,Gov,GovCert,Cert,Certs,Deleg,Pool,NewEpoch,Epoch,Tickf,Mempool,HardFork,Enact,Ratify}.hs`
- Upstream Conway BBODY ref-script size limit:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Bbody.hs`
  (`BodyRefScriptsSizeTooBig`, `totalRefScriptSizeInBlock`)
