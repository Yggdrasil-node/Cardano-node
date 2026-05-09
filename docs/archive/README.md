---
title: Archive
layout: default
parent: Reference
nav_order: 99
has_children: true
permalink: /archive/
---

# Archive

Historical documents preserved as audit trail. Each file in this
directory was once a living doc, then closed by a specific R-arc round
or superseded by a later document. None of the contents are
authoritative for the current code state — see the living docs at
[`docs/PARITY_PLAN.md`](../PARITY_PLAN.md),
[`docs/PARITY_SUMMARY.md`](../PARITY_SUMMARY.md),
[`docs/PARITY_PROOF.md`](../PARITY_PROOF.md),
[`docs/UPSTREAM_PARITY.md`](../UPSTREAM_PARITY.md), and the
per-round records in [`docs/operational-runs/`](../operational-runs/)
for what is currently true.

## Contents

| File | Closed in | Reason |
|---|---|---|
| [`code-audit.md`](code-audit.md) | R287 | 2026-04-27 audit; all C-1, H-1, H-2, M-1..M-8, L-1..L-9 findings closed in the 2026-Q3 operational pass. Each finding heading carries an inline `[CLOSED in 2026-Q3]` annotation; body preserved verbatim as audit-trail evidence. |
| [`REFACTOR_BLUEPRINT.md`](REFACTOR_BLUEPRINT.md) | R287 | R256 Phase A–G monolith-split planning doc; all phases shipped via R269–R281. Each phase header carries a `[DONE in RNNN]` annotation pointing at its closing round. |
| [`AUDIT_VERIFICATION_2026Q2.md`](AUDIT_VERIFICATION_2026Q2.md) | R287 (implicitly) | 2026-Q2 sanity audit verifying every gap flagged in the parity documentation. Closed once the audit's conclusions were absorbed into the live `PARITY_*.md` docs and the R270–R273 work shipped against them. |

## Archive policy

Files moved here remain in git history; cross-references from
historical `docs/operational-runs/*.md` round-docs point at the
archived paths. Adding new entries to this directory requires:

1. The doc is no longer authoritative for current code state.
2. Each finding/section the doc tracks has either shipped, been
   superseded, or been explicitly retired with a closure annotation
   in the doc body.
3. Living docs that reference the archived doc are updated to point
   at `docs/archive/<file>.md`.

This README is the index; update it when adding entries.
