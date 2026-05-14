//! Minimal bidirectional Mux dispatcher.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side bare-bones SDU
//! demultiplexer for the upstream Network.Mux multiplexer at
//! `.reference-haskell-cardano-node/deps/ouroboros-network/network-mux/src/Network/Mux.hs`.
//! Upstream's Mux runs every mini-protocol concurrently on a
//! shared bearer with per-mini-protocol ingress queues, an egress
//! scheduler that round-robins among ready writers, and a bearer-
//! task lifecycle that supervises both halves. Yggdrasil's
//! [`MuxConnection`] is intentionally smaller: a write-API that
//! serializes outbound SDUs through a tokio mutex, plus a
//! read-task that reads SDUs and routes the payload to per-mini-
//! protocol `mpsc::UnboundedSender` channels.
//!
//! What this is good for: a one-mini-protocol-at-a-time conversation
//! over the same bearer (cardano-tracer use case: the Handshake
//! initiator finishes first, then the TraceObject forwarder runs
//! until shutdown). What it isn't good for: concurrent
//! mini-protocol activity with backpressure / fairness / cancel
//! semantics. The bidirectional Mux state-machine driver that
//! implements those properties is the one remaining sub-item in
//! `docs/TECH-DEBT.md` "cardano-tracer Mux Layer 2/3"; this module
//! is a subset that unblocks operator-side end-to-end soaks today.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use super::bearer::{Bearer, BearerError};
use super::mux::SduHeader;

/// One full received SDU exposed to per-mini-protocol consumers
/// through the channel returned by [`MuxConnection::subscribe`].
#[derive(Clone, Debug)]
pub struct InboundSdu {
    /// The original SDU header — operators care about the
    /// direction and timestamp.
    pub header: SduHeader,
    /// The payload bytes.
    pub payload: Vec<u8>,
}

/// Errors surfaced from `MuxConnection::send_sdu` or returned from
/// the read-loop's outcome.
#[derive(Debug)]
pub enum MuxConnectionError {
    /// Bearer-level read or write failure.
    Bearer(BearerError),
}

impl core::fmt::Display for MuxConnectionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bearer(e) => write!(f, "mux connection bearer error: {e}"),
        }
    }
}

impl std::error::Error for MuxConnectionError {}

/// Subscriber registry for inbound SDUs — protected by a tokio
/// Mutex so the read-task and the subscribe API can mutate it
/// independently.
type SubscriberMap = HashMap<u16, mpsc::UnboundedSender<InboundSdu>>;

/// Multiplexer connection: wraps a [`Bearer<S>`] with a per-mini-
/// protocol dispatch table.
pub struct MuxConnection<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    /// The bearer used for both outbound and inbound SDUs. Wrapped
    /// in a tokio Mutex so concurrent `send_sdu` callers serialize
    /// access — a single outbound SDU is never partially
    /// interleaved with another.
    bearer: Arc<Mutex<Bearer<S>>>,
    /// Per-mini-protocol channel registry. `subscribe(num)`
    /// inserts a new entry; the read-task reads from
    /// `bearer.read_sdu()` and forwards to the corresponding
    /// Sender (silently dropping the SDU if no subscriber is
    /// registered).
    subscribers: Arc<Mutex<SubscriberMap>>,
}

impl<S> MuxConnection<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    /// Construct a new MuxConnection from a Bearer. Call
    /// [`Self::spawn_read_task`] separately to start dispatching
    /// inbound SDUs to subscribers; until that's spawned, the
    /// bearer never reads.
    pub fn new(bearer: Bearer<S>) -> Self {
        Self {
            bearer: Arc::new(Mutex::new(bearer)),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Send one outbound SDU. Serializes against other `send_sdu`
    /// callers through the bearer mutex so SDUs hit the wire as
    /// atomic units.
    pub async fn send_sdu(
        &self,
        header: &SduHeader,
        payload: &[u8],
    ) -> Result<(), MuxConnectionError> {
        let mut bearer = self.bearer.lock().await;
        bearer
            .write_sdu(header, payload)
            .await
            .map_err(MuxConnectionError::Bearer)
    }

    /// Subscribe to inbound SDUs on the given mini-protocol
    /// number. Returns the Receiver half of a tokio mpsc channel;
    /// the read-task pushes each inbound SDU with a matching
    /// `mini_protocol_num` into this channel. If multiple
    /// subscribers register for the same number, the LATEST one
    /// wins (previous Receiver gets dropped on the inserting
    /// side); callers should subscribe at most once per
    /// mini-protocol.
    pub async fn subscribe(&self, mini_protocol_num: u16) -> mpsc::UnboundedReceiver<InboundSdu> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut subscribers = self.subscribers.lock().await;
        subscribers.insert(mini_protocol_num, tx);
        rx
    }

    /// Run the initiator side of the Handshake mini-protocol
    /// (mini-protocol num 0) over this Mux connection.
    ///
    /// Equivalent to
    /// [`super::handshake_driver::run_initiator_handshake`] but
    /// composes cleanly with the per-mini-protocol channel-based
    /// subscription instead of taking a raw `&mut Bearer<S>` —
    /// useful when other mini-protocols share the same bearer
    /// (cardano-tracer use case: handshake → TraceObject forwarding,
    /// both on the same Unix socket).
    ///
    /// MUST be called BEFORE [`Self::spawn_read_task`] starts the
    /// read-loop — this function performs its own blocking
    /// `bearer.read_sdu()` to consume the responder's Accept /
    /// Refuse reply. After it returns, spawn the read-task to
    /// dispatch subsequent SDUs.
    pub async fn run_initiator_handshake(
        &self,
        versions: std::collections::BTreeMap<u32, Vec<u8>>,
    ) -> Result<super::handshake_driver::AgreedVersion, super::handshake_driver::HandshakeDriverError>
    {
        // Take the bearer mutex for the full handshake duration.
        let mut bearer = self.bearer.lock().await;
        super::handshake_driver::run_initiator_handshake(&mut bearer, versions).await
    }

    /// Spawn the read-task that dispatches inbound SDUs to
    /// subscribers. Returns the `JoinHandle` so the caller can
    /// await it on shutdown.
    ///
    /// The task runs until the bearer returns `UnexpectedEof` (or
    /// any other bearer error). On any subscriber that hasn't
    /// been registered, the inbound SDU is silently dropped.
    pub fn spawn_read_task(&self) -> tokio::task::JoinHandle<Result<(), MuxConnectionError>> {
        let bearer = Arc::clone(&self.bearer);
        let subscribers = Arc::clone(&self.subscribers);
        tokio::spawn(async move {
            loop {
                // Take the lock for the duration of one full SDU
                // read (header + payload). Outbound writes can't
                // progress while we're mid-read; that's the
                // simple-dispatcher trade-off versus a real Mux
                // that splits the bearer into independent
                // read/write halves.
                let read_outcome = {
                    let mut bearer_guard = bearer.lock().await;
                    bearer_guard.read_sdu().await
                };
                match read_outcome {
                    Ok((header, payload)) => {
                        let subscribers_guard = subscribers.lock().await;
                        if let Some(tx) = subscribers_guard.get(&header.mini_protocol_num) {
                            let _ = tx.send(InboundSdu { header, payload });
                            // The send-error case is "subscriber's
                            // Receiver was dropped"; that's
                            // unsubscription and we silently
                            // discard the SDU.
                        }
                        // No subscriber: drop the SDU. A real Mux
                        // would buffer for late-registering
                        // subscribers, but the cardano-tracer use
                        // case registers everything before
                        // spawning the read-task.
                    }
                    Err(err) => {
                        return Err(MuxConnectionError::Bearer(err));
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod mux_connection_tests {
    use super::*;
    use crate::trace_forwarder::mux::{
        HANDSHAKE_MINI_PROTOCOL_NUM, MiniProtocolDir, TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
    };
    use std::time::Duration;

    /// Send an SDU on the connection's write half, read it back
    /// off the bearer's read half, confirm header + payload are
    /// preserved.
    #[tokio::test]
    async fn mux_connection_send_round_trips_through_bearer() {
        let (client, server) = tokio::io::duplex(4096);
        let client_bearer = Bearer::new(client);
        let mut server_bearer = Bearer::new(server);
        let conn = MuxConnection::new(client_bearer);

        let header = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Initiator,
            length: 4,
        };
        conn.send_sdu(&header, b"abcd").await.expect("send_sdu");

        let (got_header, got_payload) = server_bearer.read_sdu().await.expect("read");
        assert_eq!(got_header, header);
        assert_eq!(got_payload, b"abcd");
    }

    /// Spawn the read-task; an SDU arriving on the bearer is
    /// dispatched to the matching subscriber's channel.
    #[tokio::test]
    async fn mux_connection_dispatches_inbound_sdu_to_subscriber() {
        let (client, mut server) = tokio::io::duplex(4096);
        let client_bearer = Bearer::new(client);
        let conn = MuxConnection::new(client_bearer);

        // Subscribe to mini-protocol 2 (TraceObject).
        let mut rx = conn.subscribe(TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM).await;

        // Spawn the read-task.
        let _read_task = conn.spawn_read_task();

        // From the server side, write one Responder-direction SDU
        // on mini-protocol 2.
        let outbound_header = SduHeader {
            timestamp: 42,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: 5,
        };
        let outbound_bytes = super::super::mux::encode_sdu_header(&outbound_header)
            .expect("encode header");
        use tokio::io::AsyncWriteExt;
        server.write_all(&outbound_bytes).await.expect("write header");
        server.write_all(b"hello").await.expect("write payload");

        // The subscriber should receive the SDU within a
        // reasonable timeout.
        let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("subscriber receive within 1s")
            .expect("subscriber channel produced an SDU");
        assert_eq!(received.header, outbound_header);
        assert_eq!(received.payload, b"hello");
    }

    /// Compose handshake_driver with MuxConnection: invoke the
    /// handshake via `run_initiator_handshake` (taking the bearer
    /// mutex internally), then spawn the read-task afterwards so
    /// subsequent mini-protocol SDUs dispatch normally.
    #[tokio::test]
    async fn mux_connection_run_initiator_handshake_round_trip() {
        use crate::trace_forwarder::handshake::{HandshakeMessage, decode_message, encode_message};
        use std::collections::BTreeMap;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (client, mut server) = tokio::io::duplex(4096);
        let conn = MuxConnection::new(Bearer::new(client));

        // Fake responder: read Propose, reply Accept on version 2.
        let server_task = tokio::spawn(async move {
            let mut hdr = [0u8; 8];
            server.read_exact(&mut hdr).await.expect("read hdr");
            let hdr_decoded =
                crate::trace_forwarder::mux::decode_sdu_header(&hdr).expect("decode");
            let mut payload = vec![0u8; hdr_decoded.length as usize];
            server.read_exact(&mut payload).await.expect("read payload");
            // Sanity: it was a ProposeVersions in Idle state.
            let _ = decode_message(&payload, true).expect("decode propose");

            // Reply with AcceptVersion(2, <data>).
            let reply = HandshakeMessage::AcceptVersion {
                version: 2,
                data_cbor: {
                    use yggdrasil_ledger::cbor::Encoder;
                    let mut enc = Encoder::new();
                    enc.unsigned(764_824_073u64);
                    enc.into_bytes()
                },
            };
            let reply_payload = encode_message(&reply);
            let reply_hdr = SduHeader {
                timestamp: 0,
                mini_protocol_num: HANDSHAKE_MINI_PROTOCOL_NUM,
                direction: MiniProtocolDir::Responder,
                length: reply_payload.len() as u16,
            };
            let reply_hdr_bytes =
                crate::trace_forwarder::mux::encode_sdu_header(&reply_hdr).expect("encode");
            server.write_all(&reply_hdr_bytes).await.expect("write hdr");
            server.write_all(&reply_payload).await.expect("write payload");
        });

        let mut versions = BTreeMap::new();
        let v_data = {
            use yggdrasil_ledger::cbor::Encoder;
            let mut enc = Encoder::new();
            enc.unsigned(1u64);
            enc.into_bytes()
        };
        versions.insert(1u32, v_data.clone());
        versions.insert(2u32, v_data);
        let agreed = conn
            .run_initiator_handshake(versions)
            .await
            .expect("handshake accept");
        assert_eq!(agreed.version, 2);
        let _ = server_task.await;
    }

    /// SDUs arriving on a mini-protocol that has no subscriber are
    /// silently dropped (the read-task doesn't error).
    #[tokio::test]
    async fn mux_connection_drops_unsubscribed_sdu() {
        let (client, mut server) = tokio::io::duplex(4096);
        let client_bearer = Bearer::new(client);
        let conn = MuxConnection::new(client_bearer);
        // Subscribe to handshake (0) only.
        let mut handshake_rx = conn.subscribe(HANDSHAKE_MINI_PROTOCOL_NUM).await;
        let _read_task = conn.spawn_read_task();

        // Send an SDU on TRACE_OBJECT mini-protocol (no subscriber).
        let trace_header = SduHeader {
            timestamp: 0,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: 3,
        };
        let trace_hdr_bytes = super::super::mux::encode_sdu_header(&trace_header)
            .expect("encode");
        use tokio::io::AsyncWriteExt;
        server.write_all(&trace_hdr_bytes).await.expect("write");
        server.write_all(b"xxx").await.expect("write payload");

        // Now send one on handshake (which IS subscribed).
        let hs_header = SduHeader {
            timestamp: 0,
            mini_protocol_num: HANDSHAKE_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Responder,
            length: 1,
        };
        let hs_bytes = super::super::mux::encode_sdu_header(&hs_header).expect("encode");
        server.write_all(&hs_bytes).await.expect("write");
        server.write_all(b"y").await.expect("write payload");

        // The handshake subscriber must receive the handshake SDU,
        // not the trace-object SDU.
        let received = tokio::time::timeout(Duration::from_secs(1), handshake_rx.recv())
            .await
            .expect("hs receive within 1s")
            .expect("subscriber channel produced an SDU");
        assert_eq!(received.header.mini_protocol_num, HANDSHAKE_MINI_PROTOCOL_NUM);
        assert_eq!(received.payload, b"y");
    }
}
