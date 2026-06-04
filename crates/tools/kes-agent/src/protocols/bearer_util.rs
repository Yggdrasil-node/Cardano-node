//! Duplex raw-bearer helper vocabulary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/BearerUtil.hs.
//!
//! Mirrors the public helper vocabulary and byte-buffering contract of
//! upstream `Cardano.KESAgent.Protocols.BearerUtil`. The concrete
//! `Ouroboros.Network.RawBearer` socket wrapper lands in the
//! daemon/socket follow-on; this module keeps the one-byte buffering
//! semantics testable without raw socket I/O.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

/// Upstream `bufferSize` used by `withDuplexBearer` receiver reads.
pub const BUFFER_SIZE: usize = 1024;

/// Idiomatic Rust casing for upstream local `bufferSize`.
pub const fn buffer_size() -> usize {
    BUFFER_SIZE
}

/// Error raised when the receiver side observes EOF. Mirrors upstream
/// `BearerConnectionClosed`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct BearerConnectionClosed;

impl fmt::Display for BearerConnectionClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BearerConnectionClosed")
    }
}

impl Error for BearerConnectionClosed {}

/// In-memory receive channel used to model upstream `TChan m Word8`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DuplexRecvChan {
    bytes: VecDeque<u8>,
}

impl DuplexRecvChan {
    /// Construct an empty receive channel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of buffered bytes currently available.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the receive channel currently has no buffered bytes.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Write bytes into the receive channel one byte at a time, matching
    /// upstream `forM_ [0 .. bytesRead - 1] ... writeTChan`.
    pub fn write_bytes_one_at_a_time(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.bytes.push_back(*byte);
        }
    }

    /// Read exactly `num_bytes` from the wrapped channel. Upstream blocks
    /// until enough bytes are available; the pure mirror reports
    /// `None` when a test asks for bytes that have not been buffered.
    pub fn recv_exact(&mut self, num_bytes: usize) -> Option<Vec<u8>> {
        if self.bytes.len() < num_bytes {
            return None;
        }
        Some(
            (0..num_bytes)
                .map(|_| self.bytes.pop_front().expect("length checked"))
                .collect(),
        )
    }
}

/// Result of one upstream receiver read loop step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DuplexReceiverStep {
    /// Bytes were forwarded into the receive channel.
    Forwarded(usize),
    /// EOF was detected and upstream would return
    /// `BearerConnectionClosed`.
    Closed(BearerConnectionClosed),
}

/// Model one receiver step from upstream `withDuplexBearer`.
pub fn duplex_receiver_step(
    recv_chan: &mut DuplexRecvChan,
    bytes_read: &[u8],
) -> DuplexReceiverStep {
    if bytes_read.is_empty() {
        DuplexReceiverStep::Closed(BearerConnectionClosed)
    } else {
        recv_chan.write_bytes_one_at_a_time(bytes_read);
        DuplexReceiverStep::Forwarded(bytes_read.len())
    }
}

/// Model upstream `withDuplexBearer` for already-captured input chunks.
///
/// Every non-empty chunk is forwarded through the byte channel in order.
/// The first empty chunk represents EOF and returns
/// `BearerConnectionClosed`, matching the receiver side that wins the
/// upstream `race`.
pub fn with_duplex_bearer(chunks: &[&[u8]]) -> Result<DuplexRecvChan, BearerConnectionClosed> {
    let mut recv_chan = DuplexRecvChan::new();
    for chunk in chunks {
        match duplex_receiver_step(&mut recv_chan, chunk) {
            DuplexReceiverStep::Forwarded(_) => {}
            DuplexReceiverStep::Closed(err) => return Err(err),
        }
    }
    Ok(recv_chan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_size_matches_upstream_constant() {
        assert_eq!(BUFFER_SIZE, 1024);
        assert_eq!(buffer_size(), 1024);
    }

    #[test]
    fn bearer_connection_closed_displays_like_upstream_show() {
        assert_eq!(BearerConnectionClosed.to_string(), "BearerConnectionClosed");
    }

    #[test]
    fn receiver_step_forwards_data_one_byte_at_a_time() {
        let mut chan = DuplexRecvChan::new();
        assert_eq!(
            duplex_receiver_step(&mut chan, &[1, 2, 3]),
            DuplexReceiverStep::Forwarded(3)
        );
        assert_eq!(chan.len(), 3);
        assert_eq!(chan.recv_exact(2), Some(vec![1, 2]));
        assert_eq!(chan.recv_exact(1), Some(vec![3]));
        assert!(chan.is_empty());
    }

    #[test]
    fn receiver_step_detects_eof_as_connection_closed() {
        let mut chan = DuplexRecvChan::new();
        assert_eq!(
            duplex_receiver_step(&mut chan, &[]),
            DuplexReceiverStep::Closed(BearerConnectionClosed)
        );
        assert!(chan.is_empty());
    }

    #[test]
    fn recv_exact_waits_for_enough_buffered_bytes() {
        let mut chan = DuplexRecvChan::new();
        chan.write_bytes_one_at_a_time(&[1, 2]);
        assert_eq!(chan.recv_exact(3), None);
        chan.write_bytes_one_at_a_time(&[3]);
        assert_eq!(chan.recv_exact(3), Some(vec![1, 2, 3]));
    }

    #[test]
    fn with_duplex_bearer_preserves_chunk_order_until_eof() {
        let mut chan = with_duplex_bearer(&[&[1, 2], &[3, 4]]).expect("no EOF");
        assert_eq!(chan.recv_exact(4), Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn with_duplex_bearer_returns_connection_closed_on_empty_chunk() {
        assert_eq!(
            with_duplex_bearer(&[&[1, 2], &[]]),
            Err(BearerConnectionClosed)
        );
    }
}
