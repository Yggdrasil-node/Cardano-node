---
name: storage-tests
description: Guidance for storage trait conformance and persistence regression tests.
---

Use this directory to pin persistence and rollback expectations for storage backends.

## Scope
- Trait conformance tests.
- Reopen persistence, rollback deletion, and snapshot coverage.

##  Rules *Non-Negotiable*
- Tests here MUST validate behavior visible through storage traits, not private implementation details alone.
- File-backed regressions MUST preserve deterministic on-disk behavior for the current format.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [ChainDB test suite](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/ChainDB)
- [ImmutableDB implementation](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/ImmutableDB)
- [VolatileDB implementation](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/VolatileDB)
- [LedgerDB implementation](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/LedgerDB)

## Current Phase
- Tests in this directory verify trait conformance, persistence after reopen, rollback deletion, snapshot-visible behavior, and volatile→immutable promotion for storage backends.
- Volatile garbage collection tests: `garbage_collect` removes blocks below a slot threshold (keeps at/above), slot-zero is a no-op, GC-all empties store. `block_count` returns correct count. File-backed GC verifies disk file deletion. `compact` removes orphaned `.cbor`/`.json` and stale `.tmp` files not in the index. `chain_db_gc_volatile_before_slot` verifies ChainDb coordination. 80 tests total.