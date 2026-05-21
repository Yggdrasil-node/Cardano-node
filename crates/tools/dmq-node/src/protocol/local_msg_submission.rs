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

use crate::protocol::sig_submission::{Sig, SigValidationError};

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
        PosixTime, SigBody, SigColdKey, SigHash, SigId, SigKesSignature, SigOpCertificate, SigRaw,
        SigRawWithSignedBytes,
    };
    use yggdrasil_consensus::OpCert;
    use yggdrasil_crypto::{KesSignature, Signature, SumKesVerificationKey, VerificationKey};

    /// A minimal placeholder `Sig` for protocol-shape tests.
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
            sig_raw_kes_signature: SigKesSignature(KesSignature([0; 64])),
        };
        Sig {
            sig_raw_bytes: vec![],
            sig_raw_with_signed_bytes: SigRawWithSignedBytes {
                sig_raw_signed_bytes: vec![],
                sig_raw,
            },
        }
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
}
