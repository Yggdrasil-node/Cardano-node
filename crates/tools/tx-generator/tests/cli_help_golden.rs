//! Golden test: yggdrasil-tx-generator `--help` / `--version` outputs are
//! byte-equivalent to the upstream `tx-generator` binary.

use std::process::Command;

const UPSTREAM_HELP: &str = include_str!("fixtures/upstream-help.txt");
const UPSTREAM_VERSION: &str = include_str!("fixtures/upstream-version.txt");

fn cargo_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_tx-generator"))
}

#[test]
fn help_long_flag_matches_upstream() {
    let output = Command::new(cargo_bin())
        .arg("--help")
        .output()
        .expect("spawn");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert_eq!(stdout, UPSTREAM_HELP);
}

#[test]
fn version_long_flag_matches_upstream() {
    let output = Command::new(cargo_bin())
        .arg("--version")
        .output()
        .expect("spawn");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert_eq!(stdout, UPSTREAM_VERSION);
}

#[test]
fn json_command_reaches_typed_dispatch_sentinel() {
    let output = Command::new(cargo_bin())
        .args(["json", "script.json"])
        .output()
        .expect("spawn");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf-8");
    assert!(stderr.contains("`json` command execution not yet implemented"));
}

#[test]
fn unknown_command_is_rejected_before_dispatch() {
    let output = Command::new(cargo_bin())
        .arg("bogus")
        .output()
        .expect("spawn");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf-8");
    assert!(stderr.contains("Invalid argument `bogus`"));
}
