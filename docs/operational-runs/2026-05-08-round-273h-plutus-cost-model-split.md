## Round 273h — `plutus/cost_model.rs` split (extract step + expr + memory helpers)

Date: 2026-05-08
Branch: main
Type: Filename-mirror refactor (Phase γ R273 eighth slice — plutus crate)

### Slice scope

Split `crates/plutus/src/cost_model.rs` (1,718 lines) into a 1,306-line
parent `cost_model.rs` shell + three new sub-modules:

- `crates/plutus/src/cost_model/step.rs` (134 lines): `StepKind` +
  `StepCosts`.
- `crates/plutus/src/cost_model/expr.rs` (225 lines): `CostExpr`
  algebraic cost-expression evaluator.
- `crates/plutus/src/cost_model/memory.rs` (90 lines): `ex_memory`
  family + helpers (`integer_ex_memory`, `bytestring_ex_memory`,
  `constant_ex_memory`, `data_ex_memory`, `integer_ex_memory_bigint`).

The residual `cost_model.rs` keeps the module-level docstring,
`pub mod` declarations, `pub use` re-exports of the public surface,
the larger `CostModelError`, `BuiltinSemanticsVariant`,
`BuiltinCostEntry`, `CostModel` types + impls, the
`build_per_builtin_costs` (~920-line builder), the `named_value`
helper, and the `#[cfg(test)] mod tests` declaration.

### Content distribution

**`cost_model/step.rs`** — mirrors upstream
`UntypedPlutusCore.Evaluation.Machine.Cek.Internal::StepKind` +
`PlutusCore.Evaluation.Machine.MachineParameters::CostingPart`
per-step cost wiring:

- `pub enum StepKind` — CEK machine operation kinds (Constant, Var,
  LamAbs, Apply, Delay, Force, Builtin, Constr, Case).
- `impl StepKind` — discriminant access.
- `pub struct StepCosts` — per-step CPU + memory charges (one CPU/mem
  pair per kind).
- `impl Default for StepCosts` (default to all-zero for tests) +
  `impl StepCosts` (`step_cost(kind)` lookup).

**`cost_model/expr.rs`** — mirrors upstream
`PlutusCore.Cost.CostingFun` cost-function shapes:

- `pub enum CostExpr` (17 variants: `Constant`, `LinearInX/Y/Z`,
  `LinearForm`, `AddedSizes`, `MaxSize`, `MinSize`,
  `SubtractedSizes`, `MaxSizeYZ`, `ExpModCost`, `MultipliedSizes`,
  `LinearOnDiagonal`, `ConstAboveDiagonal`, `TwoVarQuadratic`,
  `QuadraticInY`, `QuadraticInZ`, `LiteralInYOrLinearInZ`).
- `impl CostExpr::evaluate` — evaluates the expression against the
  pre-computed per-argument sizes to produce the i64 cost.

**`cost_model/memory.rs`** — mirrors upstream Plutus
`ExMemoryUsage` type class:

- `pub fn ex_memory` — top-level dispatcher over `Value`.
- `pub fn integer_ex_memory<N: Into<BigInt>>` — primitive 64-bit-word
  integer measure.
- `pub fn bytestring_ex_memory` — primitive byte-length measure.
- `pub(super) fn constant_ex_memory` — internal `Constant`
  dispatcher.
- `pub(super) fn integer_ex_memory_bigint` — used by both
  `integer_ex_memory` (the public API) and `data_ex_memory`.
- `pub(super) fn data_ex_memory` — Plutus `Data` recursive measure
  (4 per node + leaf-specific costs).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/plutus/src/cost_model.rs` (shell) | `PlutusCore.Evaluation.Machine.MachineParameters` (top-level cost-model entry point) |
| `crates/plutus/src/cost_model/step.rs` | `UntypedPlutusCore.Evaluation.Machine.Cek.Internal::StepKind` + `MachineParameters::CostingPart` |
| `crates/plutus/src/cost_model/expr.rs` | `PlutusCore.Cost.CostingFun` (cost-function shapes) + `CostingFun.Core` |
| `crates/plutus/src/cost_model/memory.rs` | `PlutusCore.Evaluation.Machine.ExMemoryUsage` |

### Cross-module dependencies

- step.rs reaches `crate::types::ExBudget` directly.
- expr.rs is self-contained (only `i64` arithmetic).
- memory.rs reaches `super::ex_memory_internals` only via the
  `pub(super) fn` interface; otherwise self-contained.
- The 8-item public surface (`StepKind`, `StepCosts`, `CostExpr`,
  `ex_memory`, `integer_ex_memory`, `bytestring_ex_memory`, plus
  `BuiltinCostEntry`/`BuiltinSemanticsVariant`/`CostModel`/`CostModelError`
  that stayed in the shell) preserved via `pub use` re-exports —
  no `lib.rs` edits needed.

### Visibility / dependency fixups

1. **Orphan doc comment + `#[derive]` boundaries** — three orphans
   carried by the bulk extract were trimmed inline:
   - `BuiltinCostEntry` derive moved into the `cost_model.rs` shell
     re-attached above the `pub struct BuiltinCostEntry` (line 94).
   - `CostExpr` derive moved into the `expr.rs` header re-attached
     above the `pub enum CostExpr` (line 14).
   - `StepKind`-section header carried past the cut into `step.rs`
     trimmed.
2. **`CostModelError` derive carried separately** — the original
   `#[derive(Clone, Debug, Eq, PartialEq, Error)]` was on the line
   above `pub enum CostModelError`, outside the slice range.
   Re-attached inline above the enum in the residual file.
3. **`pub(super)` promotions** — `constant_ex_memory`,
   `data_ex_memory`, `integer_ex_memory_bigint` promoted to
   `pub(super)` so the `cost_model/tests.rs` module can reach them
   directly (also imported there via
   `use crate::cost_model::memory::data_ex_memory;` for the existing
   `data_ex_memory` test).
4. **`use num_bigint::BigInt;`** added to `cost_model/tests.rs`
   since the file-level imports moved into sub-modules.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/plutus/src/cost_model.rs` | 1,718 | 1,306 | −412 |
| `crates/plutus/src/cost_model/step.rs` | (new) | 134 | +134 |
| `crates/plutus/src/cost_model/expr.rs` | (new) | 225 | +225 |
| `crates/plutus/src/cost_model/memory.rs` | (new) | 90 | +90 |

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
| R273g (plutus/types) | `types/{term,default_fun,runtime}.rs` | 1,707 → 944 (−763) |
| **R273h (plutus/cost_model)** | **`cost_model/{step,expr,memory}.rs`** | **1,718 → 1,306 (−412)** |

Total moved: ~4,036 lines across 18 sub-modules.

### Stop point — R273i candidates

cost_model.rs is still 1,306 lines because the giant
`build_per_builtin_costs` builder (~920 lines) stays in the shell —
extracting it would need promoting `BuiltinCostEntry`'s constructor
helpers + `BuiltinSemanticsVariant` to `pub(super)` and would
require ~30 cross-module super:: imports. That one is best left
intact unless a parity-driven reason emerges.

R273i candidates per the plan:

| File | Lines | Likely split |
|---|---|---|
| `crates/plutus/src/builtins.rs` | 1,483 | per-builtin-class (integer / bytestring / string / data / pair / bls / ecdsa) |
| `crates/plutus/src/machine.rs` | 1,460 | CEK loop core vs decoder vs context |
| `crates/plutus/src/flat.rs` | 1,245 | Flat encoder vs decoder |
| `crates/crypto/src/vrf.rs` | 1,254 | per-VRF-mode (ietfdraft03 vs ietfdraft13) |

R273i candidate: `crates/plutus/src/flat.rs` (1,245 lines) since
flat encoding/decoding is a clean encoder/decoder split.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R273
- R273g closure: `2026-05-08-round-273g-plutus-types-split.md`
- Upstream cost-model:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/cost-model/`
- Upstream `ExMemoryUsage`:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/plutus-core/src/PlutusCore/Evaluation/Machine/ExMemoryUsage.hs`
- Upstream `CostingFun.Core`:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/plutus-core/src/PlutusCore/Evaluation/Machine/CostingFun/Core.hs`
- Upstream `MachineParameters`:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/plutus-core/src/PlutusCore/Evaluation/Machine/MachineParameters.hs`
