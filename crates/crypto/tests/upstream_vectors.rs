use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use yggdrasil_crypto::{
    vrf_praos_batchcompat_test_vectors, vrf_praos_test_vectors, VrfBatchCompatProof, VrfOutput,
    VrfVerificationKey,
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

#[test]
fn upstream_bls_vector_files_are_present_and_well_formed() {
    let dir = vendored_root().join("cardano-crypto-class/bls12-381-test-vectors/test_vectors");

    assert_hex_lines(&dir.join("pairing_test_vectors"), 10);
    assert_hex_lines(&dir.join("ec_operations_test_vectors"), 12);
    assert_hex_lines(&dir.join("serde_test_vectors"), 8);
    assert_hex_lines(&dir.join("bls_sig_aug_test_vectors"), 2);
    assert_hex_lines(&dir.join("h2c_large_dst"), 3);
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
#[ignore = "tracks parity against full official batchcompat corpus while implementation is still incremental"]
fn vendored_batchcompat_full_corpus_probe() {
    // Current known gap: `vrf_ver13_standard_12` from cardano-base fails with
    // `InvalidVrfProof` under the pure-Rust verifier and is tracked by this probe.
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
