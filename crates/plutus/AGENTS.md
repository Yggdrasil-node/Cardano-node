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
- **Budget tracking**: CPU/memory cost accounting with configurable `CostModel`.
- **Dependencies**: `yggdrasil-crypto` (blake2b, ed25519), `yggdrasil-ledger` (CBOR, PlutusData), `sha2`.

### Module Layout
- `types.rs` — `Term`, `Constant`, `Value`, `DefaultFun` (90+ builtin variants), `ExBudget`, `Type`.
- `flat.rs` — Flat binary codec, bit-level reader, UPLC `Program` deserialization.
- `machine.rs` — CEK evaluator with budget enforcement and log collection.
- `builtins.rs` — Saturated builtin evaluation dispatch.
- `cost_model.rs` — `CostModel` with per-builtin CPU/memory cost functions.
- `error.rs` — `MachineError` variants.

### Implemented Builtins
- **Integer**: add, subtract, multiply, divide, quotient, remainder, mod, equals, less-than, less-than-equals
- **ByteString**: append, cons, slice, length, index, equals, less-than
- **Crypto**: SHA-256, Blake2b-256, Blake2b-224, Ed25519 verify
- **String**: append, encode-utf8, decode-utf8, equals
- **List**: head, tail, null, mk-cons, choose, mk-nil
- **Pair**: fst, snd, mk-pair
- **Data**: constructors, constr-data, map-data, list-data, i-data, b-data, un-variants, choose-data, equals-data, serialise-data
- **Bool**: if-then-else
- **Unit**: choose-unit, mk-unit
- **Tracing**: trace

### Unimplemented Builtins (return `UnimplementedBuiltin`)
- **PlutusV2**: secp256k1 ECDSA/Schnorr verify
- **PlutusV3**: BLS12-381 operations, keccak-256, ripemd-160, integer↔bytestring, bitwise ops, exp-mod-integer

## Next Steps
1. Add PlutusV2 secp256k1 builtins (dependent on crypto crate adding secp256k1).
2. Add PlutusV3 BLS12-381 builtins (dependent on crypto crate adding BLS12-381; upstream vectors already vendored).
3. Add PlutusV3 bitwise builtins (andByteString, orByteString, etc.).
4. Calibrate cost model against upstream cost-model JSON.
5. Add integration tests with on-chain script samples.
6. Wire into ledger redeemer execution for phase-2 validation.
7. Add SHA3-256, keccak-256, ripemd-160 hash builtins.
