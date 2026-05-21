//! DMQ `SigSubmissionV2` mini-protocol — the object-diffusion-based
//! signature diffusion protocol.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Collapses the upstream
//! `DMQ/Protocol/SigSubmissionV2/{Type,Codec,Inbound,Outbound}.hs`
//! files into one Rust file, mirroring the
//! `crates/network/src/protocols/` one-file-per-mini-protocol
//! pattern. `SigSubmissionV2` is based on upstream's
//! `Ouroboros.Network.Protocol.ObjectDiffusion` mini-protocol
//! (originally designed for Peras) — a pull-based protocol where the
//! inbound side requests signature identifiers and then signatures.
//!
//! This slice ports the `Type.hs` count newtypes and the protocol
//! state machine; the message enum, transitions, and codec land in
//! subsequent dmq-node-arc rounds.

/// Number of outstanding signature identifiers being acknowledged.
///
/// Upstream `newtype NumIdsAck = NumIdsAck Word16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumIdsAck(pub u16);

/// Number of signature identifiers being requested.
///
/// Upstream `newtype NumIdsReq = NumIdsReq Word16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumIdsReq(pub u16);

/// Number of signatures being requested.
///
/// Upstream `newtype NumReq = NumReq Word16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumReq(pub u16);

/// Number of unacknowledged signature identifiers.
///
/// Upstream `newtype NumUnacknowledged = NumUnacknowledged Word16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumUnacknowledged(pub u16);

use std::collections::BTreeMap;
use std::time::Duration;

use crate::protocol::sig_submission::{
    Sig, SigId, SigIdAndSize, decode_sig, decode_sig_id, encode_sig, encode_sig_id,
};
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

/// `shortWait` — upstream `Ouroboros.Network.Protocol.Limits.shortWait`
/// (`Just 10`).
const SHORT_WAIT: Option<Duration> = Some(Duration::from_secs(10));

/// The blocking-`StSigIds` inactivity timeout — upstream
/// `timeLimitsSigSubmissionV2`'s `Just 20`.
const BLOCKING_SIG_IDS_WAIT: Option<Duration> = Some(Duration::from_secs(20));

/// `smallByteLimit` — upstream
/// `Ouroboros.Network.Protocol.Limits.smallByteLimit` (`0xffff`).
const SMALL_BYTE_LIMIT: u64 = 0xffff;

/// `largeByteLimit` — upstream
/// `Ouroboros.Network.Protocol.Limits.largeByteLimit` (`2_500_000`).
const LARGE_BYTE_LIMIT: u64 = 2_500_000;

/// Anti-DoS pre-allocation cap for `SigSubmissionV2` indefinite-length
/// list decoding.
const SIG_SUBMISSION_V2_LIST_MAX: usize = 4_096;

/// States of the `SigSubmissionV2` mini-protocol state machine.
///
/// Upstream `data SigSubmissionV2 sigId sig where StIdle / StSigIds
/// StBlockingStyle / StSigs / StDone`. The inbound ("client") side
/// receives signatures; the outbound ("server") side sends them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SigSubmissionV2State {
    /// Client agency — request identifiers or signatures, or terminate.
    StIdle,
    /// Server agency — reply with a list of signature identifiers.
    StSigIds {
        /// Whether the request was blocking.
        blocking: bool,
    },
    /// Server agency — reply with the requested signatures.
    StSigs,
    /// Terminal state — nobody has agency.
    StDone,
}

/// Messages of the `SigSubmissionV2` mini-protocol.
///
/// Mirror of upstream `Message (SigSubmissionV2 sigId sig)`. The
/// `sigId` / `sig` type parameters collapse to the concrete DMQ
/// [`SigId`] / [`Sig`]. `MsgReplySigIds` carries a flat list of
/// `(sigId, size)` pairs; the blocking style is tracked by the state
/// (and `MsgReplyNoSigIds` is the explicit blocking-empty reply).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SigSubmissionV2Message {
    /// `MsgRequestSigIds blocking ack req` — request identifiers and
    /// acknowledge outstanding ones. `StIdle → StSigIds(blocking)`.
    MsgRequestSigIds {
        /// `true` blocking, `false` non-blocking.
        blocking: bool,
        /// Number of outstanding identifiers acknowledged.
        ack: NumIdsAck,
        /// Maximum number of new identifiers requested.
        req: NumIdsReq,
    },
    /// `MsgReplySigIds` — reply with identifiers and their sizes.
    /// `StSigIds → StIdle`.
    MsgReplySigIds {
        /// The signature identifiers and their serialized sizes.
        ids: Vec<SigIdAndSize>,
    },
    /// `MsgReplyNoSigIds` — a blocking request answered with no
    /// identifiers, letting the client regain control.
    /// `StSigIds(blocking) → StIdle`.
    MsgReplyNoSigIds,
    /// `MsgRequestSigs [sigId]` — request specific signatures.
    /// `StIdle → StSigs`.
    MsgRequestSigs {
        /// Signature identifiers to fetch.
        ids: Vec<SigId>,
    },
    /// `MsgReplySigs [sig]` — reply with the requested signatures.
    /// `StSigs → StIdle`.
    MsgReplySigs {
        /// The requested signatures (an unavailable one may be omitted).
        sigs: Vec<Sig>,
    },
    /// `MsgDone` — the client terminates the protocol. `StIdle → StDone`.
    MsgDone,
}

impl SigSubmissionV2Message {
    /// Human-readable tag name, used in transition-error and trace
    /// messages.
    pub fn tag_name(&self) -> &'static str {
        match self {
            SigSubmissionV2Message::MsgRequestSigIds { .. } => "MsgRequestSigIds",
            SigSubmissionV2Message::MsgReplySigIds { .. } => "MsgReplySigIds",
            SigSubmissionV2Message::MsgReplyNoSigIds => "MsgReplyNoSigIds",
            SigSubmissionV2Message::MsgRequestSigs { .. } => "MsgRequestSigs",
            SigSubmissionV2Message::MsgReplySigs { .. } => "MsgReplySigs",
            SigSubmissionV2Message::MsgDone => "MsgDone",
        }
    }

    /// The CBOR message-envelope tag (`encodeSigSubmissionV2`'s
    /// `encodeWord` key).
    pub fn wire_tag(&self) -> u8 {
        match self {
            SigSubmissionV2Message::MsgRequestSigIds { .. } => 1,
            SigSubmissionV2Message::MsgReplySigIds { .. } => 2,
            SigSubmissionV2Message::MsgReplyNoSigIds => 3,
            SigSubmissionV2Message::MsgRequestSigs { .. } => 4,
            SigSubmissionV2Message::MsgReplySigs { .. } => 5,
            SigSubmissionV2Message::MsgDone => 6,
        }
    }

    /// Encode this message to CBOR.
    ///
    /// Wire format — mirror of upstream `encodeSigSubmissionV2`:
    /// - `MsgRequestSigIds` is `[1, blocking, ack, req]`
    /// - `MsgReplySigIds` is `[2, <indef [[sigId, size]]>]`
    /// - `MsgReplyNoSigIds` is `[3]`
    /// - `MsgRequestSigs` is `[4, <indef [sigId]>]`
    /// - `MsgReplySigs` is `[5, <indef [sig]>]`
    /// - `MsgDone` is `[6]`
    ///
    /// The lists are CBOR *indefinite*-length arrays.
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            SigSubmissionV2Message::MsgRequestSigIds { blocking, ack, req } => {
                enc.array(4)
                    .unsigned(1)
                    .bool(*blocking)
                    .unsigned(u64::from(ack.0))
                    .unsigned(u64::from(req.0));
            }
            SigSubmissionV2Message::MsgReplySigIds { ids } => {
                enc.array(2).unsigned(2);
                enc.array_indef();
                for item in ids {
                    enc.array(2);
                    encode_sig_id(&item.sig_id, &mut enc);
                    enc.unsigned(u64::from(item.size));
                }
                enc.break_stop();
            }
            SigSubmissionV2Message::MsgReplyNoSigIds => {
                enc.array(1).unsigned(3);
            }
            SigSubmissionV2Message::MsgRequestSigs { ids } => {
                enc.array(2).unsigned(4);
                enc.array_indef();
                for id in ids {
                    encode_sig_id(id, &mut enc);
                }
                enc.break_stop();
            }
            SigSubmissionV2Message::MsgReplySigs { sigs } => {
                enc.array(2).unsigned(5);
                enc.array_indef();
                for sig in sigs {
                    encode_sig(sig, &mut enc);
                }
                enc.break_stop();
            }
            SigSubmissionV2Message::MsgDone => {
                enc.array(1).unsigned(6);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    ///
    /// Inverse of [`Self::to_cbor`]. The blocking / non-blocking
    /// distinction of `MsgReplySigIds` is a protocol-state property
    /// (enforced by [`SigSubmissionV2State::transition`]); the decoded
    /// message simply carries the identifier list.
    pub fn from_cbor(data: &[u8]) -> Result<SigSubmissionV2Message, LedgerError> {
        let mut dec = Decoder::new(data);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, len) {
            (1, 4) => SigSubmissionV2Message::MsgRequestSigIds {
                blocking: dec.bool()?,
                ack: NumIdsAck(dec.unsigned()? as u16),
                req: NumIdsReq(dec.unsigned()? as u16),
            },
            (2, 2) => SigSubmissionV2Message::MsgReplySigIds {
                ids: decode_indef(&mut dec, |d| {
                    let pair = d.array()?;
                    if pair != 2 {
                        return Err(LedgerError::CborInvalidLength {
                            expected: 2,
                            actual: pair as usize,
                        });
                    }
                    let sig_id = decode_sig_id(d)?;
                    let size = d.unsigned()? as u32;
                    Ok(SigIdAndSize { sig_id, size })
                })?,
            },
            (3, 1) => SigSubmissionV2Message::MsgReplyNoSigIds,
            (4, 2) => SigSubmissionV2Message::MsgRequestSigs {
                ids: decode_indef(&mut dec, decode_sig_id)?,
            },
            (5, 2) => SigSubmissionV2Message::MsgReplySigs {
                sigs: decode_indef(&mut dec, |d| {
                    let raw = d.bytes_owned()?;
                    decode_sig(&raw)
                })?,
            },
            (6, 1) => SigSubmissionV2Message::MsgDone,
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

/// Decode a CBOR indefinite-length array, applying `item` to each
/// element, with an anti-DoS element cap.
fn decode_indef<T>(
    dec: &mut Decoder,
    mut item: impl FnMut(&mut Decoder) -> Result<T, LedgerError>,
) -> Result<Vec<T>, LedgerError> {
    if dec.array_begin()?.is_some() {
        return Err(LedgerError::CborDecodeError(
            "SigSubmissionV2: expected an indefinite-length array".to_string(),
        ));
    }
    let mut items = Vec::new();
    while !dec.is_break() {
        if items.len() >= SIG_SUBMISSION_V2_LIST_MAX {
            return Err(LedgerError::DecodedCountTooLarge {
                count: items.len() as u64,
                max: SIG_SUBMISSION_V2_LIST_MAX,
            });
        }
        items.push(item(dec)?);
    }
    dec.consume_break()?;
    Ok(items)
}

/// An illegal `SigSubmissionV2` state transition.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("illegal SigSubmissionV2 transition: {message} not allowed in {state:?}")]
pub struct SigSubmissionV2TransitionError {
    /// The state the message arrived in.
    pub state: SigSubmissionV2State,
    /// The offending message's tag name.
    pub message: &'static str,
}

impl SigSubmissionV2State {
    /// The next state after an incoming message, or an error if the
    /// transition is illegal.
    ///
    /// Mirror of the upstream `SigSubmissionV2` `Message` transitions:
    /// `StIdle`+`MsgRequestSigIds`→`StSigIds`,
    /// `StSigIds`+`MsgReplySigIds`→`StIdle`, blocking
    /// `StSigIds`+`MsgReplyNoSigIds`→`StIdle`,
    /// `StIdle`+`MsgRequestSigs`→`StSigs`,
    /// `StSigs`+`MsgReplySigs`→`StIdle`,
    /// `StIdle`+`MsgDone`→`StDone`.
    pub fn transition(
        self,
        msg: &SigSubmissionV2Message,
    ) -> Result<SigSubmissionV2State, SigSubmissionV2TransitionError> {
        match (self, msg) {
            (
                SigSubmissionV2State::StIdle,
                SigSubmissionV2Message::MsgRequestSigIds { blocking, .. },
            ) => Ok(SigSubmissionV2State::StSigIds {
                blocking: *blocking,
            }),
            (
                SigSubmissionV2State::StSigIds { .. },
                SigSubmissionV2Message::MsgReplySigIds { .. },
            ) => Ok(SigSubmissionV2State::StIdle),
            // `MsgReplyNoSigIds` is valid only from a blocking request.
            (
                SigSubmissionV2State::StSigIds { blocking: true },
                SigSubmissionV2Message::MsgReplyNoSigIds,
            ) => Ok(SigSubmissionV2State::StIdle),
            (SigSubmissionV2State::StIdle, SigSubmissionV2Message::MsgRequestSigs { .. }) => {
                Ok(SigSubmissionV2State::StSigs)
            }
            (SigSubmissionV2State::StSigs, SigSubmissionV2Message::MsgReplySigs { .. }) => {
                Ok(SigSubmissionV2State::StIdle)
            }
            (SigSubmissionV2State::StIdle, SigSubmissionV2Message::MsgDone) => {
                Ok(SigSubmissionV2State::StDone)
            }
            (state, msg) => Err(SigSubmissionV2TransitionError {
                state,
                message: msg.tag_name(),
            }),
        }
    }

    /// The inactivity timeout for this protocol state — `None` is
    /// upstream `waitForever`.
    ///
    /// Mirror of upstream `timeLimitsSigSubmissionV2`: `StIdle` waits
    /// forever; a blocking `StSigIds` uses 20 s; a non-blocking
    /// `StSigIds` and `StSigs` use `shortWait` (10 s). The terminal
    /// `StDone` has no active timeout.
    pub fn time_limit(self) -> Option<Duration> {
        match self {
            SigSubmissionV2State::StIdle | SigSubmissionV2State::StDone => None,
            SigSubmissionV2State::StSigIds { blocking: true } => BLOCKING_SIG_IDS_WAIT,
            SigSubmissionV2State::StSigIds { blocking: false } | SigSubmissionV2State::StSigs => {
                SHORT_WAIT
            }
        }
    }

    /// The maximum inbound-message size for this protocol state.
    ///
    /// Mirror of upstream `byteLimitsSigSubmissionV2`: `StIdle` uses
    /// `smallByteLimit`; the reply states (`StSigIds`, `StSigs`) use
    /// `largeByteLimit`.
    pub fn byte_limit(self) -> u64 {
        match self {
            SigSubmissionV2State::StIdle | SigSubmissionV2State::StDone => SMALL_BYTE_LIMIT,
            SigSubmissionV2State::StSigIds { .. } | SigSubmissionV2State::StSigs => {
                LARGE_BYTE_LIMIT
            }
        }
    }
}

/// The pipelined-result type for the `SigSubmissionV2` inbound peer.
///
/// Mirror of upstream `data Collect sigId sig`
/// (`Protocol/SigSubmissionV2/Inbound.hs`). The protocol pipelines
/// requests for identifiers and signatures, so a collected response
/// is a sum: a `SigIds` reply (the original request count plus the
/// returned `(sigId, size)` pairs) or a `Sigs` reply (the requested
/// `sigId → size` map plus the returned signatures — pairing them
/// lets the peer detect signatures that are no longer needed).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Collect {
    /// `CollectSigIds NumIdsReq [(sigId, SizeInBytes)]` — the result
    /// of a pipelined `MsgRequestSigIds`.
    CollectSigIds {
        /// The number of identifiers originally requested.
        requested: NumIdsReq,
        /// The returned signature identifiers and their sizes.
        ids: Vec<SigIdAndSize>,
    },
    /// `CollectSigs (Map sigId SizeInBytes) [sig]` — the result of a
    /// pipelined `MsgRequestSigs`.
    CollectSigs {
        /// The requested identifiers paired with their sizes.
        requested: BTreeMap<SigId, u32>,
        /// The returned signatures.
        sigs: Vec<Sig>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_newtypes_wrap_word16() {
        assert_eq!(NumIdsAck(3).0, 3);
        assert_eq!(NumIdsReq::default(), NumIdsReq(0));
        assert!(NumReq(5) > NumReq(2));
        assert_ne!(NumUnacknowledged(1), NumUnacknowledged(2));
    }

    #[test]
    fn message_tag_names() {
        use crate::protocol::sig_submission::{SigHash, SigId};
        assert_eq!(
            SigSubmissionV2Message::MsgRequestSigIds {
                blocking: true,
                ack: NumIdsAck(0),
                req: NumIdsReq(3),
            }
            .tag_name(),
            "MsgRequestSigIds"
        );
        assert_eq!(
            SigSubmissionV2Message::MsgReplySigIds { ids: vec![] }.tag_name(),
            "MsgReplySigIds"
        );
        assert_eq!(
            SigSubmissionV2Message::MsgReplyNoSigIds.tag_name(),
            "MsgReplyNoSigIds"
        );
        assert_eq!(
            SigSubmissionV2Message::MsgRequestSigs {
                ids: vec![SigId(SigHash(vec![0x01]))],
            }
            .tag_name(),
            "MsgRequestSigs"
        );
        assert_eq!(
            SigSubmissionV2Message::MsgReplySigs { sigs: vec![] }.tag_name(),
            "MsgReplySigs"
        );
        assert_eq!(SigSubmissionV2Message::MsgDone.tag_name(), "MsgDone");
    }

    #[test]
    fn transition_follows_the_protocol() {
        // Idle → SigIds → Idle (the identifier exchange).
        let sig_ids = SigSubmissionV2State::StIdle
            .transition(&SigSubmissionV2Message::MsgRequestSigIds {
                blocking: true,
                ack: NumIdsAck(0),
                req: NumIdsReq(3),
            })
            .expect("request ids");
        assert_eq!(sig_ids, SigSubmissionV2State::StSigIds { blocking: true });
        assert_eq!(
            sig_ids
                .transition(&SigSubmissionV2Message::MsgReplySigIds { ids: vec![] })
                .expect("reply ids"),
            SigSubmissionV2State::StIdle
        );
        // A blocking SigIds may also be answered with MsgReplyNoSigIds.
        assert_eq!(
            sig_ids
                .transition(&SigSubmissionV2Message::MsgReplyNoSigIds)
                .expect("reply none"),
            SigSubmissionV2State::StIdle
        );
        // Idle → Sigs → Idle (the signature exchange).
        let sigs = SigSubmissionV2State::StIdle
            .transition(&SigSubmissionV2Message::MsgRequestSigs { ids: vec![] })
            .expect("request sigs");
        assert_eq!(sigs, SigSubmissionV2State::StSigs);
        assert_eq!(
            sigs.transition(&SigSubmissionV2Message::MsgReplySigs { sigs: vec![] })
                .expect("reply sigs"),
            SigSubmissionV2State::StIdle
        );
        // Termination.
        assert_eq!(
            SigSubmissionV2State::StIdle
                .transition(&SigSubmissionV2Message::MsgDone)
                .expect("done"),
            SigSubmissionV2State::StDone
        );
    }

    #[test]
    fn transition_rejects_illegal_messages() {
        // MsgReplyNoSigIds is illegal from a non-blocking StSigIds.
        let err = SigSubmissionV2State::StSigIds { blocking: false }
            .transition(&SigSubmissionV2Message::MsgReplyNoSigIds)
            .expect_err("non-blocking cannot reply-none");
        assert_eq!(err.message, "MsgReplyNoSigIds");
        assert_eq!(
            err.state,
            SigSubmissionV2State::StSigIds { blocking: false }
        );
        // MsgReplySigs is illegal in StIdle.
        assert!(
            SigSubmissionV2State::StIdle
                .transition(&SigSubmissionV2Message::MsgReplySigs { sigs: vec![] })
                .is_err()
        );
    }

    #[test]
    fn message_codec_round_trips() {
        use crate::protocol::sig_submission::{SigHash, SigId};
        let sig_id = SigId(SigHash(vec![0xAA, 0xBB]));
        let messages = vec![
            SigSubmissionV2Message::MsgRequestSigIds {
                blocking: true,
                ack: NumIdsAck(5),
                req: NumIdsReq(33),
            },
            SigSubmissionV2Message::MsgReplySigIds {
                ids: vec![SigIdAndSize {
                    sig_id: sig_id.clone(),
                    size: 2800,
                }],
            },
            SigSubmissionV2Message::MsgReplyNoSigIds,
            SigSubmissionV2Message::MsgRequestSigs {
                ids: vec![sig_id.clone()],
            },
            SigSubmissionV2Message::MsgReplySigs { sigs: vec![] },
            SigSubmissionV2Message::MsgDone,
        ];
        for msg in messages {
            let encoded = msg.to_cbor();
            let decoded = SigSubmissionV2Message::from_cbor(&encoded).expect("decodes");
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn message_envelope_bytes_and_unknown_tag() {
        // `[3]` and `[6]` — a CBOR array of one unsigned integer.
        assert_eq!(
            SigSubmissionV2Message::MsgReplyNoSigIds.to_cbor(),
            vec![0x81, 0x03]
        );
        assert_eq!(SigSubmissionV2Message::MsgDone.to_cbor(), vec![0x81, 0x06]);
        // An unknown tag is rejected.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(99);
        let err = SigSubmissionV2Message::from_cbor(&enc.into_bytes()).expect_err("rejects");
        assert!(matches!(err, LedgerError::CborTypeMismatch { .. }));
    }

    #[test]
    fn time_limits_match_upstream() {
        assert_eq!(SigSubmissionV2State::StIdle.time_limit(), None);
        assert_eq!(SigSubmissionV2State::StDone.time_limit(), None);
        assert_eq!(
            SigSubmissionV2State::StSigIds { blocking: true }.time_limit(),
            Some(std::time::Duration::from_secs(20))
        );
        let short = Some(std::time::Duration::from_secs(10));
        assert_eq!(
            SigSubmissionV2State::StSigIds { blocking: false }.time_limit(),
            short
        );
        assert_eq!(SigSubmissionV2State::StSigs.time_limit(), short);
    }

    #[test]
    fn byte_limits_match_upstream() {
        assert_eq!(SigSubmissionV2State::StIdle.byte_limit(), 0xffff);
        assert_eq!(
            SigSubmissionV2State::StSigIds { blocking: true }.byte_limit(),
            2_500_000
        );
        assert_eq!(
            SigSubmissionV2State::StSigIds { blocking: false }.byte_limit(),
            2_500_000
        );
        assert_eq!(SigSubmissionV2State::StSigs.byte_limit(), 2_500_000);
    }

    #[test]
    fn collect_variants_construct_and_compare() {
        use crate::protocol::sig_submission::{SigHash, SigId};
        let sig_id = SigId(SigHash(vec![0x01, 0x02]));
        let ids = Collect::CollectSigIds {
            requested: NumIdsReq(5),
            ids: vec![SigIdAndSize {
                sig_id: sig_id.clone(),
                size: 2800,
            }],
        };
        assert_eq!(
            ids,
            Collect::CollectSigIds {
                requested: NumIdsReq(5),
                ids: vec![SigIdAndSize {
                    sig_id: sig_id.clone(),
                    size: 2800,
                }],
            }
        );
        let mut requested = BTreeMap::new();
        requested.insert(sig_id, 2800u32);
        let sigs = Collect::CollectSigs {
            requested,
            sigs: vec![],
        };
        assert_ne!(format!("{ids:?}"), format!("{sigs:?}"));
        match sigs {
            Collect::CollectSigs { requested, sigs } => {
                assert_eq!(requested.len(), 1);
                assert!(sigs.is_empty());
            }
            Collect::CollectSigIds { .. } => panic!("wrong variant"),
        }
    }

    #[test]
    fn state_variants_compare() {
        assert_eq!(
            SigSubmissionV2State::StSigIds { blocking: true },
            SigSubmissionV2State::StSigIds { blocking: true }
        );
        assert_ne!(
            SigSubmissionV2State::StSigIds { blocking: true },
            SigSubmissionV2State::StSigIds { blocking: false }
        );
        assert_ne!(SigSubmissionV2State::StIdle, SigSubmissionV2State::StDone);
    }
}
