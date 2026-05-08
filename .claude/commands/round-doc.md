---
description: Author a Yggdrasil operational-runs doc for the current round.
argument-hint: <round-id> <slug>
---

Author `docs/operational-runs/$(date +%Y-%m-%d)-round-$1-$2.md` covering
the just-completed R-arc round.

The doc MUST include these sections (use them verbatim as headings):

```
## Round <RoundId> — <one-line scope>

Date: YYYY-MM-DD
Branch: main
Type: <e.g. Filename-mirror refactor (Phase γ R273 Nth slice)>

### Slice scope
<bulleted list of items moved/extracted; cite line counts>

### Mirror mapping
| Yggdrasil | Upstream Haskell |
|---|---|
| `<rust-path>` | `<.reference-haskell-cardano-node/<haskell-path>>` |
...

### Cross-module dependencies
<list of super:: imports, pub(super) promotions, and visibility fixups>

### Visibility / dependency fixups
1. <numbered list of non-obvious adjustments — e.g. orphan doc comment
   trims, derive boundary moves, test imports broadened>

### Diff
| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `<file>` | N | M | −X |

### Verification gates
\`\`\`
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
\`\`\`

### Cumulative <arc> progress
<table or paragraph rolling up the arc>

### Stop point — <next round candidate>
<paragraph describing the next candidate, line ranges, anticipated
cross-module surface count, etc.>

### References
- Plan: `~/.claude/plans/<plan-file>.md` (if applicable)
- Prior round closure: `<prior-doc-path>`
- Upstream Haskell modules cited via `.reference-haskell-cardano-node/<paths>`
```

After writing the doc:

1. Verify all four cargo gates already passed before authoring.
2. Verify the operational-runs filename matches
   `docs/operational-runs/YYYY-MM-DD-round-NNN-<slug>.md`.
3. Stop and surface to the user; the round commit comes after the
   user confirms the doc reads cleanly.
