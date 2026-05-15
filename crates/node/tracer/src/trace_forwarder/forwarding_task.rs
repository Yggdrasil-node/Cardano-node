//! Background task that drains TraceObjects off a tokio channel and
//! writes them to a cardano-tracer bearer.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis that wires
//! the existing [`super::event_builder`] (data transform),
//! [`super::TraceObject::to_cbor`] (Layer 1 codec),
//! [`super::mini_protocol::encode_reply`] (Layer 2 codec) and
//! [`super::bearer::Bearer`] (Layer 3 transport) into one
//! background task. The upstream `cardano-tracer` forwarder side
//! runs the equivalent via `Trace.Forward.Run.TraceObject.Forwarder.runTraceObjectForwarder`
//! at
//! `.reference-haskell-cardano-node/trace-forward/src/Trace/Forward/Run/TraceObject/Forwarder.hs`.
//!
//! This module ships the one-way write-only forwarder side — the
//! producer (node) drains TraceObjects through the channel; the task
//! batches them and writes one `MsgTraceObjectsReply` SDU per batch
//! to the bearer. The bidirectional Mux state machine (which would
//! also need an INGRESS side to receive `MsgTraceObjectsRequest`
//! from the acceptor) stays deferred per the
//! `docs/TECH-DEBT.md` "cardano-tracer Mux Layer 2/3" entry: a
//! real cardano-tracer acceptor sends pull-style requests, so this
//! task currently only works against a YGGDRASIL-side acceptor or
//! a request-tolerant test harness. The Mux state-machine driver
//! follow-on closes that interop gap.
//!
//! ## How a binary wires it
//!
//! ```ignore
//! use tokio::sync::mpsc;
//! let (tx, rx) = mpsc::unbounded_channel::<TraceObject>();
//! let socket = tokio::net::UnixStream::connect("/run/cardano-tracer.sock").await?;
//! let bearer = Bearer::new(socket);
//! tokio::spawn(forwarding_task::run(rx, bearer, ForwardingTaskConfig {
//!     batch_size: 64,
//!     flush_interval: Duration::from_millis(100),
//! }));
//! // Hand `tx` to the tracing-subscriber Layer's on_event callback.
//! ```

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::UnboundedReceiver;

use super::TraceObject;
use super::bearer::{Bearer, BearerError};
use super::mini_protocol::encode_reply;
use super::mux::{MiniProtocolDir, SduHeader, TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM};

/// Knobs for the forwarding loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForwardingTaskConfig {
    /// Maximum number of TraceObjects per outgoing
    /// `MsgTraceObjectsReply`. Defaults to 64 — a balance between
    /// per-SDU framing overhead and per-batch latency.
    pub batch_size: usize,
    /// Maximum delay between SDU writes when the buffer is below
    /// `batch_size`. Bounded so a low-rate producer doesn't keep
    /// a single event sitting in the buffer forever. Defaults to
    /// 100ms.
    pub flush_interval: Duration,
}

impl Default for ForwardingTaskConfig {
    fn default() -> Self {
        Self {
            batch_size: 64,
            flush_interval: Duration::from_millis(100),
        }
    }
}

/// Errors returned by [`run`].
#[derive(Debug)]
pub enum ForwardingTaskError {
    /// The bearer write failed. After this error the task aborts;
    /// the channel sender stays open but the task is no longer
    /// draining it (subscriber should be re-installed if the
    /// transport recovers).
    Bearer(BearerError),
    /// SDU payload exceeded the 16-bit length field
    /// (65 535 bytes). Reduce `batch_size` or shrink individual
    /// TraceObject CBOR payloads.
    PayloadTooLarge(usize),
}

impl core::fmt::Display for ForwardingTaskError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bearer(e) => write!(f, "forwarding bearer error: {e}"),
            Self::PayloadTooLarge(n) => write!(
                f,
                "forwarding SDU payload {n} bytes exceeds 16-bit length field (max 65535)"
            ),
        }
    }
}

impl std::error::Error for ForwardingTaskError {}

/// Run the forwarding loop until `rx` is closed (every Sender
/// dropped) or a bearer write fails.
///
/// Reads from `rx` with a `flush_interval` timeout: when either
/// `batch_size` items arrive or the interval elapses with at least
/// one item buffered, encodes the buffered TraceObjects as one
/// `MsgTraceObjectsReply` CBOR message, wraps in an Initiator-
/// direction TraceObject-forward SDU header, and writes the result
/// via the bearer.
///
/// On clean channel close (all senders dropped) the task flushes
/// any remaining buffered items and returns `Ok(())`.
pub async fn run<S>(
    mut rx: UnboundedReceiver<TraceObject>,
    mut bearer: Bearer<S>,
    config: ForwardingTaskConfig,
) -> Result<(), ForwardingTaskError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut buffer: Vec<TraceObject> = Vec::with_capacity(config.batch_size);

    loop {
        // Wait for the next event OR the flush timer.
        let recv_outcome = if buffer.is_empty() {
            // Empty buffer → block indefinitely (no timer pressure).
            match rx.recv().await {
                Some(to) => Some(to),
                None => break, // channel closed
            }
        } else {
            // Non-empty buffer → race the receive against the flush timer.
            tokio::select! {
                maybe = rx.recv() => maybe,
                _ = tokio::time::sleep(config.flush_interval) => None,
            }
        };

        match recv_outcome {
            Some(trace_object) => {
                buffer.push(trace_object);
                if buffer.len() >= config.batch_size {
                    flush(&mut buffer, &mut bearer).await?;
                }
            }
            None => {
                // Either timer elapsed (and buffer is non-empty) or
                // channel closed. Either way flush the buffer.
                if !buffer.is_empty() {
                    flush(&mut buffer, &mut bearer).await?;
                }
                // If the channel is closed, exit the loop.
                if rx.is_closed() && rx.is_empty() {
                    break;
                }
            }
        }
    }

    // Drain anything still in flight before returning.
    while let Ok(to) = rx.try_recv() {
        buffer.push(to);
        if buffer.len() >= config.batch_size {
            flush(&mut buffer, &mut bearer).await?;
        }
    }
    if !buffer.is_empty() {
        flush(&mut buffer, &mut bearer).await?;
    }
    Ok(())
}

/// Encode the buffered TraceObjects as one `MsgTraceObjectsReply`
/// SDU and write it via the bearer. Clears the buffer on success.
async fn flush<S>(
    buffer: &mut Vec<TraceObject>,
    bearer: &mut Bearer<S>,
) -> Result<(), ForwardingTaskError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let (header, payload) = build_reply_sdu(buffer)?;
    bearer
        .write_sdu(&header, &payload)
        .await
        .map_err(ForwardingTaskError::Bearer)?;
    buffer.clear();
    Ok(())
}

/// Build the (header, payload) pair for a flush. Shared between
/// `run` (raw Bearer) and `run_via_mux` (Bearer behind a
/// MuxConnection mutex).
fn build_reply_sdu(buffer: &[TraceObject]) -> Result<(SduHeader, Vec<u8>), ForwardingTaskError> {
    let payload = encode_reply(buffer);
    if payload.len() > u16::MAX as usize {
        return Err(ForwardingTaskError::PayloadTooLarge(payload.len()));
    }
    let header = SduHeader {
        // Forwarder timestamp is informational only on the upstream
        // protocol; 0 is acceptable until the Mux scheduler wires a
        // real RemoteClockModel tick source.
        timestamp: 0,
        mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
        direction: MiniProtocolDir::Initiator,
        length: payload.len() as u16,
    };
    Ok((header, payload))
}

/// Same as [`run`] but writes through a [`super::mux_connection::MuxConnection`]
/// instead of holding the bearer directly. Use this variant when
/// other mini-protocols share the same bearer (cardano-tracer use
/// case: forwarding_task runs concurrently with the read-task
/// dispatched by `MuxConnection::spawn_read_task`).
///
/// The MuxConnection's internal bearer-mutex serializes the
/// forwarding writes against any concurrent `send_sdu` callers,
/// so SDUs hit the wire atomically.
pub async fn run_via_mux<S>(
    mut rx: UnboundedReceiver<TraceObject>,
    mux: std::sync::Arc<super::mux_connection::MuxConnection<S>>,
    config: ForwardingTaskConfig,
) -> Result<(), ForwardingTaskError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut buffer: Vec<TraceObject> = Vec::with_capacity(config.batch_size);

    loop {
        let recv_outcome = if buffer.is_empty() {
            match rx.recv().await {
                Some(to) => Some(to),
                None => break,
            }
        } else {
            tokio::select! {
                maybe = rx.recv() => maybe,
                _ = tokio::time::sleep(config.flush_interval) => None,
            }
        };

        match recv_outcome {
            Some(trace_object) => {
                buffer.push(trace_object);
                if buffer.len() >= config.batch_size {
                    flush_via_mux(&mut buffer, &mux).await?;
                }
            }
            None => {
                if !buffer.is_empty() {
                    flush_via_mux(&mut buffer, &mux).await?;
                }
                if rx.is_closed() && rx.is_empty() {
                    break;
                }
            }
        }
    }

    while let Ok(to) = rx.try_recv() {
        buffer.push(to);
        if buffer.len() >= config.batch_size {
            flush_via_mux(&mut buffer, &mux).await?;
        }
    }
    if !buffer.is_empty() {
        flush_via_mux(&mut buffer, &mux).await?;
    }
    Ok(())
}

/// Mux-flavoured flush helper. Mirrors `flush` but routes the
/// outbound SDU through `MuxConnection::send_sdu` instead of the
/// raw bearer.
async fn flush_via_mux<S>(
    buffer: &mut Vec<TraceObject>,
    mux: &super::mux_connection::MuxConnection<S>,
) -> Result<(), ForwardingTaskError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (header, payload) = build_reply_sdu(buffer)?;
    mux.send_sdu(&header, &payload).await.map_err(|e| match e {
        super::mux_connection::MuxConnectionError::Bearer(b) => ForwardingTaskError::Bearer(b),
    })?;
    buffer.clear();
    Ok(())
}

#[cfg(test)]
mod forwarding_task_tests {
    use super::*;
    use crate::trace_forwarder::mini_protocol::{TraceForwardMessage, decode_message};
    use crate::trace_forwarder::{TraceDetail, TraceSeverity};
    use tokio::sync::mpsc;

    fn sample_trace_object(msg: &str) -> TraceObject {
        TraceObject {
            to_human: None,
            to_machine: format!("{{\"msg\":\"{msg}\"}}"),
            to_namespace: vec!["Net".to_string()],
            to_severity: TraceSeverity::Info,
            to_details: TraceDetail::DNormal,
            to_timestamp: (2026, 135, 0),
            to_hostname: "test".to_string(),
            to_thread_id: "t1".to_string(),
        }
    }

    /// Channel close: a tx-side drop flushes the buffer + ends the
    /// task cleanly with Ok.
    #[tokio::test]
    async fn forwarding_task_flushes_on_channel_close() {
        let (client, server) = tokio::io::duplex(8192);
        let client_bearer = Bearer::new(client);
        let mut server_bearer = Bearer::new(server);
        let (tx, rx) = mpsc::unbounded_channel::<TraceObject>();

        // Spawn the forwarder.
        let join = tokio::spawn(run(
            rx,
            client_bearer,
            ForwardingTaskConfig {
                batch_size: 4,
                flush_interval: Duration::from_millis(50),
            },
        ));

        // Send two events then close.
        tx.send(sample_trace_object("a")).expect("send a");
        tx.send(sample_trace_object("b")).expect("send b");
        drop(tx);

        // Read what the bearer received.
        let (header, payload) = server_bearer.read_sdu().await.expect("read sdu");
        assert_eq!(
            header.mini_protocol_num,
            TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM
        );
        assert_eq!(header.direction, MiniProtocolDir::Initiator);
        let msg = decode_message(&payload).expect("decode message");
        match msg {
            TraceForwardMessage::Reply(traces) => {
                assert_eq!(traces.len(), 2);
                assert_eq!(traces[0].to_machine, r#"{"msg":"a"}"#);
                assert_eq!(traces[1].to_machine, r#"{"msg":"b"}"#);
            }
            _ => panic!("expected MsgTraceObjectsReply"),
        }

        // Task should have completed cleanly.
        let outcome = join.await.expect("join");
        assert!(outcome.is_ok(), "task returned {outcome:?}");
    }

    /// Batch-size trigger: filling the buffer to `batch_size`
    /// flushes immediately without waiting for the timer.
    #[tokio::test]
    async fn forwarding_task_flushes_at_batch_size() {
        let (client, server) = tokio::io::duplex(8192);
        let client_bearer = Bearer::new(client);
        let mut server_bearer = Bearer::new(server);
        let (tx, rx) = mpsc::unbounded_channel::<TraceObject>();

        let join = tokio::spawn(run(
            rx,
            client_bearer,
            ForwardingTaskConfig {
                batch_size: 3,
                flush_interval: Duration::from_secs(60), // long timer; rely on batch trigger
            },
        ));

        // Send exactly batch_size events; the task should flush as
        // soon as the 3rd arrives.
        tx.send(sample_trace_object("1")).expect("send 1");
        tx.send(sample_trace_object("2")).expect("send 2");
        tx.send(sample_trace_object("3")).expect("send 3");

        // Race the SDU read against a 2-second timeout — if the
        // batch trigger doesn't fire we'd block on the 60-second
        // flush_interval.
        let (_header, payload) =
            tokio::time::timeout(Duration::from_secs(2), server_bearer.read_sdu())
                .await
                .expect("batch flush within 2s timeout")
                .expect("read sdu");

        let msg = decode_message(&payload).expect("decode");
        if let TraceForwardMessage::Reply(traces) = msg {
            assert_eq!(traces.len(), 3);
        } else {
            panic!("expected Reply");
        }
        drop(tx);
        let _ = join.await;
    }

    /// Mux-flavoured variant: route the forwarding writes through
    /// a `MuxConnection`. Composes correctly with the
    /// bearer-mutex; SDUs hit the wire identical to the
    /// non-mux `run` path.
    #[tokio::test]
    async fn forwarding_task_via_mux_round_trips() {
        use crate::trace_forwarder::mux_connection::MuxConnection;
        use std::sync::Arc;

        let (client, server) = tokio::io::duplex(8192);
        let client_bearer = Bearer::new(client);
        let mut server_bearer = Bearer::new(server);
        let mux = Arc::new(MuxConnection::new(client_bearer));
        let (tx, rx) = mpsc::unbounded_channel::<TraceObject>();

        let join = tokio::spawn(run_via_mux(
            rx,
            Arc::clone(&mux),
            ForwardingTaskConfig {
                batch_size: 4,
                flush_interval: Duration::from_millis(50),
            },
        ));

        tx.send(sample_trace_object("via-mux-1")).expect("send 1");
        tx.send(sample_trace_object("via-mux-2")).expect("send 2");
        drop(tx);

        // Read what reached the server side.
        let (header, payload) = server_bearer.read_sdu().await.expect("read sdu");
        assert_eq!(
            header.mini_protocol_num,
            TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM
        );
        let msg = decode_message(&payload).expect("decode");
        match msg {
            TraceForwardMessage::Reply(traces) => {
                assert_eq!(traces.len(), 2);
                assert_eq!(traces[0].to_machine, r#"{"msg":"via-mux-1"}"#);
                assert_eq!(traces[1].to_machine, r#"{"msg":"via-mux-2"}"#);
            }
            _ => panic!("expected Reply"),
        }
        let outcome = join.await.expect("join");
        assert!(outcome.is_ok(), "task returned {outcome:?}");
    }

    /// Flush-interval trigger: one event sits in the buffer; after
    /// `flush_interval` elapses the task flushes a single-element
    /// SDU even though batch_size hasn't been reached.
    #[tokio::test]
    async fn forwarding_task_flushes_on_interval() {
        let (client, server) = tokio::io::duplex(8192);
        let client_bearer = Bearer::new(client);
        let mut server_bearer = Bearer::new(server);
        let (tx, rx) = mpsc::unbounded_channel::<TraceObject>();

        let join = tokio::spawn(run(
            rx,
            client_bearer,
            ForwardingTaskConfig {
                batch_size: 100, // large; rely on the timer
                flush_interval: Duration::from_millis(50),
            },
        ));

        tx.send(sample_trace_object("solo")).expect("send solo");

        // The task should write within the flush_interval window.
        // Give a generous 1-second timeout to absorb scheduler jitter.
        let (_header, payload) =
            tokio::time::timeout(Duration::from_secs(1), server_bearer.read_sdu())
                .await
                .expect("interval flush within 1s timeout")
                .expect("read sdu");

        let msg = decode_message(&payload).expect("decode");
        if let TraceForwardMessage::Reply(traces) = msg {
            assert_eq!(
                traces.len(),
                1,
                "interval flush should emit just the buffered solo event"
            );
            assert_eq!(traces[0].to_machine, r#"{"msg":"solo"}"#);
        } else {
            panic!("expected Reply");
        }
        drop(tx);
        let _ = join.await;
    }
}
