---
name: ledger-integration-tests
description: Guidance for the folder-backed ledger integration test crate.
---

Keep modules in this folder focused on a single era family or ledger-state subdomain.

## Scope
- Folder-backed integration modules loaded from `tests/integration.rs` via `tests/integration/mod.rs`.
- Era codec and block-envelope regression tests.
- Ledger-state behavior tests split by operational domain.

## Rules *Non-Negotiable*
- Prefer new files when a module starts mixing unrelated eras or ledger subsystems.
- Keep era boundaries explicit: Byron, Shelley, Allegra/Mary, Alonzo, Babbage, Conway, and Praos block-envelope tests should not collapse back into one catch-all file.
- Keep ledger-state tests grouped by behavior such as era application, stake and DRep state, committee state, or pool/reward/query behavior.
- Shared helpers MUST remain scarce, obvious, and `pub(super)` when cross-module access is required.
- Module names MUST describe the rule surface being protected, not the implementation phase that added them.
- Always update this file when the integration module layout changes.

## Official Upstream References *Always research references and add or update links as needed*
- Shelley ledger tests: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/shelley/test>
- Alonzo ledger tests: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/alonzo/test>
- Conway ledger tests (governance, ratification): <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/conway/test>
- Formal ledger rules (Agda): <https://github.com/IntersectMBO/formal-ledger-specifications>
- Ledger conformance test utilities: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-conformance>

## Current Phase
- The integration test crate is organized into focused modules for core CBOR, Shelley, Allegra/Mary, Alonzo, Byron, Babbage, Conway, Praos block envelopes, governance updates, Plutus/script codecs, multi-era UTxO behavior, ledger-state subdomains, and witness validation.
- `witness_validation.rs`: end-to-end `apply_block()` validation coverage for VKey witness sufficiency, Ed25519 signature verification, native script evaluation, and Conway governance guardrails including bootstrap proposal allow-reject paths (`HardForkInitiation`, `ParameterChange`, and `InfoAction` acceptance plus non-bootstrap rejection), bootstrap vote allow-reject paths (DRep `InfoAction`, committee/SPO bootstrap-action acceptance, and non-bootstrap rejection), post-bootstrap voter permission tests (SPO security-group `ParameterChange` boundary: accepted when update touches security params, rejected when only non-security params), voter-existence checks, and governance-action validation. E2E governance lifecycle tests cover vote recast overwrites (Yes→No via separate blocks), proposal-and-vote in same block (intra-block visibility), cross-block lineage chain (HardFork v10→v11 via separate blocks), cert+proposal+vote combo (stake registration + proposal + vote in single block), and multiple proposals in one tx (distinct gov_action_index indexing). All signing-based tests use real Ed25519 signing via `yggdrasil_crypto::ed25519::SigningKey`.
- `plutus_evaluation.rs`: 6 tests covering `apply_block_validated()` with mock evaluators across Alonzo/Babbage/Conway, including minting-policy Plutus script dispatch, evaluator failure propagation, evaluator metadata assertions, and the no-evaluator soft-skip path via `apply_block()`.