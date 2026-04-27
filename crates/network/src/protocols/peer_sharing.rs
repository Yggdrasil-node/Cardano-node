/// States of the PeerSharing mini-protocol state machine.
///
/// The PeerSharing protocol lets a client request peer addresses from a
/// server to discover new peers for the peer governor.
///
/// ```text
///  MsgShareRequest     MsgSharePeers
///  StClient ──────────► StServer ──────────► StClient
///    │
///    │ MsgDone
///    ▼
///  StDone
/// ```
///
/// Reference: `Ouroboros.Network.Protocol.PeerSharing.Type`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerSharingState {
    /// Client agency — may send `MsgShareRequest` or `MsgDone`.
    StClient,
    /// Server agency — must reply with `MsgSharePeers`.
    StServer,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Shared peer address
// ---------------------------------------------------------------------------

/// A single peer address shared via the PeerSharing protocol.
///
/// Upstream encodes this as CBOR `[ip_type, ip_bytes, port]`.
/// `ip_type` 0 = IPv4 (4 bytes), 1 = IPv6 (16 bytes).
///
/// Reference: `Ouroboros.Network.PeerSelection.PeerSharing` —
/// `PeerAddress`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedPeerAddress {
    /// Socket address (IPv4 or IPv6 + port).
    pub addr: std::net::SocketAddr,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the PeerSharing mini-protocol.
///
/// CDDL wire tags:
///
/// | Tag | Message           |
/// |-----|-------------------|
/// |  0  | `MsgShareRequest` |
/// |  1  | `MsgSharePeers`   |
/// |  2  | `MsgDone`         |
///
/// Reference: `Ouroboros.Network.Protocol.PeerSharing.Type` — `Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeerSharingMessage {
    /// `[0, amount]` — client requests up to `amount` peer addresses.
    ///
    /// Transition: `StClient → StServer`.
    MsgShareRequest {
        /// Maximum number of peers the client would like to receive.
        amount: u16,
    },

    /// `[1, [peer_addr, …]]` — server replies with a list of shared peers.
    ///
    /// Transition: `StServer → StClient`.
    MsgSharePeers {
        /// List of shared peer addresses. May be shorter than requested.
        peers: Vec<SharedPeerAddress>,
    },

    /// `[2]` — client terminates the protocol.
    ///
    /// Transition: `StClient → StDone`.
    MsgDone,
}

// ---------------------------------------------------------------------------
// Transition validation
// ---------------------------------------------------------------------------

/// Errors arising from illegal PeerSharing state transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PeerSharingTransitionError {
    /// A message was received that is not legal in the current state.
    #[error("illegal peer-sharing transition from {from:?} via {msg_tag}")]
    IllegalTransition {
        /// State the machine was in.
        from: PeerSharingState,
        /// Human-readable tag of the offending message.
        msg_tag: &'static str,
    },
}

impl PeerSharingState {
    /// Computes the next state given an incoming message, or returns
    /// an error if the transition is illegal.
    pub fn transition(self, msg: &PeerSharingMessage) -> Result<Self, PeerSharingTransitionError> {
        match (self, msg) {
            (Self::StClient, PeerSharingMessage::MsgShareRequest { .. }) => Ok(Self::StServer),
            (Self::StClient, PeerSharingMessage::MsgDone) => Ok(Self::StDone),
            (Self::StServer, PeerSharingMessage::MsgSharePeers { .. }) => Ok(Self::StClient),
            (from, msg) => Err(PeerSharingTransitionError::IllegalTransition {
                from,
                msg_tag: match msg {
                    PeerSharingMessage::MsgShareRequest { .. } => "MsgShareRequest",
                    PeerSharingMessage::MsgSharePeers { .. } => "MsgSharePeers",
                    PeerSharingMessage::MsgDone => "MsgDone",
                },
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// CBOR wire codec
// ---------------------------------------------------------------------------

use crate::protocol_size_limits::peersharing as peersharing_limits;
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder, vec_with_strict_capacity};

impl SharedPeerAddress {
    /// Encode a single peer address: `[ip_type, ip_bytes, port]`.
    fn encode(&self, enc: &mut Encoder) {
        match self.addr {
            std::net::SocketAddr::V4(v4) => {
                enc.array(3).unsigned(0);
                enc.bytes(&v4.ip().octets());
                enc.unsigned(u64::from(v4.port()));
            }
            std::net::SocketAddr::V6(v6) => {
                enc.array(3).unsigned(1);
                enc.bytes(&v6.ip().octets());
                enc.unsigned(u64::from(v6.port()));
            }
        }
    }

    /// Decode a single peer address from `[ip_type, ip_bytes, port]`.
    fn decode(dec: &mut Decoder) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 3 {
            return Err(LedgerError::CborTypeMismatch {
                expected: 3,
                actual: len as u8,
            });
        }
        let ip_type = dec.unsigned()?;
        let ip_bytes = dec.bytes()?;
        let port = dec.unsigned()? as u16;
        let addr = match ip_type {
            0 => {
                if ip_bytes.len() != 4 {
                    return Err(LedgerError::CborTypeMismatch {
                        expected: 4,
                        actual: ip_bytes.len() as u8,
                    });
                }
                let ip: [u8; 4] = ip_bytes[..4].try_into().expect("length validated above");
                std::net::SocketAddr::V4(std::net::SocketAddrV4::new(ip.into(), port))
            }
            1 => {
                if ip_bytes.len() != 16 {
                    return Err(LedgerError::CborTypeMismatch {
                        expected: 16,
                        actual: ip_bytes.len() as u8,
                    });
                }
                let ip: [u8; 16] = ip_bytes[..16].try_into().expect("length validated above");
                std::net::SocketAddr::V6(std::net::SocketAddrV6::new(ip.into(), port, 0, 0))
            }
            _ => {
                return Err(LedgerError::CborTypeMismatch {
                    expected: 0,
                    actual: ip_type as u8,
                });
            }
        };
        Ok(Self { addr })
    }
}

impl PeerSharingMessage {
    /// Encode this message to CBOR bytes.
    ///
    /// Wire format:
    /// - `MsgShareRequest` → `[0, amount]`
    /// - `MsgSharePeers`   → `[1, [peer, …]]`
    /// - `MsgDone`         → `[2]`
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::MsgShareRequest { amount } => {
                enc.array(2).unsigned(0).unsigned(u64::from(*amount));
            }
            Self::MsgSharePeers { peers } => {
                enc.array(2).unsigned(1);
                enc.array(peers.len() as u64);
                for p in peers {
                    p.encode(&mut enc);
                }
            }
            Self::MsgDone => {
                enc.array(1).unsigned(2);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    pub fn from_cbor(data: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(data);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, len) {
            (0, 2) => {
                let amount = dec.unsigned()? as u16;
                Self::MsgShareRequest { amount }
            }
            (1, 2) => {
                let peer_count = dec.array()?;
                let mut peers =
                    vec_with_strict_capacity(peer_count, peersharing_limits::PEERS_MAX)?;
                for _ in 0..peer_count {
                    peers.push(SharedPeerAddress::decode(&mut dec)?);
                }
                Self::MsgSharePeers { peers }
            }
            (2, 1) => Self::MsgDone,
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};

    #[test]
    fn state_machine_transitions() {
        let s = PeerSharingState::StClient;
        let s = s
            .transition(&PeerSharingMessage::MsgShareRequest { amount: 5 })
            .expect("StClient → StServer");
        assert_eq!(s, PeerSharingState::StServer);

        let s = s
            .transition(&PeerSharingMessage::MsgSharePeers { peers: vec![] })
            .expect("StServer → StClient");
        assert_eq!(s, PeerSharingState::StClient);

        let s = s
            .transition(&PeerSharingMessage::MsgDone)
            .expect("StClient → StDone");
        assert_eq!(s, PeerSharingState::StDone);
    }

    #[test]
    fn illegal_server_done() {
        let s = PeerSharingState::StServer;
        assert!(s.transition(&PeerSharingMessage::MsgDone).is_err());
    }

    #[test]
    fn illegal_client_share_peers() {
        let s = PeerSharingState::StClient;
        assert!(
            s.transition(&PeerSharingMessage::MsgSharePeers { peers: vec![] })
                .is_err()
        );
    }

    #[test]
    fn cbor_round_trip_share_request() {
        let msg = PeerSharingMessage::MsgShareRequest { amount: 10 };
        let bytes = msg.to_cbor();
        let decoded = PeerSharingMessage::from_cbor(&bytes).expect("decode MsgShareRequest");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn cbor_round_trip_done() {
        let msg = PeerSharingMessage::MsgDone;
        let bytes = msg.to_cbor();
        let decoded = PeerSharingMessage::from_cbor(&bytes).expect("decode MsgDone");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn cbor_round_trip_share_peers_ipv4() {
        let addr = std::net::SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 1), 3001));
        let msg = PeerSharingMessage::MsgSharePeers {
            peers: vec![SharedPeerAddress { addr }],
        };
        let bytes = msg.to_cbor();
        let decoded = PeerSharingMessage::from_cbor(&bytes).expect("decode MsgSharePeers IPv4");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn cbor_round_trip_share_peers_ipv6() {
        let addr = std::net::SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
            3001,
            0,
            0,
        ));
        let msg = PeerSharingMessage::MsgSharePeers {
            peers: vec![SharedPeerAddress { addr }],
        };
        let bytes = msg.to_cbor();
        let decoded = PeerSharingMessage::from_cbor(&bytes).expect("decode MsgSharePeers IPv6");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn cbor_round_trip_share_peers_empty() {
        let msg = PeerSharingMessage::MsgSharePeers { peers: vec![] };
        let bytes = msg.to_cbor();
        let decoded = PeerSharingMessage::from_cbor(&bytes).expect("decode MsgSharePeers empty");
        assert_eq!(msg, decoded);
    }
}
