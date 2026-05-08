---
name: continuous-agent-loop
description: Yggdrasil's R-arc round-by-round development pattern — one bounded slice per round, four cargo gates green between rounds, one operational-runs doc per round, "proceed" rhythm for human-in-the-loop authorization. Use whenever the user authorizes a sustained development arc (R266-style protocol fix, R271-style filename-mirror split, R273-style subsystem split).
---

# Continuous Agent Loop — Yggdrasil R-arc

Yggdrasil parity work runs as a sequence of **R-arc rounds**. Each round
is a bounded, named slice that ships with full evidence and gate
greenness. The loop is human-in-the-loop: the user authorizes each
round with `proceed` / `continue` after seeing prior round's gates.

## Round contract

A round IS:
- A single named slice (e.g. `R271j` extracted `ReconnectingRunState`,
  `R273a` split `praos.rs` into `praos/{vrf,common}.rs`).
- All four cargo gates green at the end of the round:
  - `cargo fmt --all -- --check`
  - `cargo check-all`
  - `cargo lint`
  - `cargo test-all` (test count must match or exceed the prior round's)
- A new `docs/operational-runs/YYYY-MM-DD-round-NNN-<slug>.md` doc
  recording: scope, mirror mapping (Yggdrasil ↔ upstream Haskell paths),
  cross-module dependencies + visibility fixups, diff size, gate
  results, and the next-round candidate.
- One commit (or a small group of commits) referencing the upstream
  Haskell module by its `.reference-haskell-cardano-node/` path, not a
  github URL.

A round IS NOT:
- An open-ended refactor that crosses multiple subsystems.
- A "drive-by" cleanup that adds unrelated test or doc work.
- Anything that knowingly leaves a gate failing — even a temporary
  warning. Trim it, gate it `#[cfg(test)]`, or rollback.

## Inter-round rhythm

After each round closes (gates green, doc landed, commit recorded):

1. Post a **one-paragraph summary** with: file lines before/after, new
   sub-modules created, super:: surface count, test count, public
   surface preservation note.
2. Reference the operational-runs doc and the commit hash.
3. **Stop and wait for `proceed` / `continue`.** Do not start the next
   round on autopilot — the user reads the prior round's doc and may
   redirect.

## Gate evidence per round

Capture verbatim from the gate runs:

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

If any gate fails:
- Do NOT mask with `#[allow(dead_code)]`, broad `cfg(test)` gates, or
  spurious `_var` renaming.
- Diagnose the root cause. Most R-arc failures are: (a) parent module
  imports the moved item but forgot to update the path, (b) the
  extraction split a `#[derive(...)]` from its struct, (c) tests
  reference a now-private item via `super::*`.
- Apply the minimal fix; never broaden the round's scope.

## Patterns confirmed across R271 (runtime split arc)

- **Descendants-see-private-ancestors.** A child sub-module reads
  parent runtime.rs's private items via `use super::{...};` without
  `pub(super)` promotions. R271k–r confirmed across 80+ super::
  references.
- **Item-promotion threshold (~6).** When a target cluster needs >~6
  `pub(super)` promotions on parent-private items, extract the
  *shared dependency prelude first*. R271i (failed, rolled back)
  → R271i revised + R271j (succeeded).
- **Test-file `super::*` imports.** When `<module>/tests.rs` imports
  symbols via `super::FOO`, moving FOO to a sibling sub-module
  requires runtime.rs to keep `use sibling::FOO;` (or
  `pub use sibling::FOO;`). Test-only paths gate `#[cfg(test)]`.
- **Orphan doc comments at extraction boundaries.** When the `awk`
  line range cuts between a doc comment and the item it documents,
  the doc comment is carried into the new file as an orphan. R271m,
  R271n, R271r each hit this once; fix inline (move doc back to its
  item or trim).

## Patterns confirmed across R273 (subsystem-split arc)

- **8-item public surface preservation.** Sub-module `pub use` re-exports
  preserve the crate-level public surface; no `lib.rs` edits needed.
- **Orphan `#[derive]` boundaries.** Splits where a struct's
  `#[derive(...)]` lives on the line above the `pub struct ...`
  declaration must include the derive in the slice or re-attach it
  inline.
- **Tests `use super::*;` doesn't transitively re-export the moved
  imports.** Tests need explicit imports for items previously brought
  into the parent's namespace by the file-level `use` blocks.

## When to stop the loop

- User says `stop`, `pause`, `done`, or anything other than `proceed`
  / `continue`.
- Four cargo gates fail and the diagnosis takes more than ~10 minutes
  — surface the blocker, do not soldier through.
- The next round's scope crosses the R271i threshold (>~6 cross-module
  promotions) without a dependency-prelude pre-extraction.
- The current branch's commits exceed ~10 unpushed (signal to flush
  before depth makes review hard).

## When to switch to a parity-plan first

If the next round touches **protocol behavior** (CBOR shape, hash
input, signature domain, ledger predicate, Plutus budget, network
framing), invoke `/parity-plan <feature>` before editing code. The
filename-mirror R-arcs (R271, R273) are pure structural moves and
don't need parity plans; protocol-fix arcs (R266, R267) do.
