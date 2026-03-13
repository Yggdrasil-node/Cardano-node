---
name: storage-tests
description: Guidance for storage trait conformance and persistence regression tests.
---

Use this directory to pin persistence and rollback expectations for storage backends.

## Scope
- Trait conformance tests.
- Reopen persistence, rollback deletion, and snapshot coverage.

## Non-Negotiable Rules
- Tests here MUST validate behavior visible through storage traits, not private implementation details alone.
- File-backed regressions MUST preserve deterministic on-disk behavior for the current format.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"