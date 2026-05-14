// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::*;
use yggdrasil_node_config::{NodeConfigFile, TraceNamespaceConfig, default_config};

#[test]
fn machine_trace_line_is_json() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_option_node_name = Some("yggdrasil-test".to_owned());
    cfg.trace_options = BTreeMap::from([(
        "".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Notice".to_owned()),
            detail: None,
            backends: vec!["Stdout MachineFormat".to_owned()],
            max_frequency: None,
        },
    )]);

    let tracer = NodeTracer::from_config(&cfg);
    let rendered = tracer.format_machine_line(
        "Startup.DiffusionInit",
        "Notice",
        "starting node runtime",
        &trace_fields([
            ("peerCount", Value::from(3)),
            ("networkMagic", Value::from(764824073u64)),
        ]),
    );
    let parsed: Value = serde_json::from_str(&rendered).expect("valid json");

    assert_eq!(parsed["namespace"], Value::from("Startup.DiffusionInit"));
    assert_eq!(parsed["severity"], Value::from("Notice"));
    assert_eq!(parsed["node_name"], Value::from("yggdrasil-test"));
    assert_eq!(parsed["message"], Value::from("starting node runtime"));
    assert_eq!(parsed["data"]["peerCount"], Value::from(3));
}

#[test]
fn namespace_silence_suppresses_event() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "ChainSync.Client".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Silence".to_owned()),
            detail: None,
            backends: vec!["Stdout HumanFormatColoured".to_owned()],
            max_frequency: None,
        },
    );

    let tracer = NodeTracer::from_config(&cfg);
    assert_eq!(tracer.resolve_severity("ChainSync.Client", "Info"), None);
}

#[test]
fn human_trace_line_includes_fields() {
    let tracer = NodeTracer::from_config(&default_config());
    let line = tracer.format_human_line(
        "Net.PeerSelection",
        "Info",
        "bootstrap peer connected",
        &trace_fields([
            ("peer", Value::from("127.0.0.1:3001")),
            ("attempt", Value::from(1)),
        ]),
        false,
    );

    assert!(line.contains("Net.PeerSelection"));
    assert!(line.contains("bootstrap peer connected"));
    assert!(line.contains("peer=127.0.0.1:3001"));
    assert!(line.contains("attempt=1"));
}

#[test]
fn default_config_exposes_checkpoint_namespace_override() {
    let tracer = NodeTracer::from_config(&default_config());

    assert_eq!(
        tracer.resolve_severity("Node.Recovery.Checkpoint", "Notice"),
        Some("Notice")
    );
}

#[test]
fn root_severity_threshold_filters_lower_events() {
    let tracer = NodeTracer::from_config(&default_config());

    // Root threshold is Notice in default config.
    assert_eq!(tracer.resolve_severity("Node.Runtime", "Info"), None);
    assert_eq!(
        tracer.resolve_severity("Node.Runtime", "Warning"),
        Some("Warning")
    );
}

#[test]
fn prefix_namespace_severity_is_applied() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "Net".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Warning".to_owned()),
            detail: None,
            backends: Vec::new(),
            max_frequency: None,
        },
    );
    let tracer = NodeTracer::from_config(&cfg);

    assert_eq!(tracer.resolve_severity("Net.Handshake", "Info"), None);
    assert_eq!(
        tracer.resolve_severity("Net.Handshake", "Warning"),
        Some("Warning")
    );
}

#[test]
fn exact_namespace_overrides_prefix_threshold() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "Net".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Warning".to_owned()),
            detail: None,
            backends: Vec::new(),
            max_frequency: None,
        },
    );
    cfg.trace_options.insert(
        "Net.PeerSelection".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Info".to_owned()),
            detail: None,
            backends: Vec::new(),
            max_frequency: None,
        },
    );
    let tracer = NodeTracer::from_config(&cfg);

    assert_eq!(
        tracer.resolve_severity("Net.PeerSelection", "Info"),
        Some("Info")
    );
}

#[test]
fn prefix_namespace_frequency_is_applied() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "Node.Recovery".to_owned(),
        TraceNamespaceConfig {
            severity: None,
            detail: None,
            backends: Vec::new(),
            max_frequency: Some(2.0),
        },
    );
    let tracer = NodeTracer::from_config(&cfg);

    assert_eq!(
        tracer.min_emit_interval_ms("Node.Recovery.Custom"),
        Some(500)
    );
}

#[test]
fn namespace_frequency_override_maps_to_interval() {
    let tracer = NodeTracer::from_config(&default_config());

    assert_eq!(
        tracer.min_emit_interval_ms("Node.Recovery.Checkpoint"),
        Some(1000)
    );
}

#[test]
fn rate_limiter_blocks_repeated_namespace_events_inside_interval() {
    let tracer = NodeTracer::from_config(&default_config());

    assert!(tracer.should_emit("Node.Recovery.Checkpoint", 1_000));
    assert!(!tracer.should_emit("Node.Recovery.Checkpoint", 1_500));
    assert!(tracer.should_emit("Node.Recovery.Checkpoint", 2_000));
}

#[test]
fn node_metrics_accumulates_counters() {
    let metrics = NodeMetrics::new();

    metrics.add_blocks_synced(10);
    metrics.add_blocks_synced(5);
    metrics.add_rollbacks(1);
    metrics.inc_batches_completed();
    metrics.inc_batches_completed();
    metrics.add_stable_blocks_promoted(3);
    metrics.inc_reconnects();

    let snap = metrics.snapshot();
    assert_eq!(snap.blocks_synced, 15);
    assert_eq!(snap.rollbacks, 1);
    assert_eq!(snap.batches_completed, 2);
    assert_eq!(snap.stable_blocks_promoted, 3);
    assert_eq!(snap.reconnects, 1);
}

#[test]
fn node_metrics_tracks_slot_and_block_number() {
    let metrics = NodeMetrics::new();

    metrics.set_current_slot(42_000);
    metrics.set_current_block_number(1_234);
    metrics.set_checkpoint_slot(41_000);

    let snap = metrics.snapshot();
    assert_eq!(snap.current_slot, 42_000);
    assert_eq!(snap.current_block_number, 1_234);
    assert_eq!(snap.checkpoint_slot, 41_000);
}

#[test]
fn node_metrics_uptime_grows() {
    let metrics = NodeMetrics::new();
    let snap = metrics.snapshot();
    // Uptime should be zero or very small immediately after creation.
    assert!(snap.uptime_ms < 1000);
}

#[test]
fn node_metrics_snapshot_is_serializable() {
    let metrics = NodeMetrics::new();
    metrics.add_blocks_synced(7);
    let snap = metrics.snapshot();
    let json = serde_json::to_string(&snap).expect("serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(parsed["blocks_synced"], Value::from(7));
}

/// Invariant: every numeric field of [`MetricsSnapshot`] must appear in
/// the Prometheus text emission as `yggdrasil_<field>`. Enforces the
/// "added an AtomicU64 but forgot to emit it" drift case — without
/// this test, a new counter slips into the struct + snapshot + JSON
/// surface silently invisible to Prometheus scrapers.
///
/// Iteration is done via `serde_json::to_value` reflection over the
/// snapshot so the check stays automatic as fields are added.
/// `uptime_ms` is the only snapshot field NOT emitted verbatim —
/// it's published as `yggdrasil_uptime_seconds` (divided by 1000) so
/// the check accepts either spelling.
#[test]
fn every_metrics_snapshot_field_is_exported_in_prometheus_text() {
    let metrics = NodeMetrics::new();
    let snapshot = metrics.snapshot();
    let text = snapshot.to_prometheus_text();

    let json = serde_json::to_value(&snapshot).expect("snapshot is serializable");
    let fields = json
        .as_object()
        .expect("snapshot serialises as a JSON object");

    let mut missing: Vec<&str> = Vec::new();
    for field_name in fields.keys() {
        // Only numeric counter/gauge fields are expected to surface;
        // every current field is u64 or u128.
        let metric_canonical = format!("yggdrasil_{field_name} ");
        // Accept the documented rename for the one non-verbatim field.
        // Round 170 — `blocks_per_era` is exploded into seven
        // explicitly-named counters (`yggdrasil_blocks_byron` …
        // `yggdrasil_blocks_conway`) per Prometheus convention; check
        // each named counter is present.
        let accepts = text.contains(&metric_canonical)
                || (field_name == "uptime_ms" && text.contains("yggdrasil_uptime_seconds"))
                || (field_name == "blocks_per_era"
                    && [
                        "yggdrasil_blocks_byron ",
                        "yggdrasil_blocks_shelley ",
                        "yggdrasil_blocks_allegra ",
                        "yggdrasil_blocks_mary ",
                        "yggdrasil_blocks_alonzo ",
                        "yggdrasil_blocks_babbage ",
                        "yggdrasil_blocks_conway ",
                    ]
                    .iter()
                    .all(|name| text.contains(name)))
                // Round 200 — apply-batch histogram is rendered with
                // standard Prometheus histogram suffixes (`_bucket`,
                // `_sum`, `_count`) under one shared metric name.
                || (field_name == "apply_batch_duration_buckets"
                    && text.contains("yggdrasil_apply_batch_duration_seconds_bucket"))
                || (field_name == "apply_batch_duration_sum_micros"
                    && text.contains("yggdrasil_apply_batch_duration_seconds_sum "))
                || (field_name == "apply_batch_duration_count"
                    && text.contains("yggdrasil_apply_batch_duration_seconds_count "))
                // R217 — fetch-batch histogram (same shape as apply).
                || (field_name == "fetch_batch_duration_buckets"
                    && text.contains("yggdrasil_fetch_batch_duration_seconds_bucket"))
                || (field_name == "fetch_batch_duration_sum_micros"
                    && text.contains("yggdrasil_fetch_batch_duration_seconds_sum "))
                || (field_name == "fetch_batch_duration_count"
                    && text.contains("yggdrasil_fetch_batch_duration_seconds_count "))
                // R225 — rollback-depth histogram.
                || (field_name == "rollback_depth_buckets"
                    && text.contains("yggdrasil_rollback_depth_blocks_bucket"))
                || (field_name == "rollback_depth_sum_blocks"
                    && text.contains("yggdrasil_rollback_depth_blocks_sum "))
                || (field_name == "rollback_depth_count"
                    && text.contains("yggdrasil_rollback_depth_blocks_count "));
        if !accepts {
            missing.push(field_name);
        }
    }
    assert!(
        missing.is_empty(),
        "MetricsSnapshot fields with no Prometheus export line: {missing:?}\n\
             Every new counter must be mirrored in `MetricsSnapshot::to_prometheus_text`."
    );
}

#[test]
fn node_metrics_snapshot_renders_prometheus_text() {
    let metrics = NodeMetrics::new();
    metrics.add_blocks_synced(42);
    metrics.set_current_slot(100);
    metrics.set_peer_selection_counters(20, 10, 5, 6, 3, 1, 18, 7, 4, 4, 2, 1, 3, 2, 1);
    let text = metrics.snapshot().to_prometheus_text();

    assert!(text.contains("yggdrasil_blocks_synced 42\n"));
    assert!(text.contains("yggdrasil_current_slot 100\n"));
    assert!(text.contains("yggdrasil_target_known_peers 20\n"));
    assert!(text.contains("yggdrasil_known_big_ledger_peers 4\n"));
    assert!(text.contains("# TYPE yggdrasil_blocks_synced counter\n"));
    assert!(text.contains("# TYPE yggdrasil_current_slot gauge\n"));
    assert!(text.contains("# TYPE yggdrasil_target_known_peers gauge\n"));
    assert!(text.contains("yggdrasil_uptime_seconds"));
}

/// R231 — pin the R200 apply-batch + R217 fetch-batch
/// duration histogram contracts.  Both share
/// [`NodeMetrics::APPLY_BATCH_BUCKETS_SECONDS`] bucket
/// boundaries so dashboards can render fetch-vs-apply
/// side-by-side comparisons (R217+R218 multi-peer sync-rate
/// quantification depends on this).  Pins:
/// (1) bucket boundaries `[1ms, 5ms, 10ms, 50ms, 100ms, 500ms,
/// 1s, 5s, 10s, +Inf]` — drift means dashboards misclassify
/// latency tier;
/// (2) cumulative-bucket semantic (observation `d` increments
/// every bucket whose `le_secs` is ≥ `d`);
/// (3) Prometheus exposition shape for both metrics.
#[test]
fn node_metrics_tracks_fetch_and_apply_batch_histograms() {
    use std::time::Duration;

    // Bucket-boundary pin.  Each numeric value is load-bearing
    // for operator alerting.
    assert_eq!(
        NodeMetrics::APPLY_BATCH_BUCKETS_SECONDS,
        [
            0.001,
            0.005,
            0.01,
            0.05,
            0.1,
            0.5,
            1.0,
            5.0,
            10.0,
            f64::INFINITY
        ],
    );

    let metrics = NodeMetrics::new();

    // Default: no observations.
    let snap = metrics.snapshot();
    assert_eq!(snap.apply_batch_duration_count, 0);
    assert_eq!(snap.fetch_batch_duration_count, 0);

    // Apply observation: 200ms (a typical mainnet apply per
    // R218).  Falls into le=0.5 and higher (5 buckets).
    metrics.record_apply_batch_duration(Duration::from_millis(200));
    let snap = metrics.snapshot();
    assert_eq!(snap.apply_batch_duration_count, 1);
    assert_eq!(snap.apply_batch_duration_buckets[0], 0, "le=0.001 < 0.2s");
    assert_eq!(snap.apply_batch_duration_buckets[4], 0, "le=0.1 < 0.2s");
    assert_eq!(
        snap.apply_batch_duration_buckets[5], 1,
        "le=0.5 includes 0.2s"
    );
    assert_eq!(
        snap.apply_batch_duration_buckets[9], 1,
        "+Inf includes everything"
    );

    // Fetch observation: 12.85s (R217 mainnet single-peer
    // baseline).  Falls into +Inf only (>10s).
    metrics.record_fetch_batch_duration(Duration::from_millis(12_850));
    let snap = metrics.snapshot();
    assert_eq!(snap.fetch_batch_duration_count, 1);
    assert_eq!(snap.fetch_batch_duration_buckets[8], 0, "le=10.0 < 12.85s");
    assert_eq!(
        snap.fetch_batch_duration_buckets[9], 1,
        "+Inf includes 12.85s"
    );

    // Fetch observation: 8.56s (R218 multi-peer, 2 active
    // workers).  Falls into le=10.0 and +Inf.
    metrics.record_fetch_batch_duration(Duration::from_millis(8_560));
    let snap = metrics.snapshot();
    assert_eq!(snap.fetch_batch_duration_count, 2);
    assert_eq!(
        snap.fetch_batch_duration_buckets[8], 1,
        "le=10.0 includes 8.56s"
    );
    assert_eq!(
        snap.fetch_batch_duration_buckets[9], 2,
        "+Inf includes both"
    );

    // Prometheus text format pin.
    let text = snap.to_prometheus_text();
    assert!(text.contains("# TYPE yggdrasil_apply_batch_duration_seconds histogram\n"));
    assert!(text.contains("# TYPE yggdrasil_fetch_batch_duration_seconds histogram\n"));
    assert!(
        text.contains("yggdrasil_apply_batch_duration_seconds_bucket{le=\"0.5\"} 1\n"),
        "apply le=0.5 not exposed"
    );
    assert!(
        text.contains("yggdrasil_fetch_batch_duration_seconds_bucket{le=\"+Inf\"} 2\n"),
        "fetch +Inf not exposed"
    );
    assert!(text.contains("yggdrasil_apply_batch_duration_seconds_count 1\n"));
    assert!(text.contains("yggdrasil_fetch_batch_duration_seconds_count 2\n"));
}

/// R230 — pin the Phase D.1 rollback-depth histogram contract
/// from R225.  Bucket boundaries `[1, 2, 5, 50, 2160 (k),
/// 10_000, +Inf]` are load-bearing — operator dashboards and
/// `histogram_quantile(0.99, …)` alerts depend on them.
/// Also pins the cumulative-bucket semantic: an observation of
/// depth `d` increments every bucket whose `le` is ≥ `d` (so
/// the +Inf bucket is the total observation count).
#[test]
fn node_metrics_tracks_phase_d1_rollback_depth_histogram() {
    let metrics = NodeMetrics::new();

    // Default: zero observations.
    let snap = metrics.snapshot();
    assert_eq!(snap.rollback_depth_count, 0);
    assert_eq!(snap.rollback_depth_sum_blocks, 0);
    for bucket in &snap.rollback_depth_buckets {
        assert_eq!(*bucket, 0);
    }

    // Observation 1: depth=0 (session-start confirm rollback,
    // common case).  Falls into every bucket including le=1.
    metrics.record_rollback_depth(0);
    let snap = metrics.snapshot();
    assert_eq!(snap.rollback_depth_count, 1);
    assert_eq!(snap.rollback_depth_sum_blocks, 0);
    for (i, bucket) in snap.rollback_depth_buckets.iter().enumerate() {
        assert_eq!(*bucket, 1, "depth=0 must increment every bucket (i={i})");
    }

    // Observation 2: depth=3 (small chain reorg).  Falls into
    // le=5, le=50, le=2160, le=10_000, le=+Inf (5 buckets).
    // Does NOT fall into le=1 or le=2.
    metrics.record_rollback_depth(3);
    let snap = metrics.snapshot();
    assert_eq!(snap.rollback_depth_count, 2);
    assert_eq!(snap.rollback_depth_sum_blocks, 3);
    assert_eq!(
        snap.rollback_depth_buckets[0], 1,
        "le=1 unchanged for depth=3"
    );
    assert_eq!(
        snap.rollback_depth_buckets[1], 1,
        "le=2 unchanged for depth=3"
    );
    assert_eq!(snap.rollback_depth_buckets[2], 2, "le=5 includes depth=3");
    assert_eq!(
        snap.rollback_depth_buckets[6], 2,
        "+Inf includes everything"
    );

    // Observation 3: depth=5000 (cross-epoch range).  Falls into
    // le=10_000 and le=+Inf only.
    metrics.record_rollback_depth(5000);
    let snap = metrics.snapshot();
    assert_eq!(snap.rollback_depth_count, 3);
    assert_eq!(snap.rollback_depth_sum_blocks, 3 + 5000);
    assert_eq!(
        snap.rollback_depth_buckets[5], 3,
        "le=10_000 includes depth=5000"
    );
    assert_eq!(
        snap.rollback_depth_buckets[6], 3,
        "+Inf still includes everything"
    );
    assert_eq!(
        snap.rollback_depth_buckets[4], 2,
        "le=2160 (k) does NOT include 5000"
    );

    // Bucket boundaries pin: drift here means operator dashboards
    // misclassify rollback severity.
    assert_eq!(
        NodeMetrics::ROLLBACK_DEPTH_BUCKETS,
        [1, 2, 5, 50, 2160, 10_000, u64::MAX]
    );

    // Prometheus text format pin.
    let text = snap.to_prometheus_text();
    assert!(text.contains("# TYPE yggdrasil_rollback_depth_blocks histogram\n"));
    assert!(
        text.contains("yggdrasil_rollback_depth_blocks_bucket{le=\"1\"} 1\n"),
        "le=1 bucket value not exposed correctly"
    );
    assert!(
        text.contains("yggdrasil_rollback_depth_blocks_bucket{le=\"+Inf\"} 3\n"),
        "+Inf bucket value not exposed correctly"
    );
    assert!(text.contains("yggdrasil_rollback_depth_blocks_sum 5003\n"));
    assert!(text.contains("yggdrasil_rollback_depth_blocks_count 3\n"));
}

/// R229/R237 — pin the Phase D.2 lifetime peer-stats
/// Prometheus output contract.  The 5 counters
/// (`*_total`) MUST emit `# TYPE …_total counter`; the 1
/// gauge (`unique_peers`) MUST emit `# TYPE … gauge`.  Drift
/// in the contract (e.g. accidentally emitting a counter as a
/// gauge) silently breaks operator alerts that depend on
/// `rate(...)` semantics.
///
/// References R222–R226 (the lifetime peer-stats deliverable).
#[test]
fn node_metrics_tracks_phase_d2_lifetime_peer_stats() {
    let metrics = NodeMetrics::new();

    // Default state: all lifetime counters at zero.
    let snap = metrics.snapshot();
    assert_eq!(snap.peer_lifetime_sessions_total, 0);
    assert_eq!(snap.peer_lifetime_failures_total, 0);
    assert_eq!(snap.peer_lifetime_bytes_in_total, 0);
    assert_eq!(snap.peer_lifetime_bytes_out_total, 0);
    assert_eq!(snap.peer_lifetime_unique_peers, 0);
    assert_eq!(snap.peer_lifetime_handshakes_total, 0);

    // Simulate governor-tick aggregate updates.
    metrics.set_peer_lifetime_sessions_total(7);
    metrics.set_peer_lifetime_failures_total(2);
    metrics.set_peer_lifetime_bytes_in_total(1_500_000);
    metrics.set_peer_lifetime_bytes_out_total(750_000);
    metrics.set_peer_lifetime_unique_peers(9);
    metrics.set_peer_lifetime_handshakes_total(7);

    let snap = metrics.snapshot();
    assert_eq!(snap.peer_lifetime_sessions_total, 7);
    assert_eq!(snap.peer_lifetime_failures_total, 2);
    assert_eq!(snap.peer_lifetime_bytes_in_total, 1_500_000);
    assert_eq!(snap.peer_lifetime_bytes_out_total, 750_000);
    assert_eq!(snap.peer_lifetime_unique_peers, 9);
    assert_eq!(snap.peer_lifetime_handshakes_total, 7);

    let peer: std::net::SocketAddr = "127.0.0.1:3001".parse().expect("peer addr");
    metrics.add_keepalive_server_bytes_served_for_peer(Some(peer), 4);
    metrics.add_txsubmission_server_bytes_served_for_peer(Some(peer), 12);
    metrics.add_peersharing_server_bytes_served_for_peer(Some(peer), 20);
    assert_eq!(
        metrics.peer_lifetime_bytes_out_by_peer(),
        vec![(peer, 36)],
        "internal per-peer egress map is cumulative and unlabelled"
    );

    // Prometheus text contract — TYPE lines + value lines for
    // each metric, with correct counter / gauge
    // discrimination.
    let text = snap.to_prometheus_text();

    // 5 counters.
    for counter in [
        "yggdrasil_peer_lifetime_sessions_total",
        "yggdrasil_peer_lifetime_failures_total",
        "yggdrasil_peer_lifetime_bytes_in_total",
        "yggdrasil_peer_lifetime_bytes_out_total",
        "yggdrasil_peer_lifetime_handshakes_total",
    ] {
        assert!(
            text.contains(&format!("# TYPE {counter} counter\n")),
            "missing counter TYPE for {counter}"
        );
    }
    // 1 gauge.
    assert!(
        text.contains("# TYPE yggdrasil_peer_lifetime_unique_peers gauge\n"),
        "unique_peers must be a gauge (cardinality of map)"
    );

    // Value lines.
    assert!(text.contains("yggdrasil_peer_lifetime_sessions_total 7\n"));
    assert!(text.contains("yggdrasil_peer_lifetime_failures_total 2\n"));
    assert!(text.contains("yggdrasil_peer_lifetime_bytes_in_total 1500000\n"));
    assert!(text.contains("yggdrasil_peer_lifetime_bytes_out_total 750000\n"));
    assert!(text.contains("yggdrasil_peer_lifetime_unique_peers 9\n"));
    assert!(text.contains("yggdrasil_peer_lifetime_handshakes_total 7\n"));

    let after_egress = metrics.snapshot();
    assert_eq!(after_egress.keepalive_server_bytes_served_total, 4);
    assert_eq!(after_egress.txsubmission_server_bytes_served_total, 12);
    assert_eq!(after_egress.peersharing_server_bytes_served_total, 20);
}

#[test]
fn node_metrics_tracks_blockfetch_worker_pool_size() {
    // Phase 6 multi-peer dispatch observability: operators must
    // be able to verify activation of the multi-peer path via
    // `/metrics`.  `blockfetch_workers_registered` reports the
    // current pool size; `blockfetch_workers_migrated_total`
    // counts lifetime migrations.
    let metrics = NodeMetrics::new();
    // Default state: no workers, no migrations.
    let snap = metrics.snapshot();
    assert_eq!(snap.blockfetch_workers_registered, 0);
    assert_eq!(snap.blockfetch_workers_migrated_total, 0);

    // Simulate 2 peers being migrated.
    metrics.inc_blockfetch_workers_migrated();
    metrics.inc_blockfetch_workers_migrated();
    metrics.set_blockfetch_workers_registered(2);
    let snap = metrics.snapshot();
    assert_eq!(snap.blockfetch_workers_registered, 2);
    assert_eq!(snap.blockfetch_workers_migrated_total, 2);

    // One peer disconnects; pool size shrinks but lifetime count
    // is monotonic.
    metrics.set_blockfetch_workers_registered(1);
    let snap = metrics.snapshot();
    assert_eq!(snap.blockfetch_workers_registered, 1);
    assert_eq!(snap.blockfetch_workers_migrated_total, 2);

    // Prometheus text contains both lines for scrape parity.
    let text = metrics.snapshot().to_prometheus_text();
    assert!(text.contains("yggdrasil_blockfetch_workers_registered 1\n"));
    assert!(text.contains("yggdrasil_blockfetch_workers_migrated_total 2\n"));
    assert!(text.contains("# TYPE yggdrasil_blockfetch_workers_registered gauge\n"));
    assert!(text.contains("# TYPE yggdrasil_blockfetch_workers_migrated_total counter\n"));
}

#[test]
fn node_metrics_tracks_peer_selection_counters() {
    let metrics = NodeMetrics::new();

    metrics.set_peer_selection_counters(30, 18, 7, 9, 4, 2, 22, 11, 6, 8, 3, 1, 5, 3, 2);

    let snap = metrics.snapshot();
    assert_eq!(snap.target_known_peers, 30);
    assert_eq!(snap.target_established_peers, 18);
    assert_eq!(snap.target_active_peers, 7);
    assert_eq!(snap.target_known_big_ledger_peers, 9);
    assert_eq!(snap.target_established_big_ledger_peers, 4);
    assert_eq!(snap.target_active_big_ledger_peers, 2);
    assert_eq!(snap.known_peers, 22);
    assert_eq!(snap.established_peers, 11);
    assert_eq!(snap.active_peers, 6);
    assert_eq!(snap.known_big_ledger_peers, 8);
    assert_eq!(snap.established_big_ledger_peers, 3);
    assert_eq!(snap.active_big_ledger_peers, 1);
    assert_eq!(snap.known_local_root_peers, 5);
    assert_eq!(snap.established_local_root_peers, 3);
    assert_eq!(snap.active_local_root_peers, 2);
    assert_eq!(snap.warm_local_root_peers, 3);
    assert_eq!(snap.hot_local_root_peers, 2);
}

// -----------------------------------------------------------------------
// Coloured stdout backend tests
// -----------------------------------------------------------------------

#[test]
fn coloured_human_line_contains_ansi_codes_for_warning() {
    let tracer = NodeTracer::from_config(&default_config());
    let line = tracer.format_human_line(
        "Net.PeerSelection",
        "Warning",
        "peer timed out",
        &BTreeMap::new(),
        true,
    );

    // Yellow ANSI start + reset at end.
    assert!(line.starts_with("\x1b[33m"));
    assert!(line.ends_with("\x1b[0m"));
    assert!(line.contains("Warning"));
}

#[test]
fn coloured_human_line_no_ansi_for_info() {
    let tracer = NodeTracer::from_config(&default_config());
    let line = tracer.format_human_line("Startup", "Info", "starting", &BTreeMap::new(), true);

    // Info has no colour code, so no ANSI escape and no reset.
    assert!(!line.contains("\x1b["));
}

#[test]
fn uncoloured_human_line_has_no_ansi() {
    let tracer = NodeTracer::from_config(&default_config());
    let line = tracer.format_human_line(
        "Net.PeerSelection",
        "Error",
        "connection failed",
        &BTreeMap::new(),
        false,
    );

    assert!(!line.contains("\x1b["));
}

#[test]
fn coloured_backend_recognised_from_config_string() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Notice".to_owned()),
            detail: None,
            backends: vec!["Stdout HumanFormatColoured".to_owned()],
            max_frequency: None,
        },
    );
    let tracer = NodeTracer::from_config(&cfg);
    let backends = tracer.backends_for("Net.Handshake");
    assert_eq!(backends, vec![TraceBackend::StdoutHumanColoured]);
}

// -----------------------------------------------------------------------
// Upstream backend string recognition tests
// -----------------------------------------------------------------------

#[test]
fn ekg_backend_string_yields_no_trace_backend() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Notice".to_owned()),
            detail: None,
            backends: vec!["EKGBackend".to_owned()],
            max_frequency: None,
        },
    );
    let tracer = NodeTracer::from_config(&cfg);
    assert!(tracer.backends_for("Net").is_empty());
}

#[test]
fn forwarder_backend_string_yields_forwarder_trace_backend() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Notice".to_owned()),
            detail: None,
            backends: vec!["Forwarder".to_owned()],
            max_frequency: None,
        },
    );
    let tracer = NodeTracer::from_config(&cfg);
    assert_eq!(
        tracer.backends_for("Startup"),
        vec![TraceBackend::Forwarder]
    );
}

#[test]
fn prometheus_simple_backend_string_yields_no_trace_backend() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Notice".to_owned()),
            detail: None,
            backends: vec!["PrometheusSimple suffix 127.0.0.1 12798".to_owned()],
            max_frequency: None,
        },
    );
    let tracer = NodeTracer::from_config(&cfg);
    assert!(tracer.backends_for("ChainDB").is_empty());
}

#[test]
fn mixed_upstream_backends_resolve_correctly() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Notice".to_owned()),
            detail: None,
            backends: vec![
                "EKGBackend".to_owned(),
                "Forwarder".to_owned(),
                "PrometheusSimple suffix 127.0.0.1 12798".to_owned(),
                "Stdout HumanFormatColoured".to_owned(),
            ],
            max_frequency: None,
        },
    );
    let tracer = NodeTracer::from_config(&cfg);
    let backends = tracer.backends_for("Net");
    // Forwarder and stdout coloured backends both resolve.
    assert_eq!(
        backends,
        vec![TraceBackend::Forwarder, TraceBackend::StdoutHumanColoured]
    );
}

#[test]
fn clone_preserves_forwarder_transport_when_enabled() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Notice".to_owned()),
            detail: None,
            backends: vec!["Forwarder".to_owned()],
            max_frequency: None,
        },
    );

    let tracer = NodeTracer::from_config(&cfg);
    let cloned = tracer.clone();

    let original = tracer
        .forwarder
        .as_ref()
        .expect("forwarder should be configured on original tracer");
    let cloned_forwarder = cloned
        .forwarder
        .as_ref()
        .expect("forwarder should be configured on cloned tracer");

    assert!(Arc::ptr_eq(original, cloned_forwarder));
}

#[test]
fn clone_shares_rate_limiter_state() {
    let tracer = NodeTracer::from_config(&default_config());
    let cloned = tracer.clone();

    assert!(tracer.should_emit("Node.Recovery.Checkpoint", 1_000));
    assert!(!cloned.should_emit("Node.Recovery.Checkpoint", 1_500));
    assert!(cloned.should_emit("Node.Recovery.Checkpoint", 2_000));
}

// -----------------------------------------------------------------------
// Detail level tests
// -----------------------------------------------------------------------

#[test]
fn detail_for_returns_dnormal_when_unconfigured() {
    let tracer = NodeTracer::from_config(&default_config());
    assert_eq!(tracer.detail_for("Net.PeerSelection"), TraceDetail::DNormal);
}

#[test]
fn detail_for_respects_root_config() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Notice".to_owned()),
            detail: Some("DDetailed".to_owned()),
            backends: vec!["Stdout HumanFormat".to_owned()],
            max_frequency: None,
        },
    );
    let tracer = NodeTracer::from_config(&cfg);
    assert_eq!(tracer.detail_for("Any.Namespace"), TraceDetail::DDetailed);
}

#[test]
fn detail_for_respects_namespace_override() {
    let mut cfg: NodeConfigFile = default_config();
    cfg.trace_options.insert(
        "".to_owned(),
        TraceNamespaceConfig {
            severity: Some("Notice".to_owned()),
            detail: Some("DNormal".to_owned()),
            backends: vec!["Stdout HumanFormat".to_owned()],
            max_frequency: None,
        },
    );
    cfg.trace_options.insert(
        "Net.PeerSelection".to_owned(),
        TraceNamespaceConfig {
            severity: None,
            detail: Some("DMaximum".to_owned()),
            backends: Vec::new(),
            max_frequency: None,
        },
    );
    let tracer = NodeTracer::from_config(&cfg);
    assert_eq!(
        tracer.detail_for("Net.PeerSelection"),
        TraceDetail::DMaximum
    );
    assert_eq!(tracer.detail_for("Net.Handshake"), TraceDetail::DNormal);
}

#[test]
fn detail_from_label_parses_upstream_strings() {
    assert_eq!(
        TraceDetail::from_label("DMinimal"),
        Some(TraceDetail::DMinimal)
    );
    assert_eq!(
        TraceDetail::from_label("DNormal"),
        Some(TraceDetail::DNormal)
    );
    assert_eq!(
        TraceDetail::from_label("DDetailed"),
        Some(TraceDetail::DDetailed)
    );
    assert_eq!(
        TraceDetail::from_label("DMaximum"),
        Some(TraceDetail::DMaximum)
    );
    assert_eq!(TraceDetail::from_label("invalid"), None);
}

#[test]
fn trace_detail_ordering() {
    assert!(TraceDetail::DMinimal < TraceDetail::DNormal);
    assert!(TraceDetail::DNormal < TraceDetail::DDetailed);
    assert!(TraceDetail::DDetailed < TraceDetail::DMaximum);
}
