---
name: crypto-tests
description: Guidance for cryptographic vector and regression tests in the crypto crate.
---

Use this directory for deterministic vector-backed crypto validation.

## Scope
- Upstream vector ingestion tests.
- Encoding, signing, verification, and tamper-rejection regressions.

## Non-Negotiable Rules
- New cryptographic behavior MUST land with vectors or deterministic fixtures in this directory.
- Do not weaken exact-byte or parity assertions into loose shape-only checks.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"