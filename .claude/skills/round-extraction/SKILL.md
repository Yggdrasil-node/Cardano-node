---
name: round-extraction
description: Yggdrasil filename-mirror extraction recipe — split an oversized .rs file into upstream-aligned sub-modules without changing behavior. Use when starting an R-arc round (R271-style runtime split, R273-style subsystem split, R272-style era-rules split). Encodes the empirical patterns confirmed across 27+ rounds (R271a-s, R273a-h).
---

# Round Extraction — Yggdrasil filename-mirror split

Use this skill **only** for filename-mirror restructuring. Do NOT use
for protocol-behavior changes; those go through `/parity-plan` first.

## Inputs

- Target file (`crates/<crate>/src/<file>.rs` or `node/src/<file>.rs`).
- Round id (e.g. `R273i`).
- Upstream Haskell mirror path under `.reference-haskell-cardano-node/`.

## Workflow

### 1. Survey the target file

```bash
grep -nE "^pub fn|^fn |^pub struct|^struct|^pub enum|^enum|^impl|^pub trait|^trait|^#\[cfg\(test\)\]|^pub use|^use" <target>
wc -l <target>
grep "pub use <module>" crates/<crate>/src/lib.rs   # external surface
```

### 2. Identify the upstream split point

Find the upstream sibling files that sub-divide the same concept:

```bash
find .reference-haskell-cardano-node -name "<Concept>*.hs" | head
```

Mirror the upstream layout. **Do not invent new sub-module names** —
match upstream's discrimination (e.g. `Praos.hs` + `Praos/VRF.hs` +
`Praos/Common.hs` → `praos.rs` + `praos/{vrf,common}.rs`).

### 3. Plan the slice ranges

Read item boundaries with `Read offset=...`. Identify:
- Section boundaries (struct/enum/impl/fn).
- Doc comments above each item — these MUST move with the item.
- `#[derive(...)]` lines — these MUST move with the struct/enum.
- Cross-cluster references (which cluster reads which).

### 4. Estimate the cross-module surface

Count items that must become `pub(super)` for sibling-cluster access:

| Count | Action |
|---|---|
| 0 | Pure descendants-see-ancestors (parent → child via `use super::{...}`). |
| 1–6 | Promote inline; common pattern (R273a, R273c). |
| ≥7 | **Extract a shared dependency prelude first** (R271i lesson). |

If ≥7 promotions are needed, extract the shared prelude as a
**separate** preceding round, then do the main split in the next
round.

### 5. Build the new files

Use `cat <header>; awk 'NR>=START && NR<=END' <target>` to extract
slice ranges, with module-level docstrings authored explicitly:

```rust
//! <one-line module purpose>.
//!
//! Mirrors upstream `<HaskellModule>` (<.reference path>).
//!
//! N public items move from `<source>.rs` here:
//!
//! - `<Item1>` — <role>.
//! ...
//!
//! Extracted from `<source>.rs` in <RoundId> (Phase γ §<RArc>
//! Nth slice).

use ...;
use super::<sibling>::{<types>};

<body>
```

### 6. Trim the residual file

```bash
awk 'NR<START || NR>END' <target> > /tmp/<target>_trimmed
cp /tmp/<target>_trimmed <target>
```

Add `pub mod <child>;` and `pub use <child>::{<items>};` blocks in
order matching the original file's flow.

### 7. Iterate on errors (in order)

Run `cargo check-all` and address by class:

1. **`cannot find type/function`** — missing `use super::<child>::FOO;`
   in the parent or sibling. Add inline.
2. **`is private`** — promote the offending item to `pub(super)` in
   its new home.
3. **`expected item after doc comment`** — orphan doc comment carried
   past a slice boundary. Trim from the new file or move back to the
   source struct/fn.
4. **`unresolved import super::FOO`** — `tests.rs` references an item
   that moved. Add `use sibling::FOO;` in the parent (gated
   `#[cfg(test)]` if only tests use it).
5. **Unused-import warnings** — trim from the residual file's
   top-level `use` blocks.

### 8. Run the four cargo gates

```bash
cargo fmt --all
cargo check-all                  # MUST pass
cargo lint                       # MUST pass; -D warnings is enforced
cargo test-all                   # MUST match prior round's count
```

If `cargo lint` flags `empty_line_after_doc_comment`, that's an
orphan-doc artifact from the slice — go back to step 7 case (3).

### 9. Author the operational-runs doc

`docs/operational-runs/YYYY-MM-DD-round-NNN-<slug>.md` covers:

- Slice scope (item count, line count moved).
- Mirror mapping table (Yggdrasil ↔ upstream).
- Cross-module dependencies + visibility fixups.
- Diff (lines before / after / Δ for each touched file).
- Verification gate output (verbatim).
- Cumulative arc progress.
- Stop point — next round candidate.
- References (plan + prior round + upstream Haskell paths under
  `.reference-haskell-cardano-node/`).

### 10. Commit

```
refactor(<crate>): R273x — split <file>.rs into <child>/{...}.rs

Splits crates/<crate>/src/<file>.rs (N lines) into:
- <file>.rs (M lines, residual shell with ...)
- <file>/<child1>.rs (P lines, ...)
- <file>/<child2>.rs (Q lines, ...)

<child1>.rs mirrors upstream <Module1.hs> (...).
<child2>.rs mirrors upstream <Module2.hs> (...).

<dependency notes>.

The K-item public surface from lib.rs's `pub use ...` block is
preserved unchanged via sub-module pub use re-exports — no lib.rs
edits needed.

<test count> tests pass across all four cargo gates. <RArc> Nth slice.

Co-Authored-By: ...
```

## Constraints

- Do NOT change behavior. No new public methods, no signature changes,
  no algorithm tweaks.
- Do NOT broaden the round to "while I'm here" cleanup.
- Do NOT rename items to match upstream Haskell names — that's
  the naming-parity sweep (R268), a separate arc.
- Do NOT skip the operational-runs doc; it is the public evidence the
  round shipped cleanly.

## Stop conditions

- Cross-module promotion count exceeds 6 → switch to dependency-prelude
  pre-extraction round.
- Test count drops → diagnose and fix; do not commit.
- Lint warnings appear → fix; never `#[allow(...)]` your way out.
- Cumulative `runtime.rs` (or whatever was the original target) shrinks
  to under ~150 lines → arc is structurally complete; close the arc
  and move to the next subsystem.
