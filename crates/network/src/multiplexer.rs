/// Size of a serialized SDU header in bytes.
///
/// The Ouroboros multiplexer frames every payload with an 8-byte header:
/// `[timestamp:u32be, (direction_bit | mini_protocol_num):u16be, payload_length:u16be]`.
///
/// Reference: `network-mux/src/Network/Mux/Codec.hs` — `encodeSDU` / `decodeSDU`.
pub const SDU_HEADER_SIZE: usize = 8;

/// Direction bit mask applied to the protocol-number field of an SDU header.
///
/// Bit 15 of the 16-bit field encodes the direction:
/// - `0` → `Initiator` (client → server)
/// - `1` → `Responder` (server → client)
///
/// Reference: `network-mux/src/Network/Mux/Types.hs` — `MiniProtocolDir`.
const DIRECTION_BIT: u16 = 0x8000;

// ---------------------------------------------------------------------------
// Mini-protocol numbering
// ---------------------------------------------------------------------------

/// A multiplexed mini-protocol identifier.
///
/// The lower 15 bits of the SDU header's protocol-number field carry this
/// value; bit 15 is reserved for the direction flag.
///
/// Reference: `network-mux/src/Network/Mux/Types.hs` — `MiniProtocolNum`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MiniProtocolNum(pub u16);

impl MiniProtocolNum {
    /// Handshake — reserved protocol number 0 (shared by NtN and NtC).
    pub const HANDSHAKE: Self = Self(0);
    /// ChainSync — protocol number 2 (node-to-node).
    pub const CHAIN_SYNC: Self = Self(2);
    /// BlockFetch — protocol number 3.
    pub const BLOCK_FETCH: Self = Self(3);
    /// TxSubmission2 — protocol number 4.
    pub const TX_SUBMISSION: Self = Self(4);
    /// KeepAlive — protocol number 8.
    pub const KEEP_ALIVE: Self = Self(8);
    /// PeerSharing — protocol number 10.
    pub const PEER_SHARING: Self = Self(10);

    // -- Node-to-Client (NtC) protocol numbers ----------------------------
    // Reference: `Ouroboros.Network.NodeToClient` — `nodeToClientProtocols`.

    /// NtC LocalTxSubmission — protocol number 5.
    pub const NTC_LOCAL_TX_SUBMISSION: Self = Self(5);
    /// NtC LocalStateQuery — protocol number 7.
    pub const NTC_LOCAL_STATE_QUERY: Self = Self(7);
    /// NtC LocalTxMonitor — protocol number 9.
    pub const NTC_LOCAL_TX_MONITOR: Self = Self(9);
}

/// Direction of a multiplexed mini-protocol conversation.
///
/// In the SDU header the direction is encoded as bit 15 of the protocol-number
/// field: `0` for `Initiator`, `1` for `Responder`.
///
/// Reference: `network-mux/src/Network/Mux/Types.hs` — `MiniProtocolDir`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MiniProtocolDir {
    /// Client-to-server direction (bit 15 = 0).
    Initiator,
    /// Server-to-client direction (bit 15 = 1).
    Responder,
}

// ---------------------------------------------------------------------------
// SDU header
// ---------------------------------------------------------------------------

/// Parsed representation of a multiplexer Segment Data Unit header.
///
/// Layout (8 bytes, all fields big-endian):
///
/// | Offset | Size | Field                                |
/// |--------|------|--------------------------------------|
/// | 0      | 4    | `timestamp` (monotonic microseconds) |
/// | 4      | 2    | direction bit ∣ `protocol_num`       |
/// | 6      | 2    | `payload_length`                     |
///
/// Reference: `network-mux/src/Network/Mux/Codec.hs`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SduHeader {
    /// Monotonic timestamp in microseconds (informational, not validated).
    pub timestamp: u32,
    /// Which mini-protocol this segment belongs to.
    pub protocol_num: MiniProtocolNum,
    /// Conversation direction.
    pub direction: MiniProtocolDir,
    /// Length of the payload that follows this header.
    pub payload_length: u16,
}

/// Errors that can occur when decoding an SDU header.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SduDecodeError {
    /// The buffer is shorter than [`SDU_HEADER_SIZE`] bytes.
    #[error("SDU header requires {SDU_HEADER_SIZE} bytes, got {0}")]
    BufferTooShort(usize),
}

impl SduHeader {
    /// Encode the header into an 8-byte big-endian buffer.
    pub fn encode(&self) -> [u8; SDU_HEADER_SIZE] {
        let mut buf = [0u8; SDU_HEADER_SIZE];
        buf[0..4].copy_from_slice(&self.timestamp.to_be_bytes());

        let dir_bit = match self.direction {
            MiniProtocolDir::Initiator => 0u16,
            MiniProtocolDir::Responder => DIRECTION_BIT,
        };
        let num_and_dir = dir_bit | (self.protocol_num.0 & !DIRECTION_BIT);
        buf[4..6].copy_from_slice(&num_and_dir.to_be_bytes());

        buf[6..8].copy_from_slice(&self.payload_length.to_be_bytes());
        buf
    }

    /// Decode an SDU header from the first 8 bytes of `buf`.
    ///
    /// Returns [`SduDecodeError::BufferTooShort`] when `buf.len() < 8`.
    pub fn decode(buf: &[u8]) -> Result<Self, SduDecodeError> {
        if buf.len() < SDU_HEADER_SIZE {
            return Err(SduDecodeError::BufferTooShort(buf.len()));
        }

        let timestamp = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let num_dir = u16::from_be_bytes([buf[4], buf[5]]);
        let direction = if num_dir & DIRECTION_BIT != 0 {
            MiniProtocolDir::Responder
        } else {
            MiniProtocolDir::Initiator
        };
        let protocol_num = MiniProtocolNum(num_dir & !DIRECTION_BIT);
        let payload_length = u16::from_be_bytes([buf[6], buf[7]]);

        Ok(Self {
            timestamp,
            protocol_num,
            direction,
            payload_length,
        })
    }
}

/// A multiplexed mini-protocol channel identifier.
///
/// Legacy alias kept for backward compatibility; prefer [`MiniProtocolNum`]
/// for new code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MuxChannel(pub u16);
