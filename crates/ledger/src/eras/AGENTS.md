---
name: ledger-eras-subagent
description: Guidance for per-era ledger modules and era transition boundaries
---

Focus on per-era differences, transition boundaries, and keeping era-local details out of generic ledger plumbing.

## Scope
- Era-specific data, behavior differences, and transition markers.
- Shared naming and boundary consistency across Byron through Conway.

##  Rules *Non-Negotiable*
- One file or module SHOULD stay focused on one era concern whenever possible.
- Generic ledger logic MUST NOT be duplicated inside `eras/`.
- Each era module MUST make it clear whether it is a placeholder or reflects a real upstream rule set.
- Public era-specific types or helpers MUST have Rustdocs when the era difference is not obvious from naming alone.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Official era names, rule labels, and transition terminology from upstream ledger and node sources MUST be preferred.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Era sources and CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Formal ledger specification site: <https://intersectmbo.github.io/formal-ledger-specifications/site/>
- Formal ledger specification repository: <https://github.com/IntersectMBO/formal-ledger-specifications/>

## Current Phase
- Shelley: full block, header, tx body (keys 0–7 including certificates/withdrawals/update), `ShelleyUpdate` (typed `[{genesis_hash => protocol_param_update}, epoch]` with opaque param update values), witness set (keys 0–7: vkey, native_scripts, bootstrap_witnesses, plutus_v1/v2/v3 scripts, plutus_data, redeemers), typed `BootstrapWitness`, UTxO, and VRF/OpCert types with CBOR codecs. Block-level field names align with upstream CDDL: `block_number`, `slot`, `issuer_vkey`, `vrf_vkey`, `nonce_vrf`, `leader_vrf`, `block_body_size`, `block_body_hash`, `operational_cert`, `transaction_witness_sets`, `transaction_metadata_set`. OpCert fields: `hot_vkey`, `sequence_number`, `kes_period`, `sigma`. Also defines `PraosHeaderBody` (14-element CBOR array with single `vrf_result` instead of `nonce_vrf`+`leader_vrf`) and `PraosHeader` (`[PraosHeaderBody, kes_signature]`) used by Babbage/Conway blocks.
- Allegra: `AllegraTxBody` (keys 0–8 including certificates/withdrawals/update using `ShelleyUpdate`) and `NativeScript` (6-variant timelock enum) with CBOR codecs.
- Mary: `Value` (coin/multi-asset), `MultiAsset`, `MintAsset`, `MaryTxOut`, `MaryTxBody` (keys 0–9 including certificates/withdrawals/update using `ShelleyUpdate`) with CBOR codecs.
- Alonzo: `ExUnits`, `Redeemer` (typed `PlutusData` payload), `AlonzoTxOut` (optional datum hash), `AlonzoTxBody` (keys 0–15 including certificates/withdrawals/update using `ShelleyUpdate`), `AlonzoBlock` (5-element CBOR array: `ShelleyHeader`, tx_bodies, witnesses, auxiliary_data_set, invalid_transactions — same as Babbage/Conway block structure but with 15-element TPraos header instead of 14-element Praos header) with CBOR codecs.
- Byron: `ByronBlock` enum (EBB/MainBlock) with structural header decode — epoch, slot-in-epoch, `chain_difficulty` (block number), prev_hash, and captured raw header bytes. `header_hash()` implements correct Byron header hash: `Blake2b-256(prefix ++ raw_header_cbor)` with variant-specific prefix (`0x82 0x00` for EBB, `0x82 0x01` for Main). Full Byron transaction types: `ByronTxIn` (`[0, #6.24(cbor [txid, u32])]`), `ByronTxOut` (`[address_raw_cbor, coin]`), `ByronTx` (`[inputs, outputs, attributes]` with `tx_id()` via Blake2b-256), `ByronTxWitness` (`[type, #6.24(payload)]`), `ByronTxAux` (`[tx, [witnesses]]`). MainBlock carries `transactions: Vec<ByronTxAux>` decoded from block body `tx_payload`. All Byron TX types have CborEncode/CborDecode implementations handling CBOR tag 24 (CBOR-in-CBOR) encoding.
- Babbage: `DatumOption` (hash/inline typed `PlutusData`), `BabbageTxOut` (dual-format array+map with typed `ScriptRef`), `BabbageTxBody` (keys 0–18 including certificates/withdrawals/update using `ShelleyUpdate`), `BabbageBlock` (5-element CBOR array: `PraosHeader`, tx_bodies, witnesses, auxiliary_data_set, invalid_transactions) with CBOR codecs.
- Conway: `Vote`, `Voter` (5-variant), `GovActionId`, `Constitution` (anchor + optional guardrails script hash), `GovAction` (7-variant typed enum: ParameterChange/HardForkInitiation/TreasuryWithdrawals/NoConfidence/UpdateCommittee/NewConstitution/InfoAction), `VotingProcedure`, `ProposalProcedure` (typed `GovAction`), `VotingProcedures` (nested BTreeMap), `ConwayTxBody` (keys 0–22 including certificates/withdrawals; key 6 update removed in Conway), `ConwayBlock` (5-element CBOR array: `PraosHeader`, tx_bodies, witnesses, auxiliary_data_set, invalid_transactions) with CBOR codecs. `Anchor` moved to `types.rs` for cross-era reuse.
- Certificate hierarchy: `DCert` variants aligned with CDDL names (`AccountRegistration`, `AccountUnregistration`, `DelegationToStakePool`, `PoolRegistration`, `PoolRetirement`, `GenesisDelegation`, plus Conway `AccountRegistrationDeposit` through `DrepUpdate`).
- Full era type coverage: Byron → Conway.
- Keep additions lightweight until generated types and real transition logic land.
