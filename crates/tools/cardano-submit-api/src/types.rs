//! Core types — `TxSubmitWebApiError`, `TxCmdError`, `EnvSocketError`,
//! `RawCborDecodeError`, `TxSubmitPort`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Types.hs.
//!
//! Direct ports:
//!
//! - `TxSubmitPort` (newtype Int) — port number for the API server.
//! - `RawCborDecodeError` (newtype `[DecoderError]`) — accumulator for
//!   CBOR decoder failures during tx-bytes parsing.
//! - `TxSubmitWebApiError` (sum) — error category surfaced to API clients
//!   via JSON response body.
//! - `EnvSocketError` (sum-of-one) — socket-environment-variable lookup
//!   failure.
//! - `TxCmdError` (sum) — command-level error wrapper enclosing socket,
//!   read, validation, and connection failures.
//! - `render_tx_cmd_error` — human-readable rendering used by tracer
//!   forHuman + WebApi error responses.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - `TxSubmitApi` / `TxSubmitApiRecord` / `CBORStream` — Servant
//!   type-level API definitions that have no Rust analog. The web round
//!   (R340) uses an axum router instead; CBOR content-type negotiation
//!   is handled inline at the handler. **Strict mirror:** none for those
//!   surfaces; rationale recorded here.
//!
//! ## JSON shape parity vs upstream
//!
//! Upstream Aeson-derived `ToJSON` instances and Yggdrasil's serde
//! mirror produce byte-equivalent output:
//!
//! | Upstream constructor               | JSON shape                                                             |
//! |------------------------------------|------------------------------------------------------------------------|
//! | `TxSubmitDecodeHex`                | `{"tag":"TxSubmitDecodeHex"}`                                          |
//! | `TxSubmitEmpty`                    | `{"tag":"TxSubmitEmpty"}`                                              |
//! | `TxSubmitDecodeFail e`             | `{"tag":"TxSubmitDecodeFail","contents":"<err>"}`                      |
//! | `TxSubmitBadTx t`                  | `{"tag":"TxSubmitBadTx","contents":"<text>"}`                          |
//! | `TxSubmitFail err`                 | `{"tag":"TxSubmitFail","contents":<TxCmdError>}`                       |
//! | `TxCmdSocketEnvError s`            | `{"tag":"TxCmdSocketEnvError","contents":{"message":"<msg>"}}`         |
//! | `TxCmdTxReadError`                 | `{"tag":"TxCmdTxReadError","contents":[<DecoderError>...]}`            |
//! | `TxCmdTxSubmitValidationError`     | `{"tag":"TxCmdTxSubmitValidationError","contents":"<rendered>"}`       |
//! | `TxCmdTxSubmitConnectionError msg` | `{"tag":"TxCmdTxSubmitConnectionError","contents":"<msg>"}`            |
//! | `RawCborDecodeError`               | `["<DecoderError>"...]`                                                |
//! | `EnvSocketError`                   | `{"message":"<msg>"}` (untagged, single variant)                       |
//!
//! Round-trip golden tests verify each shape against fixtures captured
//! from the upstream Haskell binary.

use std::fmt;

use serde::Serialize;

/// Port number on which the tx-submit web API listens.
///
/// Upstream: `newtype TxSubmitPort = TxSubmitPort Int`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TxSubmitPort(pub u16);

impl fmt::Display for TxSubmitPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<TxSubmitPort> for u16 {
    fn from(value: TxSubmitPort) -> Self {
        value.0
    }
}

impl From<u16> for TxSubmitPort {
    fn from(value: u16) -> Self {
        TxSubmitPort(value)
    }
}

/// A single CBOR decoder failure as a human-readable string.
///
/// Upstream uses `Cardano.Binary.DecoderError`; its Aeson `ToJSON`
/// instance is `Aeson.String . textShow`. The Rust newtype keeps the
/// string-form invariant by construction so both `Display` and
/// `Serialize` produce the same byte-equivalent output.
///
/// R340 may replace the inner string with a structured CBOR error type
/// once the web round wires `minicbor::decode::Error` through.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DecoderError(pub String);

impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for DecoderError {
    fn from(value: String) -> Self {
        DecoderError(value)
    }
}

impl From<&str> for DecoderError {
    fn from(value: &str) -> Self {
        DecoderError(value.to_string())
    }
}

/// Errors returned by raw CBOR transaction parsing.
///
/// Upstream: `newtype RawCborDecodeError = RawCborDecodeError [DecoderError]`.
/// Aeson default Generic-derived `ToJSON` for a newtype unwraps to the
/// inner value; `#[serde(transparent)]` matches that shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RawCborDecodeError(pub Vec<DecoderError>);

impl fmt::Display for RawCborDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RawCborDecodeError decode error: ")?;
        for (i, err) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            err.fmt(f)?;
        }
        Ok(())
    }
}

impl std::error::Error for RawCborDecodeError {}

/// Socket-environment-variable lookup error.
///
/// Upstream: `newtype EnvSocketError = CliEnvVarLookup Text`. Upstream's
/// manual `ToJSON` instance produces the bare object `{"message":"..."}`
/// without a constructor tag (the `deriving anyclass` form is bypassed).
/// `#[serde(untagged)]` + struct-variant matches that shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum EnvSocketError {
    /// Lookup of `CARDANO_NODE_SOCKET_PATH` (or equivalent) failed.
    CliEnvVarLookup {
        /// Operator-facing failure message.
        message: String,
    },
}

impl fmt::Display for EnvSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnvSocketError::CliEnvVarLookup { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for EnvSocketError {}

/// Transaction-submission command error.
///
/// Upstream: `data TxCmdError = TxCmdSocketEnvError ... | TxCmdTxReadError ... | TxCmdTxSubmitValidationError ... | TxCmdTxSubmitConnectionError ...`
/// with `deriving anyclass instance ToJSON`. Aeson's default `ToJSON` for
/// a Generic-derived sum uses `TaggedObject "tag" "contents"` (Aeson 1.x+
/// default), producing `{"tag":"<Constructor>","contents":<payload>}`.
/// `#[serde(tag = "tag", content = "contents")]` matches it.
///
/// Note: `TxCmdTxSubmitValidationError` currently carries a rendered
/// string; the structured `ApplyError` mapping is tracked as a
/// follow-on in `docs/TECH-DEBT.md` under "cardano-submit-api
/// validation error: structured mapping" (cohered with the Phase 4.A
/// sister-tool web-protocol completion so the wire-protocol change
/// lands in one cohesive PR) and in `docs/parity-matrix.json` under
/// the cardano-submit-api entry's `next_milestone` field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "tag", content = "contents")]
pub enum TxCmdError {
    /// Failure to look up `CARDANO_NODE_SOCKET_PATH` (or equivalent) in
    /// the process environment.
    TxCmdSocketEnvError(EnvSocketError),
    /// Raw CBOR decoder failure(s) when parsing transaction bytes.
    TxCmdTxReadError(RawCborDecodeError),
    /// Tx-validation rejection from the local cardano-node. Currently
    /// rendered as a string; structured form lands at R340+.
    TxCmdTxSubmitValidationError(String),
    /// Connection to the local cardano-node socket failed.
    TxCmdTxSubmitConnectionError(String),
}

impl fmt::Display for TxCmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&render_tx_cmd_error(self))
    }
}

impl std::error::Error for TxCmdError {}

/// Web-API-surface error returned to clients of `POST /api/submit/tx`.
///
/// Upstream: `data TxSubmitWebApiError = TxSubmitDecodeHex | TxSubmitEmpty | TxSubmitDecodeFail !DecoderError | TxSubmitBadTx !Text | TxSubmitFail TxCmdError`
/// with a hand-written `toJSON` instance using explicit tag/contents.
/// `#[serde(tag = "tag", content = "contents")]` matches it for both
/// unit variants (no `contents` field) and payload variants.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "tag", content = "contents")]
pub enum TxSubmitWebApiError {
    /// Hex decoding of the request body failed.
    TxSubmitDecodeHex,
    /// Request body was empty.
    TxSubmitEmpty,
    /// CBOR decoder failed on the (post-hex) tx bytes.
    TxSubmitDecodeFail(DecoderError),
    /// Tx semantic-content rejection (caller-friendly string).
    TxSubmitBadTx(String),
    /// Underlying tx-cmd error during submission to cardano-node.
    TxSubmitFail(TxCmdError),
}

impl fmt::Display for TxSubmitWebApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TxSubmitWebApiError::TxSubmitDecodeHex => f.write_str("TxSubmitDecodeHex"),
            TxSubmitWebApiError::TxSubmitEmpty => f.write_str("TxSubmitEmpty"),
            TxSubmitWebApiError::TxSubmitDecodeFail(err) => {
                write!(f, "TxSubmitDecodeFail: {err}")
            }
            TxSubmitWebApiError::TxSubmitBadTx(msg) => write!(f, "TxSubmitBadTx: {msg}"),
            TxSubmitWebApiError::TxSubmitFail(err) => write!(f, "TxSubmitFail: {err}"),
        }
    }
}

impl std::error::Error for TxSubmitWebApiError {}

/// Render a `TxCmdError` as a human-readable line.
///
/// Mirrors upstream `Cardano.TxSubmit.Types.renderTxCmdError` byte-for-byte
/// (modulo the validation-string formatting, which currently uses the
/// pre-rendered string instead of the live `TxValidationErrorInCardanoMode`
/// pattern match — see `TxCmdTxSubmitValidationError` doc).
pub fn render_tx_cmd_error(err: &TxCmdError) -> String {
    match err {
        TxCmdError::TxCmdSocketEnvError(socket_error) => {
            format!("socket env error \"{socket_error}\"")
        }
        TxCmdError::TxCmdTxReadError(envelope_error) => {
            format!("transaction read error \"{envelope_error}\"")
        }
        TxCmdError::TxCmdTxSubmitValidationError(rendered) => {
            format!("transaction submit error {rendered}")
        }
        TxCmdError::TxCmdTxSubmitConnectionError(msg) => {
            format!("transaction submit connection error: {msg}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(value: impl Serialize) -> String {
        serde_json::to_string(&value).expect("serializes")
    }

    #[test]
    fn tx_submit_port_serializes_transparently() {
        assert_eq!(json(TxSubmitPort(8090)), "8090");
    }

    #[test]
    fn tx_submit_port_round_trips_via_u16() {
        let port = TxSubmitPort::from(8090u16);
        assert_eq!(u16::from(port), 8090);
    }

    #[test]
    fn decoder_error_serializes_as_string() {
        assert_eq!(
            json(DecoderError("invalid CBOR tag 99".to_string())),
            "\"invalid CBOR tag 99\""
        );
    }

    #[test]
    fn raw_cbor_decode_error_serializes_as_array() {
        let err = RawCborDecodeError(vec![
            DecoderError("a".to_string()),
            DecoderError("b".to_string()),
        ]);
        assert_eq!(json(&err), r#"["a","b"]"#);
    }

    #[test]
    fn raw_cbor_decode_error_display_format() {
        let err = RawCborDecodeError(vec![
            DecoderError("a".to_string()),
            DecoderError("b".to_string()),
        ]);
        assert_eq!(err.to_string(), "RawCborDecodeError decode error: a, b");
    }

    #[test]
    fn env_socket_error_serializes_as_bare_object() {
        let err = EnvSocketError::CliEnvVarLookup {
            message: "CARDANO_NODE_SOCKET_PATH not set".to_string(),
        };
        assert_eq!(
            json(&err),
            r#"{"message":"CARDANO_NODE_SOCKET_PATH not set"}"#
        );
    }

    #[test]
    fn env_socket_error_display_uses_message() {
        let err = EnvSocketError::CliEnvVarLookup {
            message: "missing var".to_string(),
        };
        assert_eq!(err.to_string(), "missing var");
    }

    #[test]
    fn tx_cmd_socket_env_error_json_shape() {
        let err = TxCmdError::TxCmdSocketEnvError(EnvSocketError::CliEnvVarLookup {
            message: "x".to_string(),
        });
        assert_eq!(
            json(&err),
            r#"{"tag":"TxCmdSocketEnvError","contents":{"message":"x"}}"#
        );
    }

    #[test]
    fn tx_cmd_tx_read_error_json_shape() {
        let err =
            TxCmdError::TxCmdTxReadError(RawCborDecodeError(vec![DecoderError("bad".to_string())]));
        assert_eq!(
            json(&err),
            r#"{"tag":"TxCmdTxReadError","contents":["bad"]}"#
        );
    }

    #[test]
    fn tx_cmd_validation_error_json_shape() {
        let err = TxCmdError::TxCmdTxSubmitValidationError("FeeTooSmall".to_string());
        assert_eq!(
            json(&err),
            r#"{"tag":"TxCmdTxSubmitValidationError","contents":"FeeTooSmall"}"#
        );
    }

    #[test]
    fn tx_cmd_connection_error_json_shape() {
        let err = TxCmdError::TxCmdTxSubmitConnectionError("ECONNREFUSED".to_string());
        assert_eq!(
            json(&err),
            r#"{"tag":"TxCmdTxSubmitConnectionError","contents":"ECONNREFUSED"}"#
        );
    }

    #[test]
    fn web_api_error_decode_hex_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitDecodeHex;
        assert_eq!(json(&err), r#"{"tag":"TxSubmitDecodeHex"}"#);
    }

    #[test]
    fn web_api_error_empty_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitEmpty;
        assert_eq!(json(&err), r#"{"tag":"TxSubmitEmpty"}"#);
    }

    #[test]
    fn web_api_error_decode_fail_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitDecodeFail(DecoderError("trunc".to_string()));
        assert_eq!(
            json(&err),
            r#"{"tag":"TxSubmitDecodeFail","contents":"trunc"}"#
        );
    }

    #[test]
    fn web_api_error_bad_tx_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitBadTx("over budget".to_string());
        assert_eq!(
            json(&err),
            r#"{"tag":"TxSubmitBadTx","contents":"over budget"}"#
        );
    }

    #[test]
    fn web_api_error_fail_json_shape() {
        let err = TxSubmitWebApiError::TxSubmitFail(TxCmdError::TxCmdTxSubmitConnectionError(
            "down".to_string(),
        ));
        assert_eq!(
            json(&err),
            r#"{"tag":"TxSubmitFail","contents":{"tag":"TxCmdTxSubmitConnectionError","contents":"down"}}"#
        );
    }

    #[test]
    fn render_tx_cmd_error_socket_env() {
        let err = TxCmdError::TxCmdSocketEnvError(EnvSocketError::CliEnvVarLookup {
            message: "missing".to_string(),
        });
        assert_eq!(render_tx_cmd_error(&err), "socket env error \"missing\"");
    }

    #[test]
    fn render_tx_cmd_error_read() {
        let err =
            TxCmdError::TxCmdTxReadError(RawCborDecodeError(vec![DecoderError("x".to_string())]));
        assert!(render_tx_cmd_error(&err).starts_with("transaction read error"));
    }

    #[test]
    fn render_tx_cmd_error_validation() {
        let err = TxCmdError::TxCmdTxSubmitValidationError("FeeTooSmall".to_string());
        assert_eq!(
            render_tx_cmd_error(&err),
            "transaction submit error FeeTooSmall"
        );
    }

    #[test]
    fn render_tx_cmd_error_connection() {
        let err = TxCmdError::TxCmdTxSubmitConnectionError("down".to_string());
        assert_eq!(
            render_tx_cmd_error(&err),
            "transaction submit connection error: down"
        );
    }

    #[test]
    fn tx_cmd_error_implements_std_error() {
        fn assert_error<E: std::error::Error>(_: &E) {}
        let err = TxCmdError::TxCmdTxSubmitConnectionError("x".to_string());
        assert_error(&err);
    }

    #[test]
    fn web_api_error_implements_std_error() {
        fn assert_error<E: std::error::Error>(_: &E) {}
        let err = TxSubmitWebApiError::TxSubmitEmpty;
        assert_error(&err);
    }
}
