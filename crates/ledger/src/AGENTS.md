---
name: ledger-src-subagent
description: Guidance for shared ledger internals outside era-specific modules
---

Focus on core ledger plumbing shared across eras: CBOR codec, core types, and state integration surfaces.

## Scope
- `cbor.rs`, `types.rs`, generic ledger state helpers, and module wiring under `crates/ledger/src`.
- Boundaries between shared ledger infrastructure and `eras/` era-specific logic.

##  Rules *Non-Negotiable*
- Keep CBOR behavior deterministic and round-trip tested.
- Do not duplicate era-specific rules in shared modules.
- Maintain strong type wrappers for protocol-relevant identifiers (`SlotNo`, `BlockNo`, `HeaderHash`, `Point`, `TxId`).
- Public shared APIs MUST have Rustdocs when semantics are non-obvious.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Ledger repository: <https://github.com/IntersectMBO/cardano-ledger>
- Formal specs: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- Hand-rolled CBOR encoder/decoder supports major Cardano-required primitives including signed integers (`integer()`).
- Shared typed core identifiers and point/nonce primitives are in place.
- Shared transaction plumbing now includes `compute_tx_id` plus submitted-transaction wrappers for Shelley-family and Alonzo-family wire shapes, with `MultiEraSubmittedTx` as the era-directed decode boundary for node-to-node relay work.
- Credential and address types landed: `StakeCredential` (key-hash/script-hash), `RewardAccount` (29-byte structured), `Address` (Base/Enterprise/Pointer/Reward/Byron variants), with CBOR codecs and variable-length natural encoding for pointer addresses.
- Certificate hierarchy landed in `types.rs`: `Anchor` (moved from conway.rs), `UnitInterval` (tag-30 rational), `Relay` (3-variant), `PoolMetadata`, `PoolParams` (9-field inline group), `DRep` (4-variant Conway), `DCert` (19-variant flat enum covering Shelley tags 0–5 and Conway tags 7–18), all with CBOR codecs in `cbor.rs`.
- `LedgerState` owns dual UTxO sets: `ShelleyUtxo` (legacy, for backward compat) and `MultiEraUtxo` (generalized). It also carries explicit `PoolState`, `StakeCredentials`, `CommitteeState`, `DrepState`, and `RewardAccounts` containers plus `LedgerStateSnapshot` read-only views and `LedgerStateCheckpoint` restorable checkpoints. `apply_block()` dispatches per era with atomic block application; Byron currently advances the tip as a no-op shared-layer transition, while Shelley-family stake and pool certificate updates, Conway committee hot-key/resignation state updates, Conway DRep registration/delegation updates, reward-withdrawal debits, and full CBOR decode + UTxO validation apply from Shelley onward. `RegisteredPool::relay_access_points()` and `PoolState::relay_access_points()` now expose only directly dialable single-host relay forms with declared ports so higher layers can consume immutable-ledger peers without re-implementing relay decoding.
- `LedgerStateCheckpoint`, `LedgerState`, the state-container wrappers, and both UTxO views now have deterministic CBOR round-trip support so higher layers can persist typed recovery checkpoints instead of opaque ad hoc bytes.
- `utxo.rs` module landed: `MultiEraTxOut` enum (Shelley/Mary/Alonzo/Babbage), `MultiEraUtxo` with per-era apply methods including TTL, validity interval start, coin preservation, and multi-asset preservation checks.
- Era-specific structures live under `eras/`; all eras Shelley through Conway are implemented. Shared layer should stay lightweight and stable.
- `PlutusData` is integrated into: `Redeemer.data` (typed payload), `DatumOption::Inline` (typed inline datum with tag-24 double encoding), `ShelleyWitnessSet.plutus_data` (typed `Vec<PlutusData>`).
- `protocol_params.rs`: `ProtocolParameters` struct with all Shelley-through-Conway parameter fields, `Default` (Shelley mainnet), `alonzo_defaults()`, `min_lovelace_for_utxo()`, full CBOR map-based round-trip codec. Wired into `LedgerState` as `Option<ProtocolParameters>` (array element 10, backward-compatible with 9-element legacy).
- `fees.rs`: Fee calculation and validation — `min_fee_linear()`, `script_fee()`, `total_min_fee()`, `validate_fee()`, `validate_tx_ex_units()`, `validate_tx_size()`.
- `native_script.rs`: Native (timelock) script evaluator — `evaluate_native_script()`, `native_script_hash()` (Blake2b-224 with language tag prefix), `NativeScriptContext`.
- `collateral.rs`: Alonzo+ collateral validation — `validate_collateral()` checks count limit, UTxO lookup, non-ADA rejection, and percentage-of-fee sufficiency.
- `min_utxo.rs`: Minimum UTxO output validation — `validate_min_utxo()`, `validate_all_outputs_min_utxo()` using protocol params.
- `witnesses.rs`: VKey witness sufficiency and Ed25519 signature verification — `validate_vkey_witnesses()`, `verify_vkey_signatures()` (real Ed25519 verification against tx body hash using `yggdrasil_crypto::ed25519`), `vkey_hash()` (Blake2b-224), `witness_vkey_hash_set()`. Helper functions for collecting required VKey hashes and script hashes from spending inputs, certificates, and withdrawals — both Shelley UTxO and multi-era UTxO variants. All per-era `apply_block()` inner loops now call `validate_witnesses_if_present()` to enforce both VKey witness hash sufficiency and Ed25519 signature validity when witness bytes are provided.
- `witnesses.rs` now also exposes `required_script_hashes_from_mint()` so mint policy IDs participate in required-script collection for Mary, Alonzo, Babbage, and Conway block application.
- `plutus_validation.rs`: trait-based Plutus phase-2 seam for the ledger crate — `PlutusEvaluator`, `PlutusScriptEval`, `PlutusVersion`, `ScriptPurpose`, `plutus_script_hash()`, and `validate_plutus_scripts()`. Use this path instead of adding a direct dependency from `ledger` to `plutus`.
- Per-era `apply_block()` native script validation: Allegra, Mary, Alonzo, Babbage, and Conway inner loops call `validate_native_scripts_if_present()` to evaluate required native scripts. Required-script collection now covers script-hash payment credentials in spending inputs, certificates, withdrawals, and mint policy IDs for mint-capable eras. Missing native scripts are silently skipped because they may instead be Plutus scripts resolved by `validate_plutus_scripts()`.
- `state.rs` now exposes `LedgerState::apply_block_validated()` as the evaluator-aware block-application entry point. Higher layers that have a Plutus CEK implementation should call this method and inject a `PlutusEvaluator`; `apply_block()` remains the no-evaluator convenience wrapper.
- `stake.rs`: Stake distribution snapshots and epoch-boundary snapshot rotation — `IndividualStake`, `Delegations`, `StakeSnapshot`, `StakeSnapshots` (three-snapshot ring: mark/set/go + fee pot), `PoolStakeDistribution`, `compute_stake_snapshot()`, `StakeSnapshots::rotate()`, `StakeSnapshot::pool_stake_distribution()`. All types have CBOR round-trip codecs.
- `rewards.rs`: Epoch reward calculation (Shelley spec Section 10) — `RewardParams`, `EpochRewardPot`, `PoolRewardBreakdown`, `EpochRewardDistribution`, `mul_rational()`, `compute_epoch_reward_pot()`, `max_pool_reward()`, `compute_pool_reward()`, `compute_epoch_rewards()`. Uses u128 fixed-point arithmetic with SCALE=10^12.
- `epoch_boundary.rs`: Epoch boundary orchestration (NEWEPOCH / EPOCH / SNAP / RUPD) — `apply_epoch_boundary()` performs stake snapshot rotation, reward distribution, pool retirement + deposit refunds, and treasury/reserves accounting update. `retire_pools_with_refunds()` captures reward accounts before removing pools. Returns `EpochBoundaryEvent` summary.
- `state.rs` deposit tracking: `DepositPot` (key_deposits, pool_deposits, drep_deposits) and `AccountingState` (treasury, reserves) with CBOR codecs. `LedgerState` now 12-field struct with deposit_pot and accounting. `apply_certificates_and_withdrawals()` takes 10 args (deposit_pot + key_deposit + pool_deposit) and tracks deposits across all certificate variants. `process_retirements(epoch)` on `PoolState` removes retiring pools.
- CBOR `Decoder::peek_is_null()` added for nullable field support.
