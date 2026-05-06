## Round 269n — `state.rs` per-rule split: fourteenth slice (`LedgerStateCheckpoint`)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 fourteenth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve. After thirteen prior slices, this round
extracts **`LedgerStateCheckpoint`** — the storage / rollback-recovery
sidecar wrapper around `LedgerState`. Companion to `LedgerStateSnapshot`
(extracted in R269m): both wrap `LedgerState`, but `Snapshot` is a
read-only LSQ capture of the visible query surface, while `Checkpoint`
preserves a full restorable copy of the entire mutable state.

### Slice scope

Extracted ~50 source lines from two regions of `state.rs` (lines
543–552 + 869–907) into `crates/ledger/src/state/checkpoint.rs`:

- `pub struct LedgerStateCheckpoint` (single-field wrapper of
  `LedgerState`).
- `impl CborEncode/CborDecode for LedgerStateCheckpoint` (1-element
  array codec wrapping the full `LedgerState` codec).
- `impl LedgerStateCheckpoint` accessor + restore methods:
  `current_era`, `tip`, `restore` (returns a clone of the wrapped
  `LedgerState`).

Originally interleaved with `impl CborEncode for LedgerState` (the
~310-line core LedgerState codec), the checkpoint type's
declaration spanned two non-adjacent regions of state.rs. This round
removes both regions in a single pass and reconstructs the type
contiguously in the new file.

### Wiring

`state.rs` keeps a `pub mod checkpoint;` declaration with
`pub use checkpoint::LedgerStateCheckpoint;` so all external callers
(`lib.rs` re-export, `crates/storage/src/chain_db.rs` recovery seam,
`crates/storage/src/ocert_sidecar.rs` checkpoint storage) keep their
existing paths.

### Visibility adjustments

`LedgerStateCheckpoint::state: LedgerState` field promoted from
private `state: LedgerState` to `pub(super) state: LedgerState` so
`LedgerState::checkpoint(&self) -> LedgerStateCheckpoint` in state.rs
can construct via field-initialiser syntax (`LedgerStateCheckpoint {
state: self.clone() }`). Same pattern as R269m for `LedgerStateSnapshot`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/checkpoint.rs::LedgerStateCheckpoint` | (yggdrasil-only — upstream uses chain-DB-managed snapshots; yggdrasil makes the rollback-recovery wrapping explicit at the type level) |
| `state/checkpoint.rs::LedgerStateCheckpoint::restore` | upstream `LedgerDB.restore` semantics for chain-DB rollback |
| `state/checkpoint.rs::LedgerStateCheckpoint` CBOR codec | wraps the full upstream `LedgerState` CBOR encoding (matching yggdrasil's `crates/storage/src/chain_db.rs::checkpoint_persisted` write path) |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 8,816 | 8,766 | −50 |
| `crates/ledger/src/state/checkpoint.rs` | (new) | 70 | +70 |

The `+20` net is the new file's module-level docstring + imports.

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
| R269k (CommitteeState)  | `state/committee_state.rs` (394)  | 364   | 9,257 |
| R269l (Treasury+ChainDep)| `state/{treasury,chain_dep}.rs` (49+70) | 77 | 9,180 |
| R269m (Snapshot)        | `state/snapshot.rs` (406)         | 370   | 8,810 |
| **R269n (Checkpoint)**  | **`state/checkpoint.rs` (70)**    | **50**  | **8,766** |

Net `state.rs` reduction so far: **12,704 → 8,766 lines (−3,938, ~31 %)**
with fifteen sibling files.

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
| R269 a–m | shipped | 14 sibling state submodules carved (~3,888 lines moved) |
| **R269n** | **this round** | `state/checkpoint.rs` extracted: storage rollback-recovery sidecar (50 lines). State.rs cumulative reduction 3,938 lines (12,704 → 8,766). |

### Stop point — remaining work needs explicit scope decisions

After R269n, the queue is exclusively non-mechanical:

1. **PPUP top-of-file helpers** (~1,600 lines) — needs scope decision:
   monolithic `state/ppup.rs`, or sub-split into
   `state/ppup/{state,validate,helpers}.rs` mirroring upstream rule
   structure.
2. **`LedgerState` itself** (~6,500 lines remaining) — the structural
   orchestrator carve; needs deliberate pre-design pass (split impl
   block by Conway rule? per era? leave the type and just move
   methods into per-rule submodules?).

Both should be planned, not auto-proceeded.

### Visibility-debt note (cumulative)

Across R269 a–n, accumulated `pub(super)` field/fn promotions number
in the dozens. The follow-on cleanup round queued in R269l's note
remains queued — best done after `LedgerState` itself is also carved.

### Next R-round options (require user decision, not auto-proceed)

| Option | Approx effort | Surface change |
|---|---|---|
| (a) PPUP helpers slice — single `state/ppup.rs` | ~1 day | ~1,600 lines moved |
| (b) PPUP helpers slice — sub-split per upstream rule | ~2 days | ~1,600 lines moved into 3-4 sibling files |
| (c) `LedgerState` carve | ~2–3 days; explicit pre-design pass needed | ~6,500-line refactor; touches every era-apply path |
| (d) Visibility-debt cleanup (re-tighten `pub(super)`) | ~1 day | no functional change; safer post-LedgerState carve |
| (e) Pivot back to Gap BP root cause (R266d) | operator-time wall-clock | unblocks the single open protocol-parity gap |
| (f) Pivot to R267 mainnet endurance rehearsal | operator-time wall-clock | unblocks mainnet-side parity proof |

### References

- R269 a–m closures: `2026-05-06-round-269{,b,…,m}-state-*.md`
- Plan: `~/.claude/plans/dapper-giggling-haven.md`
- Storage caller of LedgerStateCheckpoint:
  `crates/storage/src/chain_db.rs::checkpoint_persisted`,
  `crates/storage/src/ocert_sidecar.rs`
