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
/// # `#[ignore]` — DOCUMENTED PARITY GAP (task #19, outcome b)
///
/// This test is `#[ignore]`d because, run live against
/// `cardano-tracer 11.0.1`, it does **not** converge: cardano-tracer
/// receives Yggdrasil's `MsgTraceObjectsReply` SDU but writes nothing
/// to its log files. Two distinct upstream-CBOR shape mismatches
/// were pinned empirically (the vendored upstream cardano-tracer
/// binary spawned in `AcceptAt` mode, its SDUs captured on the wire):
///
/// ## 1. `NumberOfTraceObjects` is generic-`Serialise`-wrapped
///
/// cardano-tracer's acceptor sends, on mini-protocol 2, the 7-byte
/// `MsgTraceObjectsRequest` payload:
///
/// ```text
///   83 01 f5 82 00 18 64
///   ── ── ── ───────────
///   │  │  │  └─ array(2): [ uint(0), uint(100) ]   ← NumberOfTraceObjects
///   │  │  └──── true  (TokBlocking)
///   │  └─────── uint(1)  (MsgTraceObjectsRequest tag)
///   └────────── array(3)
/// ```
///
/// `NumberOfTraceObjects` (a `newtype { nTraceObjects :: Word16 }`
/// with `deriving anyclass Serialise` in
/// `trace-forward/src/Trace/Forward/Protocol/TraceObject/Type.hs`)
/// is therefore NOT a bare CBOR uint — the `DeriveAnyClass` generic
/// `Serialise` instance wraps the single-constructor record as
/// `[constructor_tag(0), <word16>]`. Yggdrasil's
/// `mini_protocol::{encode_request,decode_message}` model it as a
/// bare uint; `decode_message` fails on cardano-tracer's real
/// request with `CBOR: type mismatch (expected major 0, got 4)`.
/// (This is a separately-actionable codec bug in `mini_protocol.rs`;
/// it does NOT block delivery because `run_via_mux` never decodes
/// the request — it is recorded here as the on-wire evidence.)
///
/// ## 2. `Serialise TraceObject` shape — unverifiable from vendored source
///
/// The reply list is encoded upstream via `Codec.Serialise`'s
/// `Serialise [lo]` with `lo = Cardano.Logging.TraceObject`
/// (`codecTraceObjectForward CBOR.encode CBOR.decode …` in
/// `Trace.Forward.Run.TraceObject.Forwarder`). Yggdrasil emits, for
/// one TraceObject, a plain 8-element CBOR array, e.g.:
///
/// ```text
///   88 f6 6c 44 49 41 47 4d 41 52 4b 45 52 34 32 81 …
///   ── ── ─────────────────────────────────────────
///   │  │  └─ text "DIAGMARKER42" (toMachine) …
///   │  └──── null  (toHuman = Nothing)
///   └─────── array(8)
/// ```
///
/// wrapped into a 53-byte `MsgTraceObjectsReply` payload
/// `82 03 81 <TraceObject…>`. Yggdrasil's `run_via_mux` writes this
/// SDU successfully (the forwarding task returns `Ok`), and
/// cardano-tracer neither exits nor prints to its inherited stderr —
/// but it logs **0 bytes**. cardano-tracer's `runPeer` typed-protocol
/// decoder silently rejected the reply (the cardano-tracer Mux runs
/// with `Mux.nullTracers`, so a `DeserialiseFailure` in the mp-2
/// mini-protocol thread is swallowed without a stderr line).
///
/// The root cause is that the upstream `Serialise TraceObject`
/// instance lives in `Cardano.Logging.Types`, in the
/// **`trace-dispatcher`** package (cabal name; module namespace
/// `Cardano.Logging`). That package is **NOT vendored** under
/// `.reference-haskell-cardano-node/` — `trace-forward.cabal`
/// depends on `trace-dispatcher ^>= 2.12` but only `trace-forward/`
/// itself is checked out. Finding 1 proves the upstream codec uses
/// `DeriveAnyClass`/`Generic` `Serialise` wrapping for its
/// `trace-forward` newtypes; by the same mechanism `TraceObject`'s
/// record (and its `toTimestamp :: UTCTime`, `toSeverity`,
/// `toDetails` fields) is almost certainly NOT a bare 8-element
/// array. The exact shape cannot be confirmed without the upstream
/// source, so reshaping `TraceObject::to_cbor` here would be a guess.
///
/// ## Unblock path
///
/// Vendor `trace-dispatcher` into `.reference-haskell-cardano-node/`
/// (extend `scripts/setup-reference.sh`) so `Cardano.Logging.Types`'
/// `Serialise TraceObject` instance can be read and mirrored
/// byte-for-byte; then fix `TraceObject::to_cbor` and
/// `mini_protocol.rs`'s `NumberOfTraceObjects` codec and un-`ignore`
/// this test. Alternatively, pin a manual CBOR fixture captured from
/// a live `cardano-node` → `cardano-tracer` session.
///
/// Until then the test stays `#[ignore]`d: it still compiles, still
/// self-skips when the binary is absent, and — run with
/// `--ignored` — drives the live pipeline up to the exact,
/// documented failure point above.
///
/// Self-skips when the upstream binary is absent (same pattern as
/// the handshake test). Override with `YGGDRASIL_CARDANO_TRACER_BIN`.
#[tokio::test]
#[ignore = "documented parity gap: upstream `Serialise TraceObject` byte shape \
            cannot be confirmed — `trace-dispatcher` is not vendored. \
            See the test docstring for the captured on-wire evidence."]
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

    // Spawn the read-task so inbound SDUs (the acceptor's requests,
    // plus EKG / DataPoint mini-protocol traffic) are drained off
    // the bearer instead of back-pressuring it.
    let _read_task = conn.spawn_read_task();

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
            to_timestamp: (2026, 136, 0),
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
         within 12s — this is the DOCUMENTED PARITY GAP this `#[ignore]`d \
         test pins (see the test docstring).\n\
         Forwarding task outcome: {forward_outcome:?}  \
         (an `Ok(..)` here means Yggdrasil's `MsgTraceObjectsReply` SDU was \
         written to the bearer successfully — cardano-tracer received it and \
         still logged nothing, so its `runPeer` decoder silently rejected the \
         reply; the most likely cause is that the upstream `Serialise \
         TraceObject` byte shape disagrees with Yggdrasil's 8-element-array \
         `TraceObject::to_cbor` — see the test docstring).\n\
         Markers expected: {markers:?}\n\
         logRoot subtree contents ({} bytes):\n{log_dump}",
        log_dump.len(),
    );

    // `_harness` drops here: SIGKILL + reap cardano-tracer, rm temp dir.
}
