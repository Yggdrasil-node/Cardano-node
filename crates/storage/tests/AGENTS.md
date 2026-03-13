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
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Consensus ChainDB and storage context: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- Node storage integration reference: <https://github.com/IntersectMBO/cardano-node/>

## Current Phase
- Tests in this directory verify trait conformance, persistence after reopen, rollback deletion, and snapshot-visible behavior for storage backends.