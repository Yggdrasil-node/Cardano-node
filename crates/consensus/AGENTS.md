---
name: consensus-crate-agent
description: Guidance for Ouroboros consensus work
---

Focus on deterministic chain selection, epoch math, rollback handling, and leader-election boundaries.

## Scope
- Praos and future Genesis-specific consensus behavior.
- Chain selection, rollback coordination, and epoch or slot math.

## Non-Negotiable Rules
- Slots, epochs, density inputs, and other protocol values MUST use explicit types.
- Praos-specific logic MUST stay separate from future Genesis extensions.
- Reproducible fixtures MUST exist before any claim of parity with Cardano behavior is accepted.
- Public consensus types and functions MUST have Rustdocs when they encode protocol math, chain selection rules, or rollback semantics.
- Names MUST track official consensus and `cardano-node` terminology so traces, fixtures, and parity checks remain comparable.
- Consensus behavior MUST be explained by reference to the official node and upstream Ouroboros consensus sources before any local terminology is introduced.

## Upstream References (add or update as needed)
- Core consensus implementation: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- Consensus repository documentation: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Formal consensus Agda specification: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/agda-spec/>
- Cardano-specific consensus integration: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-cardano/>

## Current Phase
- Epoch math (`slot_to_epoch`, `epoch_first_slot`, `is_new_epoch`) operates on typed `SlotNo`/`EpochNo`/`EpochSize` from `yggdrasil-ledger`.
- Chain selection uses typed `BlockNo`/`SlotNo` with optional VRF tiebreaker (lower wins).
- Praos leader election pipeline is implemented: `vrf_input` → `check_is_leader` → `verify_leader_proof`, backed by the crypto crate's standard VRF (80-byte proofs per CDDL `vrf_cert = [bytes, bytes .size 80]` and upstream `VRF StandardCrypto = PraosVRF`).
- `Nonce` type (neutral + hash, XOR combination) lives in `yggdrasil-ledger::types`.
- Leadership threshold uses f64 arithmetic for now; deterministic fixed-point math is a future hardening target.
- Operational certificate (`OpCert`) type and verification implemented in `opcert.rs`: cold-key signature over (hot_vkey ‖ sequence_number ‖ kes_period), KES period window checks, `kes_period_of_slot` helper.
- Block header types (`HeaderBody`, `Header`) and full verification pipeline in `header.rs`: verify OpCert → check KES period → verify KES signature over header body. SumKES signing/verification at configurable depth (0–6+).
- Field names aligned with CDDL: `HeaderBody` uses `block_number`, `slot`, `issuer_vkey`, `vrf_vkey`, `block_body_size`, `block_body_hash`, `operational_cert`; `OpCert` uses `hot_vkey`, `sequence_number`.
- `SecurityParam` type (Ouroboros `k` parameter) and `ChainState` volatile chain tracker in `chain_state.rs`: roll-forward/roll-backward with max rollback depth enforcement, stability window detection (`stable_count`, `drain_stable`), non-contiguous block rejection.
- Do not add Cardano-specific protocol detail until ledger and crypto inputs are stable enough to support it.
