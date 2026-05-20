//! Golden test: yggdrasil-tx-generator `--help` / `--version` outputs are
//! byte-equivalent to the upstream `tx-generator` binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const UPSTREAM_HELP: &str = include_str!("fixtures/upstream-help.txt");
const UPSTREAM_VERSION: &str = include_str!("fixtures/upstream-version.txt");

fn cargo_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_tx-generator"))
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-cli-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
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
    let temp = TempDir::new("json");
    let script = temp.path().join("script.json");
    fs::write(
        &script,
        r#"[
  { "InitWallet": "wallet" },
  { "CancelBenchmark": [] }
]
"#,
    )
    .expect("write script");

    let output = Command::new(cargo_bin())
        .arg("json")
        .arg(&script)
        .output()
        .expect("spawn");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf-8");
    assert!(stderr.contains("action #2 failed"));
    assert!(stderr.contains("cancelBenchmark: missing AsyncBenchmarkControl"));
}

#[test]
fn compile_command_emits_generated_script() {
    let temp = TempDir::new("compile");
    let config = temp.path().join("config.json");
    fs::write(
        &config,
        r#"{
  "debugMode": false,
  "tx_count": 4,
  "tps": 10.0,
  "inputs_per_tx": 2,
  "outputs_per_tx": 3,
  "tx_fee": 212345,
  "min_utxo_value": 1000000,
  "add_tx_size": 39,
  "init_cooldown": 5.0,
  "era": "Conway",
  "localNodeSocketPath": "node.socket",
  "nodeConfigFile": "config.json",
  "sigKey": "genesis-utxo.skey",
  "targetNodes": [
    {"addr": "127.0.0.1", "port": 30000, "name": "node0"}
  ],
  "plutus": null
}
"#,
    )
    .expect("write config");

    let output = Command::new(cargo_bin())
        .arg("compile")
        .arg(&config)
        .output()
        .expect("spawn");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert!(stdout.contains("\"SetSocketPath\""));
    assert!(stdout.contains("\"StartProtocol\""));
    assert!(stdout.contains("\"PaymentSigningKeyShelley_ed25519\""));
    assert!(stdout.contains("\"WaitBenchmark\""));
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
