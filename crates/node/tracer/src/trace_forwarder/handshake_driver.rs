//! Typed-protocol state-machine driver for the Network.Mux
//! Handshake mini-protocol.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side driver for the upstream
//! `Ouroboros.Network.Protocol.Handshake` typed-protocol state
//! machine at
//! `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/framework/lib/Ouroboros/Network/Protocol/Handshake/`.
//! Composes the wire codec from
//! [`super::handshake`] with the bearer from [`super::bearer`] to
//! produce a one-shot initiator-side `run_initiator_handshake`
//! function that:
//!
//! 1. Builds a `MsgProposeVersions` from the operator-supplied
//!    version map.
//! 2. Encodes + writes one Initiator-direction SDU on mini-protocol
//!    num 0 ([`super::mux::HANDSHAKE_MINI_PROTOCOL_NUM`]).
//! 3. Reads one Responder-direction SDU back.
//! 4. Decodes the response as a `HandshakeMessage` and pattern-matches
//!    on the three valid Confirm-state outcomes (AcceptVersion / Refuse
//!    / ReplyVersions — the last only in query mode, which we don't
//!    enable so we error out on it).
//! 5. Returns either the agreed `(version, version_data_cbor)` pair or
//!    a structured failure.
//!
//! The responder side of the handshake (a Yggdrasil-side acceptor
//! that consumes ProposeVersions and selects a version) is a
//! follow-on; today's cardano-tracer use case is initiator-only
//! (the node opens the bearer; cardano-tracer is the responder).

use std::collections::BTreeMap;

use tokio::io::{AsyncRead, AsyncWrite};

use super::bearer::{Bearer, BearerError};
use super::handshake::{
    HandshakeDecodeError, HandshakeMessage, RefuseReason, decode_message, encode_message,
};
use super::mux::{HANDSHAKE_MINI_PROTOCOL_NUM, MiniProtocolDir, SduHeader};

/// One agreed (version-number, version-data) pair returned by a
/// successful handshake run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgreedVersion {
    /// Version number the responder picked.
    pub version: u32,
    /// CBOR-encoded version-data the responder accepted (the
    /// agreed-data value, which for cardano-tracer is the network
    /// magic).
    pub data_cbor: Vec<u8>,
}

/// Errors surfaced by `run_initiator_handshake`.
#[derive(Debug)]
pub enum HandshakeDriverError {
    /// Bearer read or write failed.
    Bearer(BearerError),
    /// Responder sent a message that didn't decode.
    Decode(HandshakeDecodeError),
    /// Responder refused with a structured reason.
    Refused(RefuseReason),
    /// Responder's reply SDU arrived on an unexpected
    /// mini-protocol number (should be 0) or direction (should be
    /// Responder).
    UnexpectedSdu {
        /// The mini-protocol number on the inbound SDU.
        mini_protocol_num: u16,
        /// The direction bit on the inbound SDU.
        direction: MiniProtocolDir,
    },
    /// Responder's reply was a valid HandshakeMessage but the
    /// protocol state machine forbids it in Confirm state (e.g.,
    /// the responder echoed our ProposeVersions back).
    UnexpectedMessage(HandshakeMessage),
    /// Caller asked to propose an empty version map. Upstream's
    /// `MsgProposeVersions` requires at least one entry — there's
    /// nothing for the responder to accept.
    EmptyVersionMap,
}

impl core::fmt::Display for HandshakeDriverError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bearer(e) => write!(f, "handshake bearer error: {e}"),
            Self::Decode(e) => write!(f, "handshake decode error: {e}"),
            Self::Refused(reason) => write!(f, "responder refused: {reason:?}"),
            Self::UnexpectedSdu {
                mini_protocol_num,
                direction,
            } => write!(
                f,
                "responder reply on wrong SDU: mini-protocol num {mini_protocol_num}, \
                 direction {direction:?} (expected num 0 / Responder)"
            ),
            Self::UnexpectedMessage(msg) => {
                write!(f, "responder sent unexpected message in Confirm state: {msg:?}")
            }
            Self::EmptyVersionMap => f.write_str(
                "ProposeVersions requires at least one (version, data) entry; caller passed an empty map",
            ),
        }
    }
}

impl std::error::Error for HandshakeDriverError {}

/// Run the initiator side of the Handshake mini-protocol against
/// the bearer.
///
/// `versions` is the operator-supplied version map (version-number
/// → CBOR-encoded version-data). MUST be non-empty; passing an empty
/// map errors out before any bearer I/O.
///
/// Returns `Ok(AgreedVersion)` when the responder accepts a version;
/// `Err(HandshakeDriverError::Refused(...))` when the responder
/// returns a structured refusal. Any other failure (bearer I/O,
/// malformed responder bytes, protocol-state violation) surfaces
/// through the corresponding error variant.
pub async fn run_initiator_handshake<S>(
    bearer: &mut Bearer<S>,
    versions: BTreeMap<u32, Vec<u8>>,
) -> Result<AgreedVersion, HandshakeDriverError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    if versions.is_empty() {
        return Err(HandshakeDriverError::EmptyVersionMap);
    }

    // Step 1+2: encode + send MsgProposeVersions.
    let propose = HandshakeMessage::ProposeVersions(versions);
    let payload = encode_message(&propose);
    let sdu_header = SduHeader {
        timestamp: 0,
        mini_protocol_num: HANDSHAKE_MINI_PROTOCOL_NUM,
        direction: MiniProtocolDir::Initiator,
        length: payload.len() as u16,
    };
    bearer
        .write_sdu(&sdu_header, &payload)
        .await
        .map_err(HandshakeDriverError::Bearer)?;

    // Step 3+4: read + decode responder's reply.
    let (reply_header, reply_payload) = bearer
        .read_sdu()
        .await
        .map_err(HandshakeDriverError::Bearer)?;

    if reply_header.mini_protocol_num != HANDSHAKE_MINI_PROTOCOL_NUM
        || reply_header.direction != MiniProtocolDir::Responder
    {
        return Err(HandshakeDriverError::UnexpectedSdu {
            mini_protocol_num: reply_header.mini_protocol_num,
            direction: reply_header.direction,
        });
    }

    // state_is_propose = false (we're in Confirm state expecting
    // Accept or Refuse).
    let reply_message =
        decode_message(&reply_payload, false).map_err(HandshakeDriverError::Decode)?;

    // Step 5: pattern-match.
    match reply_message {
        HandshakeMessage::AcceptVersion { version, data_cbor } => {
            Ok(AgreedVersion { version, data_cbor })
        }
        HandshakeMessage::Refuse(reason) => Err(HandshakeDriverError::Refused(reason)),
        other => Err(HandshakeDriverError::UnexpectedMessage(other)),
    }
}

#[cfg(test)]
mod handshake_driver_tests {
    use super::*;
    use crate::trace_forwarder::handshake::encode_message;
    use crate::trace_forwarder::mux::encode_sdu_header;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Build a `Vec<u8>` CBOR uint for use as a fake version-data
    /// payload.
    fn cbor_uint_bytes(n: u32) -> Vec<u8> {
        use yggdrasil_ledger::cbor::Encoder;
        let mut enc = Encoder::new();
        enc.unsigned(u64::from(n));
        enc.into_bytes()
    }

    /// Happy path: initiator proposes versions, responder accepts
    /// one, driver returns the agreed (version, data_cbor) pair.
    #[tokio::test]
    async fn initiator_handshake_accept_round_trip() {
        let (client, mut server) = tokio::io::duplex(4096);
        let mut bearer = Bearer::new(client);

        // Spawn a fake responder that reads the propose SDU,
        // selects version 2, and replies with MsgAcceptVersion.
        let server_task = tokio::spawn(async move {
            // Read the initiator's SDU header + payload.
            let mut hdr = [0u8; 8];
            server.read_exact(&mut hdr).await.expect("server read hdr");
            let hdr_decoded =
                crate::trace_forwarder::mux::decode_sdu_header(&hdr).expect("decode header");
            assert_eq!(hdr_decoded.mini_protocol_num, HANDSHAKE_MINI_PROTOCOL_NUM);
            assert_eq!(hdr_decoded.direction, MiniProtocolDir::Initiator);
            let mut payload = vec![0u8; hdr_decoded.length as usize];
            server
                .read_exact(&mut payload)
                .await
                .expect("server read payload");
            // Decode initiator's ProposeVersions (state_is_propose=true).
            let propose = decode_message(&payload, true).expect("decode propose");
            match propose {
                HandshakeMessage::ProposeVersions(map) => {
                    assert!(map.contains_key(&2), "initiator proposed version 2");
                }
                _ => panic!("expected ProposeVersions"),
            }

            // Reply with MsgAcceptVersion (version=2, data_cbor=mainnet magic).
            let reply = HandshakeMessage::AcceptVersion {
                version: 2,
                data_cbor: cbor_uint_bytes(764_824_073),
            };
            let reply_payload = encode_message(&reply);
            let reply_hdr = SduHeader {
                timestamp: 0,
                mini_protocol_num: HANDSHAKE_MINI_PROTOCOL_NUM,
                direction: MiniProtocolDir::Responder,
                length: reply_payload.len() as u16,
            };
            let reply_hdr_bytes = encode_sdu_header(&reply_hdr).expect("encode header");
            server.write_all(&reply_hdr_bytes).await.expect("write hdr");
            server
                .write_all(&reply_payload)
                .await
                .expect("write payload");
        });

        let mut versions = BTreeMap::new();
        versions.insert(1u32, cbor_uint_bytes(1));
        versions.insert(2u32, cbor_uint_bytes(764_824_073));
        let agreed = run_initiator_handshake(&mut bearer, versions)
            .await
            .expect("handshake should accept");
        assert_eq!(agreed.version, 2);
        assert_eq!(agreed.data_cbor, cbor_uint_bytes(764_824_073));

        let _ = server_task.await;
    }

    /// Refuse path: responder rejects with VersionMismatch.
    #[tokio::test]
    async fn initiator_handshake_refused() {
        let (client, mut server) = tokio::io::duplex(4096);
        let mut bearer = Bearer::new(client);

        let server_task = tokio::spawn(async move {
            // Read the initiator's SDU.
            let mut hdr = [0u8; 8];
            server.read_exact(&mut hdr).await.expect("server read hdr");
            let hdr_decoded = crate::trace_forwarder::mux::decode_sdu_header(&hdr).expect("decode");
            let mut payload = vec![0u8; hdr_decoded.length as usize];
            server.read_exact(&mut payload).await.expect("read payload");

            // Reply with MsgRefuse(VersionMismatch([5,6,7])).
            let reply = HandshakeMessage::Refuse(RefuseReason::VersionMismatch(vec![5, 6, 7]));
            let reply_payload = encode_message(&reply);
            let reply_hdr = SduHeader {
                timestamp: 0,
                mini_protocol_num: HANDSHAKE_MINI_PROTOCOL_NUM,
                direction: MiniProtocolDir::Responder,
                length: reply_payload.len() as u16,
            };
            let reply_hdr_bytes = encode_sdu_header(&reply_hdr).expect("encode");
            server.write_all(&reply_hdr_bytes).await.expect("write hdr");
            server
                .write_all(&reply_payload)
                .await
                .expect("write payload");
        });

        let mut versions = BTreeMap::new();
        versions.insert(1u32, cbor_uint_bytes(1));
        let result = run_initiator_handshake(&mut bearer, versions).await;
        match result {
            Err(HandshakeDriverError::Refused(RefuseReason::VersionMismatch(vs))) => {
                assert_eq!(vs, vec![5, 6, 7]);
            }
            other => panic!("expected Refused(VersionMismatch); got {other:?}"),
        }
        let _ = server_task.await;
    }

    /// Empty version map → EmptyVersionMap before any bearer I/O.
    #[tokio::test]
    async fn initiator_handshake_rejects_empty_versions() {
        let (client, _server) = tokio::io::duplex(64);
        let mut bearer = Bearer::new(client);
        let result = run_initiator_handshake(&mut bearer, BTreeMap::new()).await;
        assert!(
            matches!(result, Err(HandshakeDriverError::EmptyVersionMap)),
            "expected EmptyVersionMap; got {result:?}"
        );
    }

    /// Wrong-direction reply SDU surfaces as UnexpectedSdu.
    #[tokio::test]
    async fn initiator_handshake_rejects_wrong_direction_reply() {
        let (client, mut server) = tokio::io::duplex(4096);
        let mut bearer = Bearer::new(client);

        let server_task = tokio::spawn(async move {
            let mut hdr = [0u8; 8];
            server.read_exact(&mut hdr).await.expect("read hdr");
            let hdr_decoded = crate::trace_forwarder::mux::decode_sdu_header(&hdr).expect("decode");
            let mut payload = vec![0u8; hdr_decoded.length as usize];
            server.read_exact(&mut payload).await.expect("read payload");

            // Reply with valid handshake bytes but WRONG SDU direction
            // (Initiator instead of Responder).
            let reply = HandshakeMessage::AcceptVersion {
                version: 1,
                data_cbor: cbor_uint_bytes(0),
            };
            let reply_payload = encode_message(&reply);
            let reply_hdr = SduHeader {
                timestamp: 0,
                mini_protocol_num: HANDSHAKE_MINI_PROTOCOL_NUM,
                direction: MiniProtocolDir::Initiator, // WRONG
                length: reply_payload.len() as u16,
            };
            let reply_hdr_bytes = encode_sdu_header(&reply_hdr).expect("encode");
            server.write_all(&reply_hdr_bytes).await.expect("write hdr");
            server
                .write_all(&reply_payload)
                .await
                .expect("write payload");
        });

        let mut versions = BTreeMap::new();
        versions.insert(1u32, cbor_uint_bytes(1));
        let result = run_initiator_handshake(&mut bearer, versions).await;
        match result {
            Err(HandshakeDriverError::UnexpectedSdu {
                mini_protocol_num: _,
                direction: MiniProtocolDir::Initiator,
            }) => {}
            other => panic!("expected UnexpectedSdu w/ Initiator dir; got {other:?}"),
        }
        let _ = server_task.await;
    }
}
