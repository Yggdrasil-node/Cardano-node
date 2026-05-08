## Round 273g — `plutus/types.rs` split into `types/{term,default_fun,runtime}.rs`

Date: 2026-05-08
Branch: main
Type: Filename-mirror refactor (Phase γ R273 seventh slice — plutus crate)

### Slice scope

Split `crates/plutus/src/types.rs` (1,707 lines) into a 944-line
parent `types.rs` shell + three new sub-modules:

- `crates/plutus/src/types/term.rs` (141 lines): UPLC term language
  (`Program`, `Term`, `Type`, `Constant`).
- `crates/plutus/src/types/default_fun.rs` (513 lines): UPLC built-in
  function enumeration (`DefaultFun` with 71+ variants + impls).
- `crates/plutus/src/types/runtime.rs` (182 lines): CEK machine
  runtime types (`ExBudget`, `Value`, `Environment`).

The residual `types.rs` keeps the module-level docstring,
`pub mod` declarations, `pub use` re-exports of the 8-item public
surface that `crates/plutus/src/lib.rs::pub use types::{...}`
re-exports, and the unchanged tests block (~786 lines).

### Content distribution

**`types/term.rs`** — mirrors upstream `UntypedPlutusCore.Core.Type`
(`Program`, `Term`) and `PlutusCore.Core.Type` (`Type`) and
`PlutusCore.Default.Universe` (`Constant` atoms used by the untyped
core):

- `pub struct Program` — UPLC program: version triple + body term.
- `pub enum Term` — UPLC term language (10 variants: Var, LamAbs,
  Apply, Delay, Force, Constant, Builtin, Error, Constr, Case).
- `pub enum Type` — typed-PLC type expressions (carried in Flat
  constant encoding).
- `pub enum Constant` — built-in constant atoms (Integer,
  ByteString, String, Unit, Bool, ProtoList, ProtoPair, Data,
  Bls12_381_G1_Element, Bls12_381_G2_Element, Bls12_381_MlResult).
- `impl Constant::integer` — arbitrary-precision constructor.

**`types/default_fun.rs`** — mirrors upstream
`PlutusCore.Default.Builtins.DefaultFun`:

- `pub enum DefaultFun` (`#[repr(u8)]`) — 71+ built-in operations
  covering integer arithmetic, ByteString operations, Plutus Data
  constructors / destructors, Plutus V3 BLS12-381 + ECDSA + BIP-340
  primitives, plus the Plutus V1.1.0+ Constr/Case mechanism.
- `impl DefaultFun` — `from_u8` decoder, `arity`, `force_count`,
  `requires_*` accessors used by the CEK machine when applying
  builtins.

**`types/runtime.rs`** — mirrors upstream
`UntypedPlutusCore.Evaluation.Machine.Cek.Internal::Value` /
`UntypedPlutusCore.Evaluation.Machine.Cek.Internal::Env` and
`PlutusCore.Evaluation.Machine.ExBudget::ExBudget`:

- `pub struct ExBudget` (cpu, mem) + `impl ExBudget` (`new`,
  `is_non_negative`, arithmetic helpers).
- `pub enum Value` — closed values produced by reduction (Constant,
  LamAbs, DelayClosure, Builtin, Constr).
- `pub struct Environment` — `Arc`-backed cons-list mapping de
  Bruijn indices to closure values.
- `struct EnvNode` (private) — internal cons-cell linking
  `Environment` chains.
- `impl Environment` — `new`, `extend`, `lookup`.

runtime.rs reaches both other sub-modules via
`use super::default_fun::DefaultFun;` and
`use super::term::{Constant, Term};`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/plutus/src/types.rs` (shell) | n/a (top-level UPLC types entry point) |
| `crates/plutus/src/types/term.rs` | `UntypedPlutusCore.Core.Type` (Program, Term) + `PlutusCore.Core.Type` (Type) + `PlutusCore.Default.Universe` (Constant atoms) |
| `crates/plutus/src/types/default_fun.rs` | `PlutusCore.Default.Builtins.DefaultFun` |
| `crates/plutus/src/types/runtime.rs` | `UntypedPlutusCore.Evaluation.Machine.Cek.Internal` (Value, Env) + `PlutusCore.Evaluation.Machine.ExBudget` (ExBudget) |

### Cross-module dependencies

- 8-item public surface preserved via sub-module `pub use`
  re-exports — no `lib.rs` edits needed.
- `Environment` added to the parent's `pub use runtime::{...}` block
  because `crate::types::Environment` is referenced externally
  (e.g. in `cost_model.rs`).
- term.rs imports `DefaultFun` from `default_fun` (Term::Builtin
  variant carries it).
- runtime.rs imports `Constant` and `Term` from `term`, `DefaultFun`
  from `default_fun` (Value::Constant / LamAbs / DelayClosure /
  Builtin / Constr variants reference them).

### Visibility / dependency fixups

1. **Test imports** — tests `use super::*;` previously transitively
   pulled `Arc`, `BigInt`, `MachineError`, `PlutusData` via the
   file-level `use` blocks; now imported explicitly via
   `use crate::error::MachineError;`,
   `use num_bigint::BigInt;`,
   `use yggdrasil_ledger::plutus::PlutusData;`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/plutus/src/types.rs` | 1,707 | 944 | −763 |
| `crates/plutus/src/types/term.rs` | (new) | 141 | +141 |
| `crates/plutus/src/types/default_fun.rs` | (new) | 513 | +513 |
| `crates/plutus/src/types/runtime.rs` | (new) | 182 | +182 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Cumulative R273 progress

| Slice | Files moved/created | Source file Δ |
|---|---|---|
| R273a (praos) | `praos/{vrf,common}.rs` | 793 → 464 (−329) |
| R273b (nonce) | `nonce/{derivation,evolution}.rs` | 832 → 448 (−384) |
| R273c (opcert) | `opcert/{cert,counter}.rs` | 856 → 547 (−309) |
| R273d (mempool/queue) | `queue/{inner,shared}.rs` | 1,665 → 731 (−934) |
| R273e (mempool/tx_state) | `tx_state/{state,shared}.rs` | 768 → 319 (−449) |
| R273f (diffusion_pipelining) | `diffusion_pipelining/{identity,state}.rs` | 747 → 291 (−456) |
| **R273g (plutus/types)** | **`types/{term,default_fun,runtime}.rs`** | **1,707 → 944 (−763)** |

Total moved: ~3,624 lines across 15 sub-modules.

### Stop point — R273h is the next slice

Remaining ≥1,000-line files (highest impact):

| File | Lines | Likely split |
|---|---|---|
| `crates/plutus/src/cost_model.rs` | 1,718 | per-cost-class split (CPU model + Mem model + Builtin costs) |
| `crates/plutus/src/builtins.rs` | 1,483 | per-builtin-class split (integer / bytestring / string / data / pair / bls / ecdsa) |
| `crates/plutus/src/machine.rs` | 1,460 | CEK loop core vs decoder vs context |
| `crates/plutus/src/flat.rs` | 1,245 | Flat encoder vs decoder |
| `crates/crypto/src/vrf.rs` | 1,254 | per-VRF-mode (ietfdraft03 vs ietfdraft13) |
| `crates/crypto/src/sum_kes.rs` | 1,018 | per-KES-tier or signature/key/sig-derivation split |

R273h candidate: `plutus/cost_model.rs` since it logically
mirrors upstream `PlutusCore.Evaluation.Machine.ExBudgeting` +
`PlutusCore.Evaluation.Machine.MachineParameters` cost-class break
points.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R273
- R273f closure: `2026-05-08-round-273f-diffusion-pipelining-split.md`
- Upstream UPLC term:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Core/Type.hs`
- Upstream DefaultFun:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/plutus-core/src/PlutusCore/Default/Builtins.hs`
- Upstream CEK machine internal:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/Cek/Internal.hs`
