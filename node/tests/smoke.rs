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
}