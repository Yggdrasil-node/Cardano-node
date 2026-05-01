//! ChainSync mini-protocol client driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the ChainSync state machine invariants.  The driver operates entirely
//! at the client-agency level: it sends `MsgRequestNext`, `MsgFindIntersect`,
//! and `MsgDone`, and awaits the corresponding server responses.
//!
//! Per-state time limits from `protocol_limits::chainsync` are enforced on
//! every server response.  Upstream reference:
//! `Ouroboros.Network.Protocol.ChainSync.Codec.timeLimitsChainSync`.
//!
//! Reference: `Ouroboros.Network.Protocol.ChainSync.Client`.

use std::time::Duration;

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocol_limits::chainsync as cs_limits;
use crate::protocols::{ChainSyncMessage, ChainSyncState, ChainSyncTransitionError};
use yggdrasil_ledger::{CborDecode, CborEncode, Point, Tip};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// The server's response to a `MsgRequestNext`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NextResponse {
    /// A new header was rolled forward.
    RollForward {
        /// Serialised block header.
        header: Vec<u8>,
        /// Serialised tip.
        tip: Vec<u8>,
    },
    /// The chain rolled backward to a prior point.
    RollBackward {
        /// Serialised point to roll back to.
        point: Vec<u8>,
        /// Serialised tip.
        tip: Vec<u8>,
    },
    /// The server asked us to wait and then later delivered a roll-forward.
    AwaitRollForward { header: Vec<u8>, tip: Vec<u8> },
    /// The server asked us to wait and then later delivered a rolled-backward.
    AwaitRollBackward { point: Vec<u8>, tip: Vec<u8> },
}

/// The server's response to a `MsgFindIntersect`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IntersectResponse {
    /// An intersection was found.
    Found {
        /// The intersection point.
        point: Vec<u8>,
        /// Current tip.
        tip: Vec<u8>,
    },
    /// No intersection was found.
    NotFound {
        /// Current tip.
        tip: Vec<u8>,
    },
}

/// The server's response to a `MsgRequestNext`, with point and tip payloads
/// decoded into ledger `Point` values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedNextResponse {
    /// A new header was rolled forward.
    RollForward {
        /// Serialised block header.
        header: Vec<u8>,
        /// Decoded current tip.
        tip: Point,
    },
    /// The chain rolled backward to a prior point.
    RollBackward {
        /// Decoded rollback target point.
        point: Point,
        /// Decoded current tip.
        tip: Point,
    },
    /// The server asked us to wait and then later delivered a roll-forward.
    AwaitRollForward {
        /// Serialised block header.
        header: Vec<u8>,
        /// Decoded current tip.
        tip: Point,
    },
    /// The server asked us to wait and then later delivered a roll-backward.
    AwaitRollBackward {
        /// Decoded rollback target point.
        point: Point,
        /// Decoded current tip.
        tip: Point,
    },
}

/// The server's response to a `MsgFindIntersect`, with point and tip payloads
/// decoded into ledger `Point` values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedIntersectResponse {
    /// An intersection was found.
    Found {
        /// The decoded intersection point.
        point: Point,
        /// Decoded current tip.
        tip: Point,
    },
    /// No intersection was found.
    NotFound {
        /// Decoded current tip.
        tip: Point,
    },
}

/// The server's response to a `MsgRequestNext`, with both the header and any
/// point/tip payloads decoded into ledger/domain values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedHeaderNextResponse<H> {
    /// A new header was rolled forward.
    RollForward {
        /// Decoded block header.
        header: H,
        /// Decoded current tip.
        tip: Point,
    },
    /// The chain rolled backward to a prior point.
    RollBackward {
        /// Decoded rollback target point.
        point: Point,
        /// Decoded current tip.
        tip: Point,
    },
    /// The server asked us to wait and then later delivered a roll-forward.
    AwaitRollForward {
        /// Decoded block header.
        header: H,
        /// Decoded current tip.
        tip: Point,
    },
    /// The server asked us to wait and then later delivered a roll-backward.
    AwaitRollBackward {
        /// Decoded rollback target point.
        point: Point,
        /// Decoded current tip.
        tip: Point,
    },
}

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the ChainSync client driver.
#[derive(Debug, thiserror::Error)]
pub enum ChainSyncClientError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// The server did not respond within the per-state time limit.
    ///
    /// Upstream: `ExceededTimeLimit` from `ProtocolTimeLimits`.
    #[error("protocol timeout ({0:?})")]
    Timeout(Duration),

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] ChainSyncTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),

    /// Point payload decode failure.
    #[error("point decode error: {0}")]
    PointDecode(String),

    /// Header payload decode failure.
    #[error("header decode error: {0}")]
    HeaderDecode(String),
}

// ---------------------------------------------------------------------------
// ChainSyncClient
// ---------------------------------------------------------------------------

/// A ChainSync client driver maintaining the protocol state machine.
///
/// All public methods advance the state machine and return strongly-typed
/// responses.  The driver is cancel-safe: dropping it in any state is
/// allowed (the muxer will clean up the channel).
pub struct ChainSyncClient {
    channel: MessageChannel,
    state: ChainSyncState,
}

impl ChainSyncClient {
    /// Create a new client driver from a ChainSync `ProtocolHandle`.
    ///
    /// The protocol starts in `StIdle` — client agency.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: ChainSyncState::StIdle,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> ChainSyncState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(&mut self, msg: &ChainSyncMessage) -> Result<(), ChainSyncClientError> {
        let next_state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(ChainSyncClientError::Mux)?;
        self.state = next_state;
        Ok(())
    }

    /// Receive with an optional per-state time limit.
    ///
    /// When `limit` is `Some(d)`, the recv is wrapped in
    /// `tokio::time::timeout(d, …)`. `None` means wait forever.
    async fn recv_msg_timeout(
        &mut self,
        limit: Option<Duration>,
    ) -> Result<ChainSyncMessage, ChainSyncClientError> {
        let raw = match limit {
            Some(d) => tokio::time::timeout(d, self.channel.recv())
                .await
                .map_err(|_| ChainSyncClientError::Timeout(d))?
                .ok_or(ChainSyncClientError::ConnectionClosed)?,
            None => self
                .channel
                .recv()
                .await
                .ok_or(ChainSyncClientError::ConnectionClosed)?,
        };
        let msg = ChainSyncMessage::from_cbor(&raw)
            .map_err(|e| ChainSyncClientError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    fn decode_point(raw: &[u8]) -> Result<Point, ChainSyncClientError> {
        Point::from_cbor_bytes(raw).map_err(|e| ChainSyncClientError::PointDecode(e.to_string()))
    }

    fn decode_tip(raw: &[u8]) -> Result<Point, ChainSyncClientError> {
        Tip::from_cbor_bytes(raw)
            .map(|t| t.point())
            .map_err(|e| ChainSyncClientError::PointDecode(e.to_string()))
    }

    fn decode_header<H: CborDecode>(raw: &[u8]) -> Result<H, ChainSyncClientError> {
        H::from_cbor_bytes(raw).map_err(|e| ChainSyncClientError::HeaderDecode(e.to_string()))
    }

    // -- public API -------------------------------------------------------

    /// Send `MsgFindIntersect` with the given candidate points and wait
    /// for the server's `MsgIntersectFound` or `MsgIntersectNotFound`.
    ///
    /// Enforces `chainsync::ST_INTERSECT` time limit (10 s).
    ///
    /// The client must be in `StIdle`.
    pub async fn find_intersect(
        &mut self,
        points: Vec<Vec<u8>>,
    ) -> Result<IntersectResponse, ChainSyncClientError> {
        self.send_msg(&ChainSyncMessage::MsgFindIntersect { points })
            .await?;
        let msg = self.recv_msg_timeout(cs_limits::ST_INTERSECT).await?;
        match msg {
            ChainSyncMessage::MsgIntersectFound { point, tip } => {
                Ok(IntersectResponse::Found { point, tip })
            }
            ChainSyncMessage::MsgIntersectNotFound { tip } => {
                Ok(IntersectResponse::NotFound { tip })
            }
            other => Err(ChainSyncClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Send `MsgFindIntersect` with typed candidate points and decode the
    /// server response into typed ledger `Point` values.
    pub async fn find_intersect_points(
        &mut self,
        points: Vec<Point>,
    ) -> Result<TypedIntersectResponse, ChainSyncClientError> {
        let encoded = points.into_iter().map(|p| p.to_cbor_bytes()).collect();
        match self.find_intersect(encoded).await? {
            IntersectResponse::Found { point, tip } => Ok(TypedIntersectResponse::Found {
                point: Self::decode_point(&point)?,
                tip: Self::decode_tip(&tip)?,
            }),
            IntersectResponse::NotFound { tip } => Ok(TypedIntersectResponse::NotFound {
                tip: Self::decode_tip(&tip)?,
            }),
        }
    }

    /// Send `MsgRequestNext` and wait for the server's roll-forward,
    /// roll-backward, or await-then-reply sequence.
    ///
    /// Enforces `chainsync::ST_NEXT_CAN_AWAIT` (10 s) for the initial
    /// response. After `MsgAwaitReply`, the follow-up response uses
    /// `chainsync::ST_NEXT_MUST_REPLY_TRUSTABLE` (wait forever for
    /// trustable peers). Non-trustable timeout should be applied at the
    /// runtime layer using `chainsync::MUST_REPLY_MIN_SECS`..`MAX_SECS`.
    ///
    /// The client must be in `StIdle`.
    pub async fn request_next(&mut self) -> Result<NextResponse, ChainSyncClientError> {
        self.send_msg(&ChainSyncMessage::MsgRequestNext).await?;
        let msg = self.recv_msg_timeout(cs_limits::ST_NEXT_CAN_AWAIT).await?;
        match msg {
            ChainSyncMessage::MsgRollForward { header, tip } => {
                Ok(NextResponse::RollForward { header, tip })
            }
            ChainSyncMessage::MsgRollBackward { point, tip } => {
                Ok(NextResponse::RollBackward { point, tip })
            }
            ChainSyncMessage::MsgAwaitReply => {
                // After AwaitReply, wait for the real response.
                // Trustable peers: wait forever (upstream default).
                // Non-trustable peers: randomized 135–269 s — applied by
                // the runtime caller wrapping this method.
                let follow_up = self
                    .recv_msg_timeout(cs_limits::ST_NEXT_MUST_REPLY_TRUSTABLE)
                    .await?;
                match follow_up {
                    ChainSyncMessage::MsgRollForward { header, tip } => {
                        Ok(NextResponse::AwaitRollForward { header, tip })
                    }
                    ChainSyncMessage::MsgRollBackward { point, tip } => {
                        Ok(NextResponse::AwaitRollBackward { point, tip })
                    }
                    other => Err(ChainSyncClientError::UnexpectedMessage(
                        other.tag_name().to_string(),
                    )),
                }
            }
            other => Err(ChainSyncClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Send `MsgRequestNext` and decode any point or tip payloads in the
    /// server response into typed ledger `Point` values.
    pub async fn request_next_typed(&mut self) -> Result<TypedNextResponse, ChainSyncClientError> {
        match self.request_next().await? {
            NextResponse::RollForward { header, tip } => Ok(TypedNextResponse::RollForward {
                header,
                tip: Self::decode_tip(&tip)?,
            }),
            NextResponse::RollBackward { point, tip } => Ok(TypedNextResponse::RollBackward {
                point: Self::decode_point(&point)?,
                tip: Self::decode_tip(&tip)?,
            }),
            NextResponse::AwaitRollForward { header, tip } => {
                Ok(TypedNextResponse::AwaitRollForward {
                    header,
                    tip: Self::decode_tip(&tip)?,
                })
            }
            NextResponse::AwaitRollBackward { point, tip } => {
                Ok(TypedNextResponse::AwaitRollBackward {
                    point: Self::decode_point(&point)?,
                    tip: Self::decode_tip(&tip)?,
                })
            }
        }
    }

    /// Send `MsgRequestNext` and decode the returned header plus any point/tip
    /// payloads into typed values.
    pub async fn request_next_decoded_header<H: CborDecode>(
        &mut self,
    ) -> Result<DecodedHeaderNextResponse<H>, ChainSyncClientError> {
        match self.request_next_typed().await? {
            TypedNextResponse::RollForward { header, tip } => {
                Ok(DecodedHeaderNextResponse::RollForward {
                    header: Self::decode_header(&header)?,
                    tip,
                })
            }
            TypedNextResponse::RollBackward { point, tip } => {
                Ok(DecodedHeaderNextResponse::RollBackward { point, tip })
            }
            TypedNextResponse::AwaitRollForward { header, tip } => {
                Ok(DecodedHeaderNextResponse::AwaitRollForward {
                    header: Self::decode_header(&header)?,
                    tip,
                })
            }
            TypedNextResponse::AwaitRollBackward { point, tip } => {
                Ok(DecodedHeaderNextResponse::AwaitRollBackward { point, tip })
            }
        }
    }

    /// Send `MsgDone` to terminate the protocol cleanly.
    ///
    /// The client must be in `StIdle`.  After this call the driver is in
    /// `StDone` and no further operations are possible.
    pub async fn done(mut self) -> Result<(), ChainSyncClientError> {
        self.send_msg(&ChainSyncMessage::MsgDone).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ChainSyncClientError Display-content tests ─────────────────────

    #[test]
    fn display_chainsync_connection_closed() {
        let s = format!("{}", ChainSyncClientError::ConnectionClosed);
        assert!(s.to_lowercase().contains("connection closed"));
    }

    #[test]
    fn display_chainsync_timeout_surfaces_duration() {
        let e = ChainSyncClientError::Timeout(std::time::Duration::from_secs(269));
        let s = format!("{e}");
        assert!(s.contains("timeout"), "rule name: {s}");
        assert!(s.contains("269"), "must surface the duration: {s}");
    }

    #[test]
    fn display_chainsync_decode_propagates_inner_reason() {
        let e = ChainSyncClientError::Decode("expected map".into());
        let s = format!("{e}");
        assert!(s.contains("CBOR decode"));
        assert!(s.contains("expected map"));
    }

    #[test]
    fn display_chainsync_unexpected_message_propagates_inner() {
        let e = ChainSyncClientError::UnexpectedMessage("RollBackward in StIdle".into());
        let s = format!("{e}");
        assert!(s.contains("unexpected message"));
        assert!(s.contains("RollBackward in StIdle"));
    }

    #[test]
    fn display_chainsync_point_decode_propagates_inner_reason() {
        let e = ChainSyncClientError::PointDecode("bad slot bytes".into());
        let s = format!("{e}");
        assert!(s.contains("point decode"));
        assert!(s.contains("bad slot bytes"));
    }

    #[test]
    fn display_chainsync_header_decode_propagates_inner_reason() {
        let e = ChainSyncClientError::HeaderDecode("bad vrf proof size".into());
        let s = format!("{e}");
        assert!(s.contains("header decode"));
        assert!(s.contains("bad vrf proof size"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn send_failure_does_not_advance_protocol_state() {
        use crate::multiplexer::{MiniProtocolDir, MiniProtocolNum};
        use crate::mux::start_unix;
        use tokio::net::UnixStream;

        let (client_stream, _server_stream) = UnixStream::pair().expect("unix stream pair");
        let (mut handles, mux_handle) = start_unix(
            client_stream,
            MiniProtocolDir::Initiator,
            &[MiniProtocolNum::CHAIN_SYNC],
            1,
        );
        let handle = handles
            .remove(&MiniProtocolNum::CHAIN_SYNC)
            .expect("chainsync handle");
        mux_handle.abort();
        tokio::task::yield_now().await;

        let mut client = ChainSyncClient::new(handle);
        let result = client.find_intersect_points(vec![Point::Origin]).await;

        assert!(matches!(result, Err(ChainSyncClientError::Mux(_))));
        assert_eq!(client.state(), ChainSyncState::StIdle);
    }
}
