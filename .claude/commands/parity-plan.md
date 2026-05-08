---
description: Author a parity plan before substantive Yggdrasil code edits.
argument-hint: <feature-id-or-area>
---

Author a parity plan covering `$ARGUMENTS` (feature id from
`docs/parity-matrix.json`, or a free-form area like `consensus.opcert`,
`plutus.cek-machine`, `network.txsubmission2`).

A parity plan must list:

1. **Haskell modules reviewed** — concrete paths under
   `.reference-haskell-cardano-node/...`. Cite at least one path per
   semantic concern (e.g. predicate failure, CBOR shape, hash input).
2. **Rust files reviewed** — concrete paths under `crates/...` or
   `node/...`. Include the closest existing Yggdrasil counterparts
   even when partial.
3. **Semantic gaps** — the divergences or missing behaviors that block
   100% parity for the scoped feature. Be specific about what bytes,
   predicates, or trace events differ.
4. **Target production behavior** — the exact behavior to implement,
   stated as a contract (input → bytes/predicate/trace output), not
   a refactoring goal.
5. **Validation evidence** — the commands, vectors, or rehearsal
   procedures that prove parity. Acceptable forms:
   - Forensic byte-equivalence vs upstream `db-analyser` for a named
     slot/tx.
   - Golden test vectors checked into `specs/upstream-test-vectors/`.
   - 24h+ live mainnet endurance with hourly tip + per-100-epoch
     ledger-state byte-match (R267 / R275 gates).
   - Per-epoch chain-dep-state sidecar byte-match (consensus arcs).

The plan must explain how the change reaches **100% parity** for the
scoped behavior. If the reference checkout is incomplete, run
`bash scripts/setup-reference.sh --force` before deciding behavior.

Do not begin implementation until the plan is authored. If the plan
identifies a blocker that cannot be resolved from local evidence,
stop and request the missing reference data rather than editing code
on assumptions.

Standing rule: the parity reference target is the **latest IntersectMBO/
cardano-node release** (currently 11.0.1). Do not anchor a plan to a
stale tag.
