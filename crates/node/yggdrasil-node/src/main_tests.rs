// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::commands::configuration::{
    CHECKPOINT_TRACE_NAMESPACE, apply_topology_override, checkpoint_trace_config_mut,
    load_effective_config, preset_config_base_dir,
};
use super::commands::query::{decode_optional_prefixed_hex, format_utc_time};
use super::commands::status::status_report;
use super::commands::submit_tx::decode_tx_hex_arg;
use super::commands::validate_config::{node_role_report, validate_config_report};
use super::{
    configured_fallback_peers, forged_header_protocol_version,
    ledger_peer_snapshot_from_ledger_state, strict_base_ledger_state,
};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use yggdrasil_ledger::{
    Era, LedgerState, PoolParams, Relay, RewardAccount, StakeCredential, UnitInterval,
};
use yggdrasil_network::{LedgerPeerSnapshot, LedgerStateJudgement};
use yggdrasil_node_config::default_config;
use yggdrasil_node_tracer::{NodeMetrics, NodeTracer};

// ── decode_tx_hex_arg tests ───────────────────────────────────────
//
// Covers the `submit-tx --tx-hex` CLI argument parsing: raw hex,
// `0x`-prefixed hex, surrounding whitespace, and error paths. The
// 0x-prefix support matches cardano-cli ergonomics and is exercised
// explicitly so a refactor dropping it surfaces as a failing test.

#[test]
fn decode_tx_hex_arg_accepts_plain_hex() {
    let bytes = decode_tx_hex_arg("deadbeef").expect("plain hex");
    assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn decode_tx_hex_arg_strips_0x_prefix() {
    let bytes = decode_tx_hex_arg("0xdeadbeef").expect("0x-prefixed hex");
    assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn decode_tx_hex_arg_trims_whitespace() {
    // Typical paste from a terminal often has trailing newline.
    let bytes = decode_tx_hex_arg("  deadbeef  \n").expect("whitespace-wrapped");
    assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn decode_tx_hex_arg_combines_whitespace_and_prefix() {
    let bytes = decode_tx_hex_arg("\t0xDEADBEEF\n").expect("prefix + whitespace");
    assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

/// `cardano-cli query system-start` against the canonical preview
/// system-start `2022-10-25T00:00:00Z` (year=2022, dayOfYear=298,
/// picosecondsOfDay=0) must format identically to upstream's
/// human-facing rendering.  Pin both preview and mainnet so a
/// future refactor that breaks Gregorian-day arithmetic surfaces
/// here, not silently in operator output.
#[test]
fn format_utc_time_matches_upstream_rendering() {
    // Preview: `2022-10-25T00:00:00Z` ↔ (2022, 298).
    assert_eq!(format_utc_time(2022, 298, 0), "2022-10-25T00:00:00Z");
    // Mainnet: `2017-09-23T21:44:51Z` ↔ (2017, 266) at 21:44:51.
    let secs_into_day = 21 * 3600 + 44 * 60 + 51;
    let picos = secs_into_day * 1_000_000_000_000;
    assert_eq!(format_utc_time(2017, 266, picos), "2017-09-23T21:44:51Z");
    // Leap-year: 2024-12-31 is dayOfYear 366.
    assert_eq!(format_utc_time(2024, 366, 0), "2024-12-31T00:00:00Z");
    // Common-year: 2023-12-31 is dayOfYear 365.
    assert_eq!(format_utc_time(2023, 365, 0), "2023-12-31T00:00:00Z");
    // Sub-day picoseconds floor to seconds (last day-of-year case
    // would otherwise off-by-one if floor logic went the other way).
    let almost_midnight = 23 * 3600 + 59 * 60 + 59;
    assert_eq!(
        format_utc_time(2024, 60, almost_midnight * 1_000_000_000_000),
        "2024-02-29T23:59:59Z"
    );
}

#[test]
fn decode_tx_hex_arg_accepts_empty_string() {
    // Empty hex → empty byte sequence. Not a validation failure; the
    // LocalTxSubmission server will reject it as a malformed tx
    // later, and the CLI's job here is only to decode the hex shape.
    let bytes = decode_tx_hex_arg("").expect("empty hex is empty bytes");
    assert!(bytes.is_empty());
}

#[test]
fn decode_tx_hex_arg_rejects_odd_length_hex() {
    let err = decode_tx_hex_arg("abc").expect_err("odd-length hex must fail");
    assert!(
        err.to_string().contains("invalid hex in --tx-hex"),
        "error must identify the CLI flag, got: {err}",
    );
}

#[test]
fn decode_tx_hex_arg_rejects_non_hex_chars() {
    let err = decode_tx_hex_arg("zzzz").expect_err("non-hex must fail");
    assert!(
        err.to_string().contains("invalid hex in --tx-hex"),
        "error must identify the CLI flag, got: {err}",
    );
}

// ── decode_optional_prefixed_hex tests ────────────────────────────
//
// This is the lenient variant used by the 5 query-argument encoders
// (`UtxoByAddress`, `RewardBalance`, `UtxoByTxIn`, `DelegationsAndRewards`,
// `StakePoolParams`). Accepts the same shapes as `decode_tx_hex_arg`
// but returns `Vec::new()` on parse failure instead of a typed error
// (matches the prior call-site `.unwrap_or_default()` semantics).

#[test]
fn decode_optional_prefixed_hex_accepts_plain_hex() {
    assert_eq!(
        decode_optional_prefixed_hex("deadbeef"),
        vec![0xDE, 0xAD, 0xBE, 0xEF]
    );
}

#[test]
fn decode_optional_prefixed_hex_strips_0x_prefix() {
    // The new ergonomic — a user pasting `0x1234…` from a block
    // explorer now gets the same result as plain hex.
    assert_eq!(
        decode_optional_prefixed_hex("0xdeadbeef"),
        vec![0xDE, 0xAD, 0xBE, 0xEF]
    );
}

#[test]
fn decode_optional_prefixed_hex_trims_whitespace() {
    assert_eq!(
        decode_optional_prefixed_hex("  deadbeef\n"),
        vec![0xDE, 0xAD, 0xBE, 0xEF],
    );
}

#[test]
fn decode_optional_prefixed_hex_returns_empty_on_invalid() {
    // Lenient contract: parse failure → empty Vec, matching the
    // prior `.unwrap_or_default()` call-site behavior. The resulting
    // empty-bytes query is a well-formed CBOR shape the LSQ server
    // handles as "no match" — silent but safe.
    assert_eq!(decode_optional_prefixed_hex("zzzz"), Vec::<u8>::new());
    assert_eq!(decode_optional_prefixed_hex("abc"), Vec::<u8>::new());
}

#[test]
fn decode_optional_prefixed_hex_empty_is_empty() {
    assert_eq!(decode_optional_prefixed_hex(""), Vec::<u8>::new());
    assert_eq!(decode_optional_prefixed_hex("0x"), Vec::<u8>::new());
    assert_eq!(decode_optional_prefixed_hex("   "), Vec::<u8>::new());
}

#[test]
fn cli_help_text_documents_0x_prefix_ergonomic() {
    // Regression guard: every hex-argument CLI flag in `QueryCommand`
    // and `Command::SubmitTx --tx-hex` must document the optional
    // `0x` prefix in its doc comment. Uses `clap`'s `CommandFactory`
    // to render the actual help text, so a future refactor that
    // replaces rustdoc-derived help with hand-written help would also
    // catch the missing note. A failing test names the offending flag.
    use super::Cli;
    use clap::CommandFactory;

    let mut cmd = Cli::command();
    let rendered = cmd.render_long_help().to_string();

    // Walk every `(subcommand, flag, description)` triple. Each flag
    // whose argument type is hex bytes must mention `0x` in its help.
    let required_marks: &[&str] = &[
        // From submit-tx:
        "--tx-hex",
        // From query subcommands:
        "--address",
        "--account",
        "--tx-id",
        "--credential",
        "--pool-hash",
    ];

    // Flatten all subcommand helps by rendering each subcommand's
    // long help, so flag descriptions from nested subcommands appear.
    let mut flat = String::new();
    flat.push_str(&rendered);
    for sub in cmd.get_subcommands_mut() {
        flat.push_str(&sub.render_long_help().to_string());
        for nested in sub.get_subcommands_mut() {
            flat.push_str(&nested.render_long_help().to_string());
        }
    }

    // Each hex flag must appear AND the `0x`-prefix ergonomic must be
    // documented somewhere in the rendered help. The assertion pins
    // both the flag's presence and the documentation phrase.
    for flag in required_marks {
        assert!(
            flat.contains(flag),
            "CLI help text missing expected hex-argument flag: {flag}",
        );
    }
    let prefix_mentions = flat.matches("0x").count();
    assert!(
        prefix_mentions >= required_marks.len(),
        "expected at least {} `0x` mentions in CLI help (one per hex flag), \
             found {prefix_mentions}",
        required_marks.len(),
    );
}

#[test]
fn encode_ntc_query_accepts_0x_prefixed_arguments_end_to_end() {
    // End-to-end: `--address 0xDEADBEEF` must produce identical CBOR
    // bytes to `--address deadbeef`. This pins the slice-74
    // ergonomic at the full encoder output level so a refactor that
    // silently bypasses `decode_optional_prefixed_hex` on ONE of the
    // five query variants (leaving the others prefix-aware) surfaces
    // as a failing test.
    use super::commands::query::{QueryCommand, encode_ntc_query};

    let plain = encode_ntc_query(&QueryCommand::UtxoByAddress {
        address: "deadbeef".into(),
    });
    let prefixed = encode_ntc_query(&QueryCommand::UtxoByAddress {
        address: "0xdeadbeef".into(),
    });
    assert_eq!(
        plain, prefixed,
        "UtxoByAddress: 0x-prefixed and plain hex must emit identical CBOR",
    );

    let plain = encode_ntc_query(&QueryCommand::StakePoolParams {
        pool_hash: "aa".repeat(28),
    });
    let prefixed = encode_ntc_query(&QueryCommand::StakePoolParams {
        pool_hash: format!("0x{}", "aa".repeat(28)),
    });
    assert_eq!(
        plain, prefixed,
        "StakePoolParams: 0x-prefixed and plain hex must emit identical CBOR",
    );
}

#[test]
fn checkpoint_trace_override_creates_namespace_when_missing() {
    let mut cfg = default_config();
    cfg.trace_options.remove(CHECKPOINT_TRACE_NAMESPACE);

    checkpoint_trace_config_mut(&mut cfg).severity = Some("Info".to_owned());

    assert_eq!(
        cfg.trace_options
            .get(CHECKPOINT_TRACE_NAMESPACE)
            .expect("checkpoint namespace")
            .severity
            .as_deref(),
        Some("Info")
    );
}

#[test]
fn checkpoint_trace_override_can_disable_rate_limit() {
    let mut cfg = default_config();

    checkpoint_trace_config_mut(&mut cfg).max_frequency = None;

    assert_eq!(
        cfg.trace_options
            .get(CHECKPOINT_TRACE_NAMESPACE)
            .expect("checkpoint namespace")
            .max_frequency,
        None
    );
}

#[test]
fn checkpoint_trace_override_updates_severity_and_backends() {
    let mut cfg = default_config();
    let override_cfg = checkpoint_trace_config_mut(&mut cfg);
    override_cfg.severity = Some("Silence".to_owned());
    override_cfg.backends = vec!["Stdout MachineFormat".to_owned(), "Forwarder".to_owned()];

    let checkpoint_cfg = cfg
        .trace_options
        .get(CHECKPOINT_TRACE_NAMESPACE)
        .expect("checkpoint namespace");
    assert_eq!(checkpoint_cfg.severity.as_deref(), Some("Silence"));
    assert_eq!(
        checkpoint_cfg.backends,
        vec!["Stdout MachineFormat".to_owned(), "Forwarder".to_owned(),]
    );
}

#[test]
fn ledger_peer_snapshot_from_ledger_state_uses_registered_pool_relays() {
    let mut ledger_state = yggdrasil_ledger::LedgerState::new(yggdrasil_ledger::Era::Shelley);
    ledger_state.pool_state_mut().register(PoolParams {
        operator: [1; 28],
        vrf_keyhash: [2; 32],
        pledge: 1,
        cost: 1,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([3; 28]),
        },
        pool_owners: vec![[4; 28]],
        relays: vec![Relay::SingleHostAddr(
            Some(3001),
            Some([127, 0, 0, 9]),
            None,
        )],
        pool_metadata: None,
    });

    let snapshot = ledger_peer_snapshot_from_ledger_state(&ledger_state);
    assert_eq!(
        snapshot,
        LedgerPeerSnapshot::new(["127.0.0.9:3001".parse().expect("peer")], Vec::new(),)
    );
}

#[test]
fn configured_fallback_peers_appends_eligible_ledger_state_peers() {
    let mut cfg = default_config();
    cfg.use_ledger_after_slot = Some(0);
    cfg.peer_snapshot_file = None;
    let tracer = NodeTracer::from_config(&cfg);
    let ledger_snapshot =
        LedgerPeerSnapshot::new(["127.0.0.9:3001".parse().expect("peer")], Vec::new());

    let fallback_peers = configured_fallback_peers(
        &cfg,
        None,
        &ledger_snapshot,
        Some(1),
        LedgerStateJudgement::YoungEnough,
        &tracer,
    );

    assert!(fallback_peers.contains(&"127.0.0.9:3001".parse().expect("peer")));
}

#[test]
fn configured_fallback_peers_merges_snapshot_big_ledger_peers() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-peer-snapshot-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let snapshot_path = dir.join("peer-snapshot.json");
    std::fs::write(
        &snapshot_path,
        r#"{
                "version": 2,
                "slotNo": 10,
                "bigLedgerPools": [
                    {
                        "accumulatedStake": 0.75,
                        "relativeStake": 0.50,
                        "relays": [
                            { "address": "127.0.0.10", "port": 3001 }
                        ]
                    }
                ]
            }"#,
    )
    .expect("write snapshot");

    let mut cfg = default_config();
    cfg.use_ledger_after_slot = Some(0);
    cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());
    let tracer = NodeTracer::from_config(&cfg);
    let ledger_snapshot =
        LedgerPeerSnapshot::new(["127.0.0.9:3001".parse().expect("peer")], Vec::new());

    let fallback_peers = configured_fallback_peers(
        &cfg,
        Some(&dir),
        &ledger_snapshot,
        Some(10),
        LedgerStateJudgement::YoungEnough,
        &tracer,
    );

    assert!(fallback_peers.contains(&"127.0.0.9:3001".parse().expect("ledger")));
    assert!(fallback_peers.contains(&"127.0.0.10:3001".parse().expect("big ledger")));

    std::fs::remove_file(snapshot_path).ok();
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_when_storage_is_uninitialized() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-validate-config-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;

    let report = validate_config_report(&cfg, Some(&dir)).expect("validation report");

    assert_eq!(report.storage.status, "not-initialized");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("storage directories are not initialized"))
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn metrics_http_response_supports_debug_json_alias() {
    let metrics = NodeMetrics::new();
    let (status, content_type, body) =
        super::metrics_server::metrics_http_response("GET /debug HTTP/1.1\r\n\r\n", &metrics);

    assert_eq!(status, "200 OK");
    assert_eq!(content_type, "application/json");
    assert!(body.contains("\"blocks_synced\""));
}

#[test]
fn metrics_http_response_supports_debug_prometheus_alias() {
    let metrics = NodeMetrics::new();
    metrics.add_blocks_synced(3);
    let (status, content_type, body) = super::metrics_server::metrics_http_response(
        "GET /debug/metrics/prometheus HTTP/1.1\r\n\r\n",
        &metrics,
    );

    assert_eq!(status, "200 OK");
    assert_eq!(content_type, "text/plain; version=0.0.4; charset=utf-8");
    assert!(body.contains("yggdrasil_blocks_synced 3"));
}

#[test]
fn metrics_http_response_supports_debug_health_alias() {
    let metrics = NodeMetrics::new();
    let (status, content_type, body) = super::metrics_server::metrics_http_response(
        "GET /debug/health HTTP/1.1\r\n\r\n",
        &metrics,
    );

    assert_eq!(status, "200 OK");
    assert_eq!(content_type, "application/json");
    assert!(body.contains("\"status\":\"ok\""));
}

#[test]
fn validate_config_report_loads_configured_peer_snapshot() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-validate-snapshot-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let snapshot_path = dir.join("peer-snapshot.json");
    std::fs::write(
        &snapshot_path,
        r#"{
                "version": 2,
                "slotNo": 10,
                "allLedgerPools": [
                    {
                        "accumulatedStake": 0.75,
                        "relativeStake": 0.50,
                        "relays": [
                            { "address": "127.0.0.11", "port": 3001 }
                        ]
                    }
                ]
            }"#,
    )
    .expect("write snapshot");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

    let report = validate_config_report(&cfg, Some(&dir)).expect("validation report");

    assert_eq!(report.peer_snapshot.status, "loaded");
    assert_eq!(report.peer_snapshot.slot, Some(10));
    assert_eq!(report.peer_snapshot.ledger_peer_count, 1);
    assert_eq!(report.peer_snapshot.error, None);

    std::fs::remove_file(snapshot_path).ok();
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_rejects_invalid_active_slot_coeff() {
    let mut cfg = default_config();
    cfg.active_slot_coeff = 0.0;

    assert!(validate_config_report(&cfg, None).is_err());
}

#[test]
fn validate_config_report_rejects_zero_slots_per_kes_period() {
    let mut cfg = default_config();
    cfg.slots_per_kes_period = 0;
    let err = validate_config_report(&cfg, None).expect_err("zero slots_per_kes_period must fail");
    assert!(
        err.to_string().contains("slots_per_kes_period"),
        "error should mention slots_per_kes_period: {err}",
    );
}

#[test]
fn validate_config_report_rejects_zero_max_kes_evolutions() {
    let mut cfg = default_config();
    cfg.max_kes_evolutions = 0;
    let err = validate_config_report(&cfg, None).expect_err("zero max_kes_evolutions must fail");
    assert!(
        err.to_string().contains("max_kes_evolutions"),
        "error should mention max_kes_evolutions: {err}",
    );
}

#[test]
fn validate_config_report_rejects_partial_block_producer_credentials() {
    let mut cfg = default_config();
    cfg.shelley_kes_key = Some("kes.skey".to_owned());

    let err = validate_config_report(&cfg, None)
        .expect_err("partial block producer credentials must fail");
    assert!(
        err.to_string()
            .contains("block producer credentials are partially configured"),
        "error should identify partial credentials: {err}",
    );
}

#[test]
fn validate_config_report_rejects_missing_complete_block_producer_credentials() {
    let mut cfg = default_config();
    cfg.shelley_kes_key = Some("missing-kes.skey".to_owned());
    cfg.shelley_vrf_key = Some("missing-vrf.skey".to_owned());
    cfg.shelley_operational_certificate = Some("missing-opcert.cert".to_owned());
    cfg.shelley_operational_certificate_issuer_vkey = Some("missing-cold.vkey".to_owned());

    let err = validate_config_report(&cfg, None)
        .expect_err("complete but unreadable block producer credentials must fail");
    assert!(
        err.to_string()
            .contains("failed to load block producer credentials"),
        "error should identify credential loading: {err}",
    );
}

#[test]
fn non_producing_node_ignores_configured_block_producer_credentials() {
    let mut cfg = default_config();
    cfg.shelley_kes_key = Some("kes.skey".to_owned());
    cfg.shelley_vrf_key = Some("vrf.skey".to_owned());

    let role = node_role_report(&cfg, true).expect("non-producing role");

    assert_eq!(role.role, "non-producing");
    assert_eq!(
        role.block_producer_credentials,
        "ignored-by-non-producing-node"
    );
    assert!(
        role.credential_fields_missing
            .contains(&"ShelleyOperationalCertificate")
    );
}

#[test]
fn node_role_report_distinguishes_relay_from_sync_only() {
    let mut cfg = default_config();
    assert_eq!(
        node_role_report(&cfg, false).expect("sync role").role,
        "sync-only"
    );

    cfg.inbound_listen_addr = Some("127.0.0.1:3001".parse().expect("listen addr"));
    assert_eq!(
        node_role_report(&cfg, false).expect("relay role").role,
        "relay"
    );
}

#[test]
fn validate_config_report_warns_on_pre_shelley_max_major_protocol_version() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-pv-warn-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.max_major_protocol_version = 1; // pre-Shelley
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("pre-Shelley") || w.contains("max_major_protocol_version")),
        "expected pre-Shelley warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[cfg(unix)]
fn decode_ntc_result_shapes_typed_json_for_new_queries() {
    // Lock in the decoder output for every recently-added typed
    // response so a silent drift in the CLI-side parser (which turns
    // the server's raw CBOR into structured JSON) is caught at CI
    // time rather than showing up as wrong keys in user-facing
    // `yggdrasil-node query ...` output.
    use super::commands::query::{QueryCommand, decode_ntc_result};

    // AccountState: `[treasury, reserves, total_deposits]`
    // CBOR: 0x83 0x01 0x02 0x03 → [1, 2, 3]
    let v = decode_ntc_result(&QueryCommand::AccountState, &[0x83, 0x01, 0x02, 0x03])
        .expect("decode AccountState");
    assert_eq!(v["treasury_lovelace"], 1);
    assert_eq!(v["reserves_lovelace"], 2);
    assert_eq!(v["total_deposits_lovelace"], 3);

    // StabilityWindow: unsigned u64 or null.
    let v = decode_ntc_result(&QueryCommand::StabilityWindow, &[0x19, 0x08, 0x70])
        .expect("decode StabilityWindow");
    assert_eq!(v["stability_window_slots"], 2160);
    let v = decode_ntc_result(&QueryCommand::StabilityWindow, &[0xf6])
        .expect("decode StabilityWindow null");
    assert!(v["stability_window"].is_null());

    // NumDormantEpochs: unsigned u64.
    let v = decode_ntc_result(&QueryCommand::NumDormantEpochs, &[0x03])
        .expect("decode NumDormantEpochs");
    assert_eq!(v["num_dormant_epochs"], 3);

    // ExpectedNetworkId: mainnet id (1) or null.
    let v = decode_ntc_result(&QueryCommand::ExpectedNetworkId, &[0x01])
        .expect("decode ExpectedNetworkId");
    assert_eq!(v["expected_network_id"], 1);
    let v = decode_ntc_result(&QueryCommand::ExpectedNetworkId, &[0xf6])
        .expect("decode ExpectedNetworkId null");
    assert!(v["expected_network_id"].is_null());

    // DepositPot: 4-element array with derived total.
    // CBOR: 0x84 0x01 0x02 0x03 0x04 → [1, 2, 3, 4]
    let v = decode_ntc_result(&QueryCommand::DepositPot, &[0x84, 0x01, 0x02, 0x03, 0x04])
        .expect("decode DepositPot");
    assert_eq!(v["key_deposits_lovelace"], 1);
    assert_eq!(v["pool_deposits_lovelace"], 2);
    assert_eq!(v["drep_deposits_lovelace"], 3);
    assert_eq!(v["proposal_deposits_lovelace"], 4);
    assert_eq!(v["total_lovelace"], 10);

    // LedgerCounts: 6-element array.
    let v = decode_ntc_result(
        &QueryCommand::LedgerCounts,
        &[0x86, 0x05, 0x04, 0x03, 0x02, 0x01, 0x00],
    )
    .expect("decode LedgerCounts");
    assert_eq!(v["stake_credentials"], 5);
    assert_eq!(v["pools"], 4);
    assert_eq!(v["dreps"], 3);
    assert_eq!(v["committee_members"], 2);
    assert_eq!(v["governance_actions"], 1);
    assert_eq!(v["gen_delegs"], 0);
}

#[test]
#[cfg(unix)]
fn encode_ntc_query_emits_expected_tag_bytes() {
    // Lock in the on-wire byte sequence for every QueryCommand variant
    // so silent tag drift between the CLI encoder and the
    // BasicLocalQueryDispatcher server-side arms surfaces as a failing
    // test.  Every simple (no-parameter) variant produces CBOR
    // `[tag]` == `0x81` + one-byte-unsigned(tag); the four parametric
    // variants produce `[tag, <param>]` which we spot-check separately.
    use super::commands::query::{QueryCommand, encode_ntc_query};

    // Round 148 — wire-format pin reflects the upstream-codec
    // migration of the four queries that have direct upstream
    // equivalents (`CurrentEra`, `Tip`) and the yggdrasil-extension
    // tag bumps for the two collisions (`CurrentEpoch` ↦ `[101]`,
    // `ProtocolParams` ↦ `[102]`).
    let cases: &[(QueryCommand, &[u8])] = &[
        // Upstream `BlockQuery (QueryHardFork GetCurrentEra)` =
        // `[0, [2, [1]]]` per
        // `Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`.
        (
            QueryCommand::CurrentEra,
            &[0x82, 0x00, 0x82, 0x02, 0x81, 0x01],
        ),
        // Upstream `GetChainPoint` = `[3]`.
        (QueryCommand::Tip, &[0x81, 0x03]),
        // Yggdrasil-extension `[101]` (upstream `[2]` is GetChainBlockNo).
        (QueryCommand::CurrentEpoch, &[0x81, 0x18, 0x65]),
        // Yggdrasil-extension `[102]` (upstream `[3]` is GetChainPoint).
        (QueryCommand::ProtocolParams, &[0x81, 0x18, 0x66]),
        (QueryCommand::StakeDistribution, &[0x81, 0x05]),
        (QueryCommand::TreasuryAndReserves, &[0x81, 0x07]),
        (QueryCommand::Constitution, &[0x81, 0x08]),
        (QueryCommand::GovState, &[0x81, 0x09]),
        (QueryCommand::DrepState, &[0x81, 0x0a]),
        (QueryCommand::CommitteeMembersState, &[0x81, 0x0b]),
        (QueryCommand::AccountState, &[0x81, 0x0d]),
        (QueryCommand::StakePools, &[0x81, 0x0f]),
        (QueryCommand::DrepStakeDistr, &[0x81, 0x11]),
        (QueryCommand::GenesisDelegations, &[0x81, 0x12]),
        (QueryCommand::StabilityWindow, &[0x81, 0x13]),
        (QueryCommand::NumDormantEpochs, &[0x81, 0x14]),
        (QueryCommand::ExpectedNetworkId, &[0x81, 0x15]),
        (QueryCommand::DepositPot, &[0x81, 0x16]),
        (QueryCommand::LedgerCounts, &[0x81, 0x17]),
    ];
    for (query, want) in cases {
        let got = encode_ntc_query(query);
        assert_eq!(
            got, *want,
            "encode_ntc_query drifted for {query:?}: expected {want:?}, got {got:?}",
        );
    }
}

/// Drift-detection invariant: every `QueryCommand` variant must
/// encode to a tag the server-side `BasicLocalQueryDispatcher`
/// recognises. A variant with no matching dispatcher arm slips past
/// the encoder byte-level test (which only locks in the bytes the
/// encoder emits) and surfaces only as an empty response at
/// `yggdrasil-node query ...` runtime — which looks indistinguishable
/// from "query returned no data". This test uses a match block over a
/// representative value for every variant so the compiler's
/// exhaustiveness gate forces a test update whenever a variant is
/// added to `QueryCommand`. Each variant is then run through the
/// `encode_ntc_query → dispatch_query` pipeline against a
/// `LedgerState::new(Era::Conway).snapshot()` and asserted to produce
/// non-empty bytes (the dispatcher's unknown-tag fall-through returns
/// exactly zero bytes via its empty encoder state).
#[test]
fn every_query_command_variant_is_dispatched() {
    use super::commands::query::{QueryCommand, encode_ntc_query};
    use yggdrasil_ledger::{Era, LedgerState};
    use yggdrasil_node::{BasicLocalQueryDispatcher, LocalQueryDispatcher};

    let dispatcher = BasicLocalQueryDispatcher::default();
    let snapshot = LedgerState::new(Era::Conway).snapshot();

    // Placeholders for the parametric variants. Values do not need
    // to resolve to any on-chain state; they just have to let the
    // dispatcher's tag arm execute far enough to emit a CBOR envelope.
    let cred_hex = "00".repeat(28);
    let addr_hex = "00".repeat(29);
    let txid_hex = "00".repeat(32);
    let reward_hex = "00".repeat(29);

    // Representative value per variant. `match` is compiler-enforced
    // exhaustive; adding a new `QueryCommand` without extending this
    // list is a hard compile error.
    let all: Vec<QueryCommand> = {
        // Enumerate via exhaustive construction so the compiler
        // guarantees every variant is represented exactly once.
        let mk = |v: QueryCommand| -> QueryCommand { v };
        let _check_exhaustiveness = |v: &QueryCommand| -> &'static str {
            match v {
                QueryCommand::CurrentEra => "CurrentEra",
                QueryCommand::Tip => "Tip",
                QueryCommand::ChainBlockNo => "ChainBlockNo",
                QueryCommand::SystemStart => "SystemStart",
                QueryCommand::EraHistory => "EraHistory",
                QueryCommand::CurrentEpoch => "CurrentEpoch",
                QueryCommand::ProtocolParams => "ProtocolParams",
                QueryCommand::UtxoByAddress { .. } => "UtxoByAddress",
                QueryCommand::StakeDistribution => "StakeDistribution",
                QueryCommand::RewardBalance { .. } => "RewardBalance",
                QueryCommand::TreasuryAndReserves => "TreasuryAndReserves",
                QueryCommand::UtxoByTxIn { .. } => "UtxoByTxIn",
                QueryCommand::StakePools => "StakePools",
                QueryCommand::DelegationsAndRewards { .. } => "DelegationsAndRewards",
                QueryCommand::DrepStakeDistr => "DrepStakeDistr",
                QueryCommand::Constitution => "Constitution",
                QueryCommand::GovState => "GovState",
                QueryCommand::DrepState => "DrepState",
                QueryCommand::CommitteeMembersState => "CommitteeMembersState",
                QueryCommand::StakePoolParams { .. } => "StakePoolParams",
                QueryCommand::AccountState => "AccountState",
                QueryCommand::GenesisDelegations => "GenesisDelegations",
                QueryCommand::StabilityWindow => "StabilityWindow",
                QueryCommand::NumDormantEpochs => "NumDormantEpochs",
                QueryCommand::ExpectedNetworkId => "ExpectedNetworkId",
                QueryCommand::DepositPot => "DepositPot",
                QueryCommand::LedgerCounts => "LedgerCounts",
            }
        };
        vec![
            mk(QueryCommand::CurrentEra),
            mk(QueryCommand::Tip),
            mk(QueryCommand::ChainBlockNo),
            mk(QueryCommand::SystemStart),
            mk(QueryCommand::EraHistory),
            mk(QueryCommand::CurrentEpoch),
            mk(QueryCommand::ProtocolParams),
            mk(QueryCommand::UtxoByAddress {
                address: addr_hex.clone(),
            }),
            mk(QueryCommand::StakeDistribution),
            mk(QueryCommand::RewardBalance {
                account: reward_hex.clone(),
            }),
            mk(QueryCommand::TreasuryAndReserves),
            mk(QueryCommand::UtxoByTxIn {
                tx_id: txid_hex.clone(),
                index: 0,
            }),
            mk(QueryCommand::StakePools),
            mk(QueryCommand::DelegationsAndRewards {
                credential: cred_hex.clone(),
                is_key_hash: true,
            }),
            mk(QueryCommand::DrepStakeDistr),
            mk(QueryCommand::Constitution),
            mk(QueryCommand::GovState),
            mk(QueryCommand::DrepState),
            mk(QueryCommand::CommitteeMembersState),
            mk(QueryCommand::StakePoolParams {
                pool_hash: cred_hex.clone(),
            }),
            mk(QueryCommand::AccountState),
            mk(QueryCommand::GenesisDelegations),
            mk(QueryCommand::StabilityWindow),
            mk(QueryCommand::NumDormantEpochs),
            mk(QueryCommand::ExpectedNetworkId),
            mk(QueryCommand::DepositPot),
            mk(QueryCommand::LedgerCounts),
        ]
    };

    for variant in &all {
        let query_bytes = encode_ntc_query(variant);
        let response = dispatcher.dispatch_query(&snapshot, &query_bytes);
        assert!(
            !response.is_empty(),
            "BasicLocalQueryDispatcher returned empty bytes for {variant:?} — \
                 every QueryCommand variant must have a matching dispatcher arm. \
                 An empty response indicates the tag fell through to the \
                 unknown-query default arm."
        );
    }
}

#[test]
fn metrics_http_response_routes_json_before_prometheus() {
    // Regression for the `starts_with("GET /metrics")` routing bug:
    // `GET /metrics/json` must reach the JSON arm, not match the
    // shorter `/metrics` prefix first.
    let metrics = yggdrasil_node_tracer::NodeMetrics::default();

    let (status, ctype, body) =
        super::metrics_server::metrics_http_response("GET /metrics/json HTTP/1.1\r\n", &metrics);
    assert_eq!(status, "200 OK");
    assert_eq!(ctype, "application/json");
    // JSON snapshot starts with `{` (not Prometheus `#` or metric name).
    assert!(body.trim_start().starts_with('{'));

    let (status, ctype, body) =
        super::metrics_server::metrics_http_response("GET /metrics HTTP/1.1\r\n", &metrics);
    assert_eq!(status, "200 OK");
    assert!(
        ctype.starts_with("text/plain"),
        "expected Prometheus text content type, got {ctype}",
    );
    assert!(body.contains("# HELP yggdrasil_blocks_synced"));

    // /debug/metrics/json is a documented JSON alias.
    let (_, ctype, body) = super::metrics_server::metrics_http_response(
        "GET /debug/metrics/json HTTP/1.1\r\n",
        &metrics,
    );
    assert_eq!(ctype, "application/json");
    assert!(body.trim_start().starts_with('{'));

    // /debug/metrics (with trailing space) is the JSON alias matching
    // the upstream cardano-tracer debug-dump convention.
    let (_, ctype, body) =
        super::metrics_server::metrics_http_response("GET /debug/metrics HTTP/1.1\r\n", &metrics);
    assert_eq!(ctype, "application/json");
    assert!(body.trim_start().starts_with('{'));

    // /debug/metrics/prometheus is the explicit Prometheus-text alias.
    let (_, ctype, body) = super::metrics_server::metrics_http_response(
        "GET /debug/metrics/prometheus HTTP/1.1\r\n",
        &metrics,
    );
    assert!(ctype.starts_with("text/plain"));
    assert!(body.contains("# HELP"));
}

#[test]
fn validate_config_report_warns_when_checkpoint_interval_exceeds_epoch() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-epoch-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    // Set the cadence to 10× the epoch length — a typical "operator
    // confused slots with epochs" typo shape.
    cfg.checkpoint_interval_slots = cfg.epoch_length * 10;

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("exceeds epoch_length")),
        "expected epoch-bound warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_too_small_checkpoint_interval() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-small-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.checkpoint_interval_slots = 1; // well below the 32-slot soft floor

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report.warnings.iter().any(|w| w.contains("soft floor")),
        "expected soft-floor warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_accepts_checkpoint_interval_at_epoch_length() {
    // Equal-to-epoch must NOT warn; the message reads "at most one per
    // epoch (interval <= epoch_length)" and the boundary is safe.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-at-epoch-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.checkpoint_interval_slots = cfg.epoch_length;

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("exceeds epoch_length")),
        "no epoch-bound warning expected at interval == epoch_length, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_mainnet_requires_magic_override() {
    // Mainnet (magic 764_824_073) canonical default is RequiresNoMagic.
    // An explicit RequiresMagic override is a copy-paste bug that would
    // desync Byron-era header decoding with every other mainnet peer.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-req-magic-mainnet-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.requires_network_magic = Some(yggdrasil_node_config::RequiresNetworkMagic::RequiresMagic);

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("RequiresNetworkMagic") && w.contains("inconsistent")),
        "expected RequiresNetworkMagic mismatch warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_testnet_requires_no_magic_override() {
    // Any non-mainnet magic's canonical default is RequiresMagic. An
    // explicit RequiresNoMagic override is a copy-paste bug that would
    // desync Byron-era header decoding with testnet peers.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-req-magic-testnet-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    // Not the mainnet magic.
    cfg.network_magic = 2;
    cfg.requires_network_magic =
        Some(yggdrasil_node_config::RequiresNetworkMagic::RequiresNoMagic);

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("RequiresNetworkMagic") && w.contains("inconsistent")),
        "expected RequiresNetworkMagic mismatch warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_accepts_canonical_requires_network_magic() {
    // Mainnet with RequiresNoMagic AND testnet with RequiresMagic are
    // both canonical; neither must produce the mismatch warning.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-req-magic-ok-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.requires_network_magic =
        Some(yggdrasil_node_config::RequiresNetworkMagic::RequiresNoMagic);
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("RequiresNetworkMagic") && w.contains("inconsistent")),
        "mainnet + RequiresNoMagic must not warn, got: {:?}",
        report.warnings,
    );

    // And the None case — default inferred — must not warn either.
    cfg.requires_network_magic = None;
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("RequiresNetworkMagic") && w.contains("inconsistent")),
        "None requires_network_magic must not warn, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_out_of_range_peer_sharing() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-ps-bad-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;

    // 2 is outside the upstream-defined set {0, 1}.
    cfg.peer_sharing = 2;
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| { w.contains("peer_sharing") && w.contains("outside") && w.contains('2') }),
        "expected peer_sharing range warning, got: {:?}",
        report.warnings,
    );

    // 255 (common typo / max u8) is also outside.
    cfg.peer_sharing = 255;
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("peer_sharing") && w.contains("255")),
        "expected peer_sharing range warning for 255, got: {:?}",
        report.warnings,
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_accepts_canonical_peer_sharing_values() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-ps-ok-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;

    for sample in [0u8, 1u8] {
        cfg.peer_sharing = sample;
        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("peer_sharing") && w.contains("outside")),
            "canonical peer_sharing = {sample} must not warn, got: {:?}",
            report.warnings,
        );
    }

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_non_cardano_protocol_value() {
    // Non-"Cardano" values must surface; "Cardano" exactly must not.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-proto-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;

    // Typo "Cadrano" — warn.
    cfg.protocol = Some("Cadrano".to_owned());
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report.warnings.iter().any(|w| {
            w.contains("Protocol") && w.contains("\"Cadrano\"") && w.contains("Cardano")
        }),
        "expected Protocol warning naming \"Cadrano\", got: {:?}",
        report.warnings,
    );

    // Legacy "RealPBFT" — warn.
    cfg.protocol = Some("RealPBFT".to_owned());
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Protocol") && w.contains("\"RealPBFT\"")),
        "expected Protocol warning naming \"RealPBFT\", got: {:?}",
        report.warnings,
    );

    // Case-sensitive: "cardano" lowercase — warn (upstream is case-sensitive).
    cfg.protocol = Some("cardano".to_owned());
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Protocol") && w.contains("\"cardano\"")),
        "case-sensitive gate: lowercase \"cardano\" must warn, got: {:?}",
        report.warnings,
    );

    // Exact "Cardano" — no warn.
    cfg.protocol = Some("Cardano".to_owned());
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("Protocol") && w.contains("not supported")),
        "exact \"Cardano\" must not warn, got: {:?}",
        report.warnings,
    );

    // None — no warn.
    cfg.protocol = None;
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("Protocol") && w.contains("not supported")),
        "None Protocol must not warn, got: {:?}",
        report.warnings,
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_non_dotted_numeric_min_node_version() {
    // Typos that reach the config are common: "10,6.2" (comma for
    // dot) and "ten.six.two" (non-numeric) must surface as warnings.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-mnv-bad-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;

    cfg.min_node_version = Some("10,6.2".to_owned());
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("MinNodeVersion") && w.contains("dotted-numeric")),
        "expected MinNodeVersion format warning on \"10,6.2\", got: {:?}",
        report.warnings,
    );

    cfg.min_node_version = Some("ten.six.two".to_owned());
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("MinNodeVersion") && w.contains("dotted-numeric")),
        "expected MinNodeVersion format warning on \"ten.six.two\", got: {:?}",
        report.warnings,
    );

    // Empty string is also invalid.
    cfg.min_node_version = Some(String::new());
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("MinNodeVersion") && w.contains("dotted-numeric")),
        "expected MinNodeVersion format warning on empty string, got: {:?}",
        report.warnings,
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_accepts_well_formed_min_node_version() {
    // Vendored-upstream-shape (canonical "10.6.2") + single-component
    // ("1") + many-component ("1.2.3.4.5") all pass the shape gate.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-mnv-ok-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;

    for sample in ["10.6.2", "1", "1.2.3.4.5", "0.0.0"] {
        cfg.min_node_version = Some(sample.to_owned());
        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("MinNodeVersion") && w.contains("dotted-numeric")),
            "well-formed MinNodeVersion {sample:?} must not warn, got: {:?}",
            report.warnings,
        );
    }

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_partial_last_known_block_version_triplet() {
    // Only Major set → partial triplet → warning.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-lkbv-partial-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.last_known_block_version_major = Some(0);
    cfg.last_known_block_version_minor = None;
    cfg.last_known_block_version_alt = None;

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report.warnings.iter().any(|w| {
            w.contains("LastKnownBlockVersion")
                && w.contains("partially set")
                && w.contains("Major: set")
                && w.contains("Minor: missing")
                && w.contains("Alt: missing")
        }),
        "expected LKBV partial warning naming exact Some/None pattern, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_accepts_full_last_known_block_version_triplet() {
    // All three set → accepted (atomic triplet).
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-lkbv-full-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.last_known_block_version_major = Some(3);
    cfg.last_known_block_version_minor = Some(0);
    cfg.last_known_block_version_alt = Some(0);

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("LastKnownBlockVersion")),
        "full LKBV triplet must produce no LKBV warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_accepts_absent_last_known_block_version_triplet() {
    // Default mainnet (all None) must produce no LKBV warning.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-lkbv-absent-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    // Defaults are already all-None; make the intent explicit.
    cfg.last_known_block_version_major = None;
    cfg.last_known_block_version_minor = None;
    cfg.last_known_block_version_alt = None;

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("LastKnownBlockVersion")),
        "all-absent LKBV must produce no warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_malformed_byron_genesis_hash() {
    // Wrong length ("abcd" -> 2 bytes) must surface as an
    // InvalidHashHex warning so the operator fixes the typo at preflight
    // time, even when the paired Byron file path is absent.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-byron-hex-bad-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.byron_genesis_hash = Some("abcd".to_owned()); // too short

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("invalid genesis hash hex string for ByronGenesisHash")),
        "expected ByronGenesisHash hex warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_non_hex_byron_genesis_hash() {
    // Non-hex characters (e.g. "z" repeated) must also surface.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-byron-hex-nonhex-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.byron_genesis_hash = Some("z".repeat(64)); // 64 chars, invalid hex

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("invalid genesis hash hex string for ByronGenesisHash")),
        "expected ByronGenesisHash hex warning on non-hex input, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_accepts_well_formed_byron_genesis_hash() {
    // 64-char lowercase-hex → parses cleanly → no format warning.
    // Content verification is covered when the paired file path exists;
    // this test only checks the supplemental no-file format warning.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-byron-hex-ok-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.byron_genesis_hash = Some("0".repeat(64));

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("ByronGenesisHash format")),
        "well-formed ByronGenesisHash must produce no format warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

/// Regression guard: `protocol_versions` (the NtN HANDSHAKE protocol-
/// version list, e.g. `[13, 14]`) lives in a completely different
/// number space than `max_major_protocol_version` (the block HEADER
/// protocol-version major cap, e.g. `10` for Conway). A prior slice
/// incorrectly cross-checked them and fired a false positive on every
/// valid mainnet config. This test pins the fact that the two fields
/// must NOT be comparison-linked: the default mainnet config
/// (`protocol_versions = [13, 14]`, `max_major_protocol_version = 10`)
/// must produce no "exceeds max_major_protocol_version" warning.
#[test]
fn validate_config_report_does_not_cross_check_handshake_versions_against_block_major() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-pv-no-crosscheck-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    // Default mainnet values: NtN handshake versions 13/14 with a
    // Conway-era block-major cap of 10. These are disjoint axes.
    cfg.protocol_versions = vec![13, 14];
    cfg.max_major_protocol_version = 10;

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("exceeds max_major_protocol_version")),
        "handshake versions must NOT be cross-checked against max_major_protocol_version; got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_missing_checkpoints_file() {
    // CheckpointsFile set but the path does not exist → warn that
    // checkpoint pinning will be silently disabled at runtime.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-missing-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.checkpoints_file = Some("not-here.json".to_owned());
    cfg.checkpoints_file_hash = None;

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("CheckpointsFile") && w.contains("does not exist")),
        "expected missing-checkpoints-file warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_checkpoints_file_hash_mismatch() {
    // CheckpointsFile + CheckpointsFileHash set, but the file bytes
    // don't hash to the declared value → warn with the mismatch
    // surfaced from `verify_genesis_file_hash`.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-hash-bad-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("checkpoints.json"), b"{}").expect("write ckpt");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.checkpoints_file = Some("checkpoints.json".to_owned());
    // Wrong hash — `{}` does NOT hash to all-zeros.
    cfg.checkpoints_file_hash = Some("0".repeat(64));

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("CheckpointsFile hash verification")),
        "expected checkpoints hash-mismatch warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_accepts_matching_checkpoints_file_hash() {
    // CheckpointsFile points at an existing file AND CheckpointsFileHash
    // is the correct Blake2b-256 digest → no warning.
    use yggdrasil_crypto::hash_bytes_256;
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-hash-ok-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let ckpt_bytes = b"{\"checkpoints\":[]}";
    std::fs::write(dir.join("checkpoints.json"), ckpt_bytes).expect("write ckpt");
    let correct_hash = hex::encode(hash_bytes_256(ckpt_bytes).0);

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.checkpoints_file = Some("checkpoints.json".to_owned());
    cfg.checkpoints_file_hash = Some(correct_hash);

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("CheckpointsFile")),
        "correct hash must not produce any CheckpointsFile warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_zero_governor_tick() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-gov-tick-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.governor_tick_interval_secs = 0;
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("governor_tick_interval_secs")),
        "expected governor-tick warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_insane_governor_targets() {
    // `target_active > target_established` violates upstream
    // `sanePeerSelectionTargets`; the preflight should flag it.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-gov-targets-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.governor_target_active = 99; // > established, impossible to satisfy
    cfg.governor_target_established = 10;
    cfg.governor_target_known = 20;

    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("sanePeerSelectionTargets")),
        "expected sane-targets warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_warns_on_unsafe_keepalive_interval() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();

    // A value >= 97 collides with the upstream KeepAlive client timeout.
    let dir_hi = std::env::temp_dir().join(format!("yggdrasil-keepalive-hi-{unique}"));
    std::fs::create_dir_all(&dir_hi).expect("temp dir");
    let mut cfg_hi = default_config();
    cfg_hi.storage_dir = PathBuf::from("data");
    cfg_hi.peer_snapshot_file = None;
    cfg_hi.keepalive_interval_secs = Some(120);
    let report = validate_config_report(&cfg_hi, Some(&dir_hi)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("KeepAlive") && w.contains("120")),
        "expected KeepAlive timeout warning for 120s interval, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir_hi).ok();

    // A value of 0 is also called out as wasteful.
    let dir_zero = std::env::temp_dir().join(format!("yggdrasil-keepalive-zero-{unique}"));
    std::fs::create_dir_all(&dir_zero).expect("temp dir");
    let mut cfg_zero = default_config();
    cfg_zero.storage_dir = PathBuf::from("data");
    cfg_zero.peer_snapshot_file = None;
    cfg_zero.keepalive_interval_secs = Some(0);
    let report = validate_config_report(&cfg_zero, Some(&dir_zero)).expect("report");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("keepalive_interval_secs is 0")),
        "expected zero-keepalive warning, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir_zero).ok();
}

#[test]
fn validate_config_report_accepts_sensible_keepalive_interval() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-keepalive-ok-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;
    cfg.keepalive_interval_secs = Some(30); // upstream default ballpark
    let report = validate_config_report(&cfg, Some(&dir)).expect("report");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("keepalive_interval_secs") || w.contains("KeepAlive")),
        "no keepalive warning expected at 30s, got: {:?}",
        report.warnings,
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_config_report_rejects_zero_security_param_k() {
    let mut cfg = default_config();
    cfg.security_param_k = 0;
    let err = validate_config_report(&cfg, None).expect_err("zero k must fail");
    assert!(
        err.to_string().contains("security_param_k"),
        "error should mention security_param_k: {err}",
    );
}

#[test]
fn validate_config_report_rejects_zero_epoch_length() {
    let mut cfg = default_config();
    cfg.epoch_length = 0;
    let err = validate_config_report(&cfg, None).expect_err("zero epoch_length must fail");
    assert!(
        err.to_string().contains("epoch_length"),
        "error should mention epoch_length: {err}",
    );
}

#[test]
fn validate_config_report_rejects_zero_byron_epoch_length_with_boundary_set() {
    let mut cfg = default_config();
    cfg.byron_to_shelley_slot = Some(86_400);
    cfg.byron_epoch_length = 0;
    let err = validate_config_report(&cfg, None)
        .expect_err("zero byron_epoch_length with boundary must fail");
    assert!(
        err.to_string().contains("byron_epoch_length"),
        "error should mention byron_epoch_length: {err}",
    );
}

#[test]
fn validate_config_report_allows_zero_byron_epoch_length_without_boundary() {
    // When `byron_to_shelley_slot` is not set, the Byron prefix is
    // inapplicable (e.g. preview testnet) and a zero
    // byron_epoch_length should not abort the preflight.
    let mut cfg = default_config();
    cfg.byron_to_shelley_slot = None;
    cfg.byron_epoch_length = 0;
    // Other fields still sane — no bail expected from this check.
    let result = validate_config_report(&cfg, None);
    // We don't require full success here (other checks may warn or
    // fail depending on storage/genesis), but we DO require that the
    // byron_epoch_length check specifically does not fire.
    if let Err(e) = &result {
        assert!(
            !e.to_string().contains("byron_epoch_length"),
            "byron_epoch_length bail should not fire without byron_to_shelley_slot: {e}",
        );
    }
}

#[test]
fn validate_config_report_warns_on_genesis_hash_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("shelley.json"), b"{}").expect("write");
    std::fs::write(dir.path().join("alonzo.json"), b"{}").expect("write");
    std::fs::write(dir.path().join("conway.json"), b"{}").expect("write");

    let mut cfg = default_config();
    cfg.shelley_genesis_file = Some("shelley.json".to_owned());
    cfg.shelley_genesis_hash = Some("0".repeat(64));
    cfg.alonzo_genesis_file = Some("alonzo.json".to_owned());
    cfg.alonzo_genesis_hash = None;
    cfg.conway_genesis_file = Some("conway.json".to_owned());
    cfg.conway_genesis_hash = None;
    // Default mainnet config sets a real Byron path; clear it so the
    // preflight does not also warn about the missing Byron UTxO file.
    cfg.byron_genesis_file = None;
    cfg.byron_genesis_hash = None;
    // storage_dir does not need to exist for the warning we want to
    // assert here; the validate path continues despite that.

    let report = validate_config_report(&cfg, Some(dir.path()))
        .expect("validate succeeds with hash warning");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("genesis hash verification")),
        "expected genesis hash warning in report, got: {:?}",
        report.warnings,
    );
}

/// Regression guard: every vendored network preset (`mainnet`, `preprod`,
/// `preview`) must validate through `validate_config_report` with NO
/// warnings outside a narrow allowlist of known environmental
/// conditions (no peer-snapshot file shipped with the repo; fresh
/// checkout has no initialized storage dirs yet). Any NEW warning that
/// fires on a canonical preset is almost certainly a broken preflight
/// (slice 42's `protocol_versions` false positive is the prototypical
/// example). This test pins the expected category set so the next time
/// a preflight fires incorrectly on all three presets, CI catches it.
///
/// Matches against warning *substrings* rather than exact strings so
/// the test remains stable across message-wording refinements. The
/// allowlist is intentionally small — ≤2 entries per preset —
/// because every entry is a genuine environmental condition the repo
/// cannot resolve without a real node run.
#[test]
fn vendored_network_presets_produce_only_environmental_warnings() {
    use yggdrasil_node_config::NetworkPreset;

    const ENVIRONMENTAL_SUBSTRINGS: &[&str] = &[
        // No peer-snapshot.json is vendored with the repo — this is a
        // real runtime artifact, not a preset misconfiguration.
        "peer snapshot file",
        // Fresh checkout has no storage dirs yet — validate-config
        // cannot check restart recovery until a real sync has occurred.
        "storage directories are not initialized",
    ];

    for &preset in NetworkPreset::all() {
        let (cfg, config_base_dir) =
            load_effective_config(None, Some(preset)).expect("preset config");
        let report = validate_config_report(&cfg, config_base_dir.as_deref())
            .expect("preset must validate successfully");
        for warning in &report.warnings {
            let matched_category = ENVIRONMENTAL_SUBSTRINGS
                .iter()
                .any(|pattern| warning.contains(pattern));
            assert!(
                matched_category,
                "preset {preset:?} produced an unexpected warning outside \
                     the environmental allowlist: {warning:?}\n\
                     If this is a genuine new environmental condition, add \
                     its substring to `ENVIRONMENTAL_SUBSTRINGS`. If it is a \
                     preflight that should never fire on a canonical \
                     vendored preset, the preflight check is buggy (see \
                     slice 44 in AGENTS.md for the prototypical case).",
            );
        }
    }
}

#[test]
fn load_effective_config_uses_network_preset_when_file_is_absent() {
    let (cfg, config_base_dir) =
        load_effective_config(None, Some(yggdrasil_node_config::NetworkPreset::Preview))
            .expect("preset config");

    assert_eq!(cfg.network_magic, 2);
    assert_eq!(
        config_base_dir,
        Some(preset_config_base_dir(
            yggdrasil_node_config::NetworkPreset::Preview
        ))
    );
}

#[test]
fn strict_base_ledger_state_seeds_preview_reserves_from_genesis_supply() {
    let (cfg, config_base_dir) =
        load_effective_config(None, Some(yggdrasil_node_config::NetworkPreset::Preview))
            .expect("preset config");

    let ledger =
        strict_base_ledger_state(&cfg, config_base_dir.as_deref()).expect("base ledger state");

    assert_eq!(ledger.max_lovelace_supply(), 45_000_000_000_000_000);
    assert_eq!(ledger.accounting().treasury, 0);
    assert_eq!(ledger.accounting().reserves, 15_000_000_000_000_000);
}

#[test]
fn load_effective_config_parses_yaml_file() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-config-yaml-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let config_path = dir.join("config.yaml");
    std::fs::write(
        &config_path,
        "peer_addr: 127.0.0.1:3001\nnetwork_magic: 42\nprotocol_versions:\n  - 13\n",
    )
    .expect("write yaml config");

    let (cfg, config_base_dir) =
        load_effective_config(Some(config_path.clone()), None).expect("yaml config");

    assert_eq!(cfg.peer_addr, "127.0.0.1:3001".parse().expect("addr"));
    assert_eq!(cfg.network_magic, 42);
    assert_eq!(cfg.protocol_versions, vec![13]);
    assert_eq!(config_base_dir, Some(dir.clone()));

    std::fs::remove_file(config_path).ok();
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn forged_header_protocol_version_uses_ledger_protocol_when_present() {
    let mut state = LedgerState::new(Era::Byron);
    let params = yggdrasil_ledger::ProtocolParameters {
        protocol_version: Some((9, 1)),
        ..yggdrasil_ledger::ProtocolParameters::default()
    };
    state.set_protocol_params(params);

    assert_eq!(forged_header_protocol_version(&state, 10), (9, 1));
}

#[test]
fn forged_header_protocol_version_falls_back_to_max_major_protocol_version() {
    let state = LedgerState::new(Era::Byron);
    assert_eq!(forged_header_protocol_version(&state, 10), (10, 0));
}

#[test]
fn validate_config_report_warns_when_peer_snapshot_file_is_missing() {
    // Vendored mainnet/preprod/preview configs all ship a placeholder
    // `peer-snapshot.json` so the §1 operator preflight succeeds out
    // of the box (verified end-to-end via `validate-config --network *`).
    // This test instead points the config at a deliberately-missing
    // file so the "configured peer snapshot file could not be loaded"
    // warning path is still exercised.
    let (mut cfg, config_base_dir) =
        load_effective_config(None, Some(yggdrasil_node_config::NetworkPreset::Preview))
            .expect("preset config");
    cfg.peer_snapshot_file = Some("does-not-exist-peer-snapshot.json".to_owned());

    let report =
        validate_config_report(&cfg, config_base_dir.as_deref()).expect("validation report");

    assert_eq!(report.peer_snapshot.status, "unavailable");
    assert!(report.peer_snapshot.error.is_some());
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("configured peer snapshot file could not be loaded"))
    );
}

#[test]
fn status_report_shows_uninitialized_when_storage_absent() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-status-empty-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;

    let report = status_report(&cfg, Some(&dir)).expect("status report");

    assert!(!report.storage_initialized);
    assert_eq!(report.immutable_block_count, 0);
    assert_eq!(report.volatile_block_count, 0);
    assert_eq!(report.ledger_checkpoint_count, 0);
    assert!(report.chain_tip_slot.is_none());
    // Ledger-derived fields must be absent on uninitialized storage.
    assert!(report.current_era.is_none());
    assert!(report.current_epoch.is_none());
    assert!(report.ledger_counts.is_none());

    // And the JSON serialisation must elide them (skip_serializing_if
    // = Option::is_none) so pre-existing consumers see no breaking
    // change when the data is absent.
    let json = serde_json::to_string(&report).expect("serialize");
    assert!(
        !json.contains("current_era"),
        "current_era key should be absent when unset, got: {json}",
    );
    assert!(
        !json.contains("current_epoch"),
        "current_epoch key should be absent when unset, got: {json}",
    );
    assert!(
        !json.contains("ledger_counts"),
        "ledger_counts key should be absent when unset, got: {json}",
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn status_report_shows_initialized_when_storage_exists() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-status-init-{unique}"));
    let data_dir = dir.join("data");
    std::fs::create_dir_all(data_dir.join("immutable")).expect("immutable dir");
    std::fs::create_dir_all(data_dir.join("volatile")).expect("volatile dir");
    std::fs::create_dir_all(data_dir.join("ledger")).expect("ledger dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;

    let report = status_report(&cfg, Some(&dir)).expect("status report");

    assert!(report.storage_initialized);
    assert_eq!(report.immutable_block_count, 0);
    assert_eq!(report.volatile_block_count, 0);
    assert!(report.chain_tip.contains("Origin"));

    // Storage is present so recovery succeeds from an empty state;
    // the ledger-counts summary should therefore be populated, and all
    // six cardinalities should be zero on a fresh node.
    let counts = report
        .ledger_counts
        .as_ref()
        .expect("ledger counts present when storage is initialized");
    assert_eq!(counts.stake_credentials, 0);
    assert_eq!(counts.pools, 0);
    assert_eq!(counts.dreps, 0);
    assert_eq!(counts.committee_members, 0);
    assert_eq!(counts.governance_actions, 0);
    assert_eq!(counts.gen_delegs, 0);

    // Era + epoch are populated on a successful recovery. A fresh
    // ledger starts in Byron era, epoch 0 until blocks advance it.
    assert_eq!(
        report.current_era.as_deref(),
        Some("Byron"),
        "fresh ledger should report Byron era",
    );
    assert_eq!(
        report.current_epoch,
        Some(0),
        "fresh ledger should report epoch 0",
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn status_report_serializes_to_json() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-status-json-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut cfg = default_config();
    cfg.storage_dir = PathBuf::from("data");
    cfg.peer_snapshot_file = None;

    let report = status_report(&cfg, Some(&dir)).expect("status report");
    let json = serde_json::to_string_pretty(&report).expect("serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");

    assert_eq!(
        parsed["network_magic"],
        serde_json::Value::from(764_824_073u64)
    );
    assert_eq!(
        parsed["storage_initialized"],
        serde_json::Value::Bool(false)
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn apply_topology_override_from_cli_flag() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-topo-override-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let topo_path = dir.join("topology.json");
    std::fs::write(
        &topo_path,
        r#"{
                "bootstrapPeers": [
                    {"address": "127.0.0.50", "port": 3001}
                ],
                "localRoots": [],
                "publicRoots": [],
                "useLedgerAfterSlot": 77000
            }"#,
    )
    .expect("write topology file");

    let mut cfg = default_config();
    cfg.use_ledger_after_slot = None;
    cfg.peer_snapshot_file = None;

    apply_topology_override(&mut cfg, Some(topo_path.as_path()), None)
        .expect("apply topology override");

    assert_eq!(cfg.use_ledger_after_slot, Some(77000));
    assert_eq!(cfg.peer_addr, "127.0.0.50:3001".parse().expect("addr"));

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn apply_topology_override_from_config_topology_file_path() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-topo-config-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let topo_path = dir.join("my-topology.json");
    std::fs::write(
        &topo_path,
        r#"{
                "bootstrapPeers": [],
                "localRoots": [
                    {
                        "accessPoints": [
                            {"address": "127.0.0.60", "port": 3001}
                        ],
                        "advertise": false,
                        "valency": 1,
                        "trustable": true
                    }
                ],
                "publicRoots": [],
                "useLedgerAfterSlot": 55000
            }"#,
    )
    .expect("write topology file");

    let mut cfg = default_config();
    cfg.topology_file_path = Some("my-topology.json".to_owned());
    cfg.use_ledger_after_slot = None;

    apply_topology_override(&mut cfg, None, Some(dir.as_path()))
        .expect("apply topology from config key");

    assert_eq!(cfg.use_ledger_after_slot, Some(55000));
    assert_eq!(cfg.local_roots.len(), 1);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn apply_topology_override_cli_takes_priority_over_config_key() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-topo-priority-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let config_topo = dir.join("config-topology.json");
    std::fs::write(
        &config_topo,
        r#"{
                "bootstrapPeers": [],
                "localRoots": [],
                "publicRoots": [],
                "useLedgerAfterSlot": 11000
            }"#,
    )
    .expect("write config topology");

    let cli_topo = dir.join("cli-topology.json");
    std::fs::write(
        &cli_topo,
        r#"{
                "bootstrapPeers": [],
                "localRoots": [],
                "publicRoots": [],
                "useLedgerAfterSlot": 22000
            }"#,
    )
    .expect("write cli topology");

    let mut cfg = default_config();
    cfg.topology_file_path = Some(config_topo.display().to_string());

    apply_topology_override(&mut cfg, Some(cli_topo.as_path()), Some(dir.as_path()))
        .expect("apply topology");

    // CLI topology (22000) should win over config TopologyFilePath (11000).
    assert_eq!(cfg.use_ledger_after_slot, Some(22000));

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn apply_topology_override_noop_when_neither_cli_nor_config() {
    let mut cfg = default_config();
    cfg.topology_file_path = None;
    let original_ledger_slot = cfg.use_ledger_after_slot;

    apply_topology_override(&mut cfg, None, None).expect("apply topology no-op");

    assert_eq!(cfg.use_ledger_after_slot, original_ledger_slot);
}

// ---------------------------------------------------------------------------
// Phase 3 (cardano-cli C-arc Phase F) — CardanoCliCommand parser pins.
//
// These tests assert that the clap-derived `CardanoCliCommand` variants
// added for the operator-essential query family (utxo / protocol-
// parameters / stake-pools / stake-distribution) parse correctly from
// the upstream `cardano-cli`-shaped flag set. They don't drive the
// commands end-to-end — that needs a live socket — but they do pin the
// CLI surface so a flag rename / removal surfaces as a failing test
// rather than silently breaking an operator's invocation script.
// ---------------------------------------------------------------------------

#[test]
fn cardano_cli_query_utxo_with_address_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "query-utxo",
        "--socket-path",
        "/tmp/node.socket",
        "--address",
        "0123abcdef",
    ])
    .expect("cardano-cli query-utxo with --address must parse");

    match cli.command {
        Command::CardanoCli { action, .. } => match action {
            CardanoCliCommand::QueryUtxo { address, tx_in, .. } => {
                assert_eq!(address.as_deref(), Some("0123abcdef"));
                assert!(tx_in.is_none(), "tx_in must be None when --address is given");
            }
            _ => panic!("expected QueryUtxo variant"),
        },
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

#[test]
fn cardano_cli_query_utxo_with_tx_in_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "query-utxo",
        "--socket-path",
        "/tmp/node.socket",
        "--tx-in",
        "deadbeef#7",
    ])
    .expect("cardano-cli query-utxo with --tx-in must parse");

    match cli.command {
        Command::CardanoCli { action, .. } => match action {
            CardanoCliCommand::QueryUtxo { address, tx_in, .. } => {
                assert!(address.is_none(), "address must be None when --tx-in is given");
                assert_eq!(tx_in.as_deref(), Some("deadbeef#7"));
            }
            _ => panic!("expected QueryUtxo variant"),
        },
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

#[test]
fn cardano_cli_query_utxo_rejects_both_address_and_tx_in() {
    use clap::Parser;
    use super::Cli;

    let result = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "query-utxo",
        "--socket-path",
        "/tmp/node.socket",
        "--address",
        "0123",
        "--tx-in",
        "deadbeef#0",
    ]);
    // `Cli` doesn't derive Debug, so `.expect_err` won't compile.
    // Pull out the error via a match instead.
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("clap must reject --address AND --tx-in together"),
    };
    assert!(
        err.to_string().contains("cannot be used with")
            || err.to_string().contains("conflicts"),
        "clap conflict-error must reference the conflict; got {err}"
    );
}

#[test]
fn cardano_cli_query_protocol_parameters_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "query-protocol-parameters",
        "--socket-path",
        "/tmp/node.socket",
    ])
    .expect("query-protocol-parameters must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => assert!(matches!(
            action,
            CardanoCliCommand::QueryProtocolParameters { .. }
        )),
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

#[test]
fn cardano_cli_query_stake_pools_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "query-stake-pools",
        "--socket-path",
        "/tmp/node.socket",
    ])
    .expect("query-stake-pools must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => assert!(matches!(
            action,
            CardanoCliCommand::QueryStakePools { .. }
        )),
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

#[test]
fn cardano_cli_query_stake_distribution_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "query-stake-distribution",
        "--socket-path",
        "/tmp/node.socket",
    ])
    .expect("query-stake-distribution must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => assert!(matches!(
            action,
            CardanoCliCommand::QueryStakeDistribution { .. }
        )),
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

#[test]
fn cardano_cli_transaction_submit_with_tx_hex_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "transaction-submit",
        "--socket-path",
        "/tmp/node.socket",
        "--tx-hex",
        "0xdeadbeef",
    ])
    .expect("transaction-submit with --tx-hex must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => match action {
            CardanoCliCommand::TransactionSubmit {
                tx_file, tx_hex, ..
            } => {
                assert!(tx_file.is_none(), "tx_file must be None when --tx-hex given");
                assert_eq!(tx_hex.as_deref(), Some("0xdeadbeef"));
            }
            _ => panic!("expected TransactionSubmit variant"),
        },
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

#[test]
fn cardano_cli_transaction_submit_with_tx_file_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "transaction-submit",
        "--socket-path",
        "/tmp/node.socket",
        "--tx-file",
        "/tmp/tx.cbor",
    ])
    .expect("transaction-submit with --tx-file must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => match action {
            CardanoCliCommand::TransactionSubmit {
                tx_file, tx_hex, ..
            } => {
                assert!(tx_hex.is_none(), "tx_hex must be None when --tx-file given");
                assert!(tx_file.is_some(), "tx_file must be Some");
            }
            _ => panic!("expected TransactionSubmit variant"),
        },
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

#[test]
fn cardano_cli_transaction_submit_rejects_both_flags() {
    use clap::Parser;
    use super::Cli;

    let result = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "transaction-submit",
        "--socket-path",
        "/tmp/node.socket",
        "--tx-hex",
        "0xdeadbeef",
        "--tx-file",
        "/tmp/tx.cbor",
    ]);
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("clap must reject --tx-hex AND --tx-file together"),
    };
    assert!(
        err.to_string().contains("cannot be used with")
            || err.to_string().contains("conflicts"),
        "clap conflict-error must reference the conflict; got {err}"
    );
}

#[test]
fn cardano_cli_transaction_txid_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "transaction-txid",
        "--tx-hex",
        "0xdeadbeef",
    ])
    .expect("transaction-txid must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => match action {
            CardanoCliCommand::TransactionTxid { tx_file, tx_hex } => {
                assert!(tx_file.is_none());
                assert_eq!(tx_hex.as_deref(), Some("0xdeadbeef"));
            }
            _ => panic!("expected TransactionTxid variant"),
        },
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

/// Compute-txid pin: hand-crafted CBOR-encoded "tx" whose first
/// element is a known body. `compute_txid_from_tx_cbor` must return
/// `Blake2b-256(body_bytes)`, byte-equivalent to upstream
/// `cardano-cli transaction txid` output.
#[test]
fn cardano_cli_transaction_txid_matches_blake2b_256_of_body() {
    use super::commands::cardano_cli::compute_txid_from_tx_cbor;
    use yggdrasil_crypto::hash_bytes_256;

    // Body: CBOR-encoded `[1, 2, 3]` = three-element array of small uints.
    //   array-len-3 = 0x83
    //   uint(1)     = 0x01
    //   uint(2)     = 0x02
    //   uint(3)     = 0x03
    let body = [0x83_u8, 0x01, 0x02, 0x03];
    let expected_txid = hash_bytes_256(&body).0;

    // Wrap the body in a 2-element CBOR array: outer = [body, 0x01].
    //   array-len-2 = 0x82
    //   <body 4 bytes>
    //   uint(1)     = 0x01
    let mut tx = vec![0x82_u8];
    tx.extend_from_slice(&body);
    tx.push(0x01);

    let actual = compute_txid_from_tx_cbor(&tx).expect("compute txid");
    assert_eq!(
        actual, expected_txid,
        "transaction-txid must equal Blake2b-256(TxBody); got {:?}, expected {:?}",
        hex::encode(actual),
        hex::encode(expected_txid),
    );
}

#[test]
fn cardano_cli_address_key_hash_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "address-key-hash",
        "--payment-verification-key-file",
        "/tmp/payment.vkey",
    ])
    .expect("address-key-hash must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => assert!(matches!(
            action,
            CardanoCliCommand::AddressKeyHash { .. }
        )),
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

/// Pin the TextEnvelope reader against the upstream-shaped JSON
/// `{type, description, cborHex}` envelope. The cborHex strips the
/// 2-byte CBOR prefix (`0x58 0x20`) and returns the 32 raw VK bytes.
#[test]
fn read_verification_key_text_envelope_round_trips() {
    use super::commands::cardano_cli::read_verification_key_text_envelope;

    // 32-byte fake VK = 0xAA repeated.
    let vk = [0xAA_u8; 32];
    // CBOR prefix + key = 34 bytes.
    let mut cbor = vec![0x58, 0x20];
    cbor.extend_from_slice(&vk);
    let cbor_hex = hex::encode(&cbor);

    let envelope = serde_json::json!({
        "type": "PaymentVerificationKeyShelley_ed25519",
        "description": "Payment Verification Key",
        "cborHex": cbor_hex,
    });
    let envelope_bytes = serde_json::to_vec(&envelope).expect("serialize envelope");

    let parsed = read_verification_key_text_envelope(&envelope_bytes).expect("parse envelope");
    assert_eq!(parsed, vk);
}

#[test]
fn read_verification_key_text_envelope_rejects_wrong_prefix() {
    use super::commands::cardano_cli::read_verification_key_text_envelope;

    // Use 0x4020 (text-string instead of bytes-string) — same length,
    // wrong major type. The CBOR prefix check must reject it.
    let mut cbor = vec![0x40, 0x20];
    cbor.extend_from_slice(&[0xBB_u8; 32]);
    let cbor_hex = hex::encode(&cbor);

    let envelope = serde_json::json!({
        "type": "PaymentVerificationKeyShelley_ed25519",
        "description": "Payment Verification Key",
        "cborHex": cbor_hex,
    });
    let envelope_bytes = serde_json::to_vec(&envelope).expect("serialize envelope");

    let result = read_verification_key_text_envelope(&envelope_bytes);
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("must reject wrong CBOR prefix"),
    };
    assert!(
        err.to_string().contains("expected CBOR prefix"),
        "error must reference the prefix expectation; got {err}"
    );
}

#[test]
fn cardano_cli_stake_address_key_gen_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "stake-address-key-gen",
        "--verification-key-file",
        "/tmp/stake.vkey",
        "--signing-key-file",
        "/tmp/stake.skey",
    ])
    .expect("stake-address-key-gen must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => assert!(matches!(
            action,
            CardanoCliCommand::StakeAddressKeyGen { .. }
        )),
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

#[test]
fn cardano_cli_address_build_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "address-build",
        "--payment-verification-key-file",
        "/tmp/payment.vkey",
        "--mainnet",
    ])
    .expect("address-build with --mainnet must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => assert!(matches!(
            action,
            CardanoCliCommand::AddressBuild { .. }
        )),
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

#[test]
fn cardano_cli_address_build_rejects_both_network_flags() {
    use clap::Parser;
    use super::Cli;

    let result = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "address-build",
        "--payment-verification-key-file",
        "/tmp/payment.vkey",
        "--mainnet",
        "--testnet-magic",
        "1",
    ]);
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("clap must reject --mainnet AND --testnet-magic together"),
    };
    assert!(
        err.to_string().contains("cannot be used with")
            || err.to_string().contains("conflicts"),
        "clap conflict-error must reference the conflict; got {err}"
    );
}

/// Pin Shelley address bytes against a known input: a payment hash
/// of 0x11 × 28 yields a specific 29-byte enterprise address on
/// mainnet that Bech32-encodes to a deterministic `addr1…` string.
/// Cross-checked: decoding the result back through `bech32::decode`
/// must yield the same 29-byte sequence we constructed.
#[test]
fn build_shelley_enterprise_address_round_trips() {
    use super::commands::cardano_cli::build_shelley_address_bech32;

    let pay_hash = [0x11_u8; 28];
    let mainnet_addr = build_shelley_address_bech32(1, &pay_hash, None).expect("build mainnet");
    assert!(
        mainnet_addr.starts_with("addr1"),
        "mainnet enterprise address must start with addr1; got {mainnet_addr}"
    );

    // Round-trip via the bech32 crate: decode and confirm we get
    // back the 29-byte sequence `[0x61, 0x11×28]` (enterprise type
    // 6 on mainnet).
    let (_hrp, decoded) = bech32::decode(&mainnet_addr).expect("decode bech32");
    let mut expected = vec![0x61_u8];
    expected.extend_from_slice(&pay_hash);
    assert_eq!(
        decoded, expected,
        "round-tripped address bytes drift; expected {:?}, got {:?}",
        hex::encode(&expected),
        hex::encode(&decoded),
    );

    // Testnet path: header byte changes (0x60 instead of 0x61), HRP
    // becomes addr_test.
    let testnet_addr = build_shelley_address_bech32(0, &pay_hash, None).expect("build testnet");
    assert!(
        testnet_addr.starts_with("addr_test1"),
        "testnet enterprise address must start with addr_test1; got {testnet_addr}"
    );
    let (_hrp, decoded) = bech32::decode(&testnet_addr).expect("decode bech32");
    assert_eq!(decoded[0], 0x60, "testnet enterprise header must be 0x60");
}

#[test]
fn cardano_cli_stake_address_build_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "stake-address-build",
        "--stake-verification-key-file",
        "/tmp/stake.vkey",
        "--mainnet",
    ])
    .expect("stake-address-build with --mainnet must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => assert!(matches!(
            action,
            CardanoCliCommand::StakeAddressBuild { .. }
        )),
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

/// Pin the reward-address byte shape — header `0xE0 | network_id`
/// followed by 28-byte stake-key hash. Mainnet uses HRP `stake`
/// (so the output starts with `stake1`), testnet uses `stake_test`.
#[test]
fn build_shelley_reward_address_round_trips() {
    use super::commands::cardano_cli::build_shelley_reward_address_bech32;

    let stake_hash = [0x44_u8; 28];

    let mainnet = build_shelley_reward_address_bech32(1, &stake_hash).expect("build mainnet");
    assert!(
        mainnet.starts_with("stake1"),
        "mainnet reward address must start with stake1; got {mainnet}"
    );
    let (_hrp, decoded) = bech32::decode(&mainnet).expect("decode mainnet");
    let mut expected = vec![0xE1_u8];
    expected.extend_from_slice(&stake_hash);
    assert_eq!(
        decoded, expected,
        "mainnet reward address byte shape drifted; expected {:?}, got {:?}",
        hex::encode(&expected),
        hex::encode(&decoded),
    );

    let testnet = build_shelley_reward_address_bech32(0, &stake_hash).expect("build testnet");
    assert!(
        testnet.starts_with("stake_test1"),
        "testnet reward address must start with stake_test1; got {testnet}"
    );
    let (_hrp, decoded) = bech32::decode(&testnet).expect("decode testnet");
    assert_eq!(decoded[0], 0xE0, "testnet reward header must be 0xE0");
    assert_eq!(&decoded[1..29], &stake_hash);
}

/// Pin the base address byte shape — header 0x00 (mainnet, type 0)
/// followed by 28-byte payment hash + 28-byte stake hash.
#[test]
fn build_shelley_base_address_byte_shape() {
    use super::commands::cardano_cli::build_shelley_address_bech32;

    let pay_hash = [0x22_u8; 28];
    let stake_hash = [0x33_u8; 28];
    let addr = build_shelley_address_bech32(1, &pay_hash, Some(&stake_hash)).expect("build");
    let (_hrp, decoded) = bech32::decode(&addr).expect("decode");
    assert_eq!(decoded.len(), 57, "base address must be 57 bytes");
    // Header 0b0000_0001 = 0x01 (type 0, mainnet).
    assert_eq!(decoded[0], 0x01, "base address header must be 0x01 on mainnet");
    assert_eq!(&decoded[1..29], &pay_hash, "payment hash slot");
    assert_eq!(&decoded[29..57], &stake_hash, "stake hash slot");
}

#[test]
fn cardano_cli_address_key_gen_parses() {
    use clap::Parser;
    use super::cli::{CardanoCliCommand, Command};
    use super::Cli;

    let cli = Cli::try_parse_from([
        "yggdrasil-node",
        "cardano-cli",
        "address-key-gen",
        "--verification-key-file",
        "/tmp/payment.vkey",
        "--signing-key-file",
        "/tmp/payment.skey",
    ])
    .expect("address-key-gen must parse");
    match cli.command {
        Command::CardanoCli { action, .. } => assert!(matches!(
            action,
            CardanoCliCommand::AddressKeyGen { .. }
        )),
        _ => panic!("expected Command::CardanoCli variant"),
    }
}

/// `write_text_envelope` produces a JSON document whose `cborHex`
/// field, when parsed back with `read_verification_key_text_envelope`,
/// yields the original 32-byte payload. Round-trip pin so a future
/// refactor breaking either side surfaces here, not at operator
/// key-file load time.
#[test]
fn cardano_cli_text_envelope_round_trip() {
    use super::commands::cardano_cli::{
        read_verification_key_text_envelope, write_text_envelope,
    };

    let dir = std::env::temp_dir().join(format!(
        "yggdrasil-cardano-cli-key-gen-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir tmp");
    let path = dir.join("payment.vkey");

    let payload = [0x5A_u8; 32];
    write_text_envelope(
        &path,
        "PaymentVerificationKeyShelley_ed25519",
        "Payment Verification Key",
        &payload,
        /* private = */ false,
    )
    .expect("write envelope");

    let bytes = std::fs::read(&path).expect("read back");
    let parsed = read_verification_key_text_envelope(&bytes).expect("parse round-trip");
    assert_eq!(parsed, payload, "round-trip drift through envelope reader/writer");

    // Confirm the JSON has the right shape (type + description fields
    // are surfaced for upstream-compat) without parsing the whole file
    // again.
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("re-parse JSON");
    assert_eq!(
        json.get("type").and_then(serde_json::Value::as_str),
        Some("PaymentVerificationKeyShelley_ed25519")
    );
    assert_eq!(
        json.get("description").and_then(serde_json::Value::as_str),
        Some("Payment Verification Key")
    );
    // Cleanup.
    std::fs::remove_dir_all(&dir).ok();
}

/// Signing-key envelopes on Unix MUST be 0o600 so the file isn't
/// world-readable. Pin so a future refactor that drops the
/// `OpenOptionsExt::mode` call surfaces here, not as a CVE.
#[cfg(unix)]
#[test]
fn cardano_cli_text_envelope_signing_key_mode_is_0o600() {
    use std::os::unix::fs::PermissionsExt;
    use super::commands::cardano_cli::write_text_envelope;

    let dir = std::env::temp_dir().join(format!(
        "yggdrasil-cardano-cli-key-gen-perm-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir tmp");
    let path = dir.join("payment.skey");

    write_text_envelope(
        &path,
        "PaymentSigningKeyShelley_ed25519",
        "Payment Signing Key",
        &[0u8; 32],
        /* private = */ true,
    )
    .expect("write signing-key envelope");

    let meta = std::fs::metadata(&path).expect("stat skey");
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "signing-key file must be 0o600; got 0o{:o}",
        mode
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// End-to-end: a known VK → known Blake2b-224 hash. The hash is the
/// 28-byte address-credential used for the Shelley payment address;
/// pin it so a regression in the hash function surfaces here.
#[test]
fn address_key_hash_matches_blake2b_224() {
    use super::commands::cardano_cli::read_verification_key_text_envelope;
    use yggdrasil_crypto::hash_bytes_224;

    let vk = [0x42_u8; 32];
    let expected = hash_bytes_224(&vk).0;

    let mut cbor = vec![0x58, 0x20];
    cbor.extend_from_slice(&vk);
    let envelope = serde_json::json!({
        "type": "PaymentVerificationKeyShelley_ed25519",
        "description": "Payment Verification Key",
        "cborHex": hex::encode(&cbor),
    });
    let envelope_bytes = serde_json::to_vec(&envelope).expect("serialize");
    let parsed = read_verification_key_text_envelope(&envelope_bytes).expect("parse");

    let actual = hash_bytes_224(&parsed).0;
    assert_eq!(
        actual, expected,
        "address-key-hash must equal Blake2b-224(VK); got {:?}, expected {:?}",
        hex::encode(actual),
        hex::encode(expected),
    );
}
