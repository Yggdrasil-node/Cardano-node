---
name: upstream-cardano-base-vectors
description: Guidance for vendored cardano-base upstream fixture layout.
---

This directory is a vendored mirror root for upstream `cardano-base` fixture content.

## Scope
- Pinned commit snapshots of `IntersectMBO/cardano-base` vector material.

## Non-Negotiable Rules
- Do not hand-edit vendored upstream files below this directory.
- Add or update only by syncing from an explicitly pinned upstream commit.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"