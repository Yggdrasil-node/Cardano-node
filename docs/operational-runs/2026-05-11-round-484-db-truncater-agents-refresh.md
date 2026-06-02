---
title: 'R484: db-truncater AGENTS.md + parity-matrix refresh post-R349'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-484-db-truncater-agents-refresh/
---

# R484 — db-truncater AGENTS.md + parity-matrix refresh

**Date:** 2026-05-11
**Scope:** documentation-only round.

## Slice scope

Closes documentation drift left behind by the R347-R349
db-truncater Phase B.1 mini-arc closure (`ImmutableStore::trim_after_slot`
+ typed config + `Run.hs` equivalent). The crate code itself has
been functionally complete since R349; only the AGENTS.md +
parity-matrix `rust_surface.role` field still carried the
pre-R349 "skeleton" language.

Files touched:

1. **`crates/tools/db-truncater/AGENTS.md`** — full rewrite of
   the **Status**, **Current functional surface**, **Carve-out
   inventory**, and **Round roadmap** sections. Marks the crate
   as functionally complete; identifies the operator soak
   (`node/dev/evidence/compare_db_truncater_to_upstream.sh`) as the
   only remaining gate before `partial → verified_11_0_1`
   parity-matrix promotion.

2. **`docs/parity-matrix.json::sister-tool.db-truncater`** —
   updates the `rust_surface[0].role` description from "R335
   skeleton + R348 typed config; Run.hs equivalent (R349)
   pending" to a post-R349 summary referencing all four shipped
   slices (R335/R347/R348/R349).

No source code touched.

## Tests delivered

None — documentation round. Test count unchanged at 6,176.

## Verification log

```
cargo fmt --all -- --check                                  clean
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Stop point

Two AGENTS.md files now reflect post-R475-R484 reality:
db-analyser (R483) + db-truncater (R484). 7 other sister-tool
`AGENTS.md` files (cardano-tracer, tx-generator, db-synthesizer,
cardano-testnet, snapshot-converter, kes-agent, kes-agent-control,
dmq-node) still carry pre-mini-arc language but those crates are
genuinely partial / pre-implementation, so the stale language is
*accurate* about their state — refreshing requires the underlying
implementation to ship first.

## References

- Plan: `docs/COMPLETION_ROADMAP.md`
  (R326-R459 sister-tools port arc).
- R349 commit: db-truncater Run.hs port.
- R347 commit: ImmutableStore::trim_after_slot.
