//! End-to-end integration tests for the db-synthesizer forge-loop,
//! genesis-loading, and Praos block-production slices.
//!
//! These exercise the full argv → `parser::Args` → `lib::run` →
//! `run::synthesize_from_config` path and verify the synthesized ChainDB
//! is structurally valid: it can be reopened from disk by yggdrasil's
//! own `FileImmutable` and the block count / chaining invariants
//! hold.
//!
//! What these tests establish:
//! - `run()` no longer returns the `ForgeLoopDeferred` stub error.
//! - `--blocks N` produces exactly `N` Praos-forged blocks on disk
//!   under `--db` when a bulk credential set is supplied.
//! - The synthesized chain is genuinely prev-hash-threaded.
//! - `preOpenChainDB` create / append / force semantics behave.
//!
//! What they explicitly do NOT establish:
//! - Byte-equivalence with the upstream `db-synthesizer` binary's
//!   ChainDB chunk format.

#![allow(clippy::unwrap_used)]

use yggdrasil_crypto::blake2b::{hash_bytes_224, hash_bytes_256};
use yggdrasil_crypto::vrf::VrfSecretKey;
use yggdrasil_db_synthesizer::{parser, run};
use yggdrasil_ledger::{
    Address, BaseAddress, BlockNo, HeaderHash, Point, RewardAccount, SlotNo, StakeCredential,
};
use yggdrasil_storage::{FileImmutable, ImmutableStore};

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn cbor_bstr(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    if bytes.len() < 24 {
        out.push(0x40 | bytes.len() as u8);
    } else if bytes.len() <= u8::MAX as usize {
        out.push(0x58);
        out.push(bytes.len() as u8);
    } else {
        panic!("test helper only supports small byte strings");
    }
    out.extend_from_slice(bytes);
    out
}

fn text_envelope(type_tag: &str, cbor: &[u8]) -> String {
    format!(
        r#"{{"type":"{type_tag}","description":"","cborHex":"{}"}}"#,
        hex_encode(cbor)
    )
}

fn write_bulk_credentials(dir: &std::path::Path) -> std::path::PathBuf {
    let mut opcert = Vec::new();
    opcert.push(0x82); // [OCert, cold_vkey]
    opcert.push(0x84); // OCert array(4)
    opcert.extend_from_slice(&cbor_bstr(&[0x11; 32]));
    opcert.push(0x00); // sequence_number
    opcert.push(0x00); // kes_period
    opcert.extend_from_slice(&cbor_bstr(&[0x22; 64]));
    opcert.extend_from_slice(&cbor_bstr(&[0x33; 32]));

    let vrf = VrfSecretKey::from_seed([0x44; 32]).to_bytes();
    let kes_seed = [0x55; 32];
    let bulk = format!(
        "[[{},{},{}]]",
        text_envelope("NodeOperationalCertificate", &opcert),
        text_envelope("VrfSigningKey_PraosVRF", &cbor_bstr(&vrf)),
        text_envelope("KesSigningKey_ed25519_kes_2^0", &cbor_bstr(&kes_seed)),
    );
    let path = dir.join("bulk-credentials.json");
    std::fs::write(&path, bulk).unwrap();
    path
}

/// Build a `parser::Args` for a `--blocks N` invocation, writing a
/// real `config.json` + every era's genesis into `tmp`.
///
/// `lib::run` resolves the genesis (epoch length + the R3b-1
/// `GenesisBundle`) from the `--config` node config, so the integration
/// path needs a genuine config file with every era's genesis present
/// (R1 ignored the config entirely).
fn args_for(tmp: &std::path::Path, db: &std::path::Path, n: u64, mode: &str) -> parser::Args {
    let cold_vkey = [0x33; 32];
    let pool_hash = hash_bytes_224(&cold_vkey).0;
    let vrf_vkey = VrfSecretKey::from_seed([0x44; 32])
        .verification_key()
        .to_bytes();
    let vrf_hash = hash_bytes_256(&vrf_vkey).0;
    let payment_hash = [0x66; 28];
    let stake_hash = [0x77; 28];
    let stake_credential = StakeCredential::AddrKeyHash(stake_hash);
    let funded_address = Address::Base(BaseAddress {
        network: 0,
        payment: StakeCredential::AddrKeyHash(payment_hash),
        staking: stake_credential,
    })
    .to_bytes();
    let reward_account = RewardAccount {
        network: 0,
        credential: stake_credential,
    };
    let pool_hash_hex = hex_encode(&pool_hash);
    let stake_hash_hex = hex_encode(&stake_hash);
    std::fs::write(
        tmp.join("shelley-genesis.json"),
        format!(
            r#"{{
                "activeSlotsCoeff":1.0,
                "epochLength":432000,
                "initialFunds":{{"{funded_address}":45000000000000}},
                "staking":{{
                    "pools":{{
                        "{pool_hash_hex}":{{
                            "poolId":"{pool_hash_hex}",
                            "vrf":"{vrf_hash}",
                            "pledge":0,
                            "cost":0,
                            "margin":{{"numerator":0,"denominator":1}},
                            "accountAddress":"{reward_account}",
                            "owners":["{stake_hash_hex}"],
                            "relays":[],
                            "metadata":null
                        }}
                    }},
                    "stake":{{"{stake_hash_hex}":"{pool_hash_hex}"}}
                }}
            }}"#,
            funded_address = hex_encode(&funded_address),
            vrf_hash = hex_encode(&vrf_hash),
            reward_account = hex_encode(&reward_account.to_bytes()),
        ),
    )
    .unwrap();
    // R3b-1: `run` loads every era's genesis via `load_genesis_bundle`.
    std::fs::write(tmp.join("byron.json"), "{}").unwrap();
    std::fs::write(
        tmp.join("alonzo.json"),
        r#"{"executionPrices":{"prMem":{"numerator":1,"denominator":1},"prSteps":{"numerator":1,"denominator":1}},"maxTxExUnits":{"exUnitsMem":1,"exUnitsSteps":1},"maxBlockExUnits":{"exUnitsMem":1,"exUnitsSteps":1}}"#,
    )
    .unwrap();
    std::fs::write(tmp.join("conway.json"), "{}").unwrap();
    let config = tmp.join("config.json");
    std::fs::write(
        &config,
        r#"{"Protocol":"Cardano","ByronGenesisFile":"byron.json","ShelleyGenesisFile":"shelley-genesis.json","AlonzoGenesisFile":"alonzo.json","ConwayGenesisFile":"conway.json","LastKnownBlockVersion-Major":1,"LastKnownBlockVersion-Minor":0}"#,
    )
    .unwrap();
    let bulk = write_bulk_credentials(tmp);
    let mut argv = vec![
        "--config".to_string(),
        config.to_string_lossy().into_owned(),
        "--db".to_string(),
        db.to_string_lossy().into_owned(),
        "--bulk-credentials-file".to_string(),
        bulk.to_string_lossy().into_owned(),
        "--blocks".to_string(),
        n.to_string(),
    ];
    match mode {
        "force" => argv.push("-f".to_string()),
        "append" => argv.push("-a".to_string()),
        _ => {}
    }
    parser::parse_args(&argv).expect("parses")
}

#[test]
fn run_synthesizes_chain_db_that_reopens_from_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");
    let args = args_for(tmp.path(), &target, 12, "create");

    // The former `ForgeLoopDeferred` stub returned Err here.
    yggdrasil_db_synthesizer::run(&args).expect("run succeeds");

    // Verification: reopen the synthesized ChainDB with yggdrasil's
    // own FileImmutable and confirm the block count.
    let store = FileImmutable::open(&target).expect("reopens");
    assert_eq!(store.len(), 12, "12 blocks synthesized + persisted");
}

#[test]
fn synthesized_chain_is_prev_hash_threaded() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");
    let args = args_for(tmp.path(), &target, 8, "create");
    yggdrasil_db_synthesizer::run(&args).expect("run succeeds");

    let store = FileImmutable::open(&target).expect("reopens");
    let blocks = store.suffix_after(&Point::Origin).expect("walks chain");
    assert_eq!(blocks.len(), 8);

    // Genesis successor carries the all-zero prev-hash.
    assert_eq!(blocks[0].header.prev_hash, HeaderHash([0u8; 32]));
    assert_eq!(blocks[0].header.block_no, BlockNo(0));
    assert_eq!(blocks[0].header.slot_no, SlotNo(0));

    // Each subsequent block points at its real predecessor and
    // increments the block number.
    for w in blocks.windows(2) {
        assert_eq!(w[1].header.prev_hash, w[0].header.hash);
        assert_eq!(w[1].header.block_no.0, w[0].header.block_no.0 + 1);
        assert_eq!(w[1].header.slot_no.0, w[0].header.slot_no.0 + 1);
        assert!(w[1].transactions.is_empty(), "synthesized blocks are empty");
    }
}

#[test]
fn append_mode_resumes_and_extends_chain_db() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");

    // First pass: create with 5 blocks.
    let create = args_for(tmp.path(), &target, 5, "create");
    yggdrasil_db_synthesizer::run(&create).expect("create succeeds");
    assert_eq!(FileImmutable::open(&target).unwrap().len(), 5);

    // Second pass: append 7 more — total 12.
    let append = args_for(tmp.path(), &target, 7, "append");
    yggdrasil_db_synthesizer::run(&append).expect("append succeeds");

    let store = FileImmutable::open(&target).expect("reopens");
    assert_eq!(store.len(), 12);
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    // The chain stays consistently threaded across the two passes.
    for w in blocks.windows(2) {
        assert_eq!(w[1].header.prev_hash, w[0].header.hash);
    }
    assert_eq!(blocks.last().unwrap().header.block_no, BlockNo(11));
}

#[test]
fn force_mode_overwrites_existing_chain_db() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");

    let create = args_for(tmp.path(), &target, 20, "create");
    yggdrasil_db_synthesizer::run(&create).expect("create succeeds");

    // Force-recreate with a shorter chain.
    let force = args_for(tmp.path(), &target, 3, "force");
    yggdrasil_db_synthesizer::run(&force).expect("force succeeds");

    let store = FileImmutable::open(&target).expect("reopens");
    assert_eq!(store.len(), 3, "force wiped the 20-block chain");
}

#[test]
fn create_mode_refuses_existing_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");

    let create = args_for(tmp.path(), &target, 4, "create");
    yggdrasil_db_synthesizer::run(&create).expect("first create succeeds");

    // A second create against the now-existing dir must fail with the
    // upstream-shaped AlreadyExists error.
    let create_again = args_for(tmp.path(), &target, 4, "create");
    let err = yggdrasil_db_synthesizer::run(&create_again).expect_err("second create fails");
    let msg = format!("{err:#}");
    assert!(msg.contains("already exists"), "got: {msg}");
}

#[test]
fn zero_block_limit_is_a_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");
    let args = args_for(tmp.path(), &target, 0, "create");
    yggdrasil_db_synthesizer::run(&args).expect("run succeeds");

    let store = FileImmutable::open(&target).expect("reopens");
    assert_eq!(store.len(), 0);
}

#[test]
fn synthesize_default_is_deterministic_across_runs() {
    // Two independent synth runs with the same limit produce
    // byte-identical chains (deterministic structural synthesis).
    let make = || {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("chaindb");
        run::synthesize_default(
            parser::parse_args(["--config", "/c", "--db", "/unused", "--blocks", "6"])
                .unwrap()
                .options,
            &target,
        )
        .unwrap();
        let store = FileImmutable::open(&target).unwrap();
        store
            .suffix_after(&Point::Origin)
            .unwrap()
            .into_iter()
            .map(|b| b.header.hash)
            .collect::<Vec<_>>()
    };
    assert_eq!(make(), make(), "structural synthesis is deterministic");
}
