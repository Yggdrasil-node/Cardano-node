//! Memory-cost helpers: `ex_memory` family for argument-size measurement.
//!
//! Mirrors upstream Plutus `ExMemoryUsage` type class — every UPLC value
//! has an "ex memory" measure expressed in 64-bit words that the cost
//! model uses as the size argument to per-builtin costing functions.
//!
//! Public:
//!
//! - `ex_memory` — top-level dispatcher over `Value`.
//! - `integer_ex_memory` / `bytestring_ex_memory` — primitive measures.
//!
//! `pub(super)`:
//!
//! - `constant_ex_memory` / `data_ex_memory` / `integer_ex_memory_bigint`
//!   — internal helpers used by both `ex_memory` and the per-builtin cost
//!   computation in the parent module.
//!
//! Extracted from `cost_model.rs` in R273h (Phase γ §R273 eighth slice).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `PlutusCore.Evaluation.Machine.ExMemoryUsage.hs`
//! (upstream's `ExMemoryUsage` type class + helpers). File renamed
//! `memory.rs` → `ex_memory_usage.rs` in R273-rename to match
//! upstream filename.

use num_bigint::BigInt;

use crate::types::Value;

pub fn ex_memory(value: &Value) -> i64 {
    match value {
        Value::Constant(c) => constant_ex_memory(c),
        _ => 1,
    }
}

pub(super) fn constant_ex_memory(c: &crate::types::Constant) -> i64 {
    use crate::types::Constant::*;
    match c {
        Integer(n) => integer_ex_memory_bigint(n),
        ByteString(bs) => bytestring_ex_memory(bs.len()),
        String(s) => s.chars().count() as i64,
        Unit => 1,
        Bool(_) => 1,
        ProtoList(_, elems) => elems.len() as i64,
        ProtoPair(_, _, _, _) => i64::MAX,
        Data(d) => data_ex_memory(d),
        Bls12_381_G1_Element(_) => 18,
        Bls12_381_G2_Element(_) => 36,
        Bls12_381_MlResult(_) => 72,
    }
}

/// Compute the ExMemory size of an integer value.
///
/// size = ceil(bit_length(|n|) / 64), minimum 1.
/// Matches Haskell `nWords` in `ExMemoryUsage Integer`.
pub fn integer_ex_memory<N: Into<BigInt>>(n: N) -> i64 {
    let n = n.into();
    integer_ex_memory_bigint(&n)
}

pub(super) fn integer_ex_memory_bigint(n: &BigInt) -> i64 {
    if n == &BigInt::from(0u8) {
        return 1;
    }
    let words = n.bits().saturating_add(63) / 64;
    i64::try_from(words).unwrap_or(i64::MAX)
}

/// Compute the ExMemory size of a byte string.
///
/// Upstream uses `((n - 1) quot 8) + 1`, which yields 1 for the empty
/// bytestring and for lengths 1..=8.
pub fn bytestring_ex_memory(len: usize) -> i64 {
    ((len.saturating_sub(1) / 8) + 1) as i64
}

/// Compute the ExMemory size of a `PlutusData` value.
///
/// Matches upstream `dataSize` with a base cost of 4 per node.
pub(super) fn data_ex_memory(d: &yggdrasil_ledger::plutus::PlutusData) -> i64 {
    use yggdrasil_ledger::plutus::PlutusData::*;
    match d {
        Constr(_, fields) => 4 + fields.iter().map(data_ex_memory).sum::<i64>(),
        Map(pairs) => {
            4 + pairs
                .iter()
                .map(|(k, v)| data_ex_memory(k) + data_ex_memory(v))
                .sum::<i64>()
        }
        List(items) => 4 + items.iter().map(data_ex_memory).sum::<i64>(),
        Integer(n) => 4 + integer_ex_memory_bigint(n),
        Bytes(bs) => 4 + bytestring_ex_memory(bs.len()),
    }
}
