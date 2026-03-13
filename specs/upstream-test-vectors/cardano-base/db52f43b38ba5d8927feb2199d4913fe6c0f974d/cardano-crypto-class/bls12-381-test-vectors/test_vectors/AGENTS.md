---
name: upstream-bls12-381-vector-files
description: Guidance for the raw vendored BLS12-381 vector files.
---

This directory contains raw upstream BLS12-381 vector files.

## Scope
- File-level vector corpora consumed by crypto tests.

## Non-Negotiable Rules
- Files here are raw vendored fixtures and MUST remain byte-for-byte upstream copies.
- Additions or refreshes MUST come from the pinned upstream source, not local editing.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"