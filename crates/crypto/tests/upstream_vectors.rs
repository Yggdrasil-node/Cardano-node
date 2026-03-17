use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use yggdrasil_crypto::{
    bls12_381::{
        self, g1_add, g1_equal, g1_hash_to_group, g1_neg, g1_scalar_mul,
        g1_uncompress, g2_add, g2_equal, g2_neg, g2_scalar_mul, g2_uncompress,
        miller_loop, mul_ml_result, final_verify,
    },
    vrf_praos_batchcompat_test_vectors, vrf_praos_test_vectors, VrfBatchCompatProof, VrfOutput,
    VrfProof, VrfVerificationKey,
};

const CARDANO_BASE_SHA: &str = "db52f43b38ba5d8927feb2199d4913fe6c0f974d";

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

        let expected_pi_len = if vrf == "PraosBatchCompatVRF" { 256 } else { 160 };
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
    content.lines().map(|line| decode_hex_vec(line)).collect()
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
    assert!(g1_equal(&g1_add(&g1_p, &g1_q), &g1_add_expected), "G1 add mismatch");
    assert!(
        g1_equal(&g1_add(&g1_p, &g1_neg(&g1_q)), &g1_sub_expected),
        "G1 sub mismatch"
    );
    assert!(g1_equal(&g1_neg(&g1_p), &g1_neg_expected), "G1 neg mismatch");

    let scalar = decode_hex_vec("40df499974f62e2f268cd5096b0d952073900054122ffce0a27c9d96932891a5");
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

    assert!(g2_equal(&g2_add(&g2_p, &g2_q), &g2_add_expected), "G2 add mismatch");
    assert!(
        g2_equal(&g2_add(&g2_p, &g2_neg(&g2_q)), &g2_sub_expected),
        "G2 sub mismatch"
    );
    assert!(g2_equal(&g2_neg(&g2_p), &g2_neg_expected), "G2 neg mismatch");
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
    assert!(final_verify(&lhs, &rhs), "e([a]P,Q)*e([b]P,Q) != e([a+b]P,Q)");

    // Identity 5: e([a]P, [b]Q) == e(P, [a*b]Q)
    let lhs = miller_loop(&a_p, &b_q);
    let rhs = miller_loop(&p, &ab_prod_q);
    assert!(final_verify(&lhs, &rhs), "e([a]P, [b]Q) != e(P, [a*b]Q)");

    // Identity 6: e(P, [a]Q) * e(P, [b]Q) == e(P, [a+b]Q)
    let ml_a = miller_loop(&p, &a_q);
    let ml_b = miller_loop(&p, &b_q);
    let lhs = mul_ml_result(&ml_a, &ml_b);
    let rhs = miller_loop(&p, &ab_sum_q);
    assert!(final_verify(&lhs, &rhs), "e(P,[a]Q)*e(P,[b]Q) != e(P,[a+b]Q)");
}

#[test]
fn bls_serde_rejects_invalid_points() {
    let lines = read_hex_lines(&bls_vector_dir().join("serde_test_vectors"));
    assert_eq!(lines.len(), 8);

    // Line 2 (idx 1): G1 compressed, not on curve
    assert!(g1_uncompress(&lines[1]).is_err(), "G1 compressed not-on-curve should fail");

    // Line 3 (idx 2): G1 compressed, not in subgroup
    assert!(g1_uncompress(&lines[2]).is_err(), "G1 compressed not-in-group should fail");

    // Line 6 (idx 5): G2 compressed, not on curve
    assert!(g2_uncompress(&lines[5]).is_err(), "G2 compressed not-on-curve should fail");

    // Line 7 (idx 6): G2 compressed, not in subgroup
    assert!(g2_uncompress(&lines[6]).is_err(), "G2 compressed not-in-group should fail");
}

#[test]
fn bls_sig_aug_pairing_check() {
    let lines = read_hex_lines(&bls_vector_dir().join("bls_sig_aug_test_vectors"));
    assert_eq!(lines.len(), 2);

    let sig = g1_uncompress(&lines[0]).expect("sig");
    let pk = g2_uncompress(&lines[1]).expect("pk");

    let dst = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";
    let aug = b"Random value for test aug. ";
    let msg = b"blst is such a blast";
    let mut full_msg = Vec::with_capacity(aug.len() + msg.len());
    full_msg.extend_from_slice(aug);
    full_msg.extend_from_slice(msg);

    let hashed_msg = g1_hash_to_group(&full_msg, dst).expect("hash to G1");
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
    assert!(g1_equal(&result, &expected), "hash-to-curve large DST mismatch");
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
    assert_eq!(hex(&embedded_ver03.public_key), vendored_ver03["pk"], "ver03 pk mismatch");
    assert_eq!(hex(&embedded_ver03.proof), vendored_ver03["pi"], "ver03 proof mismatch");
    assert_eq!(hex(&embedded_ver03.output), vendored_ver03["beta"], "ver03 output mismatch");

    assert_eq!(
        hex(&embedded_ver13.secret_key[..32]),
        vendored_ver13["sk"],
        "ver13 seed mismatch"
    );
    assert_eq!(hex(&embedded_ver13.public_key), vendored_ver13["pk"], "ver13 pk mismatch");
    assert_eq!(hex(&embedded_ver13.proof), vendored_ver13["pi"], "ver13 proof mismatch");
    assert_eq!(hex(&embedded_ver13.output), vendored_ver13["beta"], "ver13 output mismatch");
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
        assert_eq!(kv.get("vrf").map(String::as_str), Some("PraosBatchCompatVRF"));

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

    assert!(checked > 0, "at least one mirrored batch-compatible vector should be verified");
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

    assert!(checked > 0, "at least one mirrored standard vector should be verified");
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

    assert!(checked > 0, "full batch-compatible corpus should contain vectors");
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
        && value.len() % 2 == 0
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
    assert_eq!(value.len(), N * 2, "hex value length should match output array");
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
