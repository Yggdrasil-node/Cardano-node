use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use yggdrasil_crypto::{vrf_praos_batchcompat_test_vectors, vrf_praos_test_vectors};

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

fn parse_kv(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
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

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
