//! Trace egress used by the `Forwarder` trace backend.
//!
//! # Layered design
//!
//! Upstream `cardano-tracer` forwarding has three distinct layers:
//!
//! 1. **Application codec** — each trace event is encoded as a
//!    `TraceObject` via the generic `Codec.Serialise` instance
//!    `Cardano.Logging.Types` derives (`deriving anyclass Serialise`):
//!    a 9-element CBOR array `[0, …8 fields…]`.  This is the layer
//!    [`TraceObject::to_cbor`] implements faithfully and is unit-tested
//!    against pinned upstream-shape wire bytes.
//! 2. **Mini-protocol layer** — `Trace.Forward.Protocol.TraceObject`
//!    runs as a typed `MsgRequest n` / `MsgReply [TraceObject]` /
//!    `MsgDone` state machine over a multiplexed bearer. The CBOR
//!    codec for the three message types lives in [`mini_protocol`]
//!    (Wave 6 PR 17 Phase 2.B). The driving state-machine + reply-
//!    list streaming (TraceObject decoder + non-empty-blocking
//!    enforcement on live traces) lands when the transport is wired.
//! 3. **Transport** — `AF_UNIX SOCK_STREAM` with `Network.Mux` SDU
//!    framing, plus a `cardano-tracer`-specific handshake mini-protocol.
//!    The SDU codec lives in [`mux`] (Wave 6 PR 17 Phase 2.B). The
//!    full Mux state-machine (ingress / egress / scheduler /
//!    handshake-driver / per-bearer task lifecycle) is a follow-on
//!    once a binary opens an actual `AF_UNIX SOCK_STREAM` against a
//!    live cardano-tracer.
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
//! # Wiring the full Mux Layer 2/3 forwarder (Wave 6 PR 17 Phase 2.B)
//!
//! Codecs + bearer + dispatchers for Layers 1/2/3 are now landed; the
//! pipeline composes through a single shared [`mux_connection::MuxConnection`]
//! in ~10 binary-setup lines:
//!
//! ```ignore
//! use std::collections::BTreeMap;
//! use std::sync::Arc;
//! use tokio::net::UnixStream;
//! use tokio::sync::mpsc;
//! use tracing_subscriber::prelude::*;
//!
//! let socket = UnixStream::connect("/run/cardano-tracer.sock").await?;
//! let bearer = bearer::Bearer::new(socket);
//! let mux = Arc::new(mux_connection::MuxConnection::new(bearer));
//!
//! // 1. Run handshake initiator side (mini-protocol num 0).
//! let mut versions = BTreeMap::new();
//! versions.insert(1u32, encode_network_magic(764_824_073));
//! let _agreed = mux.run_initiator_handshake(versions).await?;
//!
//! // 2. Spawn the read-task so subsequent inbound SDUs dispatch.
//! let _read_task = mux.spawn_read_task();
//!
//! // 3. Wire the tracing-subscriber Layer that builds TraceObjects.
//! let (tx, rx) = mpsc::unbounded_channel();
//! let layer = layer::TraceForwardingLayer::new(tx, "yggdrasil-node-01".into());
//! tracing_subscriber::registry().with(layer).init();
//!
//! // 4. Spawn the forwarding task that drains the channel into SDUs.
//! tokio::spawn(forwarding_task::run_via_mux(
//!     rx,
//!     Arc::clone(&mux),
//!     forwarding_task::ForwardingTaskConfig::default(),
//! ));
//! ```
//!
//! The remaining piece for full Network.Mux parity (per-mini-protocol
//! limits, scheduler fairness, bearer-task supervision, live binary-
//! against-binary conformance soak) is the single open item in
//! `docs/TECH-DEBT.md`'s "cardano-tracer Mux Layer 2/3" entry.
//!
//! Reference: `cardano-node:trace-dispatcher`, the `trace-forward`
//! Hackage package, and `Codec.Serialise` / `Codec.CBOR.Encoding` for
//! the application-layer codec primitives.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side cardano-tracer
//! trace-forwarder client (Phase D R274 work — defers the
//! actual mini-protocol implementation). Mirrors upstream
//! `Cardano.Logging.Forwarding.hs` (cardano-tracer side) plus
//! `Ouroboros.Network.Protocol.TraceForwarding.Client.hs`.
//! Upstream-side splits client / protocol / types into separate
//! modules; Yggdrasil collapses into one file pending Phase D.

use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::sync::Mutex;

use yggdrasil_ledger::cbor::Encoder;

// Wave 6 PR 17 Phase 2.B — Layer 2 + Layer 3 codecs + bearer +
// tracing::Event→TraceObject builder + write-only forwarding task
// + tracing-subscriber Layer<S> adapter + Handshake codec +
// Handshake state-machine driver + minimal Mux dispatcher.
pub mod bearer;
pub mod egress;
pub mod event_builder;
pub mod forwarding_task;
pub mod handshake;
pub mod handshake_driver;
pub mod layer;
pub mod mini_protocol;
pub mod mux;
pub mod mux_connection;

// ---------------------------------------------------------------------------
// TraceObject — application-layer codec
// ---------------------------------------------------------------------------

/// Severity classification carried in every `TraceObject`.
///
/// Mirrors upstream `Cardano.Logging.Types.SeverityS` (in the
/// `trace-dispatcher` package, vendored at
/// `.reference-haskell-cardano-node/deps/hermod-tracing/trace-dispatcher/src/Cardano/Logging/Types.hs`).
/// Upstream derives `Serialise` via `deriving anyclass` over the
/// 8-constructor nullary sum: `cborg`'s generic `GSerialiseSum`
/// encodes a nullary constructor as a 1-element CBOR array carrying
/// the constructor index (`encodeListLen 1 <> encodeWord conNumber`)
/// — NOT a CBOR text string. The index follows the declaration
/// order: `Debug = 0 … Emergency = 7`.
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
    /// Human-readable label per upstream `SeverityS`'s `Show`
    /// instance. Used for diagnostics / display only — it is NOT the
    /// CBOR wire form (see [`Self::constructor_index`]).
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

    /// Generic-`Serialise` constructor index — the value `cborg`'s
    /// `GSerialiseSum.conNumber` emits for this nullary constructor.
    pub fn constructor_index(self) -> u64 {
        match self {
            Self::Debug => 0,
            Self::Info => 1,
            Self::Notice => 2,
            Self::Warning => 3,
            Self::Error => 4,
            Self::Critical => 5,
            Self::Alert => 6,
            Self::Emergency => 7,
        }
    }

    /// Inverse of [`Self::constructor_index`].
    pub fn from_constructor_index(idx: u64) -> Option<Self> {
        Some(match idx {
            0 => Self::Debug,
            1 => Self::Info,
            2 => Self::Notice,
            3 => Self::Warning,
            4 => Self::Error,
            5 => Self::Critical,
            6 => Self::Alert,
            7 => Self::Emergency,
            _ => return None,
        })
    }
}

/// Detail level controlling per-namespace verbosity.
///
/// Mirrors upstream `Cardano.Logging.Types.DetailLevel`. Same
/// generic-`Serialise` nullary-sum encoding as [`TraceSeverity`]: a
/// 1-element CBOR array carrying the constructor index
/// `DMinimal = 0 … DMaximum = 3`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceDetail {
    DMinimal,
    DNormal,
    DDetailed,
    DMaximum,
}

impl TraceDetail {
    /// Human-readable label per upstream `DetailLevel`'s `Show`
    /// instance. Diagnostics / display only — not the CBOR wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DMinimal => "DMinimal",
            Self::DNormal => "DNormal",
            Self::DDetailed => "DDetailed",
            Self::DMaximum => "DMaximum",
        }
    }

    /// Generic-`Serialise` constructor index.
    pub fn constructor_index(self) -> u64 {
        match self {
            Self::DMinimal => 0,
            Self::DNormal => 1,
            Self::DDetailed => 2,
            Self::DMaximum => 3,
        }
    }

    /// Inverse of [`Self::constructor_index`].
    pub fn from_constructor_index(idx: u64) -> Option<Self> {
        Some(match idx {
            0 => Self::DMinimal,
            1 => Self::DNormal,
            2 => Self::DDetailed,
            3 => Self::DMaximum,
            _ => return None,
        })
    }
}

/// One trace event in the wire shape consumed by upstream
/// `cardano-tracer` over the `TraceForward` mini-protocol.
///
/// # Wire format — generic `Serialise` for `TraceObject`
///
/// The upstream record (`Cardano.Logging.Types.TraceObject`, in the
/// `trace-dispatcher` package) derives its `Serialise` instance via
/// `deriving anyclass (Serialise, NFData)` over an 8-field
/// single-constructor record:
///
/// ```haskell
/// data TraceObject = TraceObject
///   { toHuman :: !(Maybe Text), toMachine :: !Text, toNamespace :: ![Text]
///   , toSeverity :: !SeverityS, toDetails :: !DetailLevel
///   , toTimestamp :: !UTCTime, toHostname :: !Text, toThreadId :: !Text }
///   deriving stock (Eq, Show, Generic)
///   deriving anyclass (Serialise, NFData)
/// ```
///
/// `cborg`'s generic product encoder (`GSerialiseEncode (f :*: g)`)
/// serialises a single-constructor record as
/// `encodeListLen (nFields + 1) <> encodeWord 0 <> <fields…>` — i.e.
/// a **9-element CBOR array** whose first element is the constructor
/// tag `0`, followed by the 8 fields encoded sequentially with no
/// per-field wrapper:
///
/// ```text
///   [ 0                                ; constructor tag (uint)
///   , to_human     :: [] | [text]      ; Serialise (Maybe a): Nothing=[], Just x=[x]
///   , to_machine   :: text
///   , to_namespace :: [] | (_ text*)   ; Serialise [a]: []=array(0), else indef 0x9f..0xff
///   , to_severity  :: [uint]           ; nullary-sum: 1-elem array of constructor index
///   , to_details   :: [uint]           ; nullary-sum: 1-elem array of constructor index
///   , to_timestamp :: 1000({1: secs, -12: psecs})  ; Serialise UTCTime, tag 1000 map(2)
///   , to_hostname  :: text
///   , to_thread_id :: text
///   ]
/// ```
///
/// References:
/// - `Cardano.Logging.Types.TraceObject` (vendored under
///   `deps/hermod-tracing/trace-dispatcher/src/Cardano/Logging/Types.hs`)
/// - `Codec.Serialise.Class` generic instances (`well-typed/cborg`):
///   `GSerialiseEncode (f :*: g)`, `Serialise (Maybe a)`,
///   `Serialise [a]` / `defaultEncodeList`, `GSerialiseSum`,
///   `Serialise UTCTime`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceObject {
    pub to_human: Option<String>,
    pub to_machine: String,
    pub to_namespace: Vec<String>,
    pub to_severity: TraceSeverity,
    pub to_details: TraceDetail,
    /// `(posix_seconds, picoseconds_of_second)` — the decomposition
    /// `Codec.Serialise`'s `Serialise UTCTime` instance encodes:
    /// `properFraction (utcTimeToPOSIXSeconds t)` yields whole
    /// seconds since the 1970 POSIX epoch plus a fractional part,
    /// and `psecs = round (frac * 1e12)`. The first element is
    /// therefore POSIX seconds (always non-negative for node
    /// timestamps); the second is `0 ≤ picos < 1_000_000_000_000`.
    pub to_timestamp: (u64, u64),
    pub to_hostname: String,
    pub to_thread_id: String,
}

impl TraceObject {
    /// Produce the canonical CBOR wire representation that
    /// `cardano-tracer`'s `TraceForward` codec expects.  Round-trip
    /// safe with [`Self::from_cbor_bytes`].
    ///
    /// Byte-for-byte equivalent of `Codec.Serialise`'s generic
    /// `encode :: TraceObject -> Encoding` — see the type-level
    /// docstring for the derivation.
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        // Single-constructor record → 9-element array: constructor
        // tag 0 followed by the 8 fields.
        enc.array(9);
        enc.unsigned(0);
        // toHuman :: Maybe Text — Serialise (Maybe a):
        //   Nothing  → encodeListLen 0
        //   Just x   → encodeListLen 1 <> encode x
        match &self.to_human {
            None => {
                enc.array(0);
            }
            Some(t) => {
                enc.array(1);
                enc.text(t);
            }
        }
        // toMachine :: Text → encodeString.
        enc.text(&self.to_machine);
        // toNamespace :: [Text] — Serialise [a] / defaultEncodeList:
        //   []        → encodeListLen 0
        //   non-empty → encodeListLenIndef <> elems <> encodeBreak
        if self.to_namespace.is_empty() {
            enc.array(0);
        } else {
            enc.array_indef();
            for ns in &self.to_namespace {
                enc.text(ns);
            }
            enc.break_stop();
        }
        // toSeverity :: SeverityS — generic nullary-sum: 1-element
        // array carrying the constructor index.
        enc.array(1);
        enc.unsigned(self.to_severity.constructor_index());
        // toDetails :: DetailLevel — same nullary-sum encoding.
        enc.array(1);
        enc.unsigned(self.to_details.constructor_index());
        // toTimestamp :: UTCTime — Serialise UTCTime extended-time
        // form: tag 1000, map of 2 entries { 1: secs, -12: psecs }.
        let (secs, psecs) = self.to_timestamp;
        enc.tag(1000);
        enc.map(2);
        enc.unsigned(1);
        // `secs` is encoded via `encodeInt64`; for the non-negative
        // POSIX seconds of any real node timestamp that is identical
        // to a major-0 unsigned encoding.
        enc.unsigned(secs);
        enc.integer(-12);
        // `psecs` is `encodeWord64` — a plain major-0 unsigned.
        enc.unsigned(psecs);
        // toHostname :: Text
        enc.text(&self.to_hostname);
        // toThreadId :: Text
        enc.text(&self.to_thread_id);
        enc.into_bytes()
    }

    /// Decode a CBOR-encoded `TraceObject` per the upstream
    /// generic-`Serialise` wire shape produced by [`Self::to_cbor`].
    /// Inverse of the encoder.
    ///
    /// Errors out on:
    ///   - outer array length ≠ 9,
    ///   - constructor tag ≠ 0,
    ///   - a `to_human` `Maybe` envelope whose array length ≠ 0/1,
    ///   - a `to_severity` / `to_details` constructor index outside
    ///     the known range,
    ///   - a `to_timestamp` not encoded as tag-1000 `{1: …, -12: …}`,
    ///   - any underlying CBOR decode failure.
    pub fn from_cbor_bytes(bytes: &[u8]) -> Result<Self, TraceObjectDecodeError> {
        use yggdrasil_ledger::cbor::Decoder;

        let mut dec = Decoder::new(bytes);
        let outer_len = dec
            .array()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("outer array: {e}")))?;
        if outer_len != 9 {
            return Err(TraceObjectDecodeError::WrongOuterArity(outer_len));
        }
        // Constructor tag — single-constructor record → always 0.
        let ctor = dec
            .unsigned()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("constructor tag: {e}")))?;
        if ctor != 0 {
            return Err(TraceObjectDecodeError::WrongConstructorTag(ctor));
        }

        // Field 1: to_human :: Maybe Text — Serialise (Maybe a):
        //   array(0) → Nothing ; array(1) <text> → Just.
        let human_len = dec
            .array()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_human Maybe array: {e}")))?;
        let to_human = match human_len {
            0 => None,
            1 => {
                let s = dec
                    .text()
                    .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_human text: {e}")))?;
                Some(s.to_owned())
            }
            other => return Err(TraceObjectDecodeError::WrongMaybeArity(other)),
        };

        // Field 2: to_machine :: Text.
        let to_machine = dec
            .text()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_machine: {e}")))?
            .to_owned();

        // Field 3: to_namespace :: [Text] — Serialise [a] is either a
        // definite array(0) (the empty-list case) or an indefinite
        // 0x9f…0xff list (the non-empty case). `array_begin` handles
        // both: `Some(n)` definite, `None` indefinite.
        let mut to_namespace = Vec::new();
        match dec
            .array_begin()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_namespace array: {e}")))?
        {
            Some(n) => {
                for _ in 0..n {
                    let s = dec.text().map_err(|e| {
                        TraceObjectDecodeError::Cbor(format!("to_namespace element: {e}"))
                    })?;
                    to_namespace.push(s.to_owned());
                }
            }
            None => {
                while !dec.is_break() {
                    let s = dec.text().map_err(|e| {
                        TraceObjectDecodeError::Cbor(format!("to_namespace element: {e}"))
                    })?;
                    to_namespace.push(s.to_owned());
                }
                dec.consume_break().map_err(|e| {
                    TraceObjectDecodeError::Cbor(format!("to_namespace break: {e}"))
                })?;
            }
        }

        // Field 4: to_severity :: SeverityS — 1-element array of the
        // generic constructor index.
        let sev_len = dec
            .array()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_severity array: {e}")))?;
        if sev_len != 1 {
            return Err(TraceObjectDecodeError::WrongEnumArity {
                field: "to_severity",
                got: sev_len,
            });
        }
        let sev_idx = dec
            .unsigned()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_severity index: {e}")))?;
        let to_severity = TraceSeverity::from_constructor_index(sev_idx)
            .ok_or(TraceObjectDecodeError::UnknownSeverity(sev_idx))?;

        // Field 5: to_details :: DetailLevel — same nullary-sum form.
        let det_len = dec
            .array()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_details array: {e}")))?;
        if det_len != 1 {
            return Err(TraceObjectDecodeError::WrongEnumArity {
                field: "to_details",
                got: det_len,
            });
        }
        let det_idx = dec
            .unsigned()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_details index: {e}")))?;
        let to_details = TraceDetail::from_constructor_index(det_idx)
            .ok_or(TraceObjectDecodeError::UnknownDetail(det_idx))?;

        // Field 6: to_timestamp :: UTCTime — Serialise UTCTime
        // extended-time form: tag 1000, map of 2 entries keyed
        // `1` (seconds, signed) and `-12` (picoseconds, unsigned).
        let ts_tag = dec
            .tag()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("timestamp tag: {e}")))?;
        if ts_tag != 1000 {
            return Err(TraceObjectDecodeError::WrongTimestampTag(ts_tag));
        }
        let ts_map_len = dec
            .map()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("timestamp map: {e}")))?;
        if ts_map_len != 2 {
            return Err(TraceObjectDecodeError::WrongTimestampArity(ts_map_len));
        }
        let k0 = dec
            .integer()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("timestamp key 0: {e}")))?;
        if k0 != 1 {
            return Err(TraceObjectDecodeError::Cbor(format!(
                "timestamp: expected map key 1, got {k0}"
            )));
        }
        let secs_i = dec
            .integer()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("timestamp seconds: {e}")))?;
        let secs = u64::try_from(secs_i).map_err(|_| {
            TraceObjectDecodeError::Cbor(format!("timestamp seconds {secs_i} is negative"))
        })?;
        let k1 = dec
            .integer()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("timestamp key 1: {e}")))?;
        if k1 != -12 {
            return Err(TraceObjectDecodeError::Cbor(format!(
                "timestamp: expected map key -12, got {k1}"
            )));
        }
        let picos = dec
            .unsigned()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("timestamp picoseconds: {e}")))?;

        // Field 7: to_hostname :: Text.
        let to_hostname = dec
            .text()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_hostname: {e}")))?
            .to_owned();

        // Field 8: to_thread_id :: Text.
        let to_thread_id = dec
            .text()
            .map_err(|e| TraceObjectDecodeError::Cbor(format!("to_thread_id: {e}")))?
            .to_owned();

        Ok(TraceObject {
            to_human,
            to_machine,
            to_namespace,
            to_severity,
            to_details,
            to_timestamp: (secs, picos),
            to_hostname,
            to_thread_id,
        })
    }
}

/// Errors surfaced from [`TraceObject::from_cbor_bytes`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceObjectDecodeError {
    /// Underlying CBOR decode failure with the field name + reason.
    Cbor(String),
    /// Outer array length wasn't 9 (constructor tag + 8 fields).
    WrongOuterArity(u64),
    /// Constructor tag wasn't 0 (the single `TraceObject` constructor).
    WrongConstructorTag(u64),
    /// A `Maybe`-field envelope had an array length other than 0 or 1.
    WrongMaybeArity(u64),
    /// A nullary-sum-encoded enum field had an array length ≠ 1.
    WrongEnumArity {
        /// Which field carried the malformed enum envelope.
        field: &'static str,
        /// The decoded array length.
        got: u64,
    },
    /// `to_timestamp` wasn't tagged with the extended-time tag 1000.
    WrongTimestampTag(u64),
    /// `to_timestamp` map length wasn't 2.
    WrongTimestampArity(u64),
    /// `to_severity` constructor index was outside the known range.
    UnknownSeverity(u64),
    /// `to_details` constructor index was outside the known range.
    UnknownDetail(u64),
}

impl core::fmt::Display for TraceObjectDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Cbor(msg) => write!(f, "CBOR decode error: {msg}"),
            Self::WrongOuterArity(n) => write!(
                f,
                "TraceObject outer array length must be 9 (constructor tag + 8 fields) \
                 per upstream generic-Serialise wire format; got {n}"
            ),
            Self::WrongConstructorTag(n) => write!(
                f,
                "TraceObject constructor tag must be 0 (single constructor); got {n}"
            ),
            Self::WrongMaybeArity(n) => write!(
                f,
                "Maybe-field envelope array length must be 0 (Nothing) or 1 (Just); got {n}"
            ),
            Self::WrongEnumArity { field, got } => write!(
                f,
                "{field} nullary-sum enum envelope array length must be 1; got {got}"
            ),
            Self::WrongTimestampTag(n) => write!(
                f,
                "timestamp must carry the extended-time CBOR tag 1000; got tag {n}"
            ),
            Self::WrongTimestampArity(n) => write!(
                f,
                "timestamp extended-time map length must be 2 (keys 1 and -12); got {n}"
            ),
            Self::UnknownSeverity(idx) => {
                write!(f, "unknown SeverityS constructor index: {idx}")
            }
            Self::UnknownDetail(idx) => {
                write!(f, "unknown DetailLevel constructor index: {idx}")
            }
        }
    }
}

impl std::error::Error for TraceObjectDecodeError {}

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

    /// Pin the upstream `TraceObject` wire shape produced by the
    /// `Codec.Serialise` generic instance: a 9-element definite-length
    /// CBOR array whose first element is the constructor tag `0`,
    /// followed by the 8 fields in `(toHuman, toMachine, toNamespace,
    /// toSeverity, toDetails, toTimestamp, toHostname, toThreadId)`
    /// order.  A future bump to any field's encoding shows up here as
    /// a failing test rather than as silently-malformed traces seen by
    /// `cardano-tracer`.
    #[test]
    fn trace_object_cbor_round_trip_matches_upstream_shape() {
        let obj = TraceObject {
            to_human: Some("hello world".into()),
            to_machine: "{\"k\":\"v\"}".into(),
            to_namespace: vec!["Net".into(), "Governor".into()],
            to_severity: TraceSeverity::Info,
            to_details: TraceDetail::DNormal,
            // 1_767_312_000 = 2026-01-02T00:00:00Z; 250 ms → 2.5e11 ps.
            to_timestamp: (1_767_312_000, 250_000_000_000),
            to_hostname: "yggdrasil".into(),
            to_thread_id: "t1".into(),
        };
        let bytes = obj.to_cbor();

        // Walk the bytes manually with our CBOR decoder so a regression in
        // either the encoder or the array layout surfaces as a typed
        // decode error rather than as a silent byte-shape change.
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().expect("9-element array");
        assert_eq!(
            len, 9,
            "TraceObject generic-Serialise wire format must be 9-element array"
        );
        // constructor tag
        assert_eq!(dec.unsigned().expect("constructor tag"), 0);
        // toHuman :: Maybe Text — Just → array(1) <text>.
        assert_eq!(dec.array().expect("toHuman Maybe array"), 1);
        assert_eq!(dec.text().expect("toHuman text"), "hello world");
        // toMachine
        assert_eq!(dec.text().expect("toMachine text"), "{\"k\":\"v\"}");
        // toNamespace — non-empty → indefinite-length list 0x9f..0xff.
        assert_eq!(
            dec.array_begin().expect("namespace array"),
            None,
            "non-empty [Text] must encode as an indefinite-length list"
        );
        assert_eq!(dec.text().expect("ns[0]"), "Net");
        assert_eq!(dec.text().expect("ns[1]"), "Governor");
        dec.consume_break().expect("namespace break");
        // toSeverity — 1-element array of constructor index (Info = 1).
        assert_eq!(dec.array().expect("severity array"), 1);
        assert_eq!(dec.unsigned().expect("severity index"), 1);
        // toDetails — 1-element array of constructor index (DNormal = 1).
        assert_eq!(dec.array().expect("details array"), 1);
        assert_eq!(dec.unsigned().expect("details index"), 1);
        // toTimestamp — Serialise UTCTime: tag 1000, map { 1: secs, -12: psecs }.
        assert_eq!(dec.tag().expect("timestamp tag"), 1000);
        assert_eq!(dec.map().expect("timestamp map"), 2);
        assert_eq!(dec.integer().expect("ts key 0"), 1);
        assert_eq!(dec.integer().expect("ts seconds"), 1_767_312_000);
        assert_eq!(dec.integer().expect("ts key 1"), -12);
        assert_eq!(dec.unsigned().expect("ts picoseconds"), 250_000_000_000);
        // toHostname
        assert_eq!(dec.text().expect("hostname"), "yggdrasil");
        // toThreadId
        assert_eq!(dec.text().expect("thread_id"), "t1");
    }

    /// Pin the exact byte string for a minimal `TraceObject` so the
    /// generic-`Serialise` envelope (constructor tag, `Nothing` =
    /// `array(0)`, empty `[Text]` = `array(0)`, nullary-sum enum =
    /// `array(1) <idx>`, `UTCTime` = `tag(1000) map(2)`) is locked
    /// byte-for-byte against `Codec.Serialise`.
    #[test]
    fn trace_object_cbor_exact_byte_shape_minimal() {
        let obj = TraceObject {
            to_human: None,
            to_machine: String::new(),
            to_namespace: Vec::new(),
            to_severity: TraceSeverity::Debug, // index 0
            to_details: TraceDetail::DMinimal, // index 0
            to_timestamp: (0, 0),
            to_hostname: String::new(),
            to_thread_id: String::new(),
        };
        // 0x89                  array(9)
        // 0x00                  constructor tag 0
        // 0x80                  toHuman  = Nothing → array(0)
        // 0x60                  toMachine = "" (text len 0)
        // 0x80                  toNamespace = [] → array(0)
        // 0x81 0x00             toSeverity = array(1) [uint 0]
        // 0x81 0x00             toDetails  = array(1) [uint 0]
        // 0xd9 0x03 0xe8        tag(1000)
        // 0xa2                  map(2)
        // 0x01 0x00             key 1 → secs 0
        // 0x2b 0x00             key -12 (0x2b = nint 11) → psecs 0
        // 0x60                  toHostname = ""
        // 0x60                  toThreadId = ""
        let expected: Vec<u8> = vec![
            0x89, 0x00, 0x80, 0x60, 0x80, 0x81, 0x00, 0x81, 0x00, 0xd9, 0x03, 0xe8, 0xa2, 0x01,
            0x00, 0x2b, 0x00, 0x60, 0x60,
        ];
        assert_eq!(obj.to_cbor(), expected, "minimal TraceObject byte shape");
    }

    /// `toHuman` is `Maybe Text` upstream — `Serialise (Maybe a)`
    /// encodes `Nothing` as `encodeListLen 0` (a CBOR `array(0)`,
    /// byte `0x80`), NOT as CBOR `null` and NOT as an empty string.
    #[test]
    fn trace_object_to_human_none_encodes_as_empty_array() {
        let obj = TraceObject {
            to_human: None,
            to_machine: String::new(),
            to_namespace: Vec::new(),
            to_severity: TraceSeverity::Debug,
            to_details: TraceDetail::DMinimal,
            to_timestamp: (0, 0),
            to_hostname: String::new(),
            to_thread_id: String::new(),
        };
        let bytes = obj.to_cbor();
        let mut dec = Decoder::new(&bytes);
        let _ = dec.array().expect("outer array");
        let _ = dec.unsigned().expect("constructor tag");
        // toHuman Nothing must be a 0-element array.
        assert_eq!(
            dec.array().expect("toHuman Nothing must be array(0)"),
            0,
            "Serialise (Maybe a) encodes Nothing as encodeListLen 0"
        );
    }

    /// Encode → decode round-trip: every shape that the encoder
    /// produces must decode back to the same TraceObject.
    #[test]
    fn trace_object_cbor_round_trip() {
        let originals = vec![
            // All fields populated.
            TraceObject {
                to_human: Some("hello world".into()),
                to_machine: "{\"k\":\"v\"}".into(),
                to_namespace: vec!["Net".into(), "Governor".into()],
                to_severity: TraceSeverity::Info,
                to_details: TraceDetail::DNormal,
                to_timestamp: (1_767_312_000, 122),
                to_hostname: "yggdrasil".into(),
                to_thread_id: "t1".into(),
            },
            // to_human None; empty strings; empty namespace.
            TraceObject {
                to_human: None,
                to_machine: String::new(),
                to_namespace: Vec::new(),
                to_severity: TraceSeverity::Debug,
                to_details: TraceDetail::DMinimal,
                to_timestamp: (0, 0),
                to_hostname: String::new(),
                to_thread_id: String::new(),
            },
            // All severities + details exercised; large psecs value.
            TraceObject {
                to_human: Some("emergency".into()),
                to_machine: "msg".into(),
                to_namespace: vec!["A".into(), "B".into(), "C".into()],
                to_severity: TraceSeverity::Emergency,
                to_details: TraceDetail::DMaximum,
                to_timestamp: (4_102_444_800, 999_999_999_999),
                to_hostname: "h".into(),
                to_thread_id: "t".into(),
            },
        ];

        for original in originals {
            let bytes = original.to_cbor();
            let decoded = TraceObject::from_cbor_bytes(&bytes).expect("round-trip decode");
            assert_eq!(decoded, original, "round-trip drift on {original:?}");
        }
    }

    /// Decoder rejects an outer array of length ≠ 9.
    #[test]
    fn trace_object_decoder_rejects_wrong_outer_arity() {
        // Build a fake CBOR with an 8-element outer array.
        let mut enc = yggdrasil_ledger::cbor::Encoder::new();
        enc.array(8);
        for _ in 0..8 {
            enc.null();
        }
        let bytes = enc.into_bytes();
        let err = TraceObject::from_cbor_bytes(&bytes).expect_err("wrong arity must fail");
        assert!(
            matches!(err, TraceObjectDecodeError::WrongOuterArity(8)),
            "expected WrongOuterArity(8); got {err:?}"
        );
    }

    /// Decoder rejects an out-of-range `SeverityS` constructor index.
    #[test]
    fn trace_object_decoder_rejects_unknown_severity() {
        let mut enc = yggdrasil_ledger::cbor::Encoder::new();
        enc.array(9);
        enc.unsigned(0); // constructor tag
        enc.array(0); // to_human = Nothing
        enc.text(""); // to_machine
        enc.array(0); // to_namespace = []
        enc.array(1);
        enc.unsigned(99); // to_severity index — out of range
        enc.array(1);
        enc.unsigned(0); // to_details
        enc.tag(1000);
        enc.map(2);
        enc.unsigned(1);
        enc.unsigned(0);
        enc.integer(-12);
        enc.unsigned(0);
        enc.text(""); // to_hostname
        enc.text(""); // to_thread_id
        let bytes = enc.into_bytes();
        let err = TraceObject::from_cbor_bytes(&bytes).expect_err("unknown severity must fail");
        assert!(
            matches!(err, TraceObjectDecodeError::UnknownSeverity(99)),
            "expected UnknownSeverity(99); got {err:?}"
        );
    }

    /// All severities have stable upstream constructor indices
    /// (`Debug = 0 … Emergency = 7`, the declaration order).
    #[test]
    fn trace_severity_constructor_indices_match_upstream() {
        for (variant, idx) in [
            (TraceSeverity::Debug, 0),
            (TraceSeverity::Info, 1),
            (TraceSeverity::Notice, 2),
            (TraceSeverity::Warning, 3),
            (TraceSeverity::Error, 4),
            (TraceSeverity::Critical, 5),
            (TraceSeverity::Alert, 6),
            (TraceSeverity::Emergency, 7),
        ] {
            assert_eq!(
                variant.constructor_index(),
                idx,
                "constructor-index drift on {variant:?}"
            );
            assert_eq!(
                TraceSeverity::from_constructor_index(idx),
                Some(variant),
                "round-trip drift on {variant:?}"
            );
        }
        assert_eq!(TraceSeverity::from_constructor_index(8), None);
    }

    /// All detail levels have stable upstream constructor indices
    /// (`DMinimal = 0 … DMaximum = 3`).
    #[test]
    fn trace_detail_constructor_indices_match_upstream() {
        for (variant, idx) in [
            (TraceDetail::DMinimal, 0),
            (TraceDetail::DNormal, 1),
            (TraceDetail::DDetailed, 2),
            (TraceDetail::DMaximum, 3),
        ] {
            assert_eq!(
                variant.constructor_index(),
                idx,
                "constructor-index drift on {variant:?}"
            );
            assert_eq!(
                TraceDetail::from_constructor_index(idx),
                Some(variant),
                "round-trip drift on {variant:?}"
            );
        }
        assert_eq!(TraceDetail::from_constructor_index(4), None);
    }
}
