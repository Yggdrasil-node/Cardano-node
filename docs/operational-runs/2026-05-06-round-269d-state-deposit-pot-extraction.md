## Round 269d — `state.rs` per-rule split: fourth slice (`DepositPot`)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 fourth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve of `crates/ledger/src/state.rs`. After
`state/{mir,ratify,enact}.rs` shipped in R269 a–c, this slice extracts
the **`DepositPot`** aggregate-deposit accounting struct.

### Slice scope

Extracted ~106 source lines from `state.rs` lines 2143–2253 into
`crates/ledger/src/state/deposit_pot.rs`:

- `pub struct DepositPot` (key_deposits, pool_deposits, drep_deposits,
  proposal_deposits) — mirrors upstream `Obligations` record.
- 8 `add_*_deposit` / `return_*_deposit` methods (one pair per category).
- `total()` — mirrors upstream `sumObligation`.
- `impl CborEncode for DepositPot` (4-element array CBOR encoder).
- `impl CborDecode for DepositPot` (accepts 3-element legacy or 4-element
  current arrays — proposal_deposits defaults to 0 for the 3-element case).

`state.rs` keeps a `pub mod deposit_pot;` declaration with
`pub use deposit_pot::DepositPot;` so all external callers — `lib.rs`'s
`pub use state::DepositPot`, `node/src/commands/query.rs`'s LSQ
`DepositPot` query — keep their existing path
(`yggdrasil_ledger::DepositPot`).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/ledger/src/state/deposit_pot.rs::DepositPot` | `Cardano.Ledger.State.CertState::Obligations` |
| `crates/ledger/src/state/deposit_pot.rs::DepositPot::total` | `Cardano.Ledger.State.CertState::sumObligation` |
| `crates/ledger/src/state/deposit_pot.rs::DepositPot::key_deposits` | upstream `oblStake` |
| `crates/ledger/src/state/deposit_pot.rs::DepositPot::pool_deposits` | upstream `oblPool` |
| `crates/ledger/src/state/deposit_pot.rs::DepositPot::drep_deposits` | upstream `oblDRep` |
| `crates/ledger/src/state/deposit_pot.rs::DepositPot::proposal_deposits` | upstream `oblProposal` |

Field is wired into `LedgerState.deposits` and serialised by
`Cardano.Ledger.Shelley.LedgerState::utxosDeposited` upstream — the same
slot in yggdrasil's `LedgerState` CBOR codec.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 11,602 | 11,496 | −106 |
| `crates/ledger/src/state/deposit_pot.rs` | (new) | 124 | +124 |

The `+18` net (124 − 106) is the new file's module-level docstring +
imports — actual code body is byte-identical to the original section.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  (MIR)         | `state/mir.rs` (123)         | 110   | 12,596 |
| R269b (Ratify)      | `state/ratify.rs` (675)      | 657   | 11,939 |
| R269c (Enact)       | `state/enact.rs` (362)       | 343   | 11,602 |
| **R269d (DepositPot)** | **`state/deposit_pot.rs` (124)** | **106** | **11,496** |

Net `state.rs` reduction so far: **12,704 → 11,496 lines (−1,208)** with
four new sibling files.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged from R269c)
```

Pure code-move refactor — no test changes. The DepositPot LSQ query in
`node/src/commands/query.rs` and existing CBOR round-trip tests
continue to pass.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 / R266b / R266c | shipped | Gap BP narrowed to deep ScriptContext field encoding (operator-time-blocked) |
| R269 a–c | shipped | `state/{mir,ratify,enact}.rs` extracted (1,102 lines moved) |
| **R269d** | **this round** | `state/deposit_pot.rs` extracted: aggregate deposit accounting (106 lines). State.rs cumulative reduction 1,208 lines (12,704 → 11,496). Four sibling files now mirror upstream rule/state modules. |

### Next R269 slices (queued)

1. **`state/phase1_validation.rs`** — Phase-1 transaction validation
   helpers (`state.rs` lines ~10810 to end of Phase-1 section, ~790
   lines). Mirrors `Cardano.Ledger.Alonzo.Rules.Utxo::feesOK` and
   surrounding validation predicates. Largest remaining bounded slice.
2. **`state/ppup.rs`** — PPUP helpers section (`state.rs` lines
   45–~2140, ~2,098 lines). Mirrors `Cardano.Ledger.Shelley.Rules.Ppup`.
   Would dwarf prior slices — possibly best split into `ppup/{state,
   validate,apply}.rs` sub-files for ergonomics.
3. **`state/treasury.rs`** — `AccountingState` treasury/reserves
   tracking (small, ~30 lines if isolated).
4. Per-type files for `LedgerState`, `LedgerStateSnapshot`,
   `LedgerStateCheckpoint`, `PoolState`, etc. The remaining structural
   bulk in the middle of state.rs.

### References

- R269 first slice: `2026-05-06-round-269-state-mir-extraction.md`
- R269 second slice: `2026-05-06-round-269b-state-ratify-extraction.md`
- R269 third slice: `2026-05-06-round-269c-state-enact-extraction.md`
- Plan: `docs/COMPLETION_ROADMAP.md`
- Upstream Obligations record:
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/State/CertState.hs`
- Upstream `utxosDeposited`:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs`
