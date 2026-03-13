---
name: ledger-eras-subagent
description: Guidance for per-era ledger modules and era transition boundaries
---

Focus on per-era differences, transition boundaries, and keeping era-local details out of generic ledger plumbing.

## Scope
- Era-specific data, behavior differences, and transition markers.
- Shared naming and boundary consistency across Byron through Conway.

## Non-Negotiable Rules
- One file or module SHOULD stay focused on one era concern whenever possible.
- Generic ledger logic MUST NOT be duplicated inside `eras/`.
- Each era module MUST make it clear whether it is a placeholder or reflects a real upstream rule set.
- Public era-specific types or helpers MUST have Rustdocs when the era difference is not obvious from naming alone.
- Official era names, rule labels, and transition terminology from upstream ledger and node sources MUST be preferred.

## Upstream References (add or update as needed)
- Era sources and CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras>
- Formal ledger specification site: <https://intersectmbo.github.io/formal-ledger-specifications/site>
- Formal ledger specification repository: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- Shelley: full block, header, tx body, witness set, UTxO, and VRF/OpCert types with CBOR codecs.
- Allegra: `AllegraTxBody` (optional TTL + validity interval start) and `NativeScript` (6-variant timelock enum) with CBOR codecs.
- Mary, Alonzo, Babbage, Conway, Byron: naming scaffolds only.
- Keep additions lightweight until generated types and real transition logic land.
