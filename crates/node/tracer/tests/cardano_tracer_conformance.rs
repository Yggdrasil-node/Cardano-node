//! Live conformance test — Yggdrasil trace-forwarder handshake against
//! the upstream `cardano-tracer` binary.
//!
//! Task #19 (Phase 2.B closeout), remaining-work item 2. This test
//! spawns the vendored upstream `cardano-tracer` 11.0.1 binary in
//! `AcceptAt` mode on a temp Unix socket, then drives Yggdrasil's
//! [`MuxConnection::run_initiator_handshake`] against it and asserts the
//! `Network.Mux` Handshake mini-protocol completes — cardano-tracer
//! replies `MsgAcceptVersion(ForwardingV_1, <network-magic>)`.
//!
//! Scope is bounded to the **handshake**. TraceObject delivery and
//! log-content assertions are separate follow-on rounds; a green
//! handshake is itself the parity evidence that Yggdrasil's handshake
//! CBOR codec, SDU framing, direction bit, and version negotiation
//! match upstream byte-for-byte.
//!
//! Upstream facts pinned here, verified against the vendored Haskell
//! source by the haskell-reference-auditor:
//!   * cardano-tracer is the socket-accept side and the Mux RESPONDER;
//!     Yggdrasil connects and is the Mux INITIATOR (sends
//!     `MsgProposeVersions` first).
//!   * cardano-tracer 11.0.1 offers exactly `ForwardingV_1` (version 1)
//!     — `Trace.Forward.Utils.Version` + `Cardano.Tracer.Acceptors.Server`.
//!   * the handshake version-data is a bare CBOR uint equal to the
//!     tracer's configured `networkMagic`; a mismatch is refused.
//!   * the Handshake mini-protocol runs on Mux mini-protocol number 0.
//!
//! The test self-skips when the upstream binary is absent (CI's
//! `--sources-only` reference tree, or no reference tree at all).
//! Override the binary path with `YGGDRASIL_CARDANO_TRACER_BIN`;
//! materialize the default with `bash scripts/setup-reference.sh`.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use yggdrasil_ledger::cbor::Encoder;
use yggdrasil_node_tracer::trace_forwarder::bearer::Bearer;
use yggdrasil_node_tracer::trace_forwarder::forwarding_task::{ForwardingTaskConfig, run_via_mux};
use yggdrasil_node_tracer::trace_forwarder::mux::TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM;
use yggdrasil_node_tracer::trace_forwarder::mux_connection::MuxConnection;
use yggdrasil_node_tracer::trace_forwarder::{TraceDetail, TraceObject, TraceSeverity};

/// Network magic used on both sides. Arbitrary — the only requirement
/// is that the value written into the tracer config's `networkMagic`
/// equals the version-data uint Yggdrasil proposes, or cardano-tracer
/// refuses the handshake. `764824073` is the mainnet magic.
const NETWORK_MAGIC: u64 = 764_824_073;

/// Locate the vendored upstream `cardano-tracer` binary. Honors the
/// `YGGDRASIL_CARDANO_TRACER_BIN` override; otherwise looks at the
/// canonical vendored install path relative to the crate manifest.
/// Returns `None` when the binary is absent.
fn cardano_tracer_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("YGGDRASIL_CARDANO_TRACER_BIN") {
        let pb = PathBuf::from(p);
        return pb.exists().then_some(pb);
    }
    // CARGO_MANIFEST_DIR = <workspace>/crates/node/tracer — the
    // vendored reference tree sits at the workspace root.
    let pb = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.reference-haskell-cardano-node/install/bin/cardano-tracer");
    pb.exists().then_some(pb)
}

/// RAII guard: SIGKILLs + reaps the cardano-tracer child and removes
/// the temp directory on drop — including on test-panic unwind, so a
/// failed assertion never leaks a daemon process or a temp dir.
struct Harness {
    child: Child,
    dir: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Per-process temp layout for one cardano-tracer instance: the
/// accept-socket path, the `logRoot` directory, and the generated
/// config file path. Both conformance tests share this so the two
/// processes never collide on a socket / log dir.
struct TracerLayout {
    dir: PathBuf,
    socket_path: PathBuf,
    log_dir: PathBuf,
    config_path: PathBuf,
}

/// Materialize a fresh temp dir with a minimal `FileMode` /
/// `ForMachine` tracer config. `tag` disambiguates the temp-dir
/// name so two tests in the same process don't share state.
///
/// The config shape mirrors the vendored
/// `install/share/<net>/tracer-config.json`: `AcceptAt` on a Unix
/// socket, `FileMode` logging to `logRoot`, no EKG / Prometheus
/// servers (omitted keys → `Nothing`, so no TCP ports are bound).
fn make_tracer_layout(tag: &str) -> TracerLayout {
    let dir = std::env::temp_dir().join(format!("ygg-tracer-conf-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let socket_path = dir.join("tracer.socket");
    let log_dir = dir.join("logs");
    std::fs::create_dir_all(&log_dir).expect("create log dir");
    let config_path = dir.join("tracer-config.json");
    let config = format!(
        r#"{{
  "networkMagic": {magic},
  "network": {{ "tag": "AcceptAt", "contents": "{socket}" }},
  "logging": [
    {{ "logFormat": "ForMachine", "logMode": "FileMode", "logRoot": "{logroot}" }}
  ],
  "rotation": {{
    "rpFrequencySecs": 60,
    "rpKeepFilesNum": 14,
    "rpLogLimitBytes": 10000000,
    "rpMaxAgeHours": 24
  }},
  "ekgRequestFreq": null,
  "ekgRequestFull": null,
  "loRequestNum": null,
  "metricsHelp": null,
  "metricsNoSuffix": null,
  "resourceFreq": null,
  "verbosity": null
}}"#,
        magic = NETWORK_MAGIC,
        socket = socket_path.display(),
        logroot = log_dir.display(),
    );
    std::fs::write(&config_path, config).expect("write tracer config");
    TracerLayout {
        dir,
        socket_path,
        log_dir,
        config_path,
    }
}

/// Spawn the upstream cardano-tracer daemon against `layout`'s
/// config and poll-connect its accept socket. stderr is inherited
/// so a config / startup / decode failure is visible under
/// `cargo test -- --nocapture`. Returns the connected `UnixStream`
/// plus the RAII `Harness` (SIGKILL + reap on drop).
async fn spawn_and_connect(
    bin: &PathBuf,
    layout: &TracerLayout,
) -> (tokio::net::UnixStream, Harness) {
    let child = Command::new(bin)
        .arg("--config")
        .arg(&layout.config_path)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn cardano-tracer");
    let mut harness = Harness {
        child,
        dir: layout.dir.clone(),
    };

    let deadline = Instant::now() + Duration::from_secs(10);
    let socket = loop {
        if let Some(status) = harness.child.try_wait().expect("try_wait cardano-tracer") {
            panic!(
                "cardano-tracer exited early ({status}) before accepting a \
                 connection — check the generated config / stderr above"
            );
        }
        match tokio::net::UnixStream::connect(&layout.socket_path).await {
            Ok(stream) => break stream,
            Err(e) => {
                if Instant::now() >= deadline {
                    panic!("cardano-tracer socket never became connectable within 10s: {e}");
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    };
    (socket, harness)
}

#[tokio::test]
async fn handshake_completes_against_upstream_cardano_tracer() {
    let Some(bin) = cardano_tracer_bin() else {
        eprintln!(
            "SKIP handshake_completes_against_upstream_cardano_tracer: \
             upstream cardano-tracer binary not found — run \
             `bash scripts/setup-reference.sh` or set \
             YGGDRASIL_CARDANO_TRACER_BIN"
        );
        return;
    };

    // Per-process temp layout (socket + logRoot + config) and the
    // spawned + poll-connected cardano-tracer daemon. The `Harness`
    // SIGKILLs + reaps the child and removes the temp dir on drop.
    let layout = make_tracer_layout("handshake");
    let (socket, _harness) = spawn_and_connect(&bin, &layout).await;

    // Drive the Network.Mux Handshake initiator side. version-data is
    // a bare CBOR uint == the configured network magic.
    let bearer = Bearer::new(socket);
    let conn = MuxConnection::new(bearer);
    let version_data = {
        let mut enc = Encoder::new();
        enc.unsigned(NETWORK_MAGIC);
        enc.into_bytes()
    };
    let mut versions = BTreeMap::new();
    versions.insert(1u32, version_data);

    let outcome = tokio::time::timeout(
        Duration::from_secs(5),
        conn.run_initiator_handshake(versions),
    )
    .await;

    match outcome {
        Err(_elapsed) => {
            panic!("handshake timed out — cardano-tracer sent no reply SDU within 5s")
        }
        Ok(Err(e)) => {
            panic!("handshake failed against upstream cardano-tracer 11.0.1: {e}")
        }
        Ok(Ok(agreed)) => {
            assert_eq!(
                agreed.version, 1,
                "cardano-tracer 11.0.1 offers only ForwardingV_1; \
                 got accepted version {}",
                agreed.version
            );
        }
    }

    // `_harness` drops here: SIGKILL + reap cardano-tracer, rm temp dir.
}

/// Recursively collect the contents of every regular file under
/// `root` into one `String`. cardano-tracer writes the per-node
/// log under `<logRoot>/<nodeName>/<file>` and rotates via a
/// symlink, so the exact filename isn't stable — we scan the whole
/// subtree and concatenate.
fn read_all_files_recursive(root: &std::path::Path) -> String {
    let mut acc = String::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return acc;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            acc.push_str(&read_all_files_recursive(&path));
        } else if meta.is_file()
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            acc.push_str(&content);
        }
    }
    acc
}

/// Live conformance test — Yggdrasil forwards a known stream of
/// `TraceObject`s to the upstream `cardano-tracer` binary over the
/// TraceForward mini-protocol (Mux mini-protocol number 2) and
/// verifies delivery by reading cardano-tracer's `FileMode` log
/// output.
///
/// Task #19 (Phase 2.B closeout), remaining-work item:
/// "TraceObject-delivery conformance". This test extends the
/// handshake conformance test one protocol layer deeper. After the
/// `Network.Mux` Handshake completes, cardano-tracer's TraceObject
/// acceptor (it is the protocol *requester* — see
/// `trace-forward/src/Trace/Forward/Protocol/TraceObject/Acceptor.hs`,
/// `SendMsgTraceObjectsRequest TokBlocking …`) sends
/// `MsgTraceObjectsRequest`. Yggdrasil's `run_via_mux` forwarding
/// task batches buffered `TraceObject`s into a `MsgTraceObjectsReply`
/// SDU. cardano-tracer's `traceObjectsHandler` →
/// `writeTraceObjectsToFile` then writes each TraceObject's
/// `toMachine` text verbatim, one line per object, into
/// `<logRoot>/<nodeName>/<file>` (`Cardano.Tracer.Handlers.Logs.File`,
/// `ForMachine` branch).
///
/// Delivery assertion: each forwarded TraceObject carries a unique
/// marker string inside its `to_machine` field; the test polls the
/// `logRoot` subtree and asserts every marker appears.
///
/// # Parity status — GREEN (task #19 closeout, outcome a)
///
/// This test runs live against `cardano-tracer 11.0.1` and
/// converges: cardano-tracer accepts Yggdrasil's
/// `MsgTraceObjectsReply` SDU and writes each forwarded
/// TraceObject's `to_machine` text verbatim into its `FileMode`
/// log. It was un-`ignore`d once the trace-forward CBOR codecs were
/// corrected to the upstream `Codec.Serialise` byte shapes.
///
/// Three distinct CBOR-shape bugs were pinned empirically (the
/// vendored upstream cardano-tracer binary spawned in `AcceptAt`
/// mode, its SDUs captured on the wire) and then fixed by reading
/// the now-vendored upstream source — `trace-dispatcher` is checked
/// out at `.reference-haskell-cardano-node/deps/hermod-tracing/`
/// (it was extracted from `cardano-node` into the standalone
/// `IntersectMBO/hermod-tracing` repo at `trace-dispatcher 2.12.x`)
/// and `Codec.Serialise`'s generic instances come from
/// `well-typed/cborg`'s `Codec/Serialise/Class.hs`:
///
/// 1. **`NumberOfTraceObjects` is generic-`Serialise`-wrapped.**
///    cardano-tracer's acceptor sends `MsgTraceObjectsRequest` as
///    `83 01 f5 82 00 18 64` = `[1, true, [0, 100]]`.
///    `NumberOfTraceObjects` (`newtype { nTraceObjects :: Word16 }`,
///    `deriving anyclass Serialise`) is NOT a bare uint — `cborg`'s
///    `GSerialiseEncode (K1 i a)` wraps the single-field newtype as
///    `[0, word16]`. `mini_protocol.rs` now encodes/decodes that
///    2-element envelope.
///
/// 2. **`Serialise TraceObject` is a 9-element array, not 8.** The
///    upstream record (`Cardano.Logging.Types.TraceObject`, 8
///    fields, `deriving anyclass Serialise`) serialises via
///    `cborg`'s generic product encoder
///    `GSerialiseEncode (f :*: g)` as
///    `encodeListLen (8 + 1) <> encodeWord 0 <> <fields…>` — a
///    9-element array led by the constructor tag `0`. Each field
///    has its own `Serialise` shape: `Maybe a` is `[]`/`[x]`,
///    `[Text]` is `array(0)` (empty) or an indefinite list
///    (non-empty), `SeverityS`/`DetailLevel` are nullary-sum
///    `[idx]`, and `UTCTime` is the extended-time form
///    `tag(1000) {1: secs, -12: psecs}`. `TraceObject::to_cbor`
///    now emits exactly that.
///
/// 3. **The reply list is an indefinite-length list.** The reply is
///    `Serialise [TraceObject]`; `cborg`'s `defaultEncodeList`
///    encodes a non-empty list as `0x9f … 0xff`, not a
///    definite-length array. `mini_protocol::encode_reply` now does
///    so (an empty list stays the definite `array(0)`).
///
/// Self-skips when the upstream binary is absent (same pattern as
/// the handshake test). Override with `YGGDRASIL_CARDANO_TRACER_BIN`.
#[tokio::test]
async fn trace_objects_delivered_to_upstream_cardano_tracer() {
    let Some(bin) = cardano_tracer_bin() else {
        eprintln!(
            "SKIP trace_objects_delivered_to_upstream_cardano_tracer: \
             upstream cardano-tracer binary not found — run \
             `bash scripts/setup-reference.sh` or set \
             YGGDRASIL_CARDANO_TRACER_BIN"
        );
        return;
    };

    let layout = make_tracer_layout("deliver");
    let (socket, _harness) = spawn_and_connect(&bin, &layout).await;

    // Build the Mux connection and run the Handshake initiator side
    // (mini-protocol num 0). version-data is a bare CBOR uint == the
    // configured network magic.
    let conn = Arc::new(MuxConnection::new(Bearer::new(socket)));
    let version_data = {
        let mut enc = Encoder::new();
        enc.unsigned(NETWORK_MAGIC);
        enc.into_bytes()
    };
    let mut versions = BTreeMap::new();
    versions.insert(1u32, version_data);
    let agreed = tokio::time::timeout(
        Duration::from_secs(5),
        conn.run_initiator_handshake(versions),
    )
    .await
    .expect("handshake did not time out")
    .expect("handshake accepted by upstream cardano-tracer");
    assert_eq!(
        agreed.version, 1,
        "cardano-tracer 11.0.1 offers ForwardingV_1"
    );

    // Subscribe to the TraceObject mini-protocol (num 2) BEFORE
    // spawning the read-task: cardano-tracer's acceptor sends its
    // first `MsgTraceObjectsRequest` immediately after the handshake,
    // and `MuxConnection`'s read-task silently drops SDUs with no
    // registered subscriber. We don't consume the requests here —
    // `run_via_mux` pushes replies on its own batch/flush schedule,
    // which a request-tolerant acceptor accepts — but the
    // subscription keeps the inbound request SDUs from being routed
    // into the void in a way that could wedge the bearer.
    let _trace_rx = conn.subscribe(TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM).await;

    // Run the supervised bearer lifecycle — slice (c). `run` forks
    // BOTH the read-task (the demuxer analogue, draining the
    // acceptor's request SDUs + EKG / DataPoint traffic off the
    // bearer) AND the egress scheduler (`muxer`) into one
    // `tokio::task::JoinSet`, and supervises them: the first job to
    // fail tears the other down. This mirrors upstream
    // `Network.Mux.run`, which forks the `muxer`/`demuxer` job pair
    // into a `JobPool` AFTER the Handshake mini-protocol has
    // completed — exactly the ordering here (handshake above → `run`
    // here). The default `EgressConfig` uses a `u16::MAX` `sdu_size`,
    // so a `MsgTraceObjectsReply` SDU is written un-segmented — one
    // `send_sdu` → one SDU on the wire, byte-identical to the
    // pre-scheduler direct write. `run` is spawned detached; the
    // `Harness` SIGKILLs cardano-tracer on drop, which EOFs the
    // bearer and winds the supervised jobs down.
    let mux_run_conn = Arc::clone(&conn);
    let _mux_run = tokio::spawn(async move {
        mux_run_conn
            .run(Some(
                yggdrasil_node_tracer::trace_forwarder::egress::EgressConfig::default(),
            ))
            .await
    });

    // The known TraceObject stream. Each `to_machine` carries a
    // unique marker so the log-file assertion is unambiguous. The
    // process id keeps markers distinct across concurrent test runs.
    let marker_base = format!("ygg-conf-marker-{}", std::process::id());
    let trace_count = 4usize;
    let markers: Vec<String> = (0..trace_count)
        .map(|i| format!("{marker_base}-{i}"))
        .collect();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<TraceObject>();
    let forward_join = tokio::spawn(run_via_mux(
        rx,
        Arc::clone(&conn),
        ForwardingTaskConfig {
            // Small batch + short flush so all four objects ship
            // promptly in one or two MsgTraceObjectsReply SDUs.
            batch_size: trace_count,
            flush_interval: Duration::from_millis(100),
        },
    ));

    for (i, marker) in markers.iter().enumerate() {
        tx.send(TraceObject {
            to_human: None,
            // `ForMachine` logging writes this string verbatim.
            to_machine: format!(r#"{{"marker":"{marker}","i":{i}}}"#),
            to_namespace: vec!["Yggdrasil".into(), "Conformance".into()],
            to_severity: TraceSeverity::Info,
            to_details: TraceDetail::DNormal,
            // (posix_seconds, picoseconds_of_second) — the
            // decomposition `Serialise UTCTime` encodes.
            // 1_778_889_600 = 2026-05-16T00:00:00Z.
            to_timestamp: (1_778_889_600, 0),
            to_hostname: "yggdrasil-conformance".into(),
            to_thread_id: format!("t{i}"),
        })
        .expect("send TraceObject into forwarding channel");
    }
    // Drop the sender so `run_via_mux` flushes the buffer and exits.
    drop(tx);

    // Poll the logRoot subtree for every marker. cardano-tracer's
    // acceptor-side handler timeout is 15s once it decides to stop;
    // 12s of polling stays inside that and is generous for a
    // single-batch flush + file write + hFlush.
    let deadline = Instant::now() + Duration::from_secs(12);
    let (all_delivered, log_dump) = loop {
        let log_dump = read_all_files_recursive(&layout.log_dir);
        if markers.iter().all(|m| log_dump.contains(m.as_str())) {
            break (true, log_dump);
        }
        if Instant::now() >= deadline {
            break (false, log_dump);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    };

    // Surface the forwarding task's outcome — a bearer write error
    // is itself a parity signal worth printing.
    let forward_outcome = tokio::time::timeout(Duration::from_secs(2), forward_join).await;

    assert!(
        all_delivered,
        "TraceObject delivery to upstream cardano-tracer 11.0.1 not observed \
         within 12s — this is a REGRESSION in the trace-forward CBOR codecs \
         (see the test docstring for the three byte shapes this test locks: \
         the `NumberOfTraceObjects` newtype envelope, the 9-element \
         generic-Serialise `TraceObject` array, and the indefinite-length \
         reply list).\n\
         Forwarding task outcome: {forward_outcome:?}  \
         (an `Ok(..)` here means Yggdrasil's `MsgTraceObjectsReply` SDU was \
         written to the bearer successfully — if cardano-tracer received it \
         and still logged nothing, its `runPeer` decoder rejected the reply, \
         so the most likely cause is `TraceObject::to_cbor` or \
         `mini_protocol::encode_reply` drifting off the upstream \
         `Codec.Serialise` byte shape).\n\
         Markers expected: {markers:?}\n\
         logRoot subtree contents ({} bytes):\n{log_dump}",
        log_dump.len(),
    );

    // `_harness` drops here: SIGKILL + reap cardano-tracer, rm temp dir.
}
