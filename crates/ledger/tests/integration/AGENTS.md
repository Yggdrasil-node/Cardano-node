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

## Official Upstream References *Always research referances and add or update links as needed*
- Ledger test corpus root: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Formal ledger rules: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- The integration test crate is organized into focused modules for core CBOR, Shelley, Allegra/Mary, Alonzo, Byron, Babbage, Conway, Praos block envelopes, governance updates, Plutus/script codecs, multi-era UTxO behavior, ledger-state subdomains, and witness validation.
- `witness_validation.rs`: 12 tests covering VKey witness sufficiency (accept valid, reject missing/wrong, skip when absent, reject empty set), Ed25519 signature verification (reject forged signature, reject signature on wrong body), and native script evaluation through `apply_block()` (ScriptPubkey accept, InvalidBefore/InvalidHereafter timelock rejection, timelock in-range accept, ScriptAll multisig accept and reject with missing key). All tests use real Ed25519 signing via `yggdrasil_crypto::ed25519::SigningKey`.
- `plutus_evaluation.rs`: 6 tests covering `apply_block_validated()` with mock evaluators across Alonzo/Babbage/Conway, including minting-policy Plutus script dispatch, evaluator failure propagation, evaluator metadata assertions, and the no-evaluator soft-skip path via `apply_block()`.