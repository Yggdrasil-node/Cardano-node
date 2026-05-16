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
use std::time::{Duration, Instant};

use yggdrasil_ledger::cbor::Encoder;
use yggdrasil_node_tracer::trace_forwarder::bearer::Bearer;
use yggdrasil_node_tracer::trace_forwarder::mux_connection::MuxConnection;

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

    // Per-process temp dir for the accept socket + log root.
    let dir = std::env::temp_dir().join(format!("ygg-tracer-conf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let socket_path = dir.join("tracer.socket");
    let log_dir = dir.join("logs");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    // Minimal tracer config: accept on the Unix socket, log to a file
    // dir, no EKG / Prometheus servers (omitted keys → `Nothing`, so
    // no TCP ports are bound). Shape mirrors the vendored
    // install/share/<net>/tracer-config.json.
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

    // Spawn the upstream daemon. stderr is inherited so a config or
    // startup failure is visible under `cargo test -- --nocapture`.
    let child = Command::new(&bin)
        .arg("--config")
        .arg(&config_path)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn cardano-tracer");
    let mut harness = Harness { child, dir };

    // Poll-connect: the socket appears after cardano-tracer's bind +
    // listen. Fail fast if the daemon exits early (bad config).
    let deadline = Instant::now() + Duration::from_secs(10);
    let socket = loop {
        if let Some(status) = harness.child.try_wait().expect("try_wait cardano-tracer") {
            panic!(
                "cardano-tracer exited early ({status}) before accepting a \
                 connection — check the generated config / stderr above"
            );
        }
        match tokio::net::UnixStream::connect(&socket_path).await {
            Ok(stream) => break stream,
            Err(e) => {
                if Instant::now() >= deadline {
                    panic!("cardano-tracer socket never became connectable within 10s: {e}");
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    };

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

    // `harness` drops here: SIGKILL + reap cardano-tracer, rm temp dir.
}
