//! Golden test: yggdrasil-db-synthesizer `--help` / `--version` outputs are
//! byte-equivalent to the upstream `db-synthesizer` binary.

use std::process::Command;

const UPSTREAM_HELP: &str = include_str!("fixtures/upstream-help.txt");
const UPSTREAM_VERSION: &str = include_str!("fixtures/upstream-version.txt");

fn cargo_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_db-synthesizer"))
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
