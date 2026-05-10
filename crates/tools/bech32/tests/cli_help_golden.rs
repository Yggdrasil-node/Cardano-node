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
fn empty_stdin_emits_string_too_short_error() {
    // R333: the binary now does real encode/decode. With no args
    // and empty stdin, it should emit the upstream-equivalent
    // `StringToDecodeTooShort` error (mirrors upstream Haskell's
    // `bech32: user error (StringToDecodeTooShort)` behavior).
    let mut child = std::process::Command::new(cargo_bin())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn yggdrasil bech32");
    drop(child.stdin.take()); // close stdin (EOF)
    let output = child.wait_with_output().expect("wait yggdrasil bech32");
    assert!(
        !output.status.success(),
        "bech32 with empty stdin must fail",
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("StringToDecodeTooShort"),
        "must emit StringToDecodeTooShort error matching upstream; got: {stderr}",
    );
}

#[test]
fn upstream_example_base16_to_bech32_via_stdin() {
    // From upstream `bech32 --help`: `$ bech32 base16_ <<< 706174617465`
    // expected stdout: `base16_1wpshgct5v5r5mxh0\n`
    use std::io::Write;
    let mut child = std::process::Command::new(cargo_bin())
        .arg("base16_")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn yggdrasil bech32");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"706174617465")
        .expect("write stdin");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "must succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert_eq!(stdout, "base16_1wpshgct5v5r5mxh0\n");
}

#[test]
fn upstream_example_decode_to_base16_via_stdin() {
    // From upstream `bech32 --help`: `$ bech32 <<< base16_1wpshgct5v5r5mxh0`
    // expected stdout: `706174617465\n`
    use std::io::Write;
    let mut child = std::process::Command::new(cargo_bin())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"base16_1wpshgct5v5r5mxh0")
        .expect("write stdin");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert_eq!(stdout, "706174617465\n");
}
