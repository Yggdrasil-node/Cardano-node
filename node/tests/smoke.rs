use std::process::Command;

#[test]
fn node_binary_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_yggdrasil-node"))
        .output()
        .expect("node binary should launch for smoke test");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)
        .expect("node binary should emit valid UTF-8 output");
    assert!(stdout.contains("Yggdrasil foundation ready"));
}