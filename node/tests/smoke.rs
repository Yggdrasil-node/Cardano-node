#![allow(clippy::unwrap_used)]
use std::{path::Path, process::Command};

#[test]
fn node_binary_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_yggdrasil-node"))
        .arg("--help")
        .output()
        .expect("node binary should launch for smoke test");

    assert!(output.status.success());

    let stdout =
        String::from_utf8(output.stdout).expect("node binary should emit valid UTF-8 output");
    assert!(stdout.contains("Yggdrasil"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("default-config"));
}

#[test]
fn node_binary_run_help_lists_checkpoint_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_yggdrasil-node"))
        .args(["run", "--help"])
        .output()
        .expect("node binary should emit run help");

    assert!(output.status.success());

    let stdout =
        String::from_utf8(output.stdout).expect("node binary should emit valid UTF-8 output");
    assert!(stdout.contains("--checkpoint-interval-slots"));
    assert!(stdout.contains("--max-ledger-snapshots"));
    assert!(stdout.contains("--checkpoint-trace-max-frequency"));
    assert!(stdout.contains("--checkpoint-trace-severity"));
    assert!(stdout.contains("--checkpoint-trace-backend"));
}

#[test]
fn node_binary_default_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_yggdrasil-node"))
        .arg("default-config")
        .output()
        .expect("node binary should emit default config");

    assert!(output.status.success());

    let stdout =
        String::from_utf8(output.stdout).expect("node binary should emit valid UTF-8 output");
    assert!(stdout.contains("network_magic"));
    assert!(stdout.contains("764824073"));
    assert!(stdout.contains("storage_dir"));
    assert!(stdout.contains("checkpoint_interval_slots"));
    assert!(stdout.contains("max_ledger_snapshots"));
    assert!(stdout.contains("TurnOnLogging"));
    assert!(stdout.contains("TraceOptions"));
    assert!(stdout.contains("Node.Recovery.Checkpoint"));
    assert!(stdout.contains("maxFrequency"));
}

#[test]
fn parallel_blockfetch_soak_script_help_is_available() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("parallel_blockfetch_soak.sh");
    let output = Command::new(&script)
        .arg("--help")
        .output()
        .expect("parallel BlockFetch soak script should launch");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("script help should be valid UTF-8");
    assert!(stdout.contains("MAX_CONCURRENT_BLOCK_FETCH_PEERS"));
    assert!(stdout.contains("HASKELL_SOCK"));
    assert!(stdout.contains("EXPECT_WORKERS"));
}

#[test]
fn parallel_blockfetch_soak_script_rejects_legacy_knob() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("parallel_blockfetch_soak.sh");
    let output = Command::new(&script)
        .env("MAX_CONCURRENT_BLOCK_FETCH_PEERS", "1")
        .output()
        .expect("parallel BlockFetch soak script should launch");

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).expect("script stderr should be valid UTF-8");
    assert!(stderr.contains("MAX_CONCURRENT_BLOCK_FETCH_PEERS must be >= 2"));
}
