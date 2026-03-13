---
name: ledger-crate-agent
description: Guidance for era-aware ledger work
---

Focus on reusable state-transition interfaces and explicit era boundaries.

## Scope
- Era modeling, transaction and block state transitions, and ledger state evolution.
- Separation between generated wire types and handwritten rules.

##  Rules *Non-Negotiable*
- Specification provenance MUST stay close to each ledger rule.
- Generated data types and handwritten transition logic MUST remain separated.
- The project MUST keep a full era roadmap visible, but implementation MUST proceed one narrow slice at a time.
- Public ledger modules, types, and state-transition functions MUST have Rustdocs where rule intent or invariants are not obvious from the signature.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Era, transaction, and rule naming MUST stay close to official ledger and `cardano-node` terminology.
- Ledger behavior MUST be explained by reference to the official node, the ledger repository, and the formal ledger specifications rather than only local interpretation.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Ledger repository root: <https://github.com/IntersectMBO/cardano-ledger/>
- Era-specific sources and CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Formal ledger specification: <https://github.com/IntersectMBO/formal-ledger-specifications/>
- Published formal spec site: <https://intersectmbo.github.io/formal-ledger-specifications/site/>

## Current Phase
- Core protocol types (`SlotNo`, `BlockNo`, `EpochNo`, `HeaderHash`, `TxId`, `Point`) are landed in `types.rs`.
- Credential and address types landed in `types.rs`: `StakeCredential` enum (`AddrKeyHash`/`ScriptHash`), `RewardAccount` (29-byte with network + credential), `Address` enum (Base/Enterprise/Pointer/Reward/Byron), `AddrKeyHash`, `ScriptHash`, `PoolKeyHash`, `GenesisHash`, `GenesisDelegateHash`, `VrfKeyHash` type aliases.
- `Block` and `BlockHeader` use typed identifiers; `LedgerState` tracks tip via `Point`, owns dual UTxO sets (`ShelleyUtxo` legacy + `MultiEraUtxo` generalized), and carries explicit stake-pool, stake-credential, DRep, and reward-account state containers.
- Shared submitted-transaction abstractions now live in `tx.rs`: `compute_tx_id`, `ShelleyCompatibleSubmittedTx<TBody>` for Shelley/Allegra/Mary 3-element transaction shapes, `AlonzoCompatibleSubmittedTx<TBody>` for Alonzo/Babbage/Conway 4-element transaction shapes, and `MultiEraSubmittedTx::from_cbor_bytes_for_era()` for era-directed decode at the ledger boundary.
- Multi-era UTxO landed: `MultiEraTxOut` enum (Shelley/Mary/Alonzo/Babbage variants), `MultiEraUtxo` with era-specific apply methods (`apply_shelley_tx/.../apply_conway_tx`). Validates non-empty inputs/outputs, TTL, validity interval start, coin preservation, and multi-asset preservation (including mint/burn). New error variants: `TxNotYetValid`, `MultiAssetNotPreserved`.
- `LedgerState.apply_block()` dispatches per era: Shelley uses legacy `ShelleyUtxo`, Allegra through Conway use `MultiEraUtxo`. Byron returns `UnsupportedEra`. `LedgerState.apply_submitted_tx()` now exposes the same era-specific UTxO checks for single submitted-transaction admission while preserving atomicity on rejection.
- CBOR codec (`cbor.rs`) supports all 8 major types plus signed integer helpers (`Encoder::integer`, `Decoder::integer`). Includes `skip()` for recursive item skipping and `CborEncode`/`CborDecode` traits.
- Allegra era types landed: `AllegraTxBody` (optional TTL + validity interval start), `NativeScript` (6-variant timelock/multi-sig enum with recursive CBOR codec).
- Mary era types landed: `Value` (coin/multi-asset), `MultiAsset`, `MintAsset`, `MaryTxOut`, `MaryTxBody` (key 9 mint) with CBOR codecs; `pub(crate)` helpers shared cross-era.
- Alonzo era types landed: `ExUnits`, `Redeemer` (typed `PlutusData` payload), `AlonzoTxOut` (optional datum hash), `AlonzoTxBody` (keys 0–15 including certificates/withdrawals/update), `AlonzoBlock` (5-element CBOR array with TPraos `ShelleyHeader` and `invalid_transactions`).
- Byron envelope landed: `ByronBlock` enum (EBB/MainBlock) with lightweight decode for slot tracking, `BYRON_SLOTS_PER_EPOCH`.
- Babbage era types landed: `DatumOption` (hash or inline typed `PlutusData`), `BabbageTxOut` (dual-format decode: pre-Babbage array + post-Alonzo map, with script_ref), `BabbageTxBody` (keys 0–18 including certificates/withdrawals/update). `BabbageBlock` uses `PraosHeader` (14-element header body with single `vrf_result`).
- Conway era types landed: `Vote`, `Voter` (5-variant: CommitteeKeyHash/Script, DRepKeyHash/Script, StakePool), `GovActionId`, `Constitution` (anchor + optional guardrails script hash), `GovAction` (7-variant typed enum: ParameterChange/HardForkInitiation/TreasuryWithdrawals/NoConfidence/UpdateCommittee/NewConstitution/InfoAction), `VotingProcedure`, `ProposalProcedure` (typed `GovAction`), `VotingProcedures` (nested BTreeMap), `ConwayTxBody` (keys 0–22 including certificates/withdrawals; key 6 update removed in Conway). `ConwayBlock` uses `PraosHeader`.
- TxBody keys 4-6 landed across all eras: `certificates` (`Option<Vec<DCert>>`), `withdrawals` (`Option<BTreeMap<RewardAccount, u64>>`), and `update` (`Option<ShelleyUpdate>` typed struct with opaque param update values, Shelley–Babbage only).
- WitnessSet expansion: `ShelleyWitnessSet` now handles all CDDL keys 0–7 (`vkey_witnesses`, `native_scripts`, `bootstrap_witnesses`, `plutus_v1_scripts`, `plutus_data` (typed `Vec<PlutusData>`), `redeemers` (typed `PlutusData` payload), `plutus_v2_scripts`, `plutus_v3_scripts`). `BootstrapWitness` is a typed struct (vkey, signature, chain_code, attributes). Redeemer decode supports both array (Alonzo/Babbage) and map (Conway) formats.
- Certificate hierarchy landed in `types.rs`: `Anchor` (moved from conway.rs to types.rs), `UnitInterval` (tag-30 rational), `Relay` (3-variant: SingleHostAddr/SingleHostName/MultiHostName), `PoolMetadata`, `PoolParams` (9-field inline group), `DRep` (4-variant: KeyHash/ScriptHash/AlwaysAbstain/AlwaysNoConfidence), `DCert` (19-variant flat enum: Shelley tags 0–5, Conway tags 7–18), all with CBOR codecs.
- Ledger query surfaces now include `LedgerStateSnapshot`, merged address/balance UTxO queries, and read-only access to explicit `PoolState` and `RewardAccounts` containers.
- `LedgerState` now applies pool registration/retirement certificates atomically across Shelley through Conway, models Shelley-family stake-credential registration/unregistration/delegation-to-pool certificates plus Conway committee authorization/resignation state, Conway DRep registration/update/unregistration and DRep delegation variants, and debits reward withdrawals before UTxO preservation checks. Governance proposals and committee enactment remain outside this ledger slice.
- Full era type coverage complete: Byron → Conway.
- Block body hash: `compute_block_body_hash` computes Blake2b-256 of elements 1..N (everything after header). Supports 4-element (Shelley) and 5-element (Babbage/Conway) blocks.
- PlutusData AST landed in `plutus.rs`: recursive `PlutusData` enum (`Constr`/`Map`/`List`/`Integer`/`Bytes`) with CBOR codec supporting compact constructor tags 121–127, general form tag 102, big_uint (#6.2), big_nint (#6.3). `Script` enum (Native/PlutusV1/V2/V3) and `ScriptRef` (tag-24 double encoding). `BabbageTxOut.script_ref` is now typed `Option<ScriptRef>` instead of opaque bytes.
- Keep the full era roadmap visible, but land only narrow reusable slices.
- Prefer types and harnesses that will survive later era expansion.
