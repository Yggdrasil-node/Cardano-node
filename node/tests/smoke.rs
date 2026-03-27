#![allow(clippy::unwrap_used)]
use std::process::Command;

#[test]
fn node_binary_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_yggdrasil-node"))
        .arg("--help")
        .output()
        .expect("node binary should launch for smoke test");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)
        .expect("node binary should emit valid UTF-8 output");
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

    let stdout = String::from_utf8(output.stdout)
        .expect("node binary should emit valid UTF-8 output");
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

    let stdout = String::from_utf8(output.stdout)
        .expect("node binary should emit valid UTF-8 output");
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