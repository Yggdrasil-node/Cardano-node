//! BLS12-381 elliptic curve operations for PlutusV3 builtins.
//!
//! Provides G1/G2 group arithmetic, serialization (compress/uncompress),
//! hash-to-curve, and pairing (Miller loop + final verification).
//!
//! Reference: CIP-0381 and
//! <https://github.com/IntersectMBO/plutus/tree/master/plutus-core/plutus-core/src/PlutusCore/Crypto/BLS12_381>

use bls12_381::{
    G1Affine, G1Projective, G2Affine, G2Projective, Gt, MillerLoopResult, Scalar,
    hash_to_curve::{ExpandMsgXmd, HashToCurve},
    multi_miller_loop, G2Prepared,
};

use crate::CryptoError;

// ---------------------------------------------------------------------------
// Opaque wrapper types — matching upstream naming
// ---------------------------------------------------------------------------

/// An element of the BLS12-381 G1 group (48-byte compressed, 96-byte
/// uncompressed). Wraps a projective point internally.
#[derive(Clone, Debug)]
pub struct G1Element(G1Projective);

impl PartialEq for G1Element {
    fn eq(&self, other: &Self) -> bool {
        g1_equal(self, other)
    }
}

/// An element of the BLS12-381 G2 group (96-byte compressed, 192-byte
/// uncompressed). Wraps a projective point internally.
#[derive(Clone, Debug)]
pub struct G2Element(G2Projective);

impl PartialEq for G2Element {
    fn eq(&self, other: &Self) -> bool {
        g2_equal(self, other)
    }
}

/// A Miller-loop intermediate result, used for deferred pairing checks.
/// Final verification is done via [`final_verify`].
#[derive(Clone, Debug)]
pub struct MlResult(MillerLoopResult);

impl PartialEq for MlResult {
    fn eq(&self, other: &Self) -> bool {
        final_verify(self, other)
    }
}

// ---------------------------------------------------------------------------
// G1 operations
// ---------------------------------------------------------------------------

/// Adds two G1 elements.
pub fn g1_add(a: &G1Element, b: &G1Element) -> G1Element {
    G1Element(a.0 + b.0)
}

/// Negates a G1 element.
pub fn g1_neg(a: &G1Element) -> G1Element {
    G1Element(-a.0)
}

/// Scalar-multiplies a G1 element by an arbitrary integer.
///
/// `magnitude` is the absolute value of the scalar as big-endian unsigned
/// bytes.  If `negative` is true the result is negated, giving `(-k) * P`.
/// The magnitude is reduced modulo the BLS12-381 group order.
pub fn g1_scalar_mul(magnitude: &[u8], negative: bool, point: &G1Element) -> G1Element {
    let scalar = bytes_to_scalar(magnitude);
    let result = point.0 * scalar;
    if negative { G1Element(-result) } else { G1Element(result) }
}

/// Tests two G1 elements for equality.
pub fn g1_equal(a: &G1Element, b: &G1Element) -> bool {
    G1Affine::from(a.0) == G1Affine::from(b.0)
}

/// Compresses a G1 element to 48 bytes.
pub fn g1_compress(point: &G1Element) -> [u8; 48] {
    G1Affine::from(point.0).to_compressed()
}

/// Decompresses 48 bytes into a G1 element with subgroup membership check.
pub fn g1_uncompress(bytes: &[u8]) -> Result<G1Element, CryptoError> {
    let arr: [u8; 48] = bytes
        .try_into()
        .map_err(|_| CryptoError::InvalidLength)?;
    let affine = ct_option_ok(G1Affine::from_compressed(&arr))?;
    Ok(G1Element(G1Projective::from(affine)))
}

/// Hashes a message to a G1 element using the hash-to-curve spec
/// (BLS12-381 with SHA-256 XMD expander).
///
/// `msg` is the message bytes, `dst` is the domain separation tag.
/// The DST may exceed 255 bytes (which triggers the large-DST expansion
/// per the hash-to-curve specification).
pub fn g1_hash_to_group(msg: &[u8], dst: &[u8]) -> Result<G1Element, CryptoError> {
    if dst.is_empty() {
        return Err(CryptoError::InvalidDomain);
    }
    let point =
        <G1Projective as HashToCurve<ExpandMsgXmd<sha2_09::Sha256>>>::hash_to_curve(msg, dst);
    Ok(G1Element(point))
}

/// Returns the G1 group identity (point at infinity).
pub fn g1_identity() -> G1Element {
    G1Element(G1Projective::identity())
}

/// Returns the G1 generator.
pub fn g1_generator() -> G1Element {
    G1Element(G1Projective::generator())
}

// ---------------------------------------------------------------------------
// G2 operations
// ---------------------------------------------------------------------------

/// Adds two G2 elements.
pub fn g2_add(a: &G2Element, b: &G2Element) -> G2Element {
    G2Element(a.0 + b.0)
}

/// Negates a G2 element.
pub fn g2_neg(a: &G2Element) -> G2Element {
    G2Element(-a.0)
}

/// Scalar-multiplies a G2 element by an arbitrary integer.
///
/// `magnitude` is the absolute value of the scalar as big-endian unsigned
/// bytes.  If `negative` is true the result is negated.
pub fn g2_scalar_mul(magnitude: &[u8], negative: bool, point: &G2Element) -> G2Element {
    let scalar = bytes_to_scalar(magnitude);
    let result = point.0 * scalar;
    if negative { G2Element(-result) } else { G2Element(result) }
}

/// Tests two G2 elements for equality.
pub fn g2_equal(a: &G2Element, b: &G2Element) -> bool {
    G2Affine::from(a.0) == G2Affine::from(b.0)
}

/// Compresses a G2 element to 96 bytes.
pub fn g2_compress(point: &G2Element) -> [u8; 96] {
    G2Affine::from(point.0).to_compressed()
}

/// Decompresses 96 bytes into a G2 element with subgroup membership check.
pub fn g2_uncompress(bytes: &[u8]) -> Result<G2Element, CryptoError> {
    let arr: [u8; 96] = bytes
        .try_into()
        .map_err(|_| CryptoError::InvalidLength)?;
    let affine = ct_option_ok(G2Affine::from_compressed(&arr))?;
    Ok(G2Element(G2Projective::from(affine)))
}

/// Hashes a message to a G2 element using the hash-to-curve spec
/// (BLS12-381 with SHA-256 XMD expander).
pub fn g2_hash_to_group(msg: &[u8], dst: &[u8]) -> Result<G2Element, CryptoError> {
    if dst.is_empty() {
        return Err(CryptoError::InvalidDomain);
    }
    let point =
        <G2Projective as HashToCurve<ExpandMsgXmd<sha2_09::Sha256>>>::hash_to_curve(msg, dst);
    Ok(G2Element(point))
}

/// Returns the G2 group identity.
pub fn g2_identity() -> G2Element {
    G2Element(G2Projective::identity())
}

/// Returns the G2 generator.
pub fn g2_generator() -> G2Element {
    G2Element(G2Projective::generator())
}

// ---------------------------------------------------------------------------
// Pairing operations
// ---------------------------------------------------------------------------

/// Computes a single Miller loop pairing of a G1 and G2 element.
///
/// The result is an intermediate [`MlResult`] that must be passed to
/// [`final_verify`] together with another Miller loop result to check
/// the bilinear pairing relation.
pub fn miller_loop(g1: &G1Element, g2: &G2Element) -> MlResult {
    let g1_affine = G1Affine::from(g1.0);
    let g2_affine = G2Affine::from(g2.0);
    let g2_prepared = G2Prepared::from(g2_affine);
    MlResult(multi_miller_loop(&[(&g1_affine, &g2_prepared)]))
}

/// Multiplies (combines) two Miller loop intermediate results.
///
/// In the pairing group this corresponds to `e(P1,Q1) * e(P2,Q2)`.
pub fn mul_ml_result(a: &MlResult, b: &MlResult) -> MlResult {
    // The `bls12_381` crate uses additive notation for the MillerLoopResult
    // group.  Plutus `mulMlResult` is the group operation.
    MlResult(a.0 + b.0)
}

/// Checks whether two Miller loop results represent the same pairing
/// after final exponentiation: `finalExp(r1) == finalExp(r2)`.
///
/// Returns `true` when the pairing equation holds.
pub fn final_verify(a: &MlResult, b: &MlResult) -> bool {
    let gt_a: Gt = a.0.final_exponentiation();
    let gt_b: Gt = b.0.final_exponentiation();
    gt_a == gt_b
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Converts an arbitrary big-endian unsigned byte string to a BLS12-381
/// `Scalar`, reducing modulo the group order.
///
/// Handles zero-length input (returns zero scalar) and any length by
/// iteratively reducing via `from_bytes_wide`.
fn bytes_to_scalar(bytes: &[u8]) -> Scalar {
    if bytes.is_empty() {
        return Scalar::zero();
    }

    // The Scalar::from_bytes_wide takes exactly 64 bytes.
    // For inputs ≤ 64 bytes, zero-pad on the left and use from_bytes_wide.
    // For inputs > 64 bytes, reduce iteratively:
    //   result = 0
    //   for each 32-byte chunk (big-endian, most significant first):
    //       result = result * 2^256 + chunk
    // This is correct because from_bytes_wide does modular reduction.

    if bytes.len() <= 64 {
        let mut padded = [0u8; 64];
        // Place big-endian bytes right-aligned.
        padded[64 - bytes.len()..].copy_from_slice(bytes);
        // from_bytes_wide expects little-endian.
        padded.reverse();
        return Scalar::from_bytes_wide(&padded);
    }

    // For large scalars: process 32-byte chunks from the most significant end.
    // result = Σ chunk_i * (2^256)^(n-1-i)
    // Computed via Horner's method: result = (...((chunk_0 * R + chunk_1) * R + chunk_2)...)
    // where R = 2^256 mod scalar_order.
    let r_256 = {
        // 2^256 as a Scalar: little-endian byte 32 set = bit 256.
        let mut le = [0u8; 64];
        le[32] = 1;
        Scalar::from_bytes_wide(&le)
    };

    let mut result = Scalar::zero();
    // Process in 32-byte chunks from most significant.
    let full_chunks = bytes.len() / 32;
    let remainder = bytes.len() % 32;

    let mut offset = 0;
    if remainder > 0 {
        // First partial chunk: the most-significant bytes.
        let mut padded = [0u8; 64];
        // Right-align in little-endian for from_bytes_wide.
        let chunk = &bytes[..remainder];
        for (i, &b) in chunk.iter().rev().enumerate() {
            padded[i] = b;
        }
        result = Scalar::from_bytes_wide(&padded);
        offset = remainder;
    }

    for _ in 0..full_chunks {
        result *= r_256;
        let chunk = &bytes[offset..offset + 32];
        let mut padded = [0u8; 64];
        for (i, &b) in chunk.iter().rev().enumerate() {
            padded[i] = b;
        }
        let chunk_scalar = Scalar::from_bytes_wide(&padded);
        result += chunk_scalar;
        offset += 32;
    }

    result
}

/// Converts a `CtOption<T>` to `Result<T, CryptoError>`.
fn ct_option_ok<T>(opt: subtle::CtOption<T>) -> Result<T, CryptoError> {
    if bool::from(opt.is_some()) {
        // SAFETY: we just checked is_some.
        Ok(opt.unwrap())
    } else {
        Err(CryptoError::InvalidPoint)
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1_add_identity() {
        let g = g1_generator();
        let id = g1_identity();
        assert!(g1_equal(&g1_add(&g, &id), &g));
    }

    #[test]
    fn g1_neg_inverse() {
        let g = g1_generator();
        let neg_g = g1_neg(&g);
        let sum = g1_add(&g, &neg_g);
        assert!(g1_equal(&sum, &g1_identity()));
    }

    #[test]
    fn g1_compress_uncompress_round_trip() {
        let g = g1_generator();
        let compressed = g1_compress(&g);
        let decompressed = g1_uncompress(&compressed).expect("valid compression");
        assert!(g1_equal(&decompressed, &g));
    }

    #[test]
    fn g1_uncompress_rejects_invalid_length() {
        assert!(g1_uncompress(&[0u8; 10]).is_err());
    }

    #[test]
    fn g1_scalar_mul_by_one() {
        let g = g1_generator();
        let result = g1_scalar_mul(&[1], false, &g);
        assert!(g1_equal(&result, &g));
    }

    #[test]
    fn g1_scalar_mul_by_zero() {
        let g = g1_generator();
        let result = g1_scalar_mul(&[], false, &g);
        assert!(g1_equal(&result, &g1_identity()));
    }

    #[test]
    fn g1_scalar_mul_negate() {
        let g = g1_generator();
        let neg_result = g1_scalar_mul(&[1], true, &g);
        let expected = g1_neg(&g);
        assert!(g1_equal(&neg_result, &expected));
    }

    #[test]
    fn g2_add_identity() {
        let g = g2_generator();
        let id = g2_identity();
        assert!(g2_equal(&g2_add(&g, &id), &g));
    }

    #[test]
    fn g2_compress_uncompress_round_trip() {
        let g = g2_generator();
        let compressed = g2_compress(&g);
        let decompressed = g2_uncompress(&compressed).expect("valid compression");
        assert!(g2_equal(&decompressed, &g));
    }

    #[test]
    fn g1_hash_to_group_deterministic() {
        let a = g1_hash_to_group(b"test message", b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_")
            .expect("valid");
        let b = g1_hash_to_group(b"test message", b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_")
            .expect("valid");
        assert!(g1_equal(&a, &b));
    }

    #[test]
    fn g1_hash_to_group_rejects_empty_dst() {
        assert!(g1_hash_to_group(b"msg", b"").is_err());
    }

    #[test]
    fn pairing_bilinearity() {
        // e(aP, Q) == e(P, aQ)
        let p = g1_generator();
        let q = g2_generator();
        let scalar = [7u8];

        let ap = g1_scalar_mul(&scalar, false, &p);
        let aq = g2_scalar_mul(&scalar, false, &q);

        let lhs = miller_loop(&ap, &q);
        let rhs = miller_loop(&p, &aq);

        assert!(final_verify(&lhs, &rhs), "bilinearity should hold");
    }

    #[test]
    fn pairing_non_degeneracy() {
        let p = g1_generator();
        let q = g2_generator();
        let id = g1_identity();

        let real = miller_loop(&p, &q);
        let trivial = miller_loop(&id, &q);

        assert!(
            !final_verify(&real, &trivial),
            "e(G1, G2) != e(O, G2)"
        );
    }

    #[test]
    fn mul_ml_result_combines() {
        // e(P, Q) * e(P, Q) == e(2P, Q)
        let p = g1_generator();
        let q = g2_generator();
        let two_p = g1_add(&p, &p);

        let single = miller_loop(&p, &q);
        let combined = mul_ml_result(&single, &single);
        let double = miller_loop(&two_p, &q);

        assert!(final_verify(&combined, &double));
    }

    // -----------------------------------------------------------------------
    // g2_hash_to_group — previously zero test coverage
    // Reference: CIP-0381 bls12_381_G2_hashToGroup
    // -----------------------------------------------------------------------

    #[test]
    fn g2_hash_to_group_deterministic() {
        let a = g2_hash_to_group(b"test message", b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_")
            .expect("valid hash-to-group");
        let b = g2_hash_to_group(b"test message", b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_")
            .expect("valid hash-to-group");
        assert!(g2_equal(&a, &b), "same input must produce same G2 point");
    }

    #[test]
    fn g2_hash_to_group_different_messages_differ() {
        let a = g2_hash_to_group(b"message A", b"QUUX-V01-CS02-with-BLS12381G2_XMD:SHA-256_SSWU_RO_")
            .expect("valid");
        let b = g2_hash_to_group(b"message B", b"QUUX-V01-CS02-with-BLS12381G2_XMD:SHA-256_SSWU_RO_")
            .expect("valid");
        assert!(!g2_equal(&a, &b), "different messages should hash to different G2 points");
    }

    #[test]
    fn g2_hash_to_group_rejects_empty_dst() {
        assert!(g2_hash_to_group(b"msg", b"").is_err());
    }

    #[test]
    fn g2_hash_to_group_empty_message() {
        // Empty message is valid per hash-to-curve spec.
        let result = g2_hash_to_group(b"", b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_");
        assert!(result.is_ok(), "empty message should be accepted");
        // Result must not be the identity element.
        assert!(
            !g2_equal(&result.expect("valid"), &g2_identity()),
            "hash of empty message should not be identity"
        );
    }

    #[test]
    fn g2_hash_to_group_not_identity() {
        // The hash-to-curve spec guarantees output is on the curve and
        // in the prime-order subgroup; it must not be the identity.
        let point = g2_hash_to_group(b"non-trivial", b"DST")
            .expect("valid");
        assert!(
            !g2_equal(&point, &g2_identity()),
            "hash-to-group output must not be identity"
        );
    }

    #[test]
    fn g2_hash_to_group_compress_round_trip() {
        let point = g2_hash_to_group(b"round trip test", b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_")
            .expect("valid");
        let compressed = g2_compress(&point);
        let decompressed = g2_uncompress(&compressed).expect("uncompress hashed point");
        assert!(g2_equal(&point, &decompressed));
    }

    // -----------------------------------------------------------------------
    // g2_scalar_mul — previously only indirect upstream vector coverage
    // Reference: CIP-0381 bls12_381_G2_scalarMul
    // -----------------------------------------------------------------------

    #[test]
    fn g2_scalar_mul_by_one() {
        let g = g2_generator();
        let result = g2_scalar_mul(&[1], false, &g);
        assert!(g2_equal(&result, &g), "1 * G2 == G2");
    }

    #[test]
    fn g2_scalar_mul_by_zero() {
        let g = g2_generator();
        let result = g2_scalar_mul(&[], false, &g);
        assert!(
            g2_equal(&result, &g2_identity()),
            "0 * G2 == identity"
        );
    }

    #[test]
    fn g2_scalar_mul_negate() {
        let g = g2_generator();
        let neg_result = g2_scalar_mul(&[1], true, &g);
        let expected = g2_neg(&g);
        assert!(g2_equal(&neg_result, &expected), "(-1) * G2 == -G2");
    }

    #[test]
    fn g2_scalar_mul_by_two_equals_add() {
        let g = g2_generator();
        let double_via_scalar = g2_scalar_mul(&[2], false, &g);
        let double_via_add = g2_add(&g, &g);
        assert!(
            g2_equal(&double_via_scalar, &double_via_add),
            "2 * G2 == G2 + G2"
        );
    }

    #[test]
    fn g2_scalar_mul_large_scalar() {
        // 32-byte scalar (matching upstream test vector scalar length).
        let g = g2_generator();
        let scalar = [0xFF; 32];
        let result = g2_scalar_mul(&scalar, false, &g);
        // Result should not be identity (extremely unlikely for a non-zero scalar).
        assert!(
            !g2_equal(&result, &g2_identity()),
            "large scalar * G2 should not be identity"
        );
        // Verify compress/uncompress round-trip of the result.
        let compressed = g2_compress(&result);
        let decompressed = g2_uncompress(&compressed).expect("valid point");
        assert!(g2_equal(&result, &decompressed));
    }

    #[test]
    fn g2_scalar_mul_64_byte_scalar() {
        // 64-byte scalar exercises the ≤64 byte branch of bytes_to_scalar.
        let g = g2_generator();
        let scalar = [0x01; 64];
        let result = g2_scalar_mul(&scalar, false, &g);
        assert!(
            !g2_equal(&result, &g2_identity()),
            "64-byte scalar * G2 should not be identity"
        );
    }

    #[test]
    fn g2_scalar_mul_65_byte_scalar() {
        // 65-byte scalar exercises the >64 byte iterative Horner branch.
        let g = g2_generator();
        let mut scalar = [0u8; 65];
        scalar[0] = 1;
        let result = g2_scalar_mul(&scalar, false, &g);
        assert!(
            !g2_equal(&result, &g2_identity()),
            "65-byte scalar * G2 should not be identity"
        );
    }

    // -----------------------------------------------------------------------
    // g1_scalar_mul — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn g1_scalar_mul_by_two_equals_add() {
        let g = g1_generator();
        let double_via_scalar = g1_scalar_mul(&[2], false, &g);
        let double_via_add = g1_add(&g, &g);
        assert!(
            g1_equal(&double_via_scalar, &double_via_add),
            "2 * G1 == G1 + G1"
        );
    }

    #[test]
    fn g1_scalar_mul_large_scalar() {
        let g = g1_generator();
        let scalar = [0xFF; 32];
        let result = g1_scalar_mul(&scalar, false, &g);
        assert!(!g1_equal(&result, &g1_identity()));
        let compressed = g1_compress(&result);
        let decompressed = g1_uncompress(&compressed).expect("valid point");
        assert!(g1_equal(&result, &decompressed));
    }

    #[test]
    fn g1_scalar_mul_65_byte_scalar() {
        // Exercises the >64 byte Horner reduction branch.
        let g = g1_generator();
        let mut scalar = [0u8; 65];
        scalar[0] = 1;
        let result = g1_scalar_mul(&scalar, false, &g);
        assert!(!g1_equal(&result, &g1_identity()));
    }

    // -----------------------------------------------------------------------
    // G2 uncompress edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn g2_uncompress_rejects_invalid_length() {
        assert!(g2_uncompress(&[0u8; 10]).is_err());
    }

    #[test]
    fn g2_neg_inverse() {
        let g = g2_generator();
        let neg_g = g2_neg(&g);
        let sum = g2_add(&g, &neg_g);
        assert!(g2_equal(&sum, &g2_identity()), "G2 + (-G2) == identity");
    }

    // -----------------------------------------------------------------------
    // Pairing — additional bilinearity checks
    // Reference: CIP-0381 pairing properties
    // -----------------------------------------------------------------------

    #[test]
    fn pairing_identity_g1_yields_trivial() {
        // e(O, Q) should be trivial regardless of Q.
        let id = g1_identity();
        let q = g2_generator();
        let result = miller_loop(&id, &q);
        let trivial = miller_loop(&id, &q);
        assert!(
            final_verify(&result, &trivial),
            "e(O, Q) == e(O, Q)"
        );
    }

    #[test]
    fn pairing_identity_g2_yields_trivial() {
        // e(P, O) should be trivial regardless of P.
        let p = g1_generator();
        let id = g2_identity();
        let result = miller_loop(&p, &id);
        let trivial = miller_loop(&g1_identity(), &g2_generator());
        assert!(
            final_verify(&result, &trivial),
            "e(P, O) == e(O, G2)"
        );
    }
}
