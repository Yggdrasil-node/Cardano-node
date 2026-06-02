## Round 269f — `state.rs` per-rule split: sixth slice (`PoolState` + `RegisteredPool` + `PoolRelayAccessPoint`)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 sixth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve of `crates/ledger/src/state.rs`. After
`state/{mir,ratify,enact,deposit_pot,phase1_validation}.rs` shipped (R269 a–e),
this slice extracts the **stake-pool registry types** that mirror upstream
`Cardano.Ledger.State.PoolState`'s `StakePoolState` + Shelley `PState`.

### Slice scope

Extracted ~349 source lines from `state.rs` lines 233–581 into
`crates/ledger/src/state/pool_state.rs`:

- `pub struct RegisteredPool` (the upstream `StakePoolState` analog —
  carries `params`, `retiring_epoch`, `deposit`).
- `pub struct PoolRelayAccessPoint` (a directly dialable address+port pair
  derived from a pool's relay configuration).
- `impl CborEncode for RegisteredPool` (3-element array codec —
  forward-compat with legacy 2-element no-deposit form).
- `impl CborDecode for RegisteredPool` (accepts both 2-element and
  3-element CBOR layouts).
- `impl RegisteredPool` accessor methods (`params`, `retiring_epoch`,
  `deposit`, `relay_access_points`).
- `pub struct PoolState` (registry container modelling upstream
  `psStakePoolParams` + `psRetiring` together as `entries`,
  `psFutureStakePoolParams` as `future_params`; `psVRFKeyHashes` is
  computed on demand by `find_pool_by_vrf_key`).
- `impl CborEncode for PoolState` (CBOR map with key `0`=entries, key `1`=
  future_params; key `1` is omitted when `future_params` is empty).
- `impl CborDecode for PoolState` (accepts both new map layout and
  legacy bare-array layout).
- `impl PoolState` methods: `new`, `get`, `get_mut`, `is_registered`,
  `iter`, `len`, `is_empty`, `relay_access_points`, `register`,
  `register_with_deposit`, `retire`, `process_retirements`,
  `find_pool_by_vrf_key`, `future_params`, `adopt_future_params`.

`state.rs` keeps a `pub mod pool_state;` declaration with
`pub use pool_state::{PoolRelayAccessPoint, PoolState, RegisteredPool};`
so all external callers — `lib.rs`'s `pub use state::{PoolState, ...}`,
`stake.rs`'s `crate::state::{PoolState, ...}`, and `state/ratify.rs`'s
`super::PoolState` — keep their existing paths.

### Visibility adjustments

- `encode_optional_epoch_no` and `decode_optional_epoch_no` (top-of-state.rs
  helpers) were promoted from `fn` to `pub(super) fn` so the new submodule
  reaches them via `super::`.
- `RegisteredPool` and `PoolState` private fields were promoted from
  `field: T` to `pub(super) field: T` so `state/tests.rs` and the in-crate
  `state/{phase1_validation,ratify}` siblings keep their direct field access
  (which previously worked under state.rs's parent-private visibility).
- `state.rs` import list trimmed: `Relay`, `Ipv4Addr`, `Ipv6Addr`,
  `PoolParams` no longer referenced by the parent module after the move.
- `state/tests.rs` gained an explicit `PoolParams` import in its
  `crate::types::{...}` line — previously the test relied on state.rs's
  unqualified `PoolParams` re-leaked via `use super::*;`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/pool_state.rs::RegisteredPool` | `Cardano.Ledger.State.PoolState::StakePoolState` |
| `state/pool_state.rs::RegisteredPool::params` | upstream `spsParams` |
| `state/pool_state.rs::RegisteredPool::retiring_epoch` | upstream retirement-tracking field of `psRetiring` |
| `state/pool_state.rs::RegisteredPool::deposit` | upstream `spsDeposit` |
| `state/pool_state.rs::PoolState` | upstream Shelley `PState` |
| `state/pool_state.rs::PoolState::entries` | upstream `psStakePoolParams` ⊎ `psRetiring` |
| `state/pool_state.rs::PoolState::future_params` | upstream `psFutureStakePoolParams` |
| `state/pool_state.rs::PoolState::find_pool_by_vrf_key` | upstream `psVRFKeyHashes` (recomputed on demand) |
| `state/pool_state.rs::PoolState::adopt_future_params` | upstream SNAP rule's `psFutureStakePoolParams` → `psStakePoolParams` merge |
| `state/pool_state.rs::PoolState::process_retirements` | upstream pool-reap epoch-boundary phase |
| `state/pool_state.rs::PoolRelayAccessPoint` | (yggdrasil-only — derived dialable view of upstream `PoolRelay`) |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 10,718 | 10,369 | −349 |
| `crates/ledger/src/state/pool_state.rs` | (new) | 371 | +371 |
| `crates/ledger/src/state/tests.rs` | (unchanged) | +1 | +1 (`PoolParams` to explicit imports) |

The `+22` net (371 − 349) is the new file's module-level docstring +
imports — actual code body is byte-identical to the original section.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  (MIR)         | `state/mir.rs` (123)         | 110   | 12,596 |
| R269b (Ratify)      | `state/ratify.rs` (675)      | 657   | 11,939 |
| R269c (Enact)       | `state/enact.rs` (362)       | 343   | 11,602 |
| R269d (DepositPot)  | `state/deposit_pot.rs` (124) | 106   | 11,496 |
| R269e (Phase-1)     | `state/phase1_validation.rs` (817) | 792 | 10,714 |
| **R269f (PoolState)** | **`state/pool_state.rs` (371)** | **349** | **10,369** |

Net `state.rs` reduction so far: **12,704 → 10,369 lines (−2,335)** with
six sibling files. State.rs is now ~18 % smaller than its R269-start size.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged from R269e)
```

Pure code-move refactor. The pool-related tests in `state/tests.rs`
(`process_retirements`, `register_with_deposit`, `find_pool_by_vrf_key`,
`adopt_future_params`, CBOR round-trip with map and legacy layouts) keep
behaviour with two visibility tweaks (`pub(super)` on private fields)
and one explicit test import (`PoolParams`).

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 / R266b / R266c | shipped | Gap BP narrowed to deep ScriptContext field encoding (operator-time-blocked) |
| R269 a–e | shipped | `state/{mir,ratify,enact,deposit_pot,phase1_validation}.rs` extracted (2,008 lines moved) |
| **R269f** | **this round** | `state/pool_state.rs` extracted: 3 pool-registry types + their CBOR codecs + 14 methods (349 lines). State.rs cumulative reduction 2,335 lines (12,704 → 10,369). |

### Next R269 slices (queued)

1. **`state/reward_accounts.rs`** — `RewardAccountState` + `RewardAccounts`
   (~180 lines). Mirrors upstream `Cardano.Ledger.State.AccountState`.
2. **`state/stake_credentials.rs`** — `StakeCredentialState` +
   `StakeCredentials` (~260 lines). Mirrors upstream
   `Cardano.Ledger.State.CertState::dsState`.
3. **`state/drep_state.rs`** — `RegisteredDrep` + `DrepState`
   (~180 lines). Mirrors upstream Conway DRep registry.
4. **`state/committee_state.rs`** — `CommitteeAuthorization`,
   `CommitteeMemberState`, `CommitteeState` (~330 lines). Mirrors
   upstream Conway constitutional-committee tracking.
5. **`state/governance_action_state.rs`** — `GovernanceActionState`
   (~250 lines). Mirrors upstream `Cardano.Ledger.Conway.Governance::GovActionState`.
6. **`state/treasury.rs`** — `AccountingState` (small, ~30 lines).
7. **`state/chain_dep.rs`** — `ChainDepStateContext` (~50 lines).
8. PPUP top-of-file helpers (`PpupSlotContext`, `pv_can_follow`,
   `overlay_step`, `is_overlay_slot_for_blocks_made`,
   `encode_optional_*` / `decode_optional_*` family) — natural
   final slice covering whatever remains at top of state.rs.
9. `LedgerState`, `LedgerStateSnapshot`, `LedgerStateCheckpoint` per-type
   files — the structural bulk of remaining state.rs.

### References

- R269 a–e closures: `2026-05-06-round-269{,b,c,d,e}-state-*.md`
- Plan: `docs/COMPLETION_ROADMAP.md`
- Upstream PoolState record:
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/State/PoolState.hs`
- Upstream PState (Shelley registry):
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs`
- Upstream POOL rule (re-registration → future_params):
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Pool.hs`
