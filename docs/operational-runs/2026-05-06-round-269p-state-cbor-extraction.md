## Round 269p — `state.rs` per-rule split: sixteenth slice (LedgerState CBOR codec)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 sixteenth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve. After fifteen prior slices the inner
state.rs structure had reached the floor flagged in R269o: `LedgerState`
itself plus its impl block plus the codec. R269p picks option (c) from
the R269o stop-point menu — extract the mechanical 24-element array
codec to its own file. This is the safest of the four code-touching
options (a–d) because the codec is structurally self-contained: a
single `impl CborEncode for LedgerState` plus a single `impl CborDecode
for LedgerState`, both reading/writing the same 24 fields in the same
order, with length-conditional decode of trailing fields for
forward-compatibility with legacy 9-, 10-, and 12–23-element layouts
written by older yggdrasil releases.

The general-purpose `encode_optional_*` / `decode_optional_*` CBOR
helper family stays in `state.rs` for now (per the R269o note) — those
are used by every sibling sub-module via `super::` and re-locating them
would force a cascade of additional `pub(super) use` statements. The
extracted codec accesses them via `super::*`. They could land in their
own `state/cbor_helpers.rs` slice in a future round if the
visibility-debt cleanup decides to consolidate them.

### Slice scope

Extracted ~310 source lines from `state.rs` into
`crates/ledger/src/state/cbor.rs`:

- `impl CborEncode for LedgerState` — emits the 24-element array layout
  (current_era, tip, expected_network_id, governance_actions, pool_state,
  stake_credentials, committee_state, drep_state, reward_accounts,
  multi_era_utxo, shelley_utxo, protocol_params, deposit_pot, accounting,
  current_epoch, enact_state, gen_delegs, future_genesis_delegs, mir,
  retiring_pools, last_block_no, last_epoch_block_no, last_block_hash,
  blocks_made_current_epoch).
- `impl CborDecode for LedgerState` — length-tolerant decoder consuming
  the same 24 fields with safe defaults for any missing trailing
  positions (empty maps, `None`, `EpochNo(0)`, etc.). Forward-compatible
  with legacy 9-, 10-, and 12–23-element array layouts.

`state.rs` retains the `pub(super) mod cbor;` declaration at the bottom
of the existing module-declaration block (line 140). No `pub use`
re-export is needed — the codec is implemented as trait impls on
`LedgerState`, which are visible wherever the trait and `LedgerState`
itself are in scope. Because `state/cbor.rs` is a descendant of
`state.rs`, it can access `LedgerState`'s private fields directly
without `pub(super)` field promotions (per Rust's "private items visible
to defining module AND ITS DESCENDANTS" rule).

### Visibility note

Unlike R269e (`phase1_validation`) which used `pub(super) mod` + glob
`use` to keep 100+ in-state.rs callers unqualified, R269p uses
`pub(super) mod cbor;` without any `use cbor::*;` re-export. The codec
is consumed only through trait dispatch (`LedgerState::encode_cbor` /
`LedgerState::decode_cbor` via the `CborEncode` / `CborDecode` traits),
not by name, so no callers need adjustment.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/cbor.rs::impl CborEncode for LedgerState::encode_cbor` | upstream `Cardano.Ledger.Shelley.LedgerState::EncCBOR LedgerState` instance (and `Cardano.Ledger.Conway.LedgerState` derived instance) |
| `state/cbor.rs::impl CborDecode for LedgerState::decode_cbor` | upstream `Cardano.Ledger.Shelley.LedgerState::DecCBOR LedgerState` instance with length-tolerant tail decoding |

The codec spans the union of upstream's per-era `LedgerState` field
sets — Yggdrasil keeps a single `LedgerState` type across eras with
optional fields gated by `current_era`, where upstream uses
type-family-indexed era-specific records. The 24-element array is
yggdrasil's canonical wire shape; the length-conditional tail decode
preserves compatibility with every prior yggdrasil release's storage
format.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 8,716 | 8,409 | −307 |
| `crates/ledger/src/state/cbor.rs` | (new) | 342 | +342 |

The `+35` net is the new file's module-level docstring + imports +
`use super::*` glue.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  (MIR)             | `state/mir.rs` (123)              | 110   | 12,596 |
| R269b (Ratify)          | `state/ratify.rs` (675)           | 657   | 11,939 |
| R269c (Enact)           | `state/enact.rs` (362)            | 343   | 11,602 |
| R269d (DepositPot)      | `state/deposit_pot.rs` (124)      | 106   | 11,496 |
| R269e (Phase-1)         | `state/phase1_validation.rs` (825) | 792   | 10,714 |
| R269f (PoolState)       | `state/pool_state.rs` (371)       | 349   | 10,369 |
| R269g (RewardAccounts)  | `state/reward_accounts.rs` (193) | 176   | 10,199 |
| R269h (StakeCredentials)| `state/stake_credentials.rs` (280) | 254 | 9,950 |
| R269i (DrepState)       | `state/drep_state.rs` (236)       | 218   | 9,737 |
| R269j (GovActionState)  | `state/governance_action_state.rs` (143) | 116 | 9,621 |
| R269k (CommitteeState)  | `state/committee_state.rs` (394)  | 364   | 9,257 |
| R269l (Treasury+ChainDep)| `state/{treasury,chain_dep}.rs` (49+70) | 77 | 9,180 |
| R269m (Snapshot)        | `state/snapshot.rs` (407)         | 370   | 8,810 |
| R269n (Checkpoint)      | `state/checkpoint.rs` (70)        | 50    | 8,766 |
| R269o (PPUP helpers)    | `state/ppup.rs` (77)              | 50    | 8,716 |
| **R269p (LedgerState CBOR)** | **`state/cbor.rs` (342)** | **307** | **8,409** |

Net `state.rs` reduction so far: **12,704 → 8,409 lines (−4,295, ~34 %)**
with seventeen sibling files.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged)
```

**Note:** `cargo test-all` initially reported 14 failures in
`crates/crypto/tests/upstream_vectors.rs` reading vendored BLS / VRF
test vector files at "No such file or directory". Diagnosis: the
crypto test binary had been compiled when this checkout lived at
`/home/daniel/Cardano-node/`; the current location is
`/home/daniel/projects/fractionestate/Cardano-node/`. `CARGO_MANIFEST_DIR`
is baked into the binary at compile time, so stale build artifacts
held the old absolute path. `cargo clean -p yggdrasil-crypto`
followed by re-test cleanly resolves all 15 crypto vector tests.
Captured here so future agents who hit the same stale-cache pattern
after a project move recognise it without re-investigating.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 a–c | shipped | Gap BP narrowed (operator-time-blocked) |
| R269 a–o | shipped | 16 sibling state submodules carved (~3,988 lines moved) |
| **R269p** | **this round** | `state/cbor.rs` extracted: `LedgerState` CBOR codec (307 lines, 24-element array encoder + length-tolerant decoder). State.rs cumulative reduction 4,295 lines (12,704 → 8,409). |

### Stop point — pivot to Phase α (Gap BP)

After R269p, the inner state.rs decomposition has reached its terminal
floor for the per-type-and-codec carve. State.rs's remaining ~8,400
lines are dominated by:

- **`LedgerState` struct** (~140 lines).
- **`impl LedgerState`** method block (~7,800 lines, hundreds of
  per-era apply methods, governance helpers, query helpers, etc.).
- **A handful of private helper functions** still in state.rs body.
- **Free functions** like `accumulate_mir_from_certs`,
  era-min-protocol-major helpers, `conway_*` governance predicates.

Per the user-confirmed sequencing in `docs/COMPLETION_ROADMAP.md`,
**the next round is R266 (Gap BP root cause), not a continuation of
the LedgerState carve.** R266 is the only open code-level
protocol-parity blocker; with operator forensic time now available, it
takes priority over further γ-phase refactor work. The LedgerState
per-era split (option (a) from R269o) resumes as **R269q** after R266
closes.

### Next R-round (per playful-tickling-plum plan)

| Round | Scope | Effort |
|---|---|---|
| **R266** | Gap BP root cause: cost-model byte-diff fixture + `BuiltinSemanticsVariant` audit + per-builtin trace comparison vs upstream Haskell `db-analyser --repro-mempool-and-forge --target-slot 1462057 --tx 7bb40e40…3be5b9` + fix in `crates/plutus/src/{cost_model,machine,builtins}.rs` + regression test. Closes the only open code-level protocol-parity blocker. | ~3–6h operator wall-clock + 1–2 days agent fix |

### References

- R269 a–o closures: `2026-05-06-round-269{,b,…,o}-state-*.md`
- Plan: `docs/COMPLETION_ROADMAP.md` (refresh) and
  `docs/COMPLETION_ROADMAP.md` (long arc)
- Upstream `LedgerState` CBOR codec:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs`
  (the `EncCBOR` / `DecCBOR` instances)
- Upstream Conway derivation:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/LedgerState.hs`
- Forensic data captured from prior rounds:
  `docs/operational-runs/2026-05-06-round-266c-gap-bp-script-context.log`
  (Gap BP forensic CBOR for R266)
