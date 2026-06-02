---
title: 'R308: PARITY_PROOF.md header refresh + scripts/AGENTS.md'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-308-parity-proof-and-scripts-agents/
---

# Round 308 — `docs/PARITY_PROOF.md` header refresh + `scripts/AGENTS.md`

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R307`](2026-05-09-round-307-parity-summary-banner-refresh.md)  
**Cumulative arc:** R273-rename + R274–R307 strict 1:1 file-mirror &
tech-debt purge arc; R308 backfills two doc-side gaps surfaced after
R307 closure.

## Summary

Two small but visible doc-tree gaps closed:

1. **`docs/PARITY_PROOF.md` header was stale.** Title and front-matter
   carried "Round 248" / "4.7K+ passing" — values from the R248 refresh
   on 2026-05-02. The §1–§9 evidence body remains valid (R248 captured
   the cardano-cli LSQ surface + the BlockFetch/sync arcs and nothing
   in R249–R307 invalidated those closures). The header refresh
   replaces the round-stamp + test-count with current values and adds
   a top-of-document blockquote summarizing the R273-rename + R274–R307
   strict 1:1 file-mirror arc so a reader landing on this doc from the
   docs site can see what shifted between R248 and R307 without
   chasing the per-round operational-runs trail.

2. **`scripts/AGENTS.md` was missing.** `AGENTS.md` policy is "Every
   meaningful subdirectory has an `@AGENTS.md`" — the `scripts/`
   directory now hosts five Python validators + one shell helper, each
   with specific operational rules (CI vs local-only, four-source SHA
   pin matrix, allowlist source-of-truth wiring, etc.) and previously
   had no operational guide. R308 lands one modeled on `docs/AGENTS.md`
   + `crates/cardano-cli/AGENTS.md`: a directory shape diagram, per-
   validator section with what it checks and what failure means, the
   non-negotiable rules (kebab-case naming, stdlib-only Python,
   policy-tag single-source-of-truth, `__pycache__/` ignored), and
   pointers at the four source-of-truth files.

## Diff inventory

| Path | Change |
|---|---|
| `docs/PARITY_PROOF.md` | Front-matter title `(Round 248)` → ``; document-round line refreshed; test-count line refreshed (4.7K+ → 4,855); five-gate snapshot added; R273-rename + R274–R307 arc summary blockquote added at the top of the body. |
| `scripts/AGENTS.md` | New file (~140 lines). Directory shape diagram; per-validator `check-strict-mirror.py` / `check-parity-matrix.py` / `check-fixture-manifest.py` / `check-reference-artifacts.py` sections; refresh helper `setup-reference.sh` section; discovery script `audit-strict-mirror.py` section; non-negotiable rules; maintenance guidance; upstream references. |
| `AGENTS.md` | "AGENTS.md Files Are Primary Context" table: new row for `scripts/AGENTS.md` between `docs/AGENTS.md` and `specs/AGENTS.md`. |
| `crates/cardano-cli/src/helper.rs`, `node/src/runtime/{mempool_helpers,peer_management}.rs` | Tail repair: `cargo fmt` auto-fix on three files where pre-R308 commits left rustfmt drift (mechanical only — multi-line `format!` reflow + a wrapped `use` statement + a trailing whitespace strip). 7 insertions / 4 deletions across the three files. Necessary so the five-gate evidence claim in the new PARITY_PROOF.md header is true at HEAD. |
| `docs/operational-runs/2026-05-09-round-308-parity-proof-and-scripts-agents.md` | This round-doc. |

## Verification

```text
$ cargo fmt --all -- --check
(silent — clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.76s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.55s

$ cargo test --workspace --all-features
passed: 4855  failed: 0

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 8 entries validated against .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c consistent across pin source, fixture tree, and docs; 2 corpora validated.
```

All five cargo gates green plus the three Python parity validators
clean. Workspace test baseline preserved at 4,855 passing / 0 failing.

## Closure criterion

- `docs/PARITY_PROOF.md` header reflects the post-R307 state (round
  stamp, test count, five-gate evidence, arc summary).
- `scripts/AGENTS.md` exists and is registered in `AGENTS.md`'s
  AGENTS-files-table.
- `dev/test/check-strict-mirror.py --fail-on-violation`,
  `dev/test/check-parity-matrix.py`, and
  `dev/test/check-fixture-manifest.py` all exit zero.

All three are met. The R273-rename + R274–R307 strict 1:1 file-mirror
arc is now durably reflected in `docs/PARITY_PROOF.md` (the canonical
"what works end-to-end" reference) AND the validator-tooling tree
that polices the policy carries an operational guide.

## Out of scope (R309+ candidates)

- **Concrete cardano-cli subcommand ports beyond the current 3-command
  surface.** R296 + R297 migrated `Version` + `ShowUpstreamConfig`;
  `QueryTip` migration is gated on extracting an `LsqClient` trait
  abstraction from `node/src/commands/query::run_query`. The full
  upstream `cardano-cli` has hundreds of subcommands across Byron /
  Compatible / EraBased / EraIndependent / Legacy clusters; concrete
  ports are operator-demand-prioritized.
- **R266 step 3 — Gap BP per-builtin trace diff** against upstream
  `db-analyser`. Operator-time gated; not blocked by this arc.
- **R267 — mainnet 24h endurance.** Operator-time gated.
- **`docs/PARITY_PROOF.md` body refresh** beyond the header.
  R248-captured §1–§9 evidence remains valid and the R273-R307 arc
  did not change any closure status; a body refresh would be additive
  prose for newly-shipped capability rather than corrective. Defer
  until the next material §1–§9 closure ships.
