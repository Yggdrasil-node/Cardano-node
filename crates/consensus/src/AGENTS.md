# Guidance for consensus source modules implementing typed chain selection and nonce evolution logic.
This directory owns consensus implementation modules, not integration glue.

## Scope
- `chain_state`, `nonce`, `header`, `leader`, and operational certificate logic.
- Typed consensus math and verification rules.

##  Rules *Non-Negotiable*
- Consensus math and rollback rules MUST stay explicit and typed.
- Source modules here MUST remain independent of node runtime orchestration concerns.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Consensus source tree](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus)
- [Protocol modules (`Abstract.hs`, `Praos.hs`, `Praos/Common.hs`, `TPraos.hs`)](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-protocol/src/Ouroboros/Consensus/Protocol)
- [Cardano-specific consensus integration](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-cardano/src/)
- [Block forge and header validation](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Block)
- [Consensus Agda specification](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/agda-spec/)
- [Consensus tech report PDF](https://ouroboros-consensus.cardano.intersectmbo.org/pdfs/report.pdf)
- [Consensus Haddock](https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/)

## Current Phase
- Preserve the current separation between header verification, epoch nonce evolution, and volatile chain tracking.
- `diffusion_pipelining.rs` now owns tentative-header criterion/state primitives (`TentativeHeaderState`, `TentativeState`, `PeerPipeliningState`) aligned with upstream `SupportsDiffusionPipelining`, so node/runtime wiring can keep DPvDV policy out of orchestration code.
- `in_future.rs` now owns upstream-aligned future-header judgement primitives (`ClockSkew`, `FutureSlotJudgement`, `judge_header_slot`) so node runtime can enforce `InFutureCheck`-style far-future rejection without embedding consensus math in orchestration code.
- `opcert.rs` now owns `OcertCounters` (upstream `PraosState.csCounters`), a per-pool monotonic OpCert sequence-number tracker with `validate_and_update()` implementing the upstream `currentIssueNo` check from `Ouroboros.Consensus.Protocol.Praos`. Errors: `NoCounterForKeyHash`, `OcertCounterTooOld`, `OcertCounterTooFar`.
- `nonce.rs` now owns `NonceDerivation` enum (TPraos / Praos): TPraos uses simple `Blake2b-256(output)` (upstream `hashVerifiedVRF`); Praos uses `Blake2b-256(Blake2b-256("N" || output))` (upstream `vrfNonceValue` from `Ouroboros.Consensus.Protocol.Praos.VRF`). `derive_vrf_nonce()` dispatches by derivation variant. `apply_block()` accepts `NonceDerivation` parameter so node-level sync can select per-era derivation.
- `praos.rs` now owns era-aware VRF input construction and leader-value check via `VrfMode` (TPraos / Praos) and `VrfUsage` (Leader / Nonce): `praos_vrf_input()` (upstream `mkInputVRF` — `Blake2b-256(slot_be8 || nonce_bytes)` → 32 bytes), `tpraos_vrf_seed()` (upstream `mkSeed` — base hash XOR `Blake2b-256(CBOR(tag))` with `seedL`/`seedEta`), `check_leader_value()` mode-aware range extension (TPraos: raw 64-byte output, `certNatMax = 2^512`; Praos: `Blake2b-256("L" || output)` → 32 bytes, `certNatMax = 2^256`), `verify_nonce_proof()` cryptographically verifies TPraos `bheaderEta` nonce VRF proof using `tpraos_vrf_seed(slot, epoch_nonce, VrfUsage::Nonce)` (upstream `vrfChecks` in `Cardano.Protocol.TPraos.OCert`). All leader election and verification functions now take `VrfMode`. Reference: `Ouroboros.Consensus.Protocol.Praos.VRF` (`mkInputVRF`, `vrfLeaderValue`, `checkLeaderNatValue`), `Cardano.Protocol.TPraos.BHeader` (`mkSeed`, `checkLeaderValue`).