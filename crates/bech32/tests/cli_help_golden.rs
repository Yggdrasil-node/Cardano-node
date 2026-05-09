//! Golden test: yggdrasil-bech32 `--help` and `--version` outputs are
//! byte-equivalent to the upstream `bech32` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side acceptance test for the
//! R332 CLI parser milestone. The fixtures
//! `tests/fixtures/upstream-help.txt` +
//! `tests/fixtures/upstream-version.txt` were captured from
//! `.reference-haskell-cardano-node/install/bin/bech32 --help` and
//! `... --version` respectively at R332. The runtime help/version
//! constants (`parser::HELP_TEXT`, `parser::VERSION_TEXT`) read the
//! same fixture files via `include_str!`, so this test pins both
//! the runtime path and the fixture against drift.

use std::process::Command;

const UPSTREAM_HELP: &str = include_str!("fixtures/upstream-help.txt");
const UPSTREAM_VERSION: &str = include_str!("fixtures/upstream-version.txt");

fn cargo_bin() -> std::path::PathBuf {
    // CARGO_BIN_EXE_<name> is set by Cargo when running integration
    // tests; mirrors the pattern used by the rest of the workspace.
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_bech32"))
}

#[test]
fn help_long_flag_matches_upstream_byte_for_byte() {
    let output = Command::new(cargo_bin())
        .arg("--help")
        .output()
        .expect("yggdrasil bech32 binary failed to start");
    assert!(
        output.status.success(),
        "yggdrasil bech32 --help exited non-zero: {:?}",
        output.status,
    );
    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert_eq!(
        stdout, UPSTREAM_HELP,
        "yggdrasil bech32 --help must be byte-equivalent to upstream",
    );
}

#[test]
fn help_short_flag_matches_upstream_byte_for_byte() {
    let output = Command::new(cargo_bin())
        .arg("-h")
        .output()
        .expect("yggdrasil bech32 binary failed to start");
    assert!(
        output.status.success(),
        "yggdrasil bech32 -h exited non-zero"
    );
    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert_eq!(
        stdout, UPSTREAM_HELP,
        "yggdrasil bech32 -h must be byte-equivalent to upstream",
    );
}

#[test]
fn version_long_flag_matches_upstream_byte_for_byte() {
    let output = Command::new(cargo_bin())
        .arg("--version")
        .output()
        .expect("yggdrasil bech32 binary failed to start");
    assert!(
        output.status.success(),
        "yggdrasil bech32 --version exited non-zero",
    );
    let stdout = String::from_utf8(output.stdout).expect("version is UTF-8");
    assert_eq!(
        stdout, UPSTREAM_VERSION,
        "yggdrasil bech32 --version must be byte-equivalent to upstream",
    );
}

#[test]
fn version_short_flag_matches_upstream_byte_for_byte() {
    let output = Command::new(cargo_bin())
        .arg("-v")
        .output()
        .expect("yggdrasil bech32 binary failed to start");
    assert!(
        output.status.success(),
        "yggdrasil bech32 -v exited non-zero"
    );
    let stdout = String::from_utf8(output.stdout).expect("version is UTF-8");
    assert_eq!(stdout, UPSTREAM_VERSION);
}

#[test]
fn unknown_flag_exits_non_zero() {
    let output = Command::new(cargo_bin())
        .arg("--definitely-not-a-real-flag")
        .output()
        .expect("yggdrasil bech32 binary failed to start");
    assert!(
        !output.status.success(),
        "yggdrasil bech32 must reject unknown flags",
    );
}

#[test]
fn no_args_returns_r332_sentinel_until_r333() {
    // R332 boundary: the CLI parser is functional but the
    // encode/decode dispatch hasn't landed yet. With no args, the
    // binary should fail with the R332 sentinel — NOT crash.
    let output = Command::new(cargo_bin())
        .output()
        .expect("yggdrasil bech32 binary failed to start");
    assert!(
        !output.status.success(),
        "R332 sentinel: bech32 with no args must fail until R333 lands encode/decode",
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("R332"),
        "R332 sentinel must mention the round number; got stderr: {stderr}",
    );
}
