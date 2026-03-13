---
name: upstream-cardano-crypto-class
description: Guidance for vendored cardano-crypto-class fixture content.
---

This directory contains vendored fixture content from upstream `cardano-crypto-class`.

## Scope
- BLS12-381 upstream test-vector packages and related fixture layout.

## Non-Negotiable Rules
- Maintain upstream directory naming and corpus layout exactly.
- Treat this directory as read-only vendored input.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"