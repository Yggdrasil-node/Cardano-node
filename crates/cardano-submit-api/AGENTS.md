# Guidance for the pure-Rust port of upstream `cardano-submit-api`.

R327 skeleton. The Cargo crate exposes a placeholder `run()` that
returns "not yet implemented"; concrete subcommand implementations
land per the R326–R459 sister-tools port arc plan.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate: `python3 scripts/check-strict-mirror.py`.

## Upstream source

`.reference-haskell-cardano-node/cardano-submit-api/`

## Status

R327 skeleton: empty `lib.rs` + `main.rs` + this AGENTS.md. No
concrete subcommand implementations yet. The skeleton compiles but
calling the binary returns the "not yet implemented" sentinel.

## Round roadmap

Per the sister-tools port arc plan
(`docs/operational-runs/2026-05-09-round-326-vendored-source-survey.md`
+ R326b for vendoring + the plan file at
`/home/daniel/.claude/plans/playful-tickling-plum.md`), this crate's
implementation lands across multiple subsequent rounds:

- Skeleton: R327 (this round)
- CLI parser: separate round per tool (Tier 1/2/3/4 banding)
- Per-subcommand impls: 1–3 days each
- Integration round: end-to-end soak vs upstream binary at
  `.reference-haskell-cardano-node/install/bin/cardano-submit-api`
- Closeout round: CHANGELOG entry + AGENTS.md operational guide +
  parity-matrix → verified_11_0_1

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `cardano-submit-api` is the
  acceptance gate for any concrete implementation. Operators must
  be able to swap the upstream binary for the yggdrasil binary
  without a script change.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).

## Maintenance Guidance

- Update this AGENTS.md when concrete subcommand implementations
  land.
- Keep `## Round roadmap` in sync with the actual round-doc trail
  in `docs/operational-runs/`.
