#![allow(clippy::unwrap_used)]
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg_attr(not(windows), allow(dead_code))]
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn script_command(script: &Path) -> Command {
    #[cfg(windows)]
    {
        let git_bash = Path::new(r"C:\Program Files\Git\bin\bash.exe");
        let script_name = script
            .file_name()
            .expect("script path has a file name")
            .to_string_lossy()
            .replace('\\', "/");
        let mut command = if git_bash.exists() {
            Command::new(git_bash)
        } else {
            Command::new("bash")
        };
        command.arg(format!("scripts/{script_name}"));
        command.current_dir(workspace_root());
        command
    }

    #[cfg(not(windows))]
    {
        Command::new(script)
    }
}

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
    assert!(stdout.contains("--non-producing-node"));
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
        .join("../../../scripts")
        .join("parallel_blockfetch_soak.sh");
    let output = script_command(&script)
        .arg("--help")
        .output()
        .expect("parallel BlockFetch soak script should launch");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("script help should be valid UTF-8");
    assert!(stdout.contains("MAX_CONCURRENT_BLOCK_FETCH_PEERS"));
    assert!(stdout.contains("HASKELL_SOCK"));
    assert!(stdout.contains("REQUIRE_TIP_COMPARISON"));
    assert!(stdout.contains("EXPECT_WORKERS"));
}

#[test]
fn parallel_blockfetch_soak_script_rejects_legacy_knob() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("parallel_blockfetch_soak.sh");
    let output = script_command(&script)
        .env("MAX_CONCURRENT_BLOCK_FETCH_PEERS", "1")
        .output()
        .expect("parallel BlockFetch soak script should launch");

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).expect("script stderr should be valid UTF-8");
    assert!(stderr.contains("MAX_CONCURRENT_BLOCK_FETCH_PEERS must be >= 2"));
}

#[test]
fn parallel_blockfetch_soak_script_requires_haskell_when_tip_comparison_is_required() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("parallel_blockfetch_soak.sh");
    let output = script_command(&script)
        .env("YGG_BIN", env!("CARGO_BIN_EXE_yggdrasil-node"))
        .env("REQUIRE_TIP_COMPARISON", "1")
        .env_remove("HASKELL_SOCK")
        .output()
        .expect("parallel BlockFetch soak script should launch");

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).expect("script stderr should be valid UTF-8");
    assert!(stderr.contains("REQUIRE_TIP_COMPARISON=1 requires HASKELL_SOCK"));
}

#[test]
fn parallel_blockfetch_soak_script_guards_required_comparison_window() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("parallel_blockfetch_soak.sh");
    let script_text =
        fs::read_to_string(&script).expect("BlockFetch soak script should be readable");

    assert!(
        script_text
            .contains("require_bool01 \"REQUIRE_TIP_COMPARISON\" \"$REQUIRE_TIP_COMPARISON\""),
        "BlockFetch soak should reject invalid required-comparison flags before startup"
    );
    assert!(
        script_text.contains("REQUIRE_TIP_COMPARISON=1 but COMPARE_INTERVAL_S=$COMPARE_INTERVAL_S exceeds RUN_SECONDS=$RUN_SECONDS"),
        "required tip comparisons should fail before startup when no comparison can run"
    );
    assert!(
        script_text.contains("REQUIRE_TIP_COMPARISON=1 but no Haskell tip comparison passed"),
        "required tip comparisons should be asserted in the final summary path"
    );
}

#[test]
fn preview_real_pool_producer_script_help_is_available() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_real_pool_producer.sh");
    let output = script_command(&script)
        .arg("--help")
        .output()
        .expect("preview real-pool producer script should launch");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("script help should be valid UTF-8");
    assert!(stdout.contains("--network preview"));
    assert!(stdout.contains("KES_SKEY_PATH"));
    assert!(stdout.contains("VRF_SKEY_PATH"));
    assert!(stdout.contains("OPCERT_PATH"));
    assert!(stdout.contains("HASKELL_SOCK"));
    assert!(stdout.contains("TIP_COMPARE_CHECKPOINTS"));
    assert!(stdout.contains("REQUIRE_TIP_COMPARISON"));
    assert!(stdout.contains("METRICS_DIR"));
    assert!(stdout.contains("METRICS_SNAPSHOT_INTERVAL_S"));
}

#[test]
fn preview_real_pool_producer_script_rejects_missing_credentials() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_real_pool_producer.sh");
    let output = script_command(&script)
        .env("YGG_BIN", env!("CARGO_BIN_EXE_yggdrasil-node"))
        .env_remove("KES_SKEY_PATH")
        .env_remove("VRF_SKEY_PATH")
        .env_remove("OPCERT_PATH")
        .output()
        .expect("preview real-pool producer script should launch");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8(output.stderr).expect("script stderr should be valid UTF-8");
    assert!(stderr.contains("missing KES_SKEY_PATH file"));
}

#[test]
fn preview_real_pool_producer_script_fails_on_tip_comparison_error() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_real_pool_producer.sh");
    let script_text = fs::read_to_string(&script).expect("preview runner should be readable");

    assert!(
        script_text
            .contains("run_tip_comparison \"$checkpoint\" \"$log_file\" \"$run_id\" || exit 1"),
        "tip comparison failure must abort while the producer loop is running under set +e"
    );
}

#[test]
fn preview_real_pool_producer_script_captures_metrics_snapshots() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_real_pool_producer.sh");
    let script_text = fs::read_to_string(&script).expect("preview runner should be readable");

    assert!(
        script_text.contains("curl -fsS \"http://127.0.0.1:${METRICS_PORT}/metrics\""),
        "preview runner should capture Prometheus snapshots from its metrics endpoint"
    );
    assert!(
        script_text.contains("no metrics snapshots were captured"),
        "preview runner should fail a credentialed run that produced no metrics evidence"
    );
}

#[test]
fn preview_real_pool_producer_script_validates_runtime_numbers() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_real_pool_producer.sh");
    let script_text = fs::read_to_string(&script).expect("preview runner should be readable");

    assert!(
        script_text.contains("require_positive_uint \"RUN_SECONDS\" \"$RUN_SECONDS\""),
        "preview runner should reject invalid run windows before starting"
    );
    assert!(
        script_text.contains("require_positive_uint \"METRICS_PORT\" \"$METRICS_PORT\""),
        "preview runner should reject invalid metrics ports before starting"
    );
    assert!(
        script_text.contains(
            "require_positive_uint \"METRICS_SNAPSHOT_INTERVAL_S\" \"$METRICS_SNAPSHOT_INTERVAL_S\""
        ),
        "preview runner should reject invalid metrics snapshot intervals before starting"
    );
    assert!(
        script_text.contains("require_bool01 \"EXPECT_FORGE_EVENTS\" \"$EXPECT_FORGE_EVENTS\""),
        "preview runner should reject invalid forge requirement flags before starting"
    );
    assert!(
        script_text.contains("require_bool01 \"EXPECT_ADOPTED_EVENTS\" \"$EXPECT_ADOPTED_EVENTS\""),
        "preview runner should reject invalid adoption requirement flags before starting"
    );
    assert!(
        script_text
            .contains("require_bool01 \"REQUIRE_TIP_COMPARISON\" \"$REQUIRE_TIP_COMPARISON\""),
        "preview runner should reject invalid tip-comparison requirement flags before starting"
    );
}

#[test]
fn preview_real_pool_producer_script_asserts_validate_report_role() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_real_pool_producer.sh");
    let script_text = fs::read_to_string(&script).expect("preview runner should be readable");

    assert!(
        script_text.contains("assert_validate_report \"$validate_file\""),
        "preview runner should inspect the validate-config JSON report before starting"
    );
    assert!(
        script_text.contains(".reference-haskell-cardano-node/install/bin/cardano-cli"),
        "preview runner should prefer the pinned reference cardano-cli for Haskell tip comparison"
    );
    assert!(
        script_text.contains("\"$CARDANO_CLI\" query tip --testnet-magic 2"),
        "preview runner should fail fast when HASKELL_SOCK is not a queryable preview socket"
    );
    assert!(
        script_text.contains("expected 'block-producer'"),
        "preview runner should require the validate-config report to confirm producer mode"
    );
    assert!(
        script_text.contains("expected 'complete'"),
        "preview runner should require complete credential status"
    );
    assert!(
        script_text.contains("ShelleyOperationalCertificate"),
        "preview runner should check all three producer credential fields"
    );
}

#[test]
fn preview_real_pool_producer_script_requires_all_tip_checkpoints_when_enabled() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_real_pool_producer.sh");
    let script_text = fs::read_to_string(&script).expect("preview runner should be readable");

    assert!(
        script_text.contains(
            "ERROR: REQUIRE_TIP_COMPARISON=1 but checkpoint ${checkpoint}s exceeds RUN_SECONDS=$RUN_SECONDS"
        ),
        "required tip comparisons should fail before startup when any configured checkpoint cannot run"
    );
    assert!(
        script_text.contains(
            "ERROR: REQUIRE_TIP_COMPARISON=1 but only $tip_comparisons_run/$tip_comparisons_expected Haskell tip comparisons ran"
        ),
        "required tip comparisons should require every configured checkpoint to complete"
    );
}

#[test]
fn preview_real_pool_producer_script_writes_summary_artifact() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_real_pool_producer.sh");
    let script_text = fs::read_to_string(&script).expect("preview runner should be readable");

    assert!(
        script_text.contains("preview-real-pool-summary-${run_id}.txt"),
        "preview runner should write a run-id-scoped summary artifact"
    );
    assert!(
        script_text.contains("write_summary \"$summary_file\""),
        "preview runner should write the summary after evidence checks pass"
    );
    assert!(
        script_text.contains("tip_comparisons_run: $tip_comparisons_run"),
        "summary should record tip comparison count"
    );
    assert!(
        script_text.contains("tip_comparisons_expected: $tip_comparisons_expected"),
        "summary should record the configured tip comparison count"
    );
    assert!(
        script_text.contains("metrics_snapshots: $metrics_snapshots"),
        "summary should record metrics snapshot count"
    );
}

#[test]
fn preview_real_pool_producer_script_requires_distinct_active_pool_evidence() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_real_pool_producer.sh");
    let script_text = fs::read_to_string(&script).expect("preview runner should be readable");

    assert!(
        script_text.contains("EXPECT_FORGE_EVENTS=1 but no leader election found"),
        "preview runner should require explicit leader-election evidence"
    );
    assert!(
        script_text.contains("EXPECT_FORGE_EVENTS=1 but no forged local block found"),
        "preview runner should require explicit forged-block evidence"
    );
    assert!(
        script_text.contains("EXPECT_FORGE_EVENTS=1 but no forged-block adoption judgement found"),
        "preview runner should require an adoption/not-adoption judgement for a forged block"
    );
    assert!(
        script_text.contains("EXPECT_ADOPTED_EVENTS=1 but no adopted forged block found"),
        "preview runner should keep the stricter adopted-block gate separate"
    );
}

#[test]
fn preview_generated_pool_registration_script_help_is_available() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("register_preview_generated_pool.sh");
    let output = script_command(&script)
        .arg("--help")
        .output()
        .expect("preview generated-pool registration script should launch");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("script help should be valid UTF-8");
    assert!(stdout.contains("CRED_DIR"));
    assert!(stdout.contains("SOCKET_PATH"));
    assert!(stdout.contains("OFFLINE_BUILD"));
    assert!(stdout.contains("INPUT_LOVELACE"));
    assert!(stdout.contains("PROTOCOL_PARAMS_FILE"));
    assert!(stdout.contains("SUBMIT"));
    assert!(stdout.contains("KOIOS_SUBMIT"));
    assert!(stdout.contains("payment.skey"));
    assert!(stdout.contains("pool.reg.cert"));
}

#[test]
fn preview_generated_pool_registration_script_is_preview_only() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("register_preview_generated_pool.sh");
    let output = script_command(&script)
        .env("NETWORK_MAGIC", "1")
        .output()
        .expect("preview generated-pool registration script should launch");

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).expect("script stderr should be valid UTF-8");
    assert!(stderr.contains("preview-only"));
    assert!(stderr.contains("NETWORK_MAGIC must be 2"));
}

#[test]
fn preview_generated_pool_registration_script_orders_certificates() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("register_preview_generated_pool.sh");
    let script_text =
        fs::read_to_string(&script).expect("preview registration script should be readable");

    let stake_reg = script_text
        .find("--certificate-file \"$STAKE_REG_CERT\"")
        .expect("stake registration certificate should be included");
    let pool_reg = script_text
        .find("--certificate-file \"$POOL_REG_CERT\"")
        .expect("pool registration certificate should be included");
    let stake_deleg = script_text
        .find("--certificate-file \"$STAKE_DELEG_CERT\"")
        .expect("stake delegation certificate should be included");

    assert!(
        stake_reg < pool_reg && pool_reg < stake_deleg,
        "certificate order should be stake registration, pool registration, stake delegation"
    );
    assert!(
        script_text.contains("--witness-override 3"),
        "fee balancing should account for payment, stake, and cold key witnesses"
    );
    assert!(
        script_text.contains("--signing-key-file \"$PAYMENT_SKEY\"")
            && script_text.contains("--signing-key-file \"$STAKE_SKEY\"")
            && script_text.contains("--signing-key-file \"$COLD_SKEY\""),
        "registration transaction should be signed by payment, stake, and cold keys"
    );
}

#[test]
fn preview_generated_pool_registration_script_supports_offline_koios_submit() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("register_preview_generated_pool.sh");
    let script_text =
        fs::read_to_string(&script).expect("preview registration script should be readable");

    assert!(
        script_text.contains("OFFLINE_BUILD=1 requires TX_IN"),
        "offline mode should require an explicit funding input"
    );
    assert!(
        script_text.contains("require_positive_uint \"INPUT_LOVELACE\" \"$INPUT_LOVELACE\""),
        "offline mode should require the input lovelace amount"
    );
    assert!(
        script_text.contains("require_file \"$PROTOCOL_PARAMS_FILE\" \"PROTOCOL_PARAMS_FILE\""),
        "offline mode should require a protocol-parameters file for fee calculation"
    );
    assert!(
        script_text.contains("https://preview.koios.rest/api/v1/submittx"),
        "public preview submit endpoint should be explicit and overrideable"
    );
    assert!(
        script_text.contains("-H 'Content-Type: application/cbor'"),
        "Koios submit should send signed transaction bytes as CBOR"
    );
    assert!(
        script_text.contains("pool-registration.cborhex")
            && script_text.contains("pool-registration.cbor"),
        "the script should emit signed CBOR artifacts for review and submission"
    );
}

#[test]
fn preview_pool_activation_status_script_help_is_available() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("preview_pool_activation_status.sh");
    let output = script_command(&script)
        .arg("--help")
        .output()
        .expect("preview pool activation status script should launch");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("script help should be valid UTF-8");
    assert!(stdout.contains("POOL_ID"));
    assert!(stdout.contains("REQUIRE_ACTIVE"));
    assert!(stdout.contains("active_epoch"));
    assert!(stdout.contains("producer_command"));
}

#[test]
fn preview_pool_activation_status_script_requires_preview_magic() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("preview_pool_activation_status.sh");
    let output = script_command(&script)
        .env("NETWORK_MAGIC", "1")
        .env(
            "POOL_ID",
            "pool1rv9445xped56v36hneedxq96rg3l7hx490zg66pqkk7hcrtl26q",
        )
        .output()
        .expect("preview pool activation status script should launch");

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).expect("script stderr should be valid UTF-8");
    assert!(stderr.contains("preview-only"));
    assert!(stderr.contains("NETWORK_MAGIC must be 2"));
}

#[test]
fn preview_pool_activation_status_script_prints_active_epoch_gate() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("preview_pool_activation_status.sh");
    let script_text =
        fs::read_to_string(&script).expect("preview pool status script should be readable");

    assert!(
        script_text.contains("seconds_until_active"),
        "status output should include a countdown to active epoch"
    );
    assert!(
        script_text.contains("max(") && script_text.contains("active_epoch_no"),
        "status output should choose the latest pool update deterministically"
    );
    assert!(
        script_text
            .contains("EXPECT_FORGE_EVENTS=1 EXPECT_ADOPTED_EVENTS=1 REQUIRE_TIP_COMPARISON=1"),
        "status output should print the producer command with acceptance gates enabled"
    );
    assert!(
        script_text.contains("RUN_SECONDS=21600 TIP_COMPARE_CHECKPOINTS=900,3600,21600"),
        "status output should make the full 15m/60m/6h comparison window explicit"
    );
    assert!(
        script_text.contains("HASKELL_SOCK=/tmp/ygg-haskell-preview/preview/socket/node.socket"),
        "status output should remind operators to supply the local Haskell preview socket"
    );
    assert!(
        script_text.contains("sys.exit(3)"),
        "REQUIRE_ACTIVE=1 should have a distinct pending exit code"
    );
}

#[test]
fn preview_active_pool_signoff_script_help_is_available() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_active_pool_signoff.sh");
    let output = script_command(&script)
        .arg("--help")
        .output()
        .expect("preview active-pool signoff script should launch");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("script help should be valid UTF-8");
    assert!(stdout.contains("POOL_ID"));
    assert!(stdout.contains("RUN_SECONDS=21600"));
    assert!(stdout.contains("TIP_COMPARE_CHECKPOINTS=900,3600,21600"));
    assert!(stdout.contains("HASKELL_RUN_ROOT"));
    assert!(stdout.contains("HASKELL_SYNC_MIN_PERCENT"));
    assert!(stdout.contains("REQUIRE_ACTIVE=1"));
}

#[test]
fn preview_active_pool_signoff_script_orchestrates_required_gates() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_preview_active_pool_signoff.sh");
    let script_text =
        fs::read_to_string(&script).expect("preview active-pool signoff script should be readable");

    assert!(
        script_text.contains("preview_pool_activation_status.sh"),
        "signoff wrapper should check pool activation before starting long-running services"
    );
    assert!(
        script_text.contains("REQUIRE_ACTIVE=1"),
        "signoff wrapper should fail distinctly while the pool is not active"
    );
    assert!(
        script_text.contains("\"$CARDANO_CLI\" query tip --testnet-magic 2"),
        "signoff wrapper should require a queryable preview Haskell socket"
    );
    assert!(
        script_text.contains("wait_for_haskell_sync"),
        "signoff wrapper should wait for the Haskell preview node to sync before starting checkpoints"
    );
    assert!(
        script_text.contains("HASKELL_SYNC_MIN_PERCENT=\"${HASKELL_SYNC_MIN_PERCENT:-99.00}\""),
        "signoff wrapper should default to a near-tip Haskell sync gate"
    );
    assert!(
        script_text
            .contains("require_percent \"HASKELL_SYNC_MIN_PERCENT\" \"$HASKELL_SYNC_MIN_PERCENT\""),
        "signoff wrapper should validate the sync threshold before waiting"
    );
    assert!(
        script_text.contains("\"$REF_RUN_NODE\" preview"),
        "signoff wrapper should be able to start the vendored Haskell preview relay"
    );
    assert!(
        script_text.contains("run_preview_real_pool_producer.sh"),
        "signoff wrapper should delegate the producer run to the real-pool runner"
    );
    assert!(
        script_text.contains("EXPECT_FORGE_EVENTS=\"${EXPECT_FORGE_EVENTS:-1}\"")
            && script_text.contains("EXPECT_ADOPTED_EVENTS=\"${EXPECT_ADOPTED_EVENTS:-1}\"")
            && script_text.contains("REQUIRE_TIP_COMPARISON=\"${REQUIRE_TIP_COMPARISON:-1}\""),
        "signoff wrapper should default all acceptance gates to required"
    );
}

#[test]
fn real_pool_relay_only_scripts_force_non_producing_node() {
    for script_name in [
        "run_preprod_real_pool_producer.sh",
        "run_mainnet_real_pool_producer.sh",
    ] {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scripts")
            .join(script_name);
        let script_text = fs::read_to_string(&script).expect("real-pool script should be readable");

        assert!(
            script_text.contains("RELAY_ONLY=1"),
            "{script_name} should document its relay-only mode"
        );
        assert!(
            script_text.contains("args+=(--non-producing-node)"),
            "{script_name} should force --non-producing-node in relay-only mode"
        );
    }
}

#[test]
fn mainnet_relay_hot_peer_check_counts_big_ledger_peers() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("run_mainnet_real_pool_producer.sh");
    let script_text = fs::read_to_string(&script).expect("mainnet script should be readable");

    assert!(
        script_text.contains("yggdrasil_active_big_ledger_peers"),
        "mainnet relay health should count big-ledger active peers, not only yggdrasil_active_peers"
    );
}

#[test]
fn upstream_drift_script_uses_config_crate_pin_source() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts")
        .join("check_upstream_drift.sh");
    let script_text = fs::read_to_string(&script).expect("drift script should be readable");

    assert!(
        script_text.contains("crates/node/config/src/upstream_pins.rs"),
        "drift script should document the current upstream pin source"
    );
    assert!(
        !script_text.contains("../src/upstream_pins.rs"),
        "drift script should not resolve pins from the old yggdrasil-node source tree"
    );
}
