## Round 269 — `crates/ledger/src/state.rs` per-rule split: first slice (MIR)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 first slice — strict 1:1 with upstream Haskell)

### Context

Per the approved 4-dimensional parity plan, Phase γ carves the 12,704-line
`crates/ledger/src/state.rs` into per-rule submodules under
`crates/ledger/src/state/` mirroring upstream
`Cardano.Ledger.Conway.Rules.*` and `Cardano.Ledger.Shelley.Rules.*` /
`Cardano.Ledger.Shelley.LedgerState`. The end target is one Rust file per
upstream rule module.

This round ships the first natural slice — the **MIR (Move Instantaneous
Rewards)** state.

### Slice scope

Extracted the `InstantaneousRewards` struct + its `is_empty` / `clear`
methods + its `CborEncode` / `CborDecode` impls (~110 source lines) from
`state.rs` lines 2467–2575 into new file
`crates/ledger/src/state/mir.rs`.

`state.rs` keeps the field definition (`pub instantaneous_rewards:
InstantaneousRewards`) and its CBOR codec wiring inside
`LedgerState::encode_cbor` / `decode_cbor` — only the type + impl block
moves out. A `pub use mir::InstantaneousRewards;` re-export at the top
of `state.rs` preserves the original public API path
(`yggdrasil_ledger::InstantaneousRewards`).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/ledger/src/state/mir.rs::InstantaneousRewards` | `Cardano.Ledger.Shelley.LedgerState::InstantaneousRewards` |
| `crates/ledger/src/state/mir.rs::InstantaneousRewards::clear` | (per-epoch reset performed inside `Cardano.Ledger.Shelley.Rules.Mir::mirTransition`) |

The MIR per-epoch processing rule itself (`Cardano.Ledger.Shelley.Rules.Mir`)
lives at `crates/ledger/src/epoch_boundary.rs::apply_mir_at_epoch_boundary`
— not moved in this round; it stays where epoch-boundary orchestration
lives. Future R-rounds can carve `epoch_boundary.rs` along upstream
`Cardano.Ledger.Conway.Rules.*` lines.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 12,704 | 12,596 | −108 |
| `crates/ledger/src/state/mir.rs` | (new) | 123 | +123 |

The `+15` net (15 = 123 − 108) is the new file's module-level docstring
(`//!`) + imports (`use ...`) — the actual code body is byte-identical to
the original section.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged from R266c)
```

No regression test added or modified — this is a pure code move; existing
tests cover the MIR semantics unchanged.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 / R266b / R266c | shipped | Gap BP narrowed to deep ScriptContext field encoding (operator-time-blocked) |
| **R269 first slice** | **this round** | First file carve from 12 704-line `state.rs`: `InstantaneousRewards` (~110 lines) extracted to `state/mir.rs` mirroring upstream `Cardano.Ledger.Shelley.LedgerState`'s MIR types. State.rs shrinks 108 lines; new file 123 lines (added imports + docstring). Per-rule extraction continues in subsequent R-rounds. |

### Next R269 slices (queued)

In rough order of cleanest cuts (clear divider, minimal cross-references):

1. **`state/ratify.rs`** — Conway RATIFY rule tally engine (`VoteTally`,
   `ratify_action`, accepted-by-CC/DRep/SPO predicates). Section
   already self-contained at lines 11926–12594 with its own `use`
   statements. ~669 lines. Mirrors `Cardano.Ledger.Conway.Rules.Ratify`.
2. **`state/enact.rs`** — Conway ENACT rule (`enact_gov_action` family
   + `EnactState`-related helpers). ~770 lines. Mirrors
   `Cardano.Ledger.Conway.Rules.Enact`.
3. **`state/ppup.rs`** — PPUP helpers (the section at the top of
   `state.rs` lines 22–1691). ~1,670 lines. Mirrors
   `Cardano.Ledger.Shelley.Rules.Ppup` /
   `Cardano.Ledger.Shelley.Rules.PoolReap`.
4. Phase-1 transaction validation helpers, deposit pot, treasury state
   — natural standalone slices for follow-on rounds.

Each subsequent slice is a standalone R-round (per per-round approval
gate) so that any introduced regression is bounded to one slice.

### References

- Plan: `~/.claude/plans/dapper-giggling-haven.md`
- Approved per-round authorization model
- Upstream MIR rule:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Mir.hs`
- Upstream `InstantaneousRewards` record:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs`
