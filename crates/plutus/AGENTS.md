---
name: plutus-crate-agent
description: Guidance for the UPLC evaluator and Plutus script execution engine
---

Focus on deterministic CEK machine behavior, cost model accuracy, and upstream parity with the official Plutus evaluator.

## Scope
- UPLC term language, Flat binary decoding, CEK machine evaluation.
- Built-in function implementations for PlutusV1/V2/V3.
- Execution budget tracking and cost model calibration.
- Integration boundary with ledger redeemer execution.

##  Rules *Non-Negotiable*
- Evaluation semantics MUST match upstream `plutus-core` CEK machine behavior.
- Builtin names and arity MUST follow upstream `DefaultFun` naming.
- Cost model parameter names MUST stay aligned with upstream `CostModel` keys.
- Budget enforcement MUST be deterministic — identical inputs produce identical budget outcomes.
- No FFI-backed dependencies.
- Stay true to the official type naming and terminology from the `IntersectMBO/plutus` repository.
- Always read the folder specific `**/AGENTS.md` files.

## Official Upstream References *Always research references and add or update links as needed*
- Plutus core repository: <https://github.com/IntersectMBO/plutus>
- CEK machine: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/Cek>
- Builtin semantics: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Default/Builtins.hs>
- Cost model parameters: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core/cost-model>
- Flat encoding spec: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core/plutus-core/src/PlutusCore/Flat>

## Current Status
- **CEK machine**: complete. De Bruijn indices, closures, partial application.
- **Flat decoder**: complete. Parses on-chain script bytes into UPLC `Program`.
- **PlutusV1 builtins**: all 60+ implemented (integer, bytestring, string, bool, list, pair, data, crypto).
- **PlutusV2 builtins**: secp256k1 ECDSA/Schnorr verify, SHA3-256, Keccak-256 — all implemented.
- **PlutusV3 builtins**: RIPEMD-160, integer↔bytestring conversion, all bitwise operations, modular exponentiation, BLS12-381 (17 builtins: G1/G2 add/neg/scalar-mul/equal/hash-to-group/compress/uncompress, miller-loop, mul-ml-result, final-verify) — all implemented.
- **Budget tracking**: CPU/memory cost accounting with configurable `CostModel`.
- **Dependencies**: `yggdrasil-crypto` (blake2b, ed25519, secp256k1), `yggdrasil-ledger` (CBOR, PlutusData), `sha2`, `sha3`, `ripemd`.

### Module Layout
- `types.rs` — `Term`, `Constant`, `Value`, `DefaultFun` (87 builtin variants), `ExBudget`, `Type`.
- `flat.rs` — Flat binary codec, bit-level reader, UPLC `Program` deserialization.
- `machine.rs` — CEK evaluator with budget enforcement and log collection.
- `builtins.rs` — Saturated builtin evaluation dispatch with helper functions for hash, crypto, bitwise, and conversion operations.
- `cost_model.rs` — `CostModel` with per-builtin CPU/memory cost functions.
- `error.rs` — `MachineError` variants.

### Integration with `node` crate
`node/src/plutus_eval.rs` implements `yggdrasil_ledger::plutus_validation::PlutusEvaluator` using `yggdrasil_plutus::evaluate_term`. `CekPlutusEvaluator` decodes script bytes via `decode_script_bytes`, builds term-level argument applications (datum if spending, redeemer, placeholder `ScriptContext`), and evaluates with the `ExBudget` declared by the transaction. The current simplified flat `CostModel` can now be calibrated from the upstream named Alonzo genesis `costModels.PlutusV1` map via `CostModel::from_alonzo_genesis_params()`, which maps shared CEK step costs (`Var`/`Const`/`Lam`/`Delay`/`Force`/`Apply`) and `cekBuiltinCost-*` onto the crate's flat four-field model. Full per-builtin parameterized costing and full `ScriptContext` / `TxInfo` construction remain future milestones.

### Implemented Builtins
- **Integer**: add, subtract, multiply, divide, quotient, remainder, mod, equals, less-than, less-than-equals
- **ByteString**: append, cons, slice, length, index, equals, less-than
- **Crypto**: SHA-256, SHA3-256, Blake2b-256, Blake2b-224, Keccak-256, RIPEMD-160, Ed25519 verify, secp256k1 ECDSA verify, secp256k1 Schnorr verify
- **String**: append, encode-utf8, decode-utf8, equals
- **List**: head, tail, null, mk-cons, choose, mk-nil
- **Pair**: fst, snd, mk-pair
- **Data**: constructors, constr-data, map-data, list-data, i-data, b-data, un-variants, choose-data, equals-data, serialise-data
- **Bool**: if-then-else
- **Unit**: choose-unit, mk-unit
- **Tracing**: trace
- **Conversion**: integerToByteString, byteStringToInteger
- **Bitwise**: andByteString, orByteString, xorByteString, complementByteString, readBit, writeBits, replicateByte, shiftByteString, rotateByteString, countSetBits, findFirstSetBit
- **Modular arithmetic**: expModInteger

### BLS12-381 Builtins (PlutusV3)
- All 17 BLS12-381 builtins fully implemented: `Bls12_381_G1_add`, `Bls12_381_G1_neg`, `Bls12_381_G1_scalarMul`, `Bls12_381_G1_equal`, `Bls12_381_G1_hashToGroup`, `Bls12_381_G1_compress`, `Bls12_381_G1_uncompress`, and G2 equivalents, plus `Bls12_381_millerLoop`, `Bls12_381_mulMlResult`, `Bls12_381_finalVerify`.
- `Type` and `Constant` enums extended with `Bls12_381_G1_Element`, `Bls12_381_G2_Element`, `Bls12_381_MlResult` variants.
- Flat decoder handles tags 9/10/11 for BLS types.
- `MachineError::CryptoError(String)` variant added for BLS operation failures.

## Next Steps
1. Replace the current flat CEK model with per-builtin parameterized costing keyed by the full upstream cost-model parameter set.
2. Add integration tests with on-chain script samples.
3. Expand version-aware cost-model support so Conway `plutusV3CostModel` arrays can be mapped without falling back to Alonzo named maps.
