//! DMQ `LocalMsgSubmission` mini-protocol — local DMQ-signature
//! submission (node-to-client).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Collapses the upstream
//! `DMQ/Protocol/LocalMsgSubmission/{Type,Codec,Client,Server}.hs`
//! files into one Rust file, mirroring the
//! `crates/network/src/protocols/` one-file-per-mini-protocol
//! pattern. Upstream `type LocalMsgSubmission sig = LocalTxSubmission
//! sig SigValidationError` — the protocol *is* `LocalTxSubmission`, so
//! the states / messages mirror `crates/network`'s `LocalTxSubmission`
//! with a [`Sig`] payload and a typed [`SigValidationError`]
//! rejection. dmq-node carries its own copy because
//! `crates/network`'s `LocalTxSubmission` is concrete over the ledger
//! tx types (the R731 / R732 decision).

use crate::protocol::sig_submission::{Sig, SigValidationError, decode_sig, encode_sig};
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

/// States of the `LocalMsgSubmission` mini-protocol.
///
/// Mirror of `crates/network`'s `LocalTxSubmissionState`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMsgSubmissionState {
    /// Client agency — may send `MsgSubmitTx` or `MsgDone`.
    StIdle,
    /// Server agency — must reply with `MsgAcceptTx` or `MsgRejectTx`.
    StBusy,
    /// Terminal state — no further messages.
    StDone,
}

/// Messages of the `LocalMsgSubmission` mini-protocol.
///
/// Upstream `LocalMsgSubmission sig = LocalTxSubmission sig
/// SigValidationError`; the messages are `LocalTxSubmission`'s with a
/// [`Sig`] payload and a [`SigValidationError`] rejection (the variant
/// names keep upstream's `Tx` spelling — the protocol *is*
/// `LocalTxSubmission`). The CBOR envelope tags (`0`/`1`/`2`/`3`) are
/// byte-identical to `crates/network`'s `LocalTxSubmissionMessage`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalMsgSubmissionMessage {
    /// `[0, sig]` — client submits a DMQ signature. `StIdle → StBusy`.
    MsgSubmitTx {
        /// The submitted DMQ signature.
        sig: Sig,
    },
    /// `[1]` — server accepts the signature. `StBusy → StIdle`.
    MsgAcceptTx,
    /// `[2, reason]` — server rejects the signature. `StBusy → StIdle`.
    MsgRejectTx {
        /// Why the signature was rejected.
        reason: SigValidationError,
    },
    /// `[3]` — client terminates the protocol. `StIdle → StDone`.
    MsgDone,
}

impl LocalMsgSubmissionMessage {
    /// The CBOR message-envelope tag — byte-identical to
    /// `crates/network`'s `LocalTxSubmissionMessage::wire_tag`.
    pub fn wire_tag(&self) -> u8 {
        match self {
            LocalMsgSubmissionMessage::MsgSubmitTx { .. } => 0,
            LocalMsgSubmissionMessage::MsgAcceptTx => 1,
            LocalMsgSubmissionMessage::MsgRejectTx { .. } => 2,
            LocalMsgSubmissionMessage::MsgDone => 3,
        }
    }

    /// Human-readable tag name, used in transition-error messages.
    pub fn tag_name(&self) -> &'static str {
        match self {
            LocalMsgSubmissionMessage::MsgSubmitTx { .. } => "MsgSubmitTx",
            LocalMsgSubmissionMessage::MsgAcceptTx => "MsgAcceptTx",
            LocalMsgSubmissionMessage::MsgRejectTx { .. } => "MsgRejectTx",
            LocalMsgSubmissionMessage::MsgDone => "MsgDone",
        }
    }

    /// Encode this message to CBOR.
    ///
    /// Wire format — the `LocalTxSubmission` envelope (mirror of
    /// `crates/network`'s `LocalTxSubmissionMessage`) with a `Sig`
    /// payload and a `SigValidationError` reject:
    /// - `MsgSubmitTx` is `[0, sig]`
    /// - `MsgAcceptTx` is `[1]`
    /// - `MsgRejectTx` is `[2, reject]`
    /// - `MsgDone`     is `[3]`
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            LocalMsgSubmissionMessage::MsgSubmitTx { sig } => {
                enc.array(2).unsigned(0);
                encode_sig(sig, &mut enc);
            }
            LocalMsgSubmissionMessage::MsgAcceptTx => {
                enc.array(1).unsigned(1);
            }
            LocalMsgSubmissionMessage::MsgRejectTx { reason } => {
                enc.array(2).unsigned(2);
                encode_reject(reason, &mut enc);
            }
            LocalMsgSubmissionMessage::MsgDone => {
                enc.array(1).unsigned(3);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    ///
    /// Inverse of [`Self::to_cbor`]; rejects an unknown tag, a
    /// wrong-arity envelope, or trailing bytes.
    pub fn from_cbor(data: &[u8]) -> Result<LocalMsgSubmissionMessage, LedgerError> {
        let mut dec = Decoder::new(data);
        let arr = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, arr) {
            (0, 2) => {
                let raw = dec.bytes_owned()?;
                LocalMsgSubmissionMessage::MsgSubmitTx {
                    sig: decode_sig(&raw)?,
                }
            }
            (1, 1) => LocalMsgSubmissionMessage::MsgAcceptTx,
            (2, 2) => LocalMsgSubmissionMessage::MsgRejectTx {
                reason: decode_reject(&mut dec)?,
            },
            (3, 1) => LocalMsgSubmissionMessage::MsgDone,
            _ => {
                return Err(LedgerError::CborTypeMismatch {
                    expected: 0,
                    actual: tag as u8,
                });
            }
        };
        if !dec.is_empty() {
            return Err(LedgerError::CborTrailingBytes(dec.remaining()));
        }
        Ok(msg)
    }
}

/// Encode a [`SigValidationError`] as a `MsgRejectTx` reject reason.
///
/// Mirror of upstream `LocalMsgSubmission/Codec.hs::encodeReject`:
/// `SigDuplicate` is `[1]`, `SigExpired` is `[2]`, `SigResultOther` is
/// `[3, text]`, every other variant is `[0, text]`. Upstream's `text`
/// is Haskell `show`; the Rust port uses `Debug` — the same documented
/// `show`-vs-`Debug` divergence as `SigValidationError::to_json`
/// (identical for field-less variants; the wire *structure* is exact).
fn encode_reject(reason: &SigValidationError, enc: &mut Encoder) {
    match reason {
        SigValidationError::SigDuplicate => {
            enc.array(1).unsigned(1);
        }
        SigValidationError::SigExpired => {
            enc.array(1).unsigned(2);
        }
        SigValidationError::SigResultOther(text) => {
            enc.array(2).unsigned(3);
            enc.text(&format!("{text:?}"));
        }
        other => {
            enc.array(2).unsigned(0);
            enc.text(&format!("{other:?}"));
        }
    }
}

/// Decode a `MsgRejectTx` reject reason.
///
/// Mirror of upstream `decodeReject` — tags `0` and `3` both decode to
/// `SigResultOther` (upstream's documented `FIXME SigInvalid`: the
/// `[0, ...]` "invalid" encoding does not round-trip to the original
/// variant).
fn decode_reject(dec: &mut Decoder) -> Result<SigValidationError, LedgerError> {
    let len = dec.array()?;
    let tag = dec.unsigned()?;
    match (tag, len) {
        (0, 2) | (3, 2) => Ok(SigValidationError::SigResultOther(dec.text_owned()?)),
        (1, 1) => Ok(SigValidationError::SigDuplicate),
        (2, 1) => Ok(SigValidationError::SigExpired),
        _ => Err(LedgerError::CborDecodeError(format!(
            "decodeReject: unrecognized (tag, len) = ({tag}, {len})"
        ))),
    }
}

/// An illegal `LocalMsgSubmission` state transition.
///
/// Mirror of `crates/network`'s `LocalTxSubmissionTransitionError`.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("illegal LocalMsgSubmission transition: {message} not allowed in {state:?}")]
pub struct LocalMsgSubmissionTransitionError {
    /// The state the message arrived in.
    pub state: LocalMsgSubmissionState,
    /// The offending message's tag name.
    pub message: &'static str,
}

impl LocalMsgSubmissionState {
    /// The next state after an incoming message, or an error if the
    /// transition is illegal.
    ///
    /// Mirror of `crates/network`'s `LocalTxSubmissionState::transition`.
    pub fn transition(
        self,
        msg: &LocalMsgSubmissionMessage,
    ) -> Result<LocalMsgSubmissionState, LocalMsgSubmissionTransitionError> {
        match (self, msg) {
            (LocalMsgSubmissionState::StIdle, LocalMsgSubmissionMessage::MsgSubmitTx { .. }) => {
                Ok(LocalMsgSubmissionState::StBusy)
            }
            (LocalMsgSubmissionState::StIdle, LocalMsgSubmissionMessage::MsgDone) => {
                Ok(LocalMsgSubmissionState::StDone)
            }
            (LocalMsgSubmissionState::StBusy, LocalMsgSubmissionMessage::MsgAcceptTx) => {
                Ok(LocalMsgSubmissionState::StIdle)
            }
            (LocalMsgSubmissionState::StBusy, LocalMsgSubmissionMessage::MsgRejectTx { .. }) => {
                Ok(LocalMsgSubmissionState::StIdle)
            }
            (state, msg) => Err(LocalMsgSubmissionTransitionError {
                state,
                message: msg.tag_name(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::sig_submission::{
        DMQ_KES_DEPTH, PosixTime, SigBody, SigColdKey, SigHash, SigId, SigKesSignature,
        SigOpCertificate, SigRaw, encode_sig_raw,
    };
    use yggdrasil_consensus::OpCert;
    use yggdrasil_crypto::{Signature, SumKesSignature, SumKesVerificationKey, VerificationKey};

    /// A minimal placeholder `Sig` whose `sig_raw_bytes` is a valid
    /// encoded `SigRaw`, so it survives a `MsgSubmitTx` codec
    /// round-trip.
    fn dummy_sig() -> Sig {
        let sig_raw = SigRaw {
            sig_raw_id: SigId(SigHash(vec![0x01])),
            sig_raw_body: SigBody(vec![]),
            sig_raw_kes_period: 0,
            sig_raw_op_certificate: SigOpCertificate(OpCert {
                hot_vkey: SumKesVerificationKey([0; 32]),
                sequence_number: 0,
                kes_period: 0,
                sigma: Signature([0; 64]),
            }),
            sig_raw_cold_key: SigColdKey(VerificationKey([0; 32])),
            sig_raw_expires_at: PosixTime(0),
            sig_raw_kes_signature: SigKesSignature(
                SumKesSignature::from_bytes(
                    DMQ_KES_DEPTH,
                    &vec![0u8; SumKesSignature::expected_size(DMQ_KES_DEPTH)],
                )
                .expect("dummy kes sig"),
            ),
        };
        let mut enc = Encoder::new();
        encode_sig_raw(&sig_raw, &mut enc);
        decode_sig(&enc.into_bytes()).expect("dummy sig")
    }

    #[test]
    fn wire_tags_match_local_tx_submission() {
        assert_eq!(
            LocalMsgSubmissionMessage::MsgSubmitTx { sig: dummy_sig() }.wire_tag(),
            0
        );
        assert_eq!(LocalMsgSubmissionMessage::MsgAcceptTx.wire_tag(), 1);
        assert_eq!(
            LocalMsgSubmissionMessage::MsgRejectTx {
                reason: SigValidationError::SigExpired,
            }
            .wire_tag(),
            2
        );
        assert_eq!(LocalMsgSubmissionMessage::MsgDone.wire_tag(), 3);
    }

    #[test]
    fn transition_follows_the_protocol() {
        let busy = LocalMsgSubmissionState::StIdle
            .transition(&LocalMsgSubmissionMessage::MsgSubmitTx { sig: dummy_sig() })
            .expect("submit");
        assert_eq!(busy, LocalMsgSubmissionState::StBusy);
        assert_eq!(
            busy.transition(&LocalMsgSubmissionMessage::MsgAcceptTx)
                .expect("accept"),
            LocalMsgSubmissionState::StIdle
        );
        assert_eq!(
            busy.transition(&LocalMsgSubmissionMessage::MsgRejectTx {
                reason: SigValidationError::ClockSkew,
            })
            .expect("reject"),
            LocalMsgSubmissionState::StIdle
        );
        assert_eq!(
            LocalMsgSubmissionState::StIdle
                .transition(&LocalMsgSubmissionMessage::MsgDone)
                .expect("done"),
            LocalMsgSubmissionState::StDone
        );
    }

    #[test]
    fn transition_rejects_illegal_messages() {
        // MsgAcceptTx is illegal in StIdle.
        let err = LocalMsgSubmissionState::StIdle
            .transition(&LocalMsgSubmissionMessage::MsgAcceptTx)
            .expect_err("rejects");
        assert_eq!(err.message, "MsgAcceptTx");
        assert_eq!(err.state, LocalMsgSubmissionState::StIdle);
    }

    #[test]
    fn local_msg_submission_codec_round_trips() {
        let messages = vec![
            LocalMsgSubmissionMessage::MsgSubmitTx { sig: dummy_sig() },
            LocalMsgSubmissionMessage::MsgAcceptTx,
            // SigDuplicate round-trips cleanly (a field-less reject).
            LocalMsgSubmissionMessage::MsgRejectTx {
                reason: SigValidationError::SigDuplicate,
            },
            LocalMsgSubmissionMessage::MsgDone,
        ];
        for msg in messages {
            let encoded = msg.to_cbor();
            let decoded = LocalMsgSubmissionMessage::from_cbor(&encoded).expect("decodes");
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn reject_codec_matches_upstream() {
        let round_trip = |reason: &SigValidationError| {
            let mut enc = Encoder::new();
            encode_reject(reason, &mut enc);
            let encoded = enc.into_bytes();
            let mut dec = Decoder::new(&encoded);
            decode_reject(&mut dec).expect("decodes")
        };
        // The field-less variants round-trip cleanly.
        assert_eq!(
            round_trip(&SigValidationError::SigDuplicate),
            SigValidationError::SigDuplicate
        );
        assert_eq!(
            round_trip(&SigValidationError::SigExpired),
            SigValidationError::SigExpired
        );
        // Every other variant collapses to `SigResultOther` on decode
        // (upstream's `[0, ...]` "invalid" encoding — the documented
        // `FIXME SigInvalid`).
        assert!(matches!(
            round_trip(&SigValidationError::ClockSkew),
            SigValidationError::SigResultOther(_)
        ));
    }
}
