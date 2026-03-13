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
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Ledger test corpus root: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Formal ledger rules: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- Tests in this directory protect codec round-trips, submitted-transaction handling, UTxO evolution, and era-specific block application behavior.