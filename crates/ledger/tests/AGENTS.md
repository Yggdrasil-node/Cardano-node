---
name: ledger-tests
description: Guidance for ledger-era codec, transaction, and state-transition tests.
---

Keep tests in this directory close to ledger rules and era-specific invariants.

## Scope
- Era codec round-trips.
- UTxO, submitted-transaction, and block application behavior.
- Cross-era regression tests.

## Non-Negotiable Rules
- Tests here MUST pin rule behavior tightly enough to catch serialization and transition regressions.
- Era-specific expectations MUST stay explicit rather than being hidden behind generic helpers.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"