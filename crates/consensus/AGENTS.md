---
name: consensus-crate-agent
description: Guidance for Ouroboros consensus work
---

Focus on deterministic chain selection, epoch math, rollback handling, and leader-election boundaries.

## Scope
- Praos and future Genesis-specific consensus behavior.
- Chain selection, rollback coordination, and epoch or slot math.

##  Rules *Non-Negotiable*
- Slots, epochs, density inputs, and other protocol values MUST use explicit types.
- Praos-specific logic MUST stay separate from future Genesis extensions.
- Reproducible fixtures MUST exist before any claim of parity with Cardano behavior is accepted.
- Public consensus types and functions MUST have Rustdocs when they encode protocol math, chain selection rules, or rollback semantics.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Names MUST track official consensus and `cardano-node` terminology so traces, fixtures, and parity checks remain comparable.
- Consensus behavior MUST be explained by reference to the official node and upstream Ouroboros consensus sources before any local terminology is introduced.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Core consensus implementation: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- Consensus repository documentation: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Formal consensus Agda specification: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/agda-spec/>
- Cardano-specific consensus integration: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-cardano/>

## Current Phase
- Epoch math (`slot_to_epoch`, `epoch_first_slot`, `is_new_epoch`) operates on typed `SlotNo`/`EpochNo`/`EpochSize` from `yggdrasil-ledger`.
- Chain selection uses typed `BlockNo`/`SlotNo` with optional VRF tiebreaker (lower wins).
- Praos leader election pipeline is implemented: `vrf_input` → `check_is_leader` → `verify_leader_proof`, backed by the crypto crate's standard VRF (80-byte proofs per CDDL `vrf_cert = [bytes, bytes .size 80]` and upstream `VRF StandardCrypto = PraosVRF`).
- `Nonce` type (neutral + hash, XOR combination) lives in `yggdrasil-ledger::types`.
- Leadership threshold uses deterministic fixed-point BigUint arithmetic (`taylorExpCmp` + rational sigma) — matches upstream Haskell `checkLeaderNatValue`. `ActiveSlotCoeff` stores pre-computed `-ln(1-f)` as rational `(log_num, log_den)` BigUint. No floating-point in the chain-deciding path. Dependencies: `num-bigint`, `num-integer`, `num-traits`.
- Operational certificate (`OpCert`) type and verification implemented in `opcert.rs`: cold-key signature over (hot_vkey ‖ sequence_number ‖ kes_period), KES period window checks, `kes_period_of_slot` helper.
- Block header types (`HeaderBody`, `Header`) and full verification pipeline in `header.rs`: verify OpCert → check KES period → verify KES signature over header body. SumKES signing/verification at configurable depth (0–6+).
- Field names aligned with CDDL: `HeaderBody` uses `block_number`, `slot`, `issuer_vkey`, `vrf_vkey`, `leader_vrf_output`, `leader_vrf_proof`, `nonce_vrf_output`, `nonce_vrf_proof`, `block_body_size`, `block_body_hash`, `operational_cert`; `OpCert` uses `hot_vkey`, `sequence_number`.
- `HeaderBody` now carries VRF proof data: `leader_vrf_output`/`leader_vrf_proof` (always present) and `nonce_vrf_output`/`nonce_vrf_proof` (TPraos only, `None` for Praos). This enables `verify_leader_proof` to be called from the sync pipeline when epoch nonce and stake data are available.
- Epoch nonce evolution state machine in `nonce.rs`: `NonceEvolutionState` tracks evolving/candidate/epoch/prev-hash/lab nonces and implements the combined UPDN + TICKN rules from `cardano-protocol-tpraos`. `vrf_output_to_nonce` converts VRF output to a `Nonce` via Blake2b-256. `NonceEvolutionConfig` holds epoch size, stability window, and extra entropy. 13 integration tests cover per-block update, stability-window freezing, epoch transition, multi-epoch chains, extra entropy, and boundary conditions.
- `SecurityParam` type (Ouroboros `k` parameter) and `ChainState` volatile chain tracker in `chain_state.rs`: roll-forward/roll-backward with max rollback depth enforcement, stability window detection (`stable_count`, `drain_stable`), non-contiguous block rejection.
- Do not add Cardano-specific protocol detail until ledger and crypto inputs are stable enough to support it.
