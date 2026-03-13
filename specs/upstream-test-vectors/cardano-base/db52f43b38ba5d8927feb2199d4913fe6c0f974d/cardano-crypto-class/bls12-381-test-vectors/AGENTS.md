---
name: upstream-bls12-381-vectors
description: Guidance for vendored BLS12-381 upstream vector packages.
---

This directory groups upstream BLS12-381 vector corpora used for crypto parity validation.

## Scope
- BLS12-381 vector package layout and provenance.

## Non-Negotiable Rules
- Corpus files here are vendored upstream artifacts and MUST NOT be edited by hand.
- Keep names and grouping aligned with upstream `cardano-crypto-class` packaging.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"