//! DataPointForward mini-protocol type-level definitions
//! (state machine + message types).
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Protocol/DataPoint/Type.hs.
//!
//! Filename flattens the upstream directory; this file carries the
//! protocol's typed state machine + message envelope, mirroring
//! upstream's `Type.hs`. The CBOR codec lands in R453
//! (`Trace.Forward.Protocol.DataPoint.Codec` mirror, same file), the
//! responder driver in R454 (`Trace.Forward.Protocol.DataPoint.Acceptor`
//! mirror), and the `RunMiniProtocol` aggregator in R457
//! (`Trace.Forward.Run.DataPoint.Acceptor` mirror).
//!
//! Sibling to [`super::trace_object_forward`] (R417/R418) — the two
//! mini-protocols are structurally analogous: acceptor (client) asks,
//! forwarder (server) replies, terminal `MsgDone`. DataPoint differs
//! in that:
//!
//! - There is **no blocking-style discriminator** (TraceObject has
//!   `StBusy(StBlockingStyle)` + `BlockingReplyList`; DataPoint just
//!   has `StBusy`).
//! - The reply payload carries `(name, Maybe LBS.ByteString)` pairs
//!   rather than a generic `[lo]` list — opaque per-data-point JSON
//!   bytes keyed by name.
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                              |
//! |---------------------------------------------------------|----------------------------------------|
//! | `data DataPointForward where StIdle / StBusy / StDone`  | [`DataPointForwardState`]              |
//! | `type DataPointName = Text` (re-exported from `Cardano.Logging.Tracer.DataPoint`) | [`DataPointName`]   |
//! | `type DataPointValue = LBS.ByteString`                  | [`DataPointValue`]                     |
//! | `type DataPointValues = [(DataPointName, Maybe DataPointValue)]` | [`DataPointValues`]           |
//! | `Message DataPointForward from to`                      | [`DataPointForwardMessage`]            |
//! | `MsgDataPointsRequest [DataPointName]`                  | [`DataPointForwardMessage::MsgDataPointsRequest`] |
//! | `MsgDataPointsReply DataPointValues`                    | [`DataPointForwardMessage::MsgDataPointsReply`]   |
//! | `MsgDone`                                               | [`DataPointForwardMessage::MsgDone`]   |
//! | `type StateAgency 'StIdle = 'ClientAgency`              | [`Agency::Acceptor`] (per [`DataPointForwardState::agency`]) |
//! | `type StateAgency 'StBusy = 'ServerAgency`              | [`Agency::Forwarder`]                  |
//! | `type StateAgency 'StDone = 'NobodyAgency`              | [`Agency::Nobody`]                     |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **GADT + DataKinds + Singletons type-level encoding**: upstream
//!   uses `data DataPointForward where StIdle :: ...` to lift states
//!   into the type level + `SingDataPointForward` singletons to
//!   scrutinize them at runtime. Rust enums collapse this — the
//!   value-level enum *is* the runtime representation; type-state
//!   safety is enforced via the [`DataPointForwardState::transition`]
//!   exhaustive-match validator (matching the precedent set by
//!   `keep_alive.rs`, `chain_sync.rs`, and the sibling
//!   `trace_object_forward.rs`).
//! - **`Protocol` typeclass + `StateAgency` type family**: upstream
//!   threads agency through the typed-protocol typeclass machinery
//!   for compile-time message-direction safety. Yggdrasil exposes
//!   the same agency information via the runtime
//!   [`DataPointForwardState::agency`] method (returning [`Agency`])
//!   — same information, runtime-checked rather than compile-time.
//! - **`ShowProxy` instances**: upstream's `Show`-only-via-proxy
//!   types collapse into Rust's standard `Debug` derivation.
//! - **`DataPointName` re-export from `Cardano.Logging.Tracer.DataPoint`**:
//!   upstream's `DataPointName` is a `Text` alias defined in
//!   `cardano-logging`. Yggdrasil ports it locally as a `String`
//!   newtype because the cross-package dependency would otherwise
//!   pull all of cardano-logging into the network crate. Wire format
//!   is identical (CBOR text-string).
//!
//! Reference: `Trace.Forward.Protocol.DataPoint.Type` from the
//! upstream `trace-forward` package (vendored at
//! `.reference-haskell-cardano-node/trace-forward/`).

// ---------------------------------------------------------------------------
// Auxiliary types
// ---------------------------------------------------------------------------

/// Name of a data point — corresponds to upstream's `DataPointName`,
/// itself a `Text` alias defined in `Cardano.Logging.Tracer.DataPoint`.
///
/// Wire format: CBOR major type 3 (text string), UTF-8 encoded.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DataPointName(pub String);

impl DataPointName {
    /// Construct from a `String` (or `&str` via `Into`).
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Borrow the underlying name as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Take the underlying `String`.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for DataPointName {
    fn from(name: String) -> Self {
        Self(name)
    }
}

impl From<&str> for DataPointName {
    fn from(name: &str) -> Self {
        Self(name.to_owned())
    }
}

/// Value of a data point — corresponds to upstream's `DataPointValue`,
/// a `Data.ByteString.Lazy.ByteString` alias. The bytes are opaque
/// per-data-point JSON payloads (cardano-node produces these via
/// `Cardano.Logging.Tracer.DataPoint`'s JSON-encoded type-class).
///
/// Wire format: CBOR major type 2 (byte string).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DataPointValue(pub Vec<u8>);

impl DataPointValue {
    /// Construct from a `Vec<u8>`.
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrow the underlying bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Take the underlying `Vec<u8>`.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl From<Vec<u8>> for DataPointValue {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

/// List of `(name, Maybe bytes)` pairs returned in a reply.
///
/// Mirror of upstream `type DataPointValues = [(DataPointName, Maybe DataPointValue)]`.
/// `Option<DataPointValue>` is `None` when the forwarder does not
/// recognize the requested name (upstream `Nothing` branch).
pub type DataPointValues = Vec<(DataPointName, Option<DataPointValue>)>;

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// States of the DataPointForward mini-protocol state machine.
///
/// The protocol terminology matches upstream:
/// 1. The **forwarder** collects data-points and sends them to the
///    **acceptor** by request.
/// 2. The **acceptor** receives data-points from the forwarder.
/// 3. After the connection is established, the acceptor asks for
///    data-points; the forwarder replies. So the acceptor plays
///    the *client* role and the forwarder plays the *server* role.
///
/// ```text
///                 MsgDataPointsRequest
///   StIdle ────────────────────────────► StBusy
///     │                                       │
///     │ MsgDone                                │ MsgDataPointsReply
///     ▼                                       ▼
///   StDone                                  StIdle
/// ```
///
/// Reference: `Trace.Forward.Protocol.DataPoint.Type` —
/// `DataPointForward`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DataPointForwardState {
    /// Acceptor agency — may send `MsgDataPointsRequest` or `MsgDone`.
    StIdle,
    /// Forwarder agency — must reply with `MsgDataPointsReply`.
    StBusy,
    /// Terminal state — no further messages.
    StDone,
}

/// Which party currently has agency to send the next message in
/// the protocol. Mirror of upstream's `StateAgency` type family
/// over `DataPointForward`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Agency {
    /// The acceptor (client) sends next. Mirror of `'ClientAgency`.
    Acceptor,
    /// The forwarder (server) sends next. Mirror of `'ServerAgency`.
    Forwarder,
    /// Terminal — no party sends. Mirror of `'NobodyAgency`.
    Nobody,
}

impl DataPointForwardState {
    /// The party with agency in this state. Mirror of upstream's
    /// `StateAgency` type-family clauses.
    pub const fn agency(self) -> Agency {
        match self {
            Self::StIdle => Agency::Acceptor,
            Self::StBusy => Agency::Forwarder,
            Self::StDone => Agency::Nobody,
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the DataPointForward mini-protocol.
///
/// CBOR wire shape (R453 codec port mirrors
/// `Trace.Forward.Protocol.DataPoint.Codec`):
///
/// | Wire tag | Wire shape                            | Message              |
/// |----------|---------------------------------------|----------------------|
/// |    1     | `[1, [name, ...]]`                    | MsgDataPointsRequest |
/// |    2     | `[2]`                                 | MsgDone              |
/// |    3     | `[3, [(name, maybe-bytes), ...]]`     | MsgDataPointsReply   |
///
/// Reference: `Trace.Forward.Protocol.DataPoint.Type` — `Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataPointForwardMessage {
    /// `[1, [name, ...]]` — acceptor requests the listed data-points
    /// from the forwarder.
    ///
    /// Transition: `StIdle → StBusy`.
    MsgDataPointsRequest(Vec<DataPointName>),

    /// `[3, [(name, maybe-bytes), ...]]` — forwarder replies with
    /// the requested data-point values. `None` payload indicates the
    /// forwarder does not know that name.
    ///
    /// Transition: `StBusy → StIdle`.
    MsgDataPointsReply(DataPointValues),

    /// `[2]` — acceptor terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgDone,
}

impl DataPointForwardMessage {
    /// Human-readable tag of the message variant. Used in
    /// [`DataPointForwardTransitionError`] reports and debug logging.
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::MsgDataPointsRequest(_) => "MsgDataPointsRequest",
            Self::MsgDataPointsReply(_) => "MsgDataPointsReply",
            Self::MsgDone => "MsgDone",
        }
    }
}

// ---------------------------------------------------------------------------
// Transition validation
// ---------------------------------------------------------------------------

/// Errors arising from illegal DataPointForward state transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DataPointForwardTransitionError {
    /// A message was received that is not legal in the current state.
    #[error("illegal data-point-forward transition from {from:?} via {msg_tag}")]
    IllegalTransition {
        /// State the machine was in.
        from: DataPointForwardState,
        /// Human-readable tag of the offending message.
        msg_tag: &'static str,
    },
}

impl DataPointForwardState {
    /// Computes the next state given an incoming message, or returns
    /// an error if the transition is illegal.
    pub fn transition(
        self,
        msg: &DataPointForwardMessage,
    ) -> Result<Self, DataPointForwardTransitionError> {
        match (self, msg) {
            (Self::StIdle, DataPointForwardMessage::MsgDataPointsRequest(_)) => Ok(Self::StBusy),
            (Self::StIdle, DataPointForwardMessage::MsgDone) => Ok(Self::StDone),
            (Self::StBusy, DataPointForwardMessage::MsgDataPointsReply(_)) => Ok(Self::StIdle),
            (from, msg) => Err(DataPointForwardTransitionError::IllegalTransition {
                from,
                msg_tag: msg.tag(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// CBOR wire codec
// ---------------------------------------------------------------------------
//
// **Strict mirror:** trace-forward/src/Trace/Forward/Protocol/DataPoint/Codec.hs.
//
// Wire format mirrors upstream's `codecDataPointForward`:
//
// | Wire tag | Wire shape                                | Message              |
// |----------|-------------------------------------------|----------------------|
// |    1     | `[1, [name, ...]]`                        | MsgDataPointsRequest |
// |    2     | `[2]`                                     | MsgDone              |
// |    3     | `[3, [(name, maybe-bytes), ...]]`         | MsgDataPointsReply   |
//
// Per-name encoding (mirror of cborg `Serialise Text` instance): CBOR
// major type 3 (text string), UTF-8.
//
// Per-value encoding (mirror of cborg `Serialise (Maybe LBS.ByteString)`
// instance):
// - `Nothing` → `array(0)` (`0x80`)
// - `Just bytes` → `[1-array, bytes]` (`0x81 <bytes>`)
//
// Per-`(name, maybe-bytes)` pair: `[2-array, text, maybe-bytes]`.
//
// List encoding: upstream's call site at
// `Trace.Forward.Run.DataPoint.Acceptor.hs:55` uses `CBOR.encode` /
// `CBOR.decode` (Serialise list instance), which emits indefinite-
// length lists for non-empty input and definite-length for empty.
// Yggdrasil's encoder emits *definite-length* arrays on the wire (one
// encoding for all cases) — matching the existing
// [`super::trace_object_forward`] R418 codec precedent. The decoder
// accepts both definite- AND indefinite-length list encodings so
// messages from upstream cardano-node forwarders deserialize
// correctly. This is documented as a wire-canonicalization carve-out
// in [crates/network/src/protocols/trace_object_forward.rs] (R418)
// and reused here.
//
// Carve-outs (NOT ported, by design):
//
// - **`MonadST` constraint + `MonadST m` bound**: upstream threads
//   the `m` monad through `Codec` for ST-state-thread parametricity.
//   Yggdrasil's [`yggdrasil_ledger::cbor::Encoder`] / `Decoder` pair
//   is concrete (no monad transformer), matching the existing
//   pattern in [`super::keep_alive`] and other Yggdrasil mini-
//   protocol codecs.
// - **`SomeMessage st` existential**: upstream's `decode` returns a
//   `SomeMessage st` because the result type is dependent on the
//   state token. Yggdrasil returns [`DataPointForwardMessage`]
//   directly + relies on [`DataPointForwardState::transition`] for
//   state-validation — matching the precedent in `keep_alive::from_cbor`.
// - **`encodeRequest`/`decodeRequest`/`encodeReplyList`/`decodeReplyList`
//   closure parameters**: upstream's `codecDataPointForward` takes
//   four closures because the typed-protocol codec is generic and
//   the call site (`Run/DataPoint/Acceptor.hs:55`) always passes
//   the same Serialise-instance pair. Yggdrasil inlines the
//   monomorphic encoders + decoders since `DataPointName` /
//   `DataPointValue` / `DataPointValues` are concrete, eliminating
//   the boilerplate. This is a Rust-side simplification with no
//   wire-format consequence — bytes match the upstream pair.

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

/// Encode a list of `DataPointName` to CBOR. Mirror of upstream's
/// `encodeRequest` closure passed at the
/// `Trace.Forward.Run.DataPoint.Acceptor` call site (which uses
/// `CBOR.encode :: [Text] -> Encoding`).
fn encode_request_names(enc: &mut Encoder, names: &[DataPointName]) {
    enc.array(names.len() as u64);
    for name in names {
        enc.text(name.as_str());
    }
}

/// Decode a list of `DataPointName` from CBOR. Accepts both
/// definite- and indefinite-length encodings (the latter is what
/// upstream cborg-Serialise actually emits for non-empty lists).
fn decode_request_names(dec: &mut Decoder<'_>) -> Result<Vec<DataPointName>, LedgerError> {
    let maybe_len = dec.array_begin()?;
    match maybe_len {
        Some(len) => {
            let mut out = Vec::with_capacity(len as usize);
            for _ in 0..len {
                out.push(DataPointName::new(dec.text()?));
            }
            Ok(out)
        }
        None => {
            let mut out = Vec::new();
            while !dec.is_break() {
                out.push(DataPointName::new(dec.text()?));
            }
            dec.consume_break()?;
            Ok(out)
        }
    }
}

/// Encode a `Maybe DataPointValue` to CBOR. Mirror of the cborg
/// `Serialise (Maybe a)` instance:
/// `Nothing → encodeListLen 0`,
/// `Just v  → encodeListLen 1 <> encode v`.
fn encode_maybe_value(enc: &mut Encoder, maybe: &Option<DataPointValue>) {
    match maybe {
        None => {
            enc.array(0);
        }
        Some(v) => {
            enc.array(1).bytes(v.as_slice());
        }
    }
}

/// Decode a `Maybe DataPointValue` from CBOR. Accepts the cborg
/// canonical `[0-array]` / `[1-array, bytes]` shape; rejects any
/// other array length.
fn decode_maybe_value(dec: &mut Decoder<'_>) -> Result<Option<DataPointValue>, LedgerError> {
    let len = dec.array()?;
    match len {
        0 => Ok(None),
        1 => {
            let bytes = dec.bytes()?.to_vec();
            Ok(Some(DataPointValue::new(bytes)))
        }
        n => Err(LedgerError::CborDecodeError(format!(
            "codecDataPointForward: Maybe DataPointValue: unexpected array len {n}, \
             expected 0 (Nothing) or 1 (Just)"
        ))),
    }
}

/// Encode a `(name, maybe-bytes)` reply-list entry: `[2-array, text, maybe-bytes]`.
fn encode_reply_entry(enc: &mut Encoder, entry: &(DataPointName, Option<DataPointValue>)) {
    enc.array(2).text(entry.0.as_str());
    encode_maybe_value(enc, &entry.1);
}

/// Decode a `(name, maybe-bytes)` reply-list entry.
fn decode_reply_entry(
    dec: &mut Decoder<'_>,
) -> Result<(DataPointName, Option<DataPointValue>), LedgerError> {
    let len = dec.array()?;
    if len != 2 {
        return Err(LedgerError::CborDecodeError(format!(
            "codecDataPointForward: reply entry: expected 2-array (name, maybe-bytes), \
             got {len}-array"
        )));
    }
    let name = DataPointName::new(dec.text()?);
    let value = decode_maybe_value(dec)?;
    Ok((name, value))
}

/// Encode a `DataPointValues` reply list. Mirror of upstream's
/// `encodeReplyList` closure (which uses
/// `CBOR.encode :: [(Text, Maybe LBS.ByteString)] -> Encoding`).
fn encode_reply_list(enc: &mut Encoder, values: &DataPointValues) {
    enc.array(values.len() as u64);
    for entry in values {
        encode_reply_entry(enc, entry);
    }
}

/// Decode a `DataPointValues` reply list. Accepts both definite-
/// and indefinite-length encodings.
fn decode_reply_list(dec: &mut Decoder<'_>) -> Result<DataPointValues, LedgerError> {
    let maybe_len = dec.array_begin()?;
    match maybe_len {
        Some(len) => {
            let mut out = Vec::with_capacity(len as usize);
            for _ in 0..len {
                out.push(decode_reply_entry(dec)?);
            }
            Ok(out)
        }
        None => {
            let mut out = Vec::new();
            while !dec.is_break() {
                out.push(decode_reply_entry(dec)?);
            }
            dec.consume_break()?;
            Ok(out)
        }
    }
}

impl DataPointForwardMessage {
    /// Encode this message to CBOR bytes.
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::MsgDataPointsRequest(names) => {
                enc.array(2).unsigned(1);
                encode_request_names(&mut enc, names);
            }
            Self::MsgDone => {
                enc.array(1).unsigned(2);
            }
            Self::MsgDataPointsReply(values) => {
                enc.array(2).unsigned(3);
                encode_reply_list(&mut enc, values);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes given the current protocol
    /// state. The `state` parameter is required because the wire
    /// format does not embed state information — upstream's typed-
    /// protocol codec passes a `SingDataPointForward st` token at
    /// the same point.
    pub fn from_cbor_in_state(
        state: DataPointForwardState,
        data: &[u8],
    ) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(data);
        let len = dec.array()?;
        let key = dec.unsigned()?;
        let msg = match (key, len, state) {
            // (1, 2, StIdle): MsgDataPointsRequest
            (1, 2, DataPointForwardState::StIdle) => {
                let names = decode_request_names(&mut dec)?;
                Self::MsgDataPointsRequest(names)
            }
            // (2, 1, StIdle): MsgDone
            (2, 1, DataPointForwardState::StIdle) => Self::MsgDone,
            // (3, 2, StBusy): MsgDataPointsReply
            (3, 2, DataPointForwardState::StBusy) => {
                let values = decode_reply_list(&mut dec)?;
                Self::MsgDataPointsReply(values)
            }
            // Any other (key, len, state) is illegal. Upstream's
            // StDone branch hits `notActiveState`; we surface that
            // as a CBOR-level invariant failure so the caller's
            // protocol driver doesn't silently accept a post-terminal
            // message.
            _ => {
                return Err(LedgerError::CborTypeMismatch {
                    expected: 0,
                    actual: key as u8,
                });
            }
        };
        if !dec.is_empty() {
            return Err(LedgerError::CborTrailingBytes(dec.remaining()));
        }
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_point_name_round_trips() {
        let n = DataPointName::new("node-info");
        assert_eq!(n.as_str(), "node-info");
        assert_eq!(n.clone().into_string(), "node-info".to_owned());
        let n2: DataPointName = "tip".into();
        assert_eq!(n2.as_str(), "tip");
        let n3: DataPointName = String::from("ledger").into();
        assert_eq!(n3.as_str(), "ledger");
    }

    #[test]
    fn data_point_value_round_trips() {
        let v = DataPointValue::new(vec![1, 2, 3]);
        assert_eq!(v.as_slice(), &[1, 2, 3]);
        assert_eq!(v.clone().into_bytes(), vec![1, 2, 3]);
        let v2: DataPointValue = vec![4, 5].into();
        assert_eq!(v2.as_slice(), &[4, 5]);
    }

    #[test]
    fn agency_matches_upstream_state_agency_clauses() {
        // 'StIdle = 'ClientAgency      → Acceptor
        assert_eq!(DataPointForwardState::StIdle.agency(), Agency::Acceptor);
        // 'StBusy = 'ServerAgency      → Forwarder
        assert_eq!(DataPointForwardState::StBusy.agency(), Agency::Forwarder);
        // 'StDone = 'NobodyAgency      → Nobody
        assert_eq!(DataPointForwardState::StDone.agency(), Agency::Nobody);
    }

    #[test]
    fn message_tag_strings_match_upstream_constructor_names() {
        assert_eq!(
            DataPointForwardMessage::MsgDataPointsRequest(vec!["a".into()]).tag(),
            "MsgDataPointsRequest"
        );
        assert_eq!(
            DataPointForwardMessage::MsgDataPointsReply(vec![("a".into(), None)]).tag(),
            "MsgDataPointsReply"
        );
        assert_eq!(DataPointForwardMessage::MsgDone.tag(), "MsgDone");
    }

    #[test]
    fn idle_request_advances_to_busy() {
        let next = DataPointForwardState::StIdle
            .transition(&DataPointForwardMessage::MsgDataPointsRequest(vec![
                "node-info".into(),
                "tip".into(),
            ]))
            .expect("legal");
        assert_eq!(next, DataPointForwardState::StBusy);
    }

    #[test]
    fn idle_done_terminates() {
        let next = DataPointForwardState::StIdle
            .transition(&DataPointForwardMessage::MsgDone)
            .expect("legal");
        assert_eq!(next, DataPointForwardState::StDone);
    }

    #[test]
    fn busy_reply_returns_to_idle() {
        let next = DataPointForwardState::StBusy
            .transition(&DataPointForwardMessage::MsgDataPointsReply(vec![(
                "tip".into(),
                Some(DataPointValue::new(vec![0xDE, 0xAD])),
            )]))
            .expect("legal");
        assert_eq!(next, DataPointForwardState::StIdle);
    }

    #[test]
    fn busy_reply_with_unknown_name_returns_to_idle() {
        // Unknown name → None payload — still legal.
        let next = DataPointForwardState::StBusy
            .transition(&DataPointForwardMessage::MsgDataPointsReply(vec![(
                "unknown".into(),
                None,
            )]))
            .expect("legal");
        assert_eq!(next, DataPointForwardState::StIdle);
    }

    #[test]
    fn idle_reply_is_illegal() {
        let err = DataPointForwardState::StIdle
            .transition(&DataPointForwardMessage::MsgDataPointsReply(vec![]))
            .expect_err("illegal");
        assert_eq!(
            err,
            DataPointForwardTransitionError::IllegalTransition {
                from: DataPointForwardState::StIdle,
                msg_tag: "MsgDataPointsReply",
            }
        );
    }

    #[test]
    fn busy_request_is_illegal() {
        let err = DataPointForwardState::StBusy
            .transition(&DataPointForwardMessage::MsgDataPointsRequest(vec![
                "a".into(),
            ]))
            .expect_err("illegal");
        assert_eq!(
            err,
            DataPointForwardTransitionError::IllegalTransition {
                from: DataPointForwardState::StBusy,
                msg_tag: "MsgDataPointsRequest",
            }
        );
    }

    #[test]
    fn busy_done_is_illegal() {
        // Only the acceptor (StIdle) may send MsgDone — the forwarder
        // (StBusy) cannot terminate mid-reply.
        let err = DataPointForwardState::StBusy
            .transition(&DataPointForwardMessage::MsgDone)
            .expect_err("illegal");
        assert_eq!(
            err,
            DataPointForwardTransitionError::IllegalTransition {
                from: DataPointForwardState::StBusy,
                msg_tag: "MsgDone",
            }
        );
    }

    #[test]
    fn done_state_is_terminal_for_all_messages() {
        let done = DataPointForwardState::StDone;
        assert!(
            done.transition(&DataPointForwardMessage::MsgDataPointsRequest(vec![]))
                .is_err()
        );
        assert!(
            done.transition(&DataPointForwardMessage::MsgDataPointsReply(vec![]))
                .is_err()
        );
        assert!(done.transition(&DataPointForwardMessage::MsgDone).is_err());
    }

    #[test]
    fn empty_request_list_is_legal() {
        // Upstream's `[DataPointName]` parameter is not constrained
        // to be non-empty; an empty list MUST be accepted.
        let next = DataPointForwardState::StIdle
            .transition(&DataPointForwardMessage::MsgDataPointsRequest(vec![]))
            .expect("legal");
        assert_eq!(next, DataPointForwardState::StBusy);
    }

    #[test]
    fn empty_reply_list_is_legal() {
        // Upstream's `DataPointValues` parameter is not constrained
        // to be non-empty; an empty reply MUST be accepted (it
        // corresponds to a request with an empty name list).
        let next = DataPointForwardState::StBusy
            .transition(&DataPointForwardMessage::MsgDataPointsReply(vec![]))
            .expect("legal");
        assert_eq!(next, DataPointForwardState::StIdle);
    }

    #[test]
    fn mixed_known_and_unknown_names_in_reply_legal() {
        let reply = vec![
            ("node-info".into(), Some(DataPointValue::new(vec![1, 2, 3]))),
            ("missing".into(), None),
            ("tip".into(), Some(DataPointValue::new(vec![]))),
        ];
        let next = DataPointForwardState::StBusy
            .transition(&DataPointForwardMessage::MsgDataPointsReply(reply))
            .expect("legal");
        assert_eq!(next, DataPointForwardState::StIdle);
    }

    #[test]
    fn full_request_reply_done_round_trip() {
        // Exercise the full canonical flow: Idle → Busy → Idle → Done.
        let s0 = DataPointForwardState::StIdle;
        let s1 = s0
            .transition(&DataPointForwardMessage::MsgDataPointsRequest(vec![
                "a".into(),
                "b".into(),
            ]))
            .expect("idle→busy");
        assert_eq!(s1, DataPointForwardState::StBusy);
        let s2 = s1
            .transition(&DataPointForwardMessage::MsgDataPointsReply(vec![
                ("a".into(), Some(DataPointValue::new(vec![0xAA]))),
                ("b".into(), None),
            ]))
            .expect("busy→idle");
        assert_eq!(s2, DataPointForwardState::StIdle);
        let s3 = s2
            .transition(&DataPointForwardMessage::MsgDone)
            .expect("idle→done");
        assert_eq!(s3, DataPointForwardState::StDone);
    }

    // ----- Codec round-trip tests ------------------------------------------

    #[test]
    fn codec_request_round_trip() {
        let msg = DataPointForwardMessage::MsgDataPointsRequest(vec![
            DataPointName::new("node-info"),
            DataPointName::new("tip"),
        ]);
        let bytes = msg.to_cbor();
        let decoded =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &bytes)
                .expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn codec_request_empty_round_trip() {
        let msg = DataPointForwardMessage::MsgDataPointsRequest(vec![]);
        let bytes = msg.to_cbor();
        let decoded =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &bytes)
                .expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn codec_done_round_trip() {
        let msg = DataPointForwardMessage::MsgDone;
        let bytes = msg.to_cbor();
        let decoded =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &bytes)
                .expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn codec_reply_round_trip_mixed_known_unknown() {
        let msg = DataPointForwardMessage::MsgDataPointsReply(vec![
            (
                "node-info".into(),
                Some(DataPointValue::new(b"{\"version\":\"11.0.1\"}".to_vec())),
            ),
            ("unknown".into(), None),
            ("tip".into(), Some(DataPointValue::new(vec![]))),
        ]);
        let bytes = msg.to_cbor();
        let decoded =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StBusy, &bytes)
                .expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn codec_reply_empty_round_trip() {
        let msg = DataPointForwardMessage::MsgDataPointsReply(vec![]);
        let bytes = msg.to_cbor();
        let decoded =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StBusy, &bytes)
                .expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn codec_request_in_busy_state_rejected() {
        // The wire bytes for a valid MsgDataPointsRequest, but
        // attempting to decode in StBusy state (which only accepts
        // MsgDataPointsReply) — must error.
        let req = DataPointForwardMessage::MsgDataPointsRequest(vec!["a".into()]);
        let bytes = req.to_cbor();
        let result =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StBusy, &bytes);
        assert!(
            matches!(result, Err(LedgerError::CborTypeMismatch { .. })),
            "expected CborTypeMismatch in StBusy, got: {result:?}"
        );
    }

    #[test]
    fn codec_reply_in_idle_state_rejected() {
        let rep = DataPointForwardMessage::MsgDataPointsReply(vec![("a".into(), None)]);
        let bytes = rep.to_cbor();
        let result =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &bytes);
        assert!(
            matches!(result, Err(LedgerError::CborTypeMismatch { .. })),
            "expected CborTypeMismatch in StIdle, got: {result:?}"
        );
    }

    #[test]
    fn codec_decode_in_done_state_always_errors() {
        let done = DataPointForwardMessage::MsgDone;
        let bytes = done.to_cbor();
        let result =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StDone, &bytes);
        assert!(
            matches!(result, Err(LedgerError::CborTypeMismatch { .. })),
            "expected CborTypeMismatch in StDone, got: {result:?}"
        );
    }

    #[test]
    fn codec_request_wire_format_is_byte_stable() {
        // Lock down the wire format for MsgDataPointsRequest(["a"]):
        //   [1, ["a"]] in definite-length CBOR is:
        //     0x82  array(2)
        //     0x01  unsigned 1 (key)
        //     0x81  array(1)
        //     0x61  text(1)
        //     0x61  'a'
        let msg = DataPointForwardMessage::MsgDataPointsRequest(vec!["a".into()]);
        let bytes = msg.to_cbor();
        assert_eq!(bytes, vec![0x82, 0x01, 0x81, 0x61, 0x61]);
    }

    #[test]
    fn codec_done_wire_format_is_byte_stable() {
        // [2] → 0x81 array(1), 0x02 unsigned 2.
        let msg = DataPointForwardMessage::MsgDone;
        let bytes = msg.to_cbor();
        assert_eq!(bytes, vec![0x81, 0x02]);
    }

    #[test]
    fn codec_reply_wire_format_is_byte_stable() {
        // Lock down the wire format for MsgDataPointsReply([("a", Just [0xAA])]):
        //   [3, [["a", [0xAA]]]] in definite-length CBOR is:
        //     0x82  array(2)
        //     0x03  unsigned 3 (key)
        //     0x81  array(1)        — reply list (1 entry)
        //     0x82  array(2)        — (name, maybe) pair
        //     0x61  text(1)
        //     0x61  'a'
        //     0x81  array(1)        — Just
        //     0x41  bytes(1)
        //     0xAA
        let msg = DataPointForwardMessage::MsgDataPointsReply(vec![(
            "a".into(),
            Some(DataPointValue::new(vec![0xAA])),
        )]);
        let bytes = msg.to_cbor();
        assert_eq!(
            bytes,
            vec![0x82, 0x03, 0x81, 0x82, 0x61, 0x61, 0x81, 0x41, 0xAA]
        );
    }

    #[test]
    fn codec_reply_nothing_wire_format_is_byte_stable() {
        // MsgDataPointsReply([("x", Nothing)]):
        //   0x82  array(2)
        //   0x03  unsigned 3
        //   0x81  array(1)        — reply list
        //   0x82  array(2)        — pair
        //   0x61  text(1)
        //   0x78  'x'
        //   0x80  array(0)        — Nothing
        let msg = DataPointForwardMessage::MsgDataPointsReply(vec![("x".into(), None)]);
        let bytes = msg.to_cbor();
        assert_eq!(bytes, vec![0x82, 0x03, 0x81, 0x82, 0x61, 0x78, 0x80]);
    }

    #[test]
    fn codec_decoder_accepts_indefinite_request_list() {
        // Manually build a request with an indefinite-length name list
        // (what upstream cborg-Serialise actually emits for non-empty
        // [Text]). The decoder must accept it.
        //   [1, names]
        //   0x82 array(2)
        //   0x01 key
        //   0x9F indef array
        //     0x61 'a'  (text "a")
        //     0x61 'b'  (text "b")
        //   0xFF break
        let bytes = vec![0x82, 0x01, 0x9F, 0x61, 0x61, 0x61, 0x62, 0xFF];
        let decoded =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &bytes)
                .expect("decode");
        assert_eq!(
            decoded,
            DataPointForwardMessage::MsgDataPointsRequest(vec!["a".into(), "b".into()])
        );
    }

    #[test]
    fn codec_decoder_accepts_indefinite_reply_list() {
        // [3, indef [["a", [0xAA]], ["b", []]]]
        //   0x82 array(2)
        //   0x03 key
        //   0x9F indef array
        //     0x82 array(2)
        //     0x61 'a'
        //     0x81 0x41 0xAA   — Just [0xAA]
        //     0x82 array(2)
        //     0x61 'b'
        //     0x80              — Nothing
        //   0xFF break
        let bytes = vec![
            0x82, 0x03, 0x9F, 0x82, 0x61, 0x61, 0x81, 0x41, 0xAA, 0x82, 0x61, 0x62, 0x80, 0xFF,
        ];
        let decoded =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StBusy, &bytes)
                .expect("decode");
        assert_eq!(
            decoded,
            DataPointForwardMessage::MsgDataPointsReply(vec![
                ("a".into(), Some(DataPointValue::new(vec![0xAA]))),
                ("b".into(), None),
            ])
        );
    }

    #[test]
    fn codec_reply_maybe_with_invalid_len_rejected() {
        // Construct a reply with a malformed Maybe — array-len 2 is
        // illegal (only 0 = Nothing or 1 = Just are valid).
        //   0x82 array(2)
        //   0x03 key (reply)
        //   0x81 array(1)         — reply list
        //   0x82 array(2)         — pair
        //   0x61 'a'
        //   0x82                  — Maybe with invalid len 2
        //   0x40                    bytes(0)
        //   0x40                    bytes(0)
        let bytes = vec![0x82, 0x03, 0x81, 0x82, 0x61, 0x61, 0x82, 0x40, 0x40];
        let result =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StBusy, &bytes);
        match result {
            Err(LedgerError::CborDecodeError(s)) => {
                assert!(
                    s.contains("Maybe DataPointValue: unexpected array len 2"),
                    "unexpected error message: {s}"
                );
            }
            other => panic!("expected CborDecodeError, got {other:?}"),
        }
    }
}
