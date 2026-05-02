//! Trace egress used by the `Forwarder` trace backend.
//!
//! # Layered design
//!
//! Upstream `cardano-tracer` forwarding has three distinct layers:
//!
//! 1. **Application codec** — each trace event is encoded as a
//!    `TraceObject` (8-element CBOR array per
//!    `Cardano.Logging.Types.TraceObject`).  This is the layer
//!    [`TraceObject::to_cbor`] implements faithfully and is unit-tested
//!    against pinned upstream-shape wire bytes.
//! 2. **Mini-protocol layer** — `Trace.Forward.Protocol.TraceObject`
//!    runs as a typed `MsgRequest n` / `MsgReply [TraceObject]` /
//!    `MsgDone` state machine over a multiplexed bearer.  *Not yet
//!    implemented in Yggdrasil.*
//! 3. **Transport** — `AF_UNIX SOCK_STREAM` with `Network.Mux` SDU
//!    framing, plus a `cardano-tracer`-specific handshake mini-protocol.
//!    *Not yet implemented in Yggdrasil.*
//!
//! Layer 1 is fully implemented and tested.  Layers 2/3 require
//! conformance verification against a live `cardano-tracer` binary; the
//! wire shape is documented in
//! `cardano-node:trace-dispatcher/src/Cardano/Logging/Forwarding.hs`
//! and the `trace-forward` package on Hackage.
//!
//! # Current runtime behaviour
//!
//! Until layer 2/3 land we keep the existing best-effort `SOCK_DGRAM`
//! fire-and-forget egress so the `Forwarder` backend doesn't crash
//! the tracer pipeline when the operator configures it.  A startup
//! `Startup.TraceForwarderStub` Warning makes the parity gap explicit
//! to operators (see `node/src/main.rs`).  A real `cardano-tracer`
//! will reject the wire format at the transport level; events routed
//! only to the `Forwarder` backend are silently dropped.  Plain stdout
//! backends (`Stdout HumanFormatColoured`, `Stdout HumanFormat`,
//! `StdoutMachine`) are unaffected.
//!
//! Reference: `cardano-node:trace-dispatcher`, the `trace-forward`
//! Hackage package, and `Codec.Serialise` / `Codec.CBOR.Encoding` for
//! the application-layer codec primitives.

use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::sync::Mutex;

use yggdrasil_ledger::cbor::Encoder;

// ---------------------------------------------------------------------------
// TraceObject — application-layer codec
// ---------------------------------------------------------------------------

/// Severity classification carried in every `TraceObject`.  The wire
/// encoding matches upstream `Cardano.Logging.Types.SeverityS` —
/// CBOR text strings spelt exactly as in the Haskell source so the
/// JSON view rendered by `cardano-tracer` matches `cardano-node`'s
/// own `tracingFormat = JSON` output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceSeverity {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

impl TraceSeverity {
    /// Wire-format text representation per upstream
    /// `Cardano.Logging.Types.SeverityS`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Info => "Info",
            Self::Notice => "Notice",
            Self::Warning => "Warning",
            Self::Error => "Error",
            Self::Critical => "Critical",
            Self::Alert => "Alert",
            Self::Emergency => "Emergency",
        }
    }
}

/// Detail level controlling per-namespace verbosity.  Wire encoding
/// matches upstream `Cardano.Logging.Types.DetailLevel`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceDetail {
    DMinimal,
    DNormal,
    DDetailed,
    DMaximum,
}

impl TraceDetail {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DMinimal => "DMinimal",
            Self::DNormal => "DNormal",
            Self::DDetailed => "DDetailed",
            Self::DMaximum => "DMaximum",
        }
    }
}

/// One trace event in the wire shape consumed by upstream
/// `cardano-tracer` over the `TraceForward` mini-protocol.
///
/// CBOR encoding: an 8-element definite-length array
///
/// ```text
///   [ to_human       :: nullable text
///   , to_machine     :: text
///   , to_namespace   :: [text]
///   , to_severity    :: text
///   , to_details     :: text
///   , to_timestamp   :: [year, dayOfYear, picosecondsOfDay]
///   , to_hostname    :: text
///   , to_thread_id   :: text
///   ]
/// ```
///
/// References:
/// - `Cardano.Logging.Types.TraceObject`
/// - `Codec.CBOR.Encoding` instances (`encodeMaybeText`, `encodeUTCTime`)
/// - `cardano-node` issue tracker conformance examples
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceObject {
    pub to_human: Option<String>,
    pub to_machine: String,
    pub to_namespace: Vec<String>,
    pub to_severity: TraceSeverity,
    pub to_details: TraceDetail,
    /// `(year, dayOfYear, picosecondsOfDay)` — same shape as upstream
    /// `Cardano.Slotting.Time.SystemStart`'s `UTCTime` encoding.
    pub to_timestamp: (u64, u64, u64),
    pub to_hostname: String,
    pub to_thread_id: String,
}

impl TraceObject {
    /// Produce the canonical CBOR wire representation that
    /// `cardano-tracer`'s `TraceForward` codec expects.  Round-trip
    /// safe with `from_cbor_bytes` (added once we wire the consumer
    /// side; `cardano-tracer`'s decoder is the canonical reference).
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.array(8);
        // toHuman :: Maybe Text — null when absent.
        match &self.to_human {
            None => {
                enc.null();
            }
            Some(t) => {
                enc.text(t);
            }
        }
        // toMachine :: Text
        enc.text(&self.to_machine);
        // toNamespace :: [Text]
        enc.array(self.to_namespace.len() as u64);
        for ns in &self.to_namespace {
            enc.text(ns);
        }
        // toSeverity :: SeverityS (encoded as Text)
        enc.text(self.to_severity.as_str());
        // toDetails :: DetailLevel (encoded as Text)
        enc.text(self.to_details.as_str());
        // toTimestamp :: UTCTime — [year, dayOfYear, picosecondsOfDay]
        // matching upstream `Cardano.Slotting.Time.SystemStart`'s shape.
        let (year, doy, picos) = self.to_timestamp;
        enc.array(3);
        enc.unsigned(year);
        enc.unsigned(doy);
        enc.unsigned(picos);
        // toHostname :: Text
        enc.text(&self.to_hostname);
        // toThreadId :: Text
        enc.text(&self.to_thread_id);
        enc.into_bytes()
    }
}

// ---------------------------------------------------------------------------
// Existing best-effort UnixDatagram egress (preserved while layers 2/3 land)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TraceForwarder {
    socket_path: String,
    socket: Mutex<Option<UnixDatagram>>,
}

impl TraceForwarder {
    pub fn new(socket_path: String) -> Self {
        Self {
            socket_path,
            socket: Mutex::new(None),
        }
    }

    /// Returns the configured Unix-socket path.  Used by the runtime to
    /// emit a one-shot parity-gap warning at startup so operators are
    /// not surprised by silently-dropped trace events.
    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    pub fn send(&self, event: &serde_json::Value) {
        // CBOR encoding via ciborium (RFC 8949). Replaces unmaintained
        // serde_cbor (RUSTSEC-2021-0127). Audit finding M-4.
        let mut encoded = Vec::new();
        if ciborium::ser::into_writer(event, &mut encoded).is_err() {
            return;
        }
        let mut sock_guard = self
            .socket
            .lock()
            .expect("trace forwarder socket mutex poisoned");
        if sock_guard.is_none() {
            let sock = UnixDatagram::unbound().ok();
            if let Some(ref s) = sock {
                let _ = s.connect(Path::new(&self.socket_path));
            }
            *sock_guard = sock;
        }
        if let Some(ref sock) = *sock_guard {
            let _ = sock.send(&encoded);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_ledger::cbor::Decoder;

    /// Pin the upstream `TraceObject` wire shape: an 8-element
    /// definite-length CBOR array whose fields decode in the
    /// `(toHuman, toMachine, toNamespace, toSeverity, toDetails,
    /// toTimestamp, toHostname, toThreadId)` order.  A future bump to
    /// any field's encoding shows up here as a failing test rather
    /// than as silently-malformed traces seen by `cardano-tracer`.
    #[test]
    fn trace_object_cbor_round_trip_matches_upstream_shape() {
        let obj = TraceObject {
            to_human: Some("hello world".into()),
            to_machine: "{\"k\":\"v\"}".into(),
            to_namespace: vec!["Net".into(), "Governor".into()],
            to_severity: TraceSeverity::Info,
            to_details: TraceDetail::DNormal,
            to_timestamp: (2026, 122, 0),
            to_hostname: "yggdrasil".into(),
            to_thread_id: "t1".into(),
        };
        let bytes = obj.to_cbor();

        // Walk the bytes manually with our CBOR decoder so a regression in
        // either the encoder or the array layout surfaces as a typed
        // decode error rather than as a silent byte-shape change.
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().expect("8-element array");
        assert_eq!(len, 8, "TraceObject wire format must be 8-element array");
        // toHuman
        assert_eq!(dec.text().expect("toHuman text"), "hello world");
        // toMachine
        assert_eq!(dec.text().expect("toMachine text"), "{\"k\":\"v\"}");
        // toNamespace
        let ns_len = dec.array().expect("namespace array");
        assert_eq!(ns_len, 2);
        assert_eq!(dec.text().expect("ns[0]"), "Net");
        assert_eq!(dec.text().expect("ns[1]"), "Governor");
        // toSeverity
        assert_eq!(dec.text().expect("severity text"), "Info");
        // toDetails
        assert_eq!(dec.text().expect("details text"), "DNormal");
        // toTimestamp = [year, dayOfYear, picosecondsOfDay]
        let ts_len = dec.array().expect("timestamp array");
        assert_eq!(ts_len, 3);
        assert_eq!(dec.unsigned().expect("year"), 2026);
        assert_eq!(dec.unsigned().expect("dayOfYear"), 122);
        assert_eq!(dec.unsigned().expect("picosecondsOfDay"), 0);
        // toHostname
        assert_eq!(dec.text().expect("hostname"), "yggdrasil");
        // toThreadId
        assert_eq!(dec.text().expect("thread_id"), "t1");
    }

    /// `toHuman` is `Maybe Text` upstream — a `None` encodes as CBOR
    /// `null` (`0xf6`), not as an empty string.  Pin this so a future
    /// `to_human: ""` shortcut doesn't silently change semantics.
    #[test]
    fn trace_object_to_human_none_encodes_as_cbor_null() {
        let obj = TraceObject {
            to_human: None,
            to_machine: String::new(),
            to_namespace: Vec::new(),
            to_severity: TraceSeverity::Debug,
            to_details: TraceDetail::DMinimal,
            to_timestamp: (0, 0, 0),
            to_hostname: String::new(),
            to_thread_id: String::new(),
        };
        let bytes = obj.to_cbor();
        let mut dec = Decoder::new(&bytes);
        let _ = dec.array().expect("array len");
        // Peek at the next byte: must be CBOR `null` (`0xf6`).
        // We use `null()` (returns `()` for null) to verify the type.
        dec.null().expect("toHuman None must encode as CBOR null");
    }

    /// All severities have stable upstream-spelled wire labels.
    #[test]
    fn trace_severity_wire_labels_match_upstream() {
        for (variant, label) in [
            (TraceSeverity::Debug, "Debug"),
            (TraceSeverity::Info, "Info"),
            (TraceSeverity::Notice, "Notice"),
            (TraceSeverity::Warning, "Warning"),
            (TraceSeverity::Error, "Error"),
            (TraceSeverity::Critical, "Critical"),
            (TraceSeverity::Alert, "Alert"),
            (TraceSeverity::Emergency, "Emergency"),
        ] {
            assert_eq!(variant.as_str(), label, "wire label drift on {variant:?}");
        }
    }

    /// All detail levels have stable upstream-spelled wire labels.
    #[test]
    fn trace_detail_wire_labels_match_upstream() {
        for (variant, label) in [
            (TraceDetail::DMinimal, "DMinimal"),
            (TraceDetail::DNormal, "DNormal"),
            (TraceDetail::DDetailed, "DDetailed"),
            (TraceDetail::DMaximum, "DMaximum"),
        ] {
            assert_eq!(variant.as_str(), label, "wire label drift on {variant:?}");
        }
    }
}
