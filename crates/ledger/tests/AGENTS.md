---
name: ledger-tests
description: Guidance for ledger-era codec, transaction, and state-transition tests.
---

Keep tests in this directory close to ledger rules and era-specific invariants.

## Scope
- Era codec round-trips.
- UTxO, submitted-transaction, and block application behavior.
- Cross-era regression tests.
- Folder-backed integration tests under `tests/integration/`.

##  Rules *Non-Negotiable*
- Tests here MUST pin rule behavior tightly enough to catch serialization and transition regressions.
- Era-specific expectations MUST stay explicit rather than being hidden behind generic helpers.
- Keep integration modules grouped by ledger domain or era family, not by arbitrary file size.
- Shared test helpers in `tests/integration/` MUST stay minimal and use `pub(super)` visibility.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Ledger test corpus root: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Formal ledger rules: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- Tests in this directory protect codec round-trips, submitted-transaction handling, UTxO evolution, and era-specific block application behavior.
- The integration test crate is now split into focused modules for Shelley, Allegra/Mary, Alonzo, Byron, Babbage, Conway, Praos block envelopes, and ledger-state subdomains.
- Byron tests (`eras_byron.rs`): 15 tests covering EBB/MainBlock round-trips, header hash, transaction types (TxIn, TxOut, Tx, TxWitness, TxAux CBOR round-trips), transaction ID determinism, and block-with-transactions decode.
- Golden tests (`golden.rs`): 17 tests covering construct-encode-decode round-trips for all eras (Byron TX, Shelley/Allegra/Mary/Alonzo/Babbage/Conway submitted transactions, MultiEraTxOut, MultiEraSubmittedTx, PlutusData, StakeCredential, TxId determinism).
- Witness validation tests (`witness_validation.rs`): 10 tests covering VKey witness sufficiency (accept valid, reject missing/wrong, skip when absent, reject empty set) and native script evaluation (ScriptPubkey accept, InvalidBefore/InvalidHereafter timelock rejection, timelock in-range accept, ScriptAll multisig accept and reject).
- Integration coverage now also includes `plutus_evaluation.rs`: 6 tests that drive `LedgerState::apply_block_validated()` with mock `PlutusEvaluator` implementations, covering Alonzo V1, Babbage V2, and Conway V3 script dispatch, evaluator failure propagation, evaluator metadata (`script_hash`, `version`, `script_bytes`, `ex_units`), and the no-evaluator soft-skip behavior of `apply_block()`.