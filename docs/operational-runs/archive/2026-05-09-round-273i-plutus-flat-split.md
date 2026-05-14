## Round 273i — `plutus/flat.rs` split into `flat/{decoder,universe}.rs`

Date: 2026-05-09
Branch: main
Type: Filename-mirror refactor (Phase γ R273 ninth slice — plutus crate)

### Slice scope

Split `crates/plutus/src/flat.rs` (1,245 lines) into a 699-line parent
`flat.rs` shell + two new sub-modules:

- `crates/plutus/src/flat/decoder.rs` (476 lines): bit-level Flat
  reader + Term/Program/Constant decoder.
- `crates/plutus/src/flat/universe.rs` (112 lines): UPLC
  universe-tag parsing.

The residual `flat.rs` keeps the module-level docstring + bit-format
notes, the `MAX_TERM_DECODE_DEPTH` / `MAX_TYPE_DECODE_DEPTH` recursion
bounds, the public entry points (`decode_flat_program`,
`decode_script_bytes`, `decode_script_bytes_allowing_remainder`,
`decode_script_bytes_with_remainder_policy`), the closedness
validators (`validate_program_closed`, `validate_term_closed`), and
the `#[cfg(test)] mod tests` block (~553 lines of tests).

### Content distribution

**`flat/decoder.rs`** — mirrors upstream
`UntypedPlutusCore.Core.Instance.Flat` / `PlutusCore.Flat` bit-level
decoder:

- `pub(super) enum Frame` — work-stack frame for the iterative
  `decode_term` loop (ReadTerm, Wrap1, WrapApply, ReadListContinuation).
- `pub(super) enum Wrap1Op` (Delay, LamAbs, Force) — single-child
  wrappers handled by `Frame::Wrap1`.
- `pub(super) enum ListBuild` (Constr, Case) — parents that read a
  Flat list of child terms.
- `pub(super) struct FlatDecoder<'a>` — bit-level reader with `pos`
  (byte) + `bit` (0=MSB, 7=LSB) cursor.
- `impl FlatDecoder` — primary impl block (~340 lines): bit/byte
  reading helpers, natural / integer / filler / bytestring / string /
  list readers, plus `decode_program` and the iterative `decode_term`
  state-machine driver.
- `impl FlatDecoder` — additional impl block (~78 lines): constant /
  type / value decoders that compose with `TypeTagParser`.

**`flat/universe.rs`** — mirrors upstream `PlutusCore.Default.Universe`
/ `Data.Either` Flat universe-tag encoding:

- `pub(super) enum DecodedUni` — internal lookahead variant
  (`Star(Type)` plus the synthetic `ProtoList` / `ProtoPair` /
  `PartialPair` tags that take type arguments).
- `pub(super) struct TypeTagParser<'a>` — recursive-descent parser
  that consumes a universe-tag list and produces a `Type`.
- `impl TypeTagParser` — `new`, `parse`, `parse_uni`, `apply_uni`
  (the `(fun, arg)` combinator that wires `ProtoList Star` →
  `List(arg_ty)` and `ProtoPair Star Star` → `Pair(left, right)`).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/plutus/src/flat.rs` (shell) | `UntypedPlutusCore.Core.Instance.Flat` (top-level entry points) |
| `crates/plutus/src/flat/decoder.rs` | `PlutusCore.Flat` bit-level decoder + `UntypedPlutusCore.Core.Instance.Flat` Term decoder |
| `crates/plutus/src/flat/universe.rs` | `PlutusCore.Default.Universe` (Flat universe-tag instance) |

### Cross-module dependencies

- `MAX_TYPE_DECODE_DEPTH` const promoted to `pub(super)` so
  `universe.rs` can reference the recursion bound. Imported via
  `use super::MAX_TERM_DECODE_DEPTH;` in `universe.rs` (only the term
  bound is actually referenced today; the type bound stays in the
  parent for the constant-type sub-decoders).
- `decoder.rs` reaches `universe.rs` via
  `use super::universe::{DecodedUni, TypeTagParser};`.
- All `FlatDecoder` methods that are referenced from parent
  `flat.rs` (the `new`, `decode_program`, `is_empty`, etc.) are
  promoted to `pub(super)`. Because both clusters call cross-module,
  every `fn`/`async fn` in `decoder.rs` and `universe.rs` is now
  `pub(super)` — promotion count exceeds the R271i threshold but
  splitting the decoder further would fragment a single coherent
  state machine, so the broad promotion is intentional here.
- 4-item public surface preserved unchanged via the `flat.rs` shell:
  `decode_flat_program`, `decode_script_bytes`,
  `decode_script_bytes_allowing_remainder`, `MAX_TERM_DECODE_DEPTH`.

### Visibility / dependency fixups

1. **Test imports** — `flat.rs::tests::use super::*;` previously
   transitively pulled `BigInt`, `Constant`, `DefaultFun`, `Type`,
   `PlutusData` via the file-level `use` blocks. After extraction
   the tests now import `Constant`, `DefaultFun`, `Type` from
   `crate::types` and `BigInt` from `num_bigint` explicitly.
2. **`Decoder` type-name disambiguation** — both `flat.rs` and
   `decoder.rs` reference `yggdrasil_ledger::cbor::Decoder`. Kept
   the import in `flat.rs` (used by `decode_script_bytes_with_remainder_policy`)
   and dropped from `decoder.rs` since the bit-level decoder uses
   raw byte arrays, not the CBOR `Decoder`.
3. **`PlutusData::decode_cbor` trait** — needs `CborDecode` trait in
   scope; imported in `decoder.rs` for the constant-data decoder.
4. **State-machine enum visibility** — `Frame`, `Wrap1Op`, `ListBuild`
   appear in `pub(super)` method signatures and were flagged by
   clippy `private_interfaces` lint. Promoted to `pub(super)` so
   the decoder's internal state machine remains type-safe across
   the module boundary.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/plutus/src/flat.rs` | 1,245 | 699 | −546 |
| `crates/plutus/src/flat/decoder.rs` | (new) | 476 | +476 |
| `crates/plutus/src/flat/universe.rs` | (new) | 112 | +112 |

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
| R273h (plutus/cost_model) | `cost_model/{step,expr,memory}.rs` | 1,718 → 1,306 (−412) |
| **R273i (plutus/flat)** | **`flat/{decoder,universe}.rs`** | **1,245 → 699 (−546)** |

Total moved: ~4,582 lines across 20 sub-modules.

### Stop point — R273j candidates

Remaining ≥1,000-line files (highest impact):

| File | Lines | Likely split |
|---|---|---|
| `crates/plutus/src/builtins.rs` | 1,483 | per-builtin-class (integer / bytestring / string / data / pair / bls / ecdsa) |
| `crates/plutus/src/machine.rs` | 1,460 | CEK loop core vs decoder vs context |
| `crates/crypto/src/vrf.rs` | 1,254 | per-VRF-mode (ietfdraft03 vs ietfdraft13) |
| `crates/crypto/src/sum_kes.rs` | 1,018 | per-KES-tier or signature/key/sig-derivation split |

R273j candidate: `plutus/builtins.rs` (1,483 lines) — natural per-class
split mirrors upstream `PlutusCore.Default.Builtins` integer / ByteString
/ string / pair / list / data / BLS12-381 / ECDSA / BIP-340 sections.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R273
- R273h closure: `2026-05-08-round-273h-plutus-cost-model-split.md`
- Upstream UPLC Flat instance:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Core/Instance/Flat.hs`
- Upstream PlutusCore Flat:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/flat/src/PlutusCore/Flat.hs`
- Upstream universe encoding:
  `.reference-haskell-cardano-node/deps/plutus/plutus-core/plutus-core/src/PlutusCore/Default/Universe.hs`
