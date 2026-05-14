## Round 269j+k — `state.rs` per-rule split: tenth + eleventh slices (`GovernanceActionState` + Committee*)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 tenth & eleventh slices — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve. After nine prior slices
(`mir`, `ratify`, `enact`, `deposit_pot`, `phase1_validation`, `pool_state`,
`reward_accounts`, `stake_credentials`, `drep_state`), this round bundles
two adjacent extractions:

1. **`state/governance_action_state.rs`** — `GovernanceActionState`
   (the stored Conway proposal + votes + lifetime tracking).
2. **`state/committee_state.rs`** — `CommitteeAuthorization` +
   `CommitteeMemberState` + `CommitteeState`.

These two were interleaved in `state.rs` (CommitteeState struct →
GovernanceActionState bundle → CommitteeState CBOR + impl). Extracting
GovernanceActionState first made the Committee section contiguous,
which then admitted a clean second extraction.

### Slice scopes

#### R269j — `state/governance_action_state.rs` (~120 lines)

- `pub struct GovernanceActionState` — `proposal`, `votes`,
  `proposed_in`, `expires_after`.
- `impl CborEncode/CborDecode for GovernanceActionState` (4-element
  array; back-compat accepts legacy 2-element).
- `impl GovernanceActionState` — `new`, `new_with_lifetime`,
  `proposal`, `votes`, `proposed_in`, `expires_after`, `record_vote`.

Mirrors upstream
`Cardano.Ledger.Conway.Governance::GovActionState` (reduced —
yggdrasil tracks the live proposal + cast votes + lifetime triple
needed by the RATIFY rule and the EPOCH expire-action step).

#### R269k — `state/committee_state.rs` (~370 lines)

- `pub enum CommitteeAuthorization` (`CommitteeHotCredential` /
  `CommitteeMemberResigned`) + CBOR codec.
- `pub struct CommitteeMemberState` (`authorization`, `expires_at`)
  + CBOR codec (3-element new / 2-element legacy / bare-null oldest).
- `impl CommitteeMemberState` accessors + role checks: `new`,
  `with_term`, `expires_at`, `is_expired`, `authorization`,
  `hot_credential`, `resignation_anchor`, `is_resigned`,
  `is_enacted_member`, `set_authorization`.
- `pub struct CommitteeState` (`entries: BTreeMap<StakeCredential,
  CommitteeMemberState>`) + CBOR codec.
- `impl CommitteeState` — registry + lifecycle methods: `new`, `get`,
  `get_mut`, `is_member`, `iter`, `register`, `register_with_term`,
  `unregister`, `clear_membership`, `clear_all_membership`,
  `prune_non_members` (mirrors upstream `updateCommitteeState`), `len`,
  `is_empty`.

Combines upstream `Cardano.Ledger.Conway.Governance.Committee` (term-
tracking via `committeeMembers :: Map Credential EpochNo`) and
`csCommitteeCreds` (authorization-tracking) into a single map keyed by
cold credential — `expires_at` carries the term epoch,
`authorization` carries the hot-key/resignation state.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `governance_action_state.rs::GovernanceActionState` | `Cardano.Ledger.Conway.Governance::GovActionState` (reduced) |
| `governance_action_state.rs::record_vote` | upstream `addVote` from VOTING/PROPOSAL rules |
| `committee_state.rs::CommitteeAuthorization::CommitteeHotCredential` | upstream `CommitteeHotCredential` constructor of `csCommitteeCreds` value |
| `committee_state.rs::CommitteeAuthorization::CommitteeMemberResigned` | upstream `CommitteeMemberResigned` |
| `committee_state.rs::CommitteeMemberState` | combines upstream `committeeMembers` (term) + `csCommitteeCreds` (authorization) |
| `committee_state.rs::CommitteeMemberState::is_enacted_member` | yggdrasil-only — mirrors the `expires_at.is_some()` check upstream uses to distinguish enacted members from authorization-only entries |
| `committee_state.rs::CommitteeState::prune_non_members` | upstream `updateCommitteeState` (`Cardano.Ledger.Conway.Rules.Epoch`) |
| `committee_state.rs::CommitteeState::clear_all_membership` | upstream `NoConfidence` enactment effect on `committeeMembers` |

### Visibility adjustments

All struct fields promoted to `pub(super)` so the in-impl direct field
accesses (`member.expires_at = None` in `clear_membership` and friends,
`state.delegated_drep` in cleanup helpers) and any sibling-module
field reads continue to compile.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 9,737 | 9,257 | −480 |
| `crates/ledger/src/state/governance_action_state.rs` | (new) | 143 | +143 |
| `crates/ledger/src/state/committee_state.rs` | (new) | 394 | +394 |

The `+57` net (537 − 480) is the two new files' module-level
docstrings + imports.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  (MIR)             | `state/mir.rs` (123)              | 110   | 12,596 |
| R269b (Ratify)          | `state/ratify.rs` (675)           | 657   | 11,939 |
| R269c (Enact)           | `state/enact.rs` (362)            | 343   | 11,602 |
| R269d (DepositPot)      | `state/deposit_pot.rs` (124)      | 106   | 11,496 |
| R269e (Phase-1)         | `state/phase1_validation.rs` (817) | 792   | 10,714 |
| R269f (PoolState)       | `state/pool_state.rs` (371)       | 349   | 10,369 |
| R269g (RewardAccounts)  | `state/reward_accounts.rs` (193) | 176   | 10,199 |
| R269h (StakeCredentials)| `state/stake_credentials.rs` (280) | 254 | 9,950 |
| R269i (DrepState)       | `state/drep_state.rs` (236)       | 218   | 9,737 |
| R269j (GovActionState)  | `state/governance_action_state.rs` (143) | 116 | 9,621 |
| **R269k (CommitteeState)** | **`state/committee_state.rs` (394)** | **364** | **9,257** |

Net `state.rs` reduction so far: **12,704 → 9,257 lines (−3,447, ~27 %)**
with eleven sibling files.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged)
```

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 a–c | shipped | Gap BP narrowed (operator-time-blocked) |
| R269 a–i | shipped | 9 sibling state submodules carved (~3,005 lines moved) |
| **R269j+k** | **this round** | `state/governance_action_state.rs` (116 lines) + `state/committee_state.rs` (364 lines) extracted; bundled because their original placements were interleaved. State.rs cumulative reduction 3,447 lines (12,704 → 9,257). |

### Next R269 slices (queued)

1. **`state/treasury.rs`** — `AccountingState` (~30 lines).
2. **`state/chain_dep.rs`** — `ChainDepStateContext` (~50 lines).
3. PPUP top-of-file helpers (`PpupSlotContext`, `pv_can_follow`,
   `overlay_step`, `is_overlay_slot_for_blocks_made`,
   `encode_optional_*` / `decode_optional_*` family).
4. `LedgerStateSnapshot`, `LedgerStateCheckpoint` per-type files.
5. `LedgerState` itself (the structural bulk of the remaining state.rs).

### References

- R269 a–i closures: `2026-05-06-round-269{,b,c,d,e,f,g,h,i}-state-*.md`
- Plan: `~/.claude/plans/dapper-giggling-haven.md`
- Upstream GovActionState:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance.hs`
- Upstream CommitteeState + Committee record:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance.hs`
- Upstream `updateCommitteeState`:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Epoch.hs`
