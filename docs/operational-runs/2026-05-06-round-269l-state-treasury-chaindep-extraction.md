## Round 269l — `state.rs` per-rule split: twelfth slice (bundled `AccountingState` + `ChainDepStateContext`)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 twelfth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve. Per advisor guidance, the two
remaining tiny pure-data extractions are bundled into one round
because each is a sub-50-line file with no methods worth a separate
operational-runs doc:

- **`state/treasury.rs`** — `AccountingState` (treasury + reserves
  pots).
- **`state/chain_dep.rs`** — `ChainDepStateContext` (sidecar nonce +
  OCert counter mirror).

Bundling keeps the round count meaningful — each prior R269 slice
shipped one cohesive structural concern; these two are both single-
struct sidecars, and shipping them together respects "one round =
one stoppable unit of structural progress."

### Slice scopes

#### `state/chain_dep.rs` (~70 lines)

- `pub struct ChainDepStateContext` — 6 `Nonce` fields
  (`evolving_nonce`, `candidate_nonce`, `epoch_nonce`,
  `previous_epoch_nonce`, `lab_nonce`, `last_epoch_block_nonce`)
  plus `opcert_counters: BTreeMap<[u8; 28], u64>`.
- `impl Default for ChainDepStateContext` — initialises all nonces
  to `Nonce::Neutral` and an empty OCert-counter map.

Mirrors upstream
`Ouroboros.Consensus.Protocol.Praos.PraosState`. The `crates/consensus`
crate owns the canonical `NonceEvolutionState` / `OcertCounters` types
but cannot be imported from `crates/ledger` without inverting the
dependency direction; the runtime translates from those types into
this snapshot-side mirror at snapshot capture time so LSQ
`query protocol-state` dispatchers can serve live nonces and OCert
counters.

#### `state/treasury.rs` (~49 lines)

- `pub struct AccountingState` — `treasury: u64` and `reserves: u64`.
- `impl CborEncode/CborDecode for AccountingState` (2-element CBOR
  array).

Mirrors upstream
`Cardano.Ledger.Shelley.LedgerState::esAccountState`. The two pots
the protocol moves lovelace between (rewards distribution, MIR
transfers, treasury withdrawals, monetary expansion via ρ).

### Wiring

`state.rs` adds `pub mod chain_dep; pub use chain_dep::ChainDepStateContext;`
and `pub mod treasury; pub use treasury::AccountingState;` so all
external callers (`lib.rs` re-exports, sibling submodules, `epoch_boundary.rs`,
`stake.rs`) keep their existing paths.

### Trimmed unused imports

`crate::types::Nonce` was the only `Nonce` user in `state.rs` body;
after extraction it is reachable only via `state/chain_dep.rs`'s
explicit import. Removed from state.rs's `use crate::types::{...}`
list.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 9,257 | 9,180 | −77 |
| `crates/ledger/src/state/chain_dep.rs` | (new) | 70 | +70 |
| `crates/ledger/src/state/treasury.rs` | (new) | 49 | +49 |

The `+42` net (119 − 77) is the two new files' module-level docstrings +
imports. The treasury extraction also dropped a 3-line section divider
(`// ---- TreasuryState — ...`) since the sub-module's docstring
serves the same purpose.

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
| R269k (CommitteeState)  | `state/committee_state.rs` (394) | 364 | 9,257 |
| **R269l (Treasury+ChainDep)** | **`state/{treasury,chain_dep}.rs` (49+70)** | **77** | **9,180** |

Net `state.rs` reduction so far: **12,704 → 9,180 lines (−3,524, ~28 %)**
with thirteen sibling files.

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
| R269 a–k | shipped | 11 sibling state submodules carved (~3,447 lines moved) |
| **R269l** | **this round** | `state/{treasury,chain_dep}.rs` extracted (bundled): 119 lines moved across 2 sidecar pure-data types. State.rs cumulative reduction 3,524 lines (12,704 → 9,180). |

### Visibility-debt note

Across R269 a–l, several private fields and free functions were
promoted from `fn` / private-field to `pub(super) fn` /
`pub(super) field` so that newly extracted submodules and their
sibling tests retained access. Those promotions are accumulated debt:
the fields are now structurally available to all `state/` siblings
even when only the in-impl mutators that needed them require it.
A follow-on cleanup round (queued after the carve settles) should
re-tighten visibility: any field that no sibling reads can revert to
private, and any helper fn that only the originating submodule uses
can drop the `pub(super)`. Not a blocker for further slices — but
worth a single audit pass once `LedgerState` is also carved out.

### Stop point — PPUP / `LedgerState` require planning, not slicing

After R269l, the remaining queue is no longer "small slice + proceed +
small slice + proceed":

- **PPUP top-of-file helpers** (~1,600 lines total in state.rs lines
  19–267, after the R269 a–l mod declarations) are a mix of
  `PpupSlotContext`, `pv_can_follow`, `overlay_step`,
  `is_overlay_slot_for_blocks_made`, plus the
  `encode_optional_*` / `decode_optional_*` family that other
  submodules call via `super::`. Extracting them needs a scope
  decision: one big `state/ppup.rs`? Or split into
  `state/ppup/{state,validate,helpers}.rs` mirroring the upstream
  `Cardano.Ledger.Shelley.Rules.Ppup` rule structure?
- **`LedgerStateSnapshot`** (~600 lines) and
  **`LedgerStateCheckpoint`** (~400 lines) are sidecar
  capture/restore views of `LedgerState`; they are clean cuts but
  large enough to justify per-type files.
- **`LedgerState` itself** (~6,500 lines remaining) is the bulk and
  is the orchestrator: it owns every sub-module's state as a
  `pub` field, exposes hundreds of methods, and sequences era-
  specific `apply_*_block` paths. Carving it isn't a copy-paste; it
  needs a structural plan (split impl block by Conway rule? per
  era? leave the type and just move methods?).

These should not just be "next slice — proceed". They need a
deliberate scope decision before another R-round starts.

### Next R-round options (require user decision, not auto-proceed)

| Option | Approx effort | Surface change |
|---|---|---|
| (a) PPUP helpers slice (`state/ppup.rs`) | ~1 day if monolithic; ~2 days if sub-split per upstream rule | ~1,600 lines moved |
| (b) `LedgerStateSnapshot` + `LedgerStateCheckpoint` per-type files | ~1 day | ~1,000 lines moved; `state.rs` becomes mostly `LedgerState` |
| (c) `LedgerState` carve (the structural one) | ~2–3 days; explicit pre-design pass needed | ~6,500-line refactor; touches every era-apply path; biggest risk surface in R269 |
| (d) Pivot back to **Gap BP root cause** (R266d) | operator-time wall-clock for Haskell preview sync | unblocks the single open protocol-parity gap |
| (e) Pivot to **R267 mainnet endurance rehearsal** | operator-time wall-clock (24 h+) | unblocks mainnet-side parity proof |

### References

- R269 a–k closures: `2026-05-06-round-269{,b,c,d,e,f,g,h,i,j}-state-*.md`
- Plan: `~/.claude/plans/dapper-giggling-haven.md`
- Upstream PraosState:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/Ouroboros/Consensus/Protocol/Praos/PraosState.hs`
- Upstream esAccountState:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs`
