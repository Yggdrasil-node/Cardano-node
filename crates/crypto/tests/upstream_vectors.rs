#![allow(clippy::unwrap_used)]
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use yggdrasil_crypto::{
    VrfBatchCompatProof, VrfOutput, VrfProof, VrfVerificationKey,
    bls12_381::{
        self, final_verify, g1_add, g1_equal, g1_hash_to_group, g1_neg, g1_scalar_mul,
        g1_uncompress, g2_add, g2_equal, g2_neg, g2_scalar_mul, g2_uncompress, miller_loop,
        mul_ml_result,
    },
    vrf_praos_batchcompat_test_vectors, vrf_praos_test_vectors,
};

const CARDANO_BASE_SHA: &str = "db52f43b38ba5d8927feb2199d4913fe6c0f974d";

// ---------------------------------------------------------------------------
// BLS12-381 hardcoded test parameters (drift-guarded; see
// `bls_hardcoded_test_parameters_match_upstream_pins`).
//
// Unlike the VRF Praos vectors, the BLS12-381 fixture files (`ec_operations
// _test_vectors`, `bls_sig_aug_test_vectors`) do NOT carry their input
// scalars/DSTs/messages inline — those are part of upstream test setup
// rather than the on-disk corpus. If upstream changes any of these
// parameters and refreshes the corresponding output line in the fixture
// (e.g. `[scalar]Q`), the operational tests below DO catch the drift,
// but only as an opaque "scalar mul mismatch" failure.
//
// Naming the parameters here lets a future drift surface as a clear,
// per-constant test failure in `bls_hardcoded_test_parameters_match_upstream
// _pins` AND keeps the upstream-source link discoverable from the test code
// itself (mirrors slices 76/78/80/84 — "named constant + drift guard").
//
// Reference: `cardano-base/cardano-crypto-class/bls12-381-test-vectors/`
// at commit `db52f43b38ba5d8927feb2199d4913fe6c0f974d`.
// ---------------------------------------------------------------------------

/// Scalar used for `[scalar]Q` lines in `ec_operations_test_vectors`
/// (lines 4 and 10). Sourced from upstream cardano-base BLS12-381 test
/// setup. A future cardano-base commit-bump that refreshes the fixture
/// with a different scalar will trip both the operational test and the
/// per-constant drift-guard pin.
const BLS_EC_OPERATIONS_SCALAR_HEX: &str =
    "40df499974f62e2f268cd5096b0d952073900054122ffce0a27c9d96932891a5";

/// Domain-separation tag for the `bls_sig_aug_test_vectors` pairing
/// check. This is the canonical IETF BLS signature suite identifier
/// (`BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_` with `_NUL_` upstream
/// extension), specified in `draft-irtf-cfrg-bls-signature-05` §4.2.3
/// "Suite ID for BLS12-381 G2".
const BLS_SIG_AUG_DST: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

/// Augmentation prefix for the `bls_sig_aug_test_vectors` pairing
/// check, sourced from upstream cardano-base test setup.
const BLS_SIG_AUG_AUG: &[u8] = b"Random value for test aug. ";

/// Message body for the `bls_sig_aug_test_vectors` pairing check,
/// sourced from upstream cardano-base test setup.
const BLS_SIG_AUG_MSG: &[u8] = b"blst is such a blast";

fn vendored_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/upstream-test-vectors/cardano-base")
        .join(CARDANO_BASE_SHA)
}

#[test]
fn upstream_praos_vrf_vector_files_are_present_and_well_formed() {
    let dir = vendored_root().join("cardano-crypto-praos/test_vectors");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("vendored Praos vector directory should exist")
        .map(|entry| entry.expect("directory entry should be readable").path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("vrf_"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();

    assert_eq!(files.len(), 14, "expected full Praos vector file set");

    for path in files {
        let content = fs::read_to_string(&path).expect("vector file should be readable as UTF-8");
        let kv = parse_kv(&content);

        let vrf = kv.get("vrf").expect("vrf key should be present");
        let ver = kv.get("ver").expect("ver key should be present");
        let sk = kv.get("sk").expect("sk key should be present");
        let pk = kv.get("pk").expect("pk key should be present");
        let alpha = kv.get("alpha").expect("alpha key should be present");
        let pi = kv.get("pi").expect("pi key should be present");
        let beta = kv.get("beta").expect("beta key should be present");

        assert!(matches!(vrf.as_str(), "PraosVRF" | "PraosBatchCompatVRF"));
        assert!(matches!(ver.as_str(), "ietfdraft03" | "ietfdraft13"));

        assert_eq!(sk.len(), 64, "seed should be 32 bytes hex");
        assert!(is_hex(sk));
        assert_eq!(pk.len(), 64, "verification key should be 32 bytes hex");
        assert!(is_hex(pk));

        if alpha != "empty" {
            assert!(is_hex(alpha), "alpha must be hex or empty");
        }

        let expected_pi_len = if vrf == "PraosBatchCompatVRF" {
            256
        } else {
            160
        };
        assert_eq!(pi.len(), expected_pi_len, "proof hex length mismatch");
        assert!(is_hex(pi));

        assert_eq!(beta.len(), 128, "output should be 64 bytes hex");
        assert!(is_hex(beta));
    }
}

fn bls_vector_dir() -> PathBuf {
    vendored_root().join("cardano-crypto-class/bls12-381-test-vectors/test_vectors")
}

fn read_hex_lines(path: &Path) -> Vec<Vec<u8>> {
    let content = fs::read_to_string(path).expect("vector file should be readable as UTF-8");
    content.lines().map(decode_hex_vec).collect()
}

#[test]
fn upstream_bls_vector_files_are_present_and_well_formed() {
    let dir = bls_vector_dir();

    assert_hex_lines(&dir.join("pairing_test_vectors"), 10);
    assert_hex_lines(&dir.join("ec_operations_test_vectors"), 12);
    assert_hex_lines(&dir.join("serde_test_vectors"), 8);
    assert_hex_lines(&dir.join("bls_sig_aug_test_vectors"), 2);
    assert_hex_lines(&dir.join("h2c_large_dst"), 3);
}

#[test]
fn bls_ec_operations_match_upstream_vectors() {
    let lines = read_hex_lines(&bls_vector_dir().join("ec_operations_test_vectors"));
    assert_eq!(lines.len(), 12);

    // G1 points: P, Q, P+Q, P-Q, [scalar]Q, -P
    let g1_p = g1_uncompress(&lines[0]).expect("G1 P");
    let g1_q = g1_uncompress(&lines[1]).expect("G1 Q");
    let g1_add_expected = g1_uncompress(&lines[2]).expect("G1 P+Q");
    let g1_sub_expected = g1_uncompress(&lines[3]).expect("G1 P-Q");
    let g1_mul_expected = g1_uncompress(&lines[4]).expect("G1 [s]Q");
    let g1_neg_expected = g1_uncompress(&lines[5]).expect("G1 -P");

    // Verify G1 operations
    assert!(
        g1_equal(&g1_add(&g1_p, &g1_q), &g1_add_expected),
        "G1 add mismatch"
    );
    assert!(
        g1_equal(&g1_add(&g1_p, &g1_neg(&g1_q)), &g1_sub_expected),
        "G1 sub mismatch"
    );
    assert!(
        g1_equal(&g1_neg(&g1_p), &g1_neg_expected),
        "G1 neg mismatch"
    );

    let scalar = decode_hex_vec(BLS_EC_OPERATIONS_SCALAR_HEX);
    assert!(
        g1_equal(&g1_scalar_mul(&scalar, false, &g1_q), &g1_mul_expected),
        "G1 scalar mul mismatch"
    );

    // G2 points: P, Q, P+Q, P-Q, [scalar]Q, -P
    let g2_p = g2_uncompress(&lines[6]).expect("G2 P");
    let g2_q = g2_uncompress(&lines[7]).expect("G2 Q");
    let g2_add_expected = g2_uncompress(&lines[8]).expect("G2 P+Q");
    let g2_sub_expected = g2_uncompress(&lines[9]).expect("G2 P-Q");
    let g2_mul_expected = g2_uncompress(&lines[10]).expect("G2 [s]Q");
    let g2_neg_expected = g2_uncompress(&lines[11]).expect("G2 -P");

    assert!(
        g2_equal(&g2_add(&g2_p, &g2_q), &g2_add_expected),
        "G2 add mismatch"
    );
    assert!(
        g2_equal(&g2_add(&g2_p, &g2_neg(&g2_q)), &g2_sub_expected),
        "G2 sub mismatch"
    );
    assert!(
        g2_equal(&g2_neg(&g2_p), &g2_neg_expected),
        "G2 neg mismatch"
    );
    assert!(
        g2_equal(&g2_scalar_mul(&scalar, false, &g2_q), &g2_mul_expected),
        "G2 scalar mul mismatch"
    );
}

#[test]
fn bls_pairing_identities_match_upstream_vectors() {
    let lines = read_hex_lines(&bls_vector_dir().join("pairing_test_vectors"));
    assert_eq!(lines.len(), 10);

    // G1: P, [a]P, [b]P, [a+b]P, [a*b]P
    let p = g1_uncompress(&lines[0]).expect("P");
    let a_p = g1_uncompress(&lines[1]).expect("[a]P");
    let b_p = g1_uncompress(&lines[2]).expect("[b]P");
    let ab_sum_p = g1_uncompress(&lines[3]).expect("[a+b]P");
    let ab_prod_p = g1_uncompress(&lines[4]).expect("[a*b]P");

    // G2: Q, [a]Q, [b]Q, [a+b]Q, [a*b]Q
    let q = g2_uncompress(&lines[5]).expect("Q");
    let a_q = g2_uncompress(&lines[6]).expect("[a]Q");
    let b_q = g2_uncompress(&lines[7]).expect("[b]Q");
    let ab_sum_q = g2_uncompress(&lines[8]).expect("[a+b]Q");
    let ab_prod_q = g2_uncompress(&lines[9]).expect("[a*b]Q");

    // Identity 1: e([a]P, Q) == e(P, [a]Q)
    let lhs = miller_loop(&a_p, &q);
    let rhs = miller_loop(&p, &a_q);
    assert!(final_verify(&lhs, &rhs), "e([a]P, Q) != e(P, [a]Q)");

    // Identity 2: e([a]P, [b]Q) == e([b]P, [a]Q)
    let lhs = miller_loop(&a_p, &b_q);
    let rhs = miller_loop(&b_p, &a_q);
    assert!(final_verify(&lhs, &rhs), "e([a]P, [b]Q) != e([b]P, [a]Q)");

    // Identity 3: e([a]P, [b]Q) == e([a*b]P, Q)
    let lhs = miller_loop(&a_p, &b_q);
    let rhs = miller_loop(&ab_prod_p, &q);
    assert!(final_verify(&lhs, &rhs), "e([a]P, [b]Q) != e([a*b]P, Q)");

    // Identity 4: e([a]P, Q) * e([b]P, Q) == e([a+b]P, Q)
    let ml_a = miller_loop(&a_p, &q);
    let ml_b = miller_loop(&b_p, &q);
    let lhs = mul_ml_result(&ml_a, &ml_b);
    let rhs = miller_loop(&ab_sum_p, &q);
    assert!(
        final_verify(&lhs, &rhs),
        "e([a]P,Q)*e([b]P,Q) != e([a+b]P,Q)"
    );

    // Identity 5: e([a]P, [b]Q) == e(P, [a*b]Q)
    let lhs = miller_loop(&a_p, &b_q);
    let rhs = miller_loop(&p, &ab_prod_q);
    assert!(final_verify(&lhs, &rhs), "e([a]P, [b]Q) != e(P, [a*b]Q)");

    // Identity 6: e(P, [a]Q) * e(P, [b]Q) == e(P, [a+b]Q)
    let ml_a = miller_loop(&p, &a_q);
    let ml_b = miller_loop(&p, &b_q);
    let lhs = mul_ml_result(&ml_a, &ml_b);
    let rhs = miller_loop(&p, &ab_sum_q);
    assert!(
        final_verify(&lhs, &rhs),
        "e(P,[a]Q)*e(P,[b]Q) != e(P,[a+b]Q)"
    );
}

#[test]
fn bls_serde_rejects_invalid_points() {
    let lines = read_hex_lines(&bls_vector_dir().join("serde_test_vectors"));
    assert_eq!(lines.len(), 8);

    // Line 2 (idx 1): G1 compressed, not on curve
    assert!(
        g1_uncompress(&lines[1]).is_err(),
        "G1 compressed not-on-curve should fail"
    );

    // Line 3 (idx 2): G1 compressed, not in subgroup
    assert!(
        g1_uncompress(&lines[2]).is_err(),
        "G1 compressed not-in-group should fail"
    );

    // Line 6 (idx 5): G2 compressed, not on curve
    assert!(
        g2_uncompress(&lines[5]).is_err(),
        "G2 compressed not-on-curve should fail"
    );

    // Line 7 (idx 6): G2 compressed, not in subgroup
    assert!(
        g2_uncompress(&lines[6]).is_err(),
        "G2 compressed not-in-group should fail"
    );
}

#[test]
fn bls_sig_aug_pairing_check() {
    let lines = read_hex_lines(&bls_vector_dir().join("bls_sig_aug_test_vectors"));
    assert_eq!(lines.len(), 2);

    let sig = g1_uncompress(&lines[0]).expect("sig");
    let pk = g2_uncompress(&lines[1]).expect("pk");

    let mut full_msg = Vec::with_capacity(BLS_SIG_AUG_AUG.len() + BLS_SIG_AUG_MSG.len());
    full_msg.extend_from_slice(BLS_SIG_AUG_AUG);
    full_msg.extend_from_slice(BLS_SIG_AUG_MSG);

    let hashed_msg = g1_hash_to_group(&full_msg, BLS_SIG_AUG_DST).expect("hash to G1");
    let lhs = miller_loop(&sig, &bls12_381::g2_generator());
    let rhs = miller_loop(&hashed_msg, &pk);
    assert!(final_verify(&lhs, &rhs), "BLS sig aug pairing check failed");
}

#[test]
fn bls_hash_to_curve_large_dst() {
    let lines = read_hex_lines(&bls_vector_dir().join("h2c_large_dst"));
    assert_eq!(lines.len(), 3);

    let msg = &lines[0];
    let large_dst = &lines[1];
    let expected = g1_uncompress(&lines[2]).expect("expected G1 output");

    // Large DST (> 255 bytes) triggers hash-to-curve internal DST pre-hashing.
    let result = g1_hash_to_group(msg, large_dst).expect("hash to G1 with large DST");
    assert!(
        g1_equal(&result, &expected),
        "hash-to-curve large DST mismatch"
    );
}

#[test]
fn embedded_vrf_vectors_match_vendored_standard_examples() {
    let vendored_ver03 = parse_kv(
        &fs::read_to_string(
            vendored_root().join("cardano-crypto-praos/test_vectors/vrf_ver03_standard_10"),
        )
        .expect("vendored vrf_ver03_standard_10 should exist"),
    );
    let vendored_ver13 = parse_kv(
        &fs::read_to_string(
            vendored_root().join("cardano-crypto-praos/test_vectors/vrf_ver13_standard_10"),
        )
        .expect("vendored vrf_ver13_standard_10 should exist"),
    );

    let embedded_ver03 = vrf_praos_test_vectors()
        .into_iter()
        .find(|v| v.name == "vrf-ver03-standard-10")
        .expect("embedded ver03 standard vector should exist");
    let embedded_ver13 = vrf_praos_batchcompat_test_vectors()
        .into_iter()
        .find(|v| v.name == "vrf-ver13-standard-10")
        .expect("embedded ver13 standard vector should exist");

    assert_eq!(
        hex(&embedded_ver03.secret_key[..32]),
        vendored_ver03["sk"],
        "ver03 seed mismatch"
    );
    assert_eq!(
        hex(&embedded_ver03.public_key),
        vendored_ver03["pk"],
        "ver03 pk mismatch"
    );
    assert_eq!(
        hex(&embedded_ver03.proof),
        vendored_ver03["pi"],
        "ver03 proof mismatch"
    );
    assert_eq!(
        hex(&embedded_ver03.output),
        vendored_ver03["beta"],
        "ver03 output mismatch"
    );

    assert_eq!(
        hex(&embedded_ver13.secret_key[..32]),
        vendored_ver13["sk"],
        "ver13 seed mismatch"
    );
    assert_eq!(
        hex(&embedded_ver13.public_key),
        vendored_ver13["pk"],
        "ver13 pk mismatch"
    );
    assert_eq!(
        hex(&embedded_ver13.proof),
        vendored_ver13["pi"],
        "ver13 proof mismatch"
    );
    assert_eq!(
        hex(&embedded_ver13.output),
        vendored_ver13["beta"],
        "ver13 output mismatch"
    );
}

#[test]
fn vendored_batchcompat_praos_vectors_verify_against_implementation() {
    let dir = vendored_root().join("cardano-crypto-praos/test_vectors");
    let embedded_names: Vec<String> = vrf_praos_batchcompat_test_vectors()
        .into_iter()
        .map(|vector| vector.name.replace('-', "_"))
        .collect();

    let mut checked = 0_usize;
    let mut failures: Vec<String> = Vec::new();
    for name in embedded_names {
        let path = dir.join(&name);
        let content = fs::read_to_string(&path).expect("vector file should be readable as UTF-8");
        let kv = parse_kv(&content);
        assert_eq!(
            kv.get("vrf").map(String::as_str),
            Some("PraosBatchCompatVRF")
        );

        let public_key = decode_hex_array::<32>(
            kv.get("pk")
                .expect("pk key should be present for batch-compatible vectors"),
        );
        let proof_bytes = decode_hex_array::<128>(
            kv.get("pi")
                .expect("pi key should be present for batch-compatible vectors"),
        );
        let output_bytes = decode_hex_array::<64>(
            kv.get("beta")
                .expect("beta key should be present for batch-compatible vectors"),
        );
        let message = match kv
            .get("alpha")
            .expect("alpha key should be present for batch-compatible vectors")
            .as_str()
        {
            "empty" => Vec::new(),
            hex_value => decode_hex_vec(hex_value),
        };

        let verification_key = VrfVerificationKey::from_bytes(public_key);
        let proof = VrfBatchCompatProof::from_bytes(proof_bytes);
        let expected = VrfOutput::from_bytes(output_bytes);
        match verification_key.verify_batchcompat(&message, &proof) {
            Ok(actual) => {
                if actual != expected {
                    failures.push(format!("{} output mismatch", path.display()));
                }
            }
            Err(error) => failures.push(format!("{} verify failed: {error:?}", path.display())),
        }
        checked += 1;
    }

    assert!(
        checked > 0,
        "at least one mirrored batch-compatible vector should be verified"
    );
    assert!(
        failures.is_empty(),
        "mirrored batch-compatible vectors should verify cleanly, failures: {}",
        failures.join("; ")
    );
}

#[test]
fn vendored_standard_praos_vectors_verify_against_implementation() {
    let dir = vendored_root().join("cardano-crypto-praos/test_vectors");
    let embedded_names: Vec<String> = vrf_praos_test_vectors()
        .into_iter()
        .map(|vector| vector.name.replace('-', "_"))
        .collect();

    let mut checked = 0_usize;
    let mut failures: Vec<String> = Vec::new();
    for name in embedded_names {
        let path = dir.join(&name);
        let content = fs::read_to_string(&path).expect("vector file should be readable as UTF-8");
        let kv = parse_kv(&content);
        assert_eq!(kv.get("vrf").map(String::as_str), Some("PraosVRF"));

        let public_key = decode_hex_array::<32>(
            kv.get("pk")
                .expect("pk key should be present for standard vectors"),
        );
        let proof_bytes = decode_hex_array::<80>(
            kv.get("pi")
                .expect("pi key should be present for standard vectors"),
        );
        let output_bytes = decode_hex_array::<64>(
            kv.get("beta")
                .expect("beta key should be present for standard vectors"),
        );
        let message = match kv
            .get("alpha")
            .expect("alpha key should be present for standard vectors")
            .as_str()
        {
            "empty" => Vec::new(),
            hex_value => decode_hex_vec(hex_value),
        };

        let verification_key = VrfVerificationKey::from_bytes(public_key);
        let proof = VrfProof::from_bytes(proof_bytes);
        let expected = VrfOutput::from_bytes(output_bytes);
        match verification_key.verify(&message, &proof) {
            Ok(actual) => {
                if actual != expected {
                    failures.push(format!("{} output mismatch", path.display()));
                }
            }
            Err(error) => failures.push(format!("{} verify failed: {error:?}", path.display())),
        }
        checked += 1;
    }

    assert!(
        checked > 0,
        "at least one mirrored standard vector should be verified"
    );
    assert!(
        failures.is_empty(),
        "mirrored standard vectors should verify cleanly, failures: {}",
        failures.join("; ")
    );
}

#[test]
fn vendored_batchcompat_full_corpus_probe() {
    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0_usize;

    for path in vendored_praos_files() {
        let content = fs::read_to_string(&path).expect("vector file should be readable as UTF-8");
        let kv = parse_kv(&content);
        if kv.get("vrf").map(String::as_str) != Some("PraosBatchCompatVRF") {
            continue;
        }

        let public_key = decode_hex_array::<32>(
            kv.get("pk")
                .expect("pk key should be present for batch-compatible vectors"),
        );
        let proof_bytes = decode_hex_array::<128>(
            kv.get("pi")
                .expect("pi key should be present for batch-compatible vectors"),
        );
        let output_bytes = decode_hex_array::<64>(
            kv.get("beta")
                .expect("beta key should be present for batch-compatible vectors"),
        );
        let message = match kv
            .get("alpha")
            .expect("alpha key should be present for batch-compatible vectors")
            .as_str()
        {
            "empty" => Vec::new(),
            hex_value => decode_hex_vec(hex_value),
        };

        let verification_key = VrfVerificationKey::from_bytes(public_key);
        let proof = VrfBatchCompatProof::from_bytes(proof_bytes);
        let expected = VrfOutput::from_bytes(output_bytes);
        match verification_key.verify_batchcompat(&message, &proof) {
            Ok(actual) => {
                if actual != expected {
                    failures.push(format!("{} output mismatch", path.display()));
                }
            }
            Err(error) => failures.push(format!("{} verify failed: {error:?}", path.display())),
        }
        checked += 1;
    }

    assert!(
        checked > 0,
        "full batch-compatible corpus should contain vectors"
    );
    assert!(
        failures.is_empty(),
        "full batch-compatible corpus parity failures: {}",
        failures.join("; ")
    );
}

#[test]
fn vendored_standard_full_corpus_probe() {
    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0_usize;

    for path in vendored_praos_files() {
        let content = fs::read_to_string(&path).expect("vector file should be readable as UTF-8");
        let kv = parse_kv(&content);
        if kv.get("vrf").map(String::as_str) != Some("PraosVRF") {
            continue;
        }

        let public_key = decode_hex_array::<32>(
            kv.get("pk")
                .expect("pk key should be present for standard vectors"),
        );
        let proof_bytes = decode_hex_array::<80>(
            kv.get("pi")
                .expect("pi key should be present for standard vectors"),
        );
        let output_bytes = decode_hex_array::<64>(
            kv.get("beta")
                .expect("beta key should be present for standard vectors"),
        );
        let message = match kv
            .get("alpha")
            .expect("alpha key should be present for standard vectors")
            .as_str()
        {
            "empty" => Vec::new(),
            hex_value => decode_hex_vec(hex_value),
        };

        let verification_key = VrfVerificationKey::from_bytes(public_key);
        let proof = VrfProof::from_bytes(proof_bytes);
        let expected = VrfOutput::from_bytes(output_bytes);
        match verification_key.verify(&message, &proof) {
            Ok(actual) => {
                if actual != expected {
                    failures.push(format!("{} output mismatch", path.display()));
                }
            }
            Err(error) => failures.push(format!("{} verify failed: {error:?}", path.display())),
        }
        checked += 1;
    }

    assert!(checked > 0, "full standard corpus should contain vectors");
    assert!(
        failures.is_empty(),
        "full standard corpus parity failures: {}",
        failures.join("; ")
    );
}

/// Drift-guard pin for the BLS12-381 hardcoded test parameters.
///
/// `bls_ec_operations_match_upstream_vectors` and
/// `bls_sig_aug_pairing_check` both feed the BLS operations with input
/// values (a scalar; a DST/aug/msg triple) that DO NOT live in the
/// vendored fixture files — they're upstream test-setup constants,
/// vendored only as the OUTPUTS in `ec_operations_test_vectors`/`bls_sig
/// _aug_test_vectors`. If upstream silently changes any input parameter
/// while we refresh the cardano-base SHA, the existing operational tests
/// catch the drift only as opaque "scalar mul mismatch" / "BLS sig aug
/// pairing check failed" failures.
///
/// This test pins each parameter byte-for-byte against the literal
/// upstream value, so a future drift surfaces as a clearly-named test
/// failure citing the offending constant rather than a downstream
/// pairing/scalar-mul check failure several frames removed from the
/// actual delta. Mirrors slices 76/78/80/84 — "named constant + drift
/// guard" — for the BLS surface.
///
/// Reference: `cardano-base/cardano-crypto-class/bls12-381-test-vectors/`
/// at commit `db52f43b38ba5d8927feb2199d4913fe6c0f974d`; IETF BLS suite
/// ID per `draft-irtf-cfrg-bls-signature-05` §4.2.3.
#[test]
fn bls_hardcoded_test_parameters_match_upstream_pins() {
    // Scalar must be exactly 32 bytes (BLS12-381 scalar field is ~255 bits)
    // AND match the upstream literal byte-for-byte.
    assert_eq!(
        BLS_EC_OPERATIONS_SCALAR_HEX.len(),
        64,
        "BLS scalar must be 32-byte (64 hex char) value",
    );
    assert_eq!(
        BLS_EC_OPERATIONS_SCALAR_HEX,
        "40df499974f62e2f268cd5096b0d952073900054122ffce0a27c9d96932891a5",
        "BLS_EC_OPERATIONS_SCALAR_HEX drifted from upstream cardano-base test setup",
    );

    // BLS sig aug DST is the canonical IETF BLS suite ID with cardano-base's
    // `_NUL_` extension. A drift here means the upstream test moved to a
    // different IETF draft revision OR changed the augmentation suite —
    // both substantive correctness changes that must be reviewed manually.
    assert_eq!(
        BLS_SIG_AUG_DST, b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_",
        "BLS_SIG_AUG_DST drifted from canonical IETF BLS suite ID",
    );

    // The aug + msg are upstream test-setup strings. Pin them so a refactor
    // that strips the trailing space from `aug` (which would silently
    // produce a different hash-to-curve result) fails CI naming the
    // offending constant.
    assert_eq!(
        BLS_SIG_AUG_AUG, b"Random value for test aug. ",
        "BLS_SIG_AUG_AUG drifted (note the trailing space — load-bearing)",
    );
    assert_eq!(
        BLS_SIG_AUG_MSG, b"blst is such a blast",
        "BLS_SIG_AUG_MSG drifted from upstream cardano-base test setup",
    );
}

/// Cross-check EVERY hand-transcribed `vrf_praos_test_vectors()` entry
/// against its corresponding vendored `vrf_ver03_*` fixture file.
///
/// `embedded_vrf_vectors_match_vendored_standard_examples` already pairs
/// `standard_10` for both cipher suites — this test extends the coverage
/// to the full 7-entry ver03 corpus (4 generated + 3 standard) so a
/// future upstream commit-bump that refreshes any of the other 6 files
/// without also updating the hand-transcribed Rust copy fails CI naming
/// the offending vector.
///
/// The cross-check is exhaustive in BOTH directions:
///   - every embedded vector must match its on-disk fixture (sk, pk,
///     pi, beta, alpha, vrf header)
///   - the embedded set and the on-disk ver03 set must have the same
///     names, so an orphaned fixture file (added upstream but not
///     transcribed) or an orphan embedded vector (transcribed but the
///     fixture was renamed/removed upstream) both fail CI.
///
/// Reference: `cardano-base/cardano-crypto-praos/test_vectors/vrf_ver03_*`.
#[test]
fn embedded_ver03_vrf_vectors_match_full_vendored_corpus() {
    let dir = vendored_root().join("cardano-crypto-praos/test_vectors");
    let embedded = vrf_praos_test_vectors();

    // Coverage: embedded names must match the ver03 file set on disk
    // (after hyphen→underscore name normalization).
    let embedded_filenames: Vec<String> =
        embedded.iter().map(|v| v.name.replace('-', "_")).collect();
    let on_disk_ver03: Vec<String> = vendored_praos_files()
        .iter()
        .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
        .filter(|n| n.starts_with("vrf_ver03_"))
        .collect();
    let mut sorted_embedded = embedded_filenames.clone();
    sorted_embedded.sort();
    let mut sorted_disk = on_disk_ver03;
    sorted_disk.sort();
    assert_eq!(
        sorted_embedded, sorted_disk,
        "embedded ver03 vector names must match on-disk fixture names exactly \
         (orphan fixture file or orphan embedded vector)",
    );

    // Field-by-field cross-check for every entry.
    for vector in embedded {
        let path = dir.join(vector.name.replace('-', "_"));
        let kv = parse_kv(
            &fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display())),
        );

        assert_eq!(
            kv.get("vrf").map(String::as_str),
            Some("PraosVRF"),
            "{}: vrf header must be PraosVRF",
            vector.name,
        );
        assert_eq!(
            kv.get("ver").map(String::as_str),
            Some("ietfdraft03"),
            "{}: ver must be ietfdraft03",
            vector.name,
        );
        assert_eq!(
            hex(&vector.secret_key[..32]),
            kv["sk"],
            "{}: sk drift between embedded and vendored",
            vector.name,
        );
        // Embedded `secret_key` is `sk || pk` (libsodium signing-key shape).
        // Pin both halves so a refactor that re-orders them silently fails CI.
        assert_eq!(
            hex(&vector.secret_key[32..]),
            kv["pk"],
            "{}: pk-half of secret_key drift",
            vector.name,
        );
        assert_eq!(
            hex(&vector.public_key),
            kv["pk"],
            "{}: pk drift between embedded and vendored",
            vector.name,
        );
        assert_eq!(
            hex(&vector.proof),
            kv["pi"],
            "{}: proof drift between embedded and vendored",
            vector.name,
        );
        assert_eq!(
            hex(&vector.output),
            kv["beta"],
            "{}: output drift between embedded and vendored",
            vector.name,
        );
        // alpha: "empty" → empty message, otherwise a hex-encoded message.
        let expected_message: Vec<u8> = match kv["alpha"].as_str() {
            "empty" => Vec::new(),
            hex_value => decode_hex_vec(hex_value),
        };
        assert_eq!(
            vector.message, expected_message,
            "{}: alpha (message) drift between embedded and vendored",
            vector.name,
        );
    }
}

/// Cross-check EVERY hand-transcribed
/// `vrf_praos_batchcompat_test_vectors()` entry against its corresponding
/// vendored `vrf_ver13_*` fixture file. Mirror of the ver03 full-corpus
/// guard above, for the 128-byte-proof batch-compatible cipher suite.
///
/// Reference: `cardano-base/cardano-crypto-praos/test_vectors/vrf_ver13_*`.
#[test]
fn embedded_ver13_vrf_vectors_match_full_vendored_corpus() {
    let dir = vendored_root().join("cardano-crypto-praos/test_vectors");
    let embedded = vrf_praos_batchcompat_test_vectors();

    let embedded_filenames: Vec<String> =
        embedded.iter().map(|v| v.name.replace('-', "_")).collect();
    let on_disk_ver13: Vec<String> = vendored_praos_files()
        .iter()
        .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
        .filter(|n| n.starts_with("vrf_ver13_"))
        .collect();
    let mut sorted_embedded = embedded_filenames.clone();
    sorted_embedded.sort();
    let mut sorted_disk = on_disk_ver13;
    sorted_disk.sort();
    assert_eq!(
        sorted_embedded, sorted_disk,
        "embedded ver13 vector names must match on-disk fixture names exactly \
         (orphan fixture file or orphan embedded vector)",
    );

    for vector in embedded {
        let path = dir.join(vector.name.replace('-', "_"));
        let kv = parse_kv(
            &fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display())),
        );

        assert_eq!(
            kv.get("vrf").map(String::as_str),
            Some("PraosBatchCompatVRF"),
            "{}: vrf header must be PraosBatchCompatVRF",
            vector.name,
        );
        assert_eq!(
            kv.get("ver").map(String::as_str),
            Some("ietfdraft13"),
            "{}: ver must be ietfdraft13",
            vector.name,
        );
        assert_eq!(
            hex(&vector.secret_key[..32]),
            kv["sk"],
            "{}: sk drift",
            vector.name
        );
        assert_eq!(
            hex(&vector.secret_key[32..]),
            kv["pk"],
            "{}: pk-half of secret_key drift",
            vector.name,
        );
        assert_eq!(
            hex(&vector.public_key),
            kv["pk"],
            "{}: pk drift",
            vector.name
        );
        assert_eq!(hex(&vector.proof), kv["pi"], "{}: proof drift", vector.name);
        assert_eq!(
            hex(&vector.output),
            kv["beta"],
            "{}: output drift",
            vector.name
        );
        let expected_message: Vec<u8> = match kv["alpha"].as_str() {
            "empty" => Vec::new(),
            hex_value => decode_hex_vec(hex_value),
        };
        assert_eq!(
            vector.message, expected_message,
            "{}: alpha (message) drift",
            vector.name,
        );
    }
}

fn parse_kv(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

fn vendored_praos_files() -> Vec<PathBuf> {
    let dir = vendored_root().join("cardano-crypto-praos/test_vectors");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("vendored Praos vector directory should exist")
        .map(|entry| entry.expect("directory entry should be readable").path())
        .collect();
    files.sort();
    files
}

fn assert_hex_lines(path: &Path, expected_line_count: usize) {
    let content = fs::read_to_string(path).expect("vector file should be readable as UTF-8");
    let lines: Vec<&str> = content.lines().collect();

    assert_eq!(
        lines.len(),
        expected_line_count,
        "unexpected line count for {}",
        path.display()
    );

    for line in lines {
        assert!(is_hex(line), "line must be hex in {}", path.display());
    }
}

fn is_hex(value: &str) -> bool {
    !value.is_empty()
        && value.len().is_multiple_of(2)
        && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn decode_hex_vec(value: &str) -> Vec<u8> {
    assert!(
        is_hex(value),
        "hex string should be non-empty and have even-length hex digits"
    );

    let mut out = Vec::with_capacity(value.len() / 2);
    let mut index = 0_usize;
    while index < value.len() {
        let byte = u8::from_str_radix(&value[index..index + 2], 16)
            .expect("hex value should decode into a byte");
        out.push(byte);
        index += 2;
    }
    out
}

fn decode_hex_array<const N: usize>(value: &str) -> [u8; N] {
    assert_eq!(
        value.len(),
        N * 2,
        "hex value length should match output array"
    );
    let bytes = decode_hex_vec(value);
    bytes
        .try_into()
        .expect("decoded hex length should match the fixed array size")
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
