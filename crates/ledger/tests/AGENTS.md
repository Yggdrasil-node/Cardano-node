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
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- Shelley ledger tests: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/shelley/test>
- Alonzo ledger tests: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/alonzo/test>
- Conway ledger tests: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/conway/test>
- Per-era CDDL conformance data: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/> (each era has `impl/cddl/data/`)
- Formal ledger rules (Agda): <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- Tests in this directory protect codec round-trips, submitted-transaction handling, UTxO evolution, and era-specific block application behavior.
- The integration test crate is now split into focused modules for Shelley, Allegra/Mary, Alonzo, Byron, Babbage, Conway, Praos block envelopes, and ledger-state subdomains.
- Byron tests (`eras_byron.rs`): 15 tests covering EBB/MainBlock round-trips, header hash, transaction types (TxIn, TxOut, Tx, TxWitness, TxAux CBOR round-trips), transaction ID determinism, and block-with-transactions decode.
- Byron UTxO transition tests (`ledger_state_era_application.rs`): 5 tests covering Byron block application with real UTxO transitions — normal spend, missing-input rejection, negative-fee rejection, atomicity rollback on failure, and multi-block chain spending.
- Governance/protocol-parameter tests now cover typed `ProtocolParameterUpdate` CBOR round-trips through both `GovAction::ParameterChange` and `ShelleyUpdate`, plus enactment-time application of typed parameter deltas to `LedgerState.protocol_params`.
- Address/bootstrap coverage now also includes strict validation tests for invalid reward-account network ids, pointer-address trailing bytes, Byron-address CRC32 validation via `Address::validate_bytes()`, and bootstrap witness semantic validation (signature success/failure and attribute-map validation).
- Golden tests (`golden.rs`): 17 tests covering construct-encode-decode round-trips for all eras (Byron TX, Shelley/Allegra/Mary/Alonzo/Babbage/Conway submitted transactions, MultiEraTxOut, MultiEraSubmittedTx, PlutusData, StakeCredential, TxId determinism).
- Witness validation tests (`witness_validation.rs`): 10 tests covering VKey witness sufficiency (accept valid, reject missing/wrong, skip when absent, reject empty set) and native script evaluation (ScriptPubkey accept, InvalidBefore/InvalidHereafter timelock rejection, timelock in-range accept, ScriptAll multisig accept and reject).
- Integration coverage now also includes `plutus_evaluation.rs`: 6 tests that drive `LedgerState::apply_block_validated()` with mock `PlutusEvaluator` implementations, covering Alonzo V1, Babbage V2, and Conway V3 script dispatch, evaluator failure propagation, evaluator metadata (`script_hash`, `version`, `script_bytes`, `ex_units`), and the no-evaluator soft-skip behavior of `apply_block()`.
- Treasury donation tests (`treasury_donation.rs`): 12 tests covering Conway `utxosDonation` accumulation — single-tx accumulation, none/zero no-op, multi-tx/multi-block accumulation, invalid-tx non-accumulation, flush to treasury (zero/nonzero/additive), CBOR round-trip with element 19, legacy-array default, and epoch-boundary treasury transfer.