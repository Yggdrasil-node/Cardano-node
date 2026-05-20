---
name: parity-plan
description: Author a parity plan before substantive Yggdrasil code edits. Use when about to change protocol, ledger, Plutus, network, or storage behavior — anything that touches CBOR, hashes, signatures, ledger predicates, network framing, or Plutus budgets. Plans must cite Haskell evidence; do not edit code on memory or assumptions.
---

# Parity Plan — Yggdrasil

Yggdrasil is a **clean-room Rust port** of `cardano-node` targeting
100% protocol/naming/functionality/filename parity with the latest
IntersectMBO release. The reference is **always the latest tag**;
currently 11.0.1.

A parity plan is the gate that prevents code edits running on
memory or assumption. **Author the plan before editing.**

## When to invoke

YES — author a plan first:
- CBOR encoder / decoder change (any era).
- Hash input shape change (Blake2b, ScriptDataHash, ...).
- Signature domain change (Ed25519, KES, VRF, BLS12-381).
- Ledger predicate failure / acceptance change.
- Plutus cost-model or builtin-cost change (R266 territory).
- Network framing change (mux, mini-protocol message shape).
- Genesis-config parsing change.

NO — skip the plan:
- Filename-mirror restructuring (R271, R273 arcs) — use the
  `round-extraction` skill instead.
- Pure documentation update.
- Cargo-alias or workspace-tooling change.
- Test-only refactor that doesn't change asserted behavior.

## Plan structure

Author as a fenced block in the conversation; do NOT commit to a
file unless the user asks. The user reads it, redirects if needed,
and explicitly authorizes implementation.

```
# Parity Plan — <feature-id-from-parity-matrix-or-area>

Reference target: IntersectMBO/cardano-node 11.0.1 (latest as of <date>)

## Haskell modules reviewed
- .reference-haskell-cardano-node/<path1>:<line-range> — <behavior>.
- .reference-haskell-cardano-node/<path2>:<line-range> — <behavior>.

## Rust files reviewed
- crates/<crate>/src/<file>.rs:<line-range> — <current behavior>.
- crates/node/<crate>/src/<file>.rs:<line-range> — <current behavior>.

## Semantic gaps
- <gap 1>: <Rust does X, Haskell does Y; observable difference: ...>.
- <gap 2>: ...

## Target production behavior
- <Contract: input → bytes/predicate/trace output>.
- Boundary cases: <list>.
- Edge cases: <list>.

## Validation evidence
- <forensic vector | golden test | live rehearsal | per-epoch byte-match>.
- Specific commands or vectors:
  - `db-analyser --target-slot N --tx <txid>`
  - `specs/upstream-test-vectors/<file>`
  - `scripts/compare_tip_to_haskell.sh`

## Next action
- <implementation steps, only after user approval>.
- <evidence collection if blocked on missing reference data>.
```

## Required citations

Every plan MUST:

- Cite at least one `.reference-haskell-cardano-node/...` path per
  semantic concern. Not github.com URLs — local paths only (the
  vendored tree is gitignored and stable across sessions).
- Cite at least one `crates/...` or `crates/node/...` path per Rust counterpart.
- State the **specific bytes / predicate / trace event** that differs.
  "Behavior is wrong" is not a gap; "tag 0x82 vs 0x84 in field
  position 3" is.
- State the **validation procedure** that proves parity is reached.
  Subjective claims like "looks right" are not validation.

## Reference snapshot freshness

If the local `.reference-haskell-cardano-node/install/bin/cardano-node
--version` does not match `docs/parity-matrix.json::reference.tag`,
run `bash scripts/setup-reference.sh --force` BEFORE deciding behavior.
The full install check requires Linux/WSL because the vendored upstream
release bundle contains Linux binaries. For path-only source research,
`bash scripts/setup-reference.sh --sources-only` is enough. Stale references
mislead parity decisions.

## When the plan reveals an unresolvable blocker

Stop. Do not edit code on assumption. Surface the blocker as the next
action item with the missing reference data named explicitly:

- "Need upstream `db-analyser` per-builtin trace for tx
  `7bb40e40...3be5b9` at preview slot 1,462,057 to identify the CEK
  divergence point" (R266 step 3).
- "Need 24h+ mainnet rehearsal evidence side-by-side with cardano-node
  10.7.1 to certify <feature>" (R267).

## Quality bar

- No technical debt is acceptable. Do not plan TODO stubs, partial
  protocol behavior, speculative documentation, or compatibility
  shortcuts.
- The plan must explain how the change reaches **100% parity** for
  the scoped behavior.
- If the plan would knowingly leave a gate failing or a wire format
  diverged, the plan is wrong. Fix the plan, not the code.
