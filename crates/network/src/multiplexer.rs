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

    /// Canonical, ascending slice of every named mini-protocol number.
    ///
    /// Returned in strictly-ascending wire-ID order so a copy-paste reorder
    /// or a missing addition fails the drift-guard test in this module.
    /// Adding a new mini-protocol upstream MUST extend this list AND its
    /// matching `pub const`. Used by both the value-pin drift guard here
    /// and (transitively) by the per-side `N2N_PROTOCOLS` / `NTC_PROTOCOLS`
    /// content-pin tests in `peer.rs` / `ntc_peer.rs`.
    ///
    /// Reference: `network-mux/src/Network/Mux/Types.hs` —
    /// `MiniProtocolNum`; `Ouroboros.Network.NodeToNode` and
    /// `Ouroboros.Network.NodeToClient` for the per-side subsets.
    pub const fn all_named() -> &'static [Self] {
        &[
            Self::HANDSHAKE,
            Self::CHAIN_SYNC,
            Self::BLOCK_FETCH,
            Self::TX_SUBMISSION,
            Self::NTC_LOCAL_TX_SUBMISSION,
            Self::NTC_LOCAL_STATE_QUERY,
            Self::KEEP_ALIVE,
            Self::NTC_LOCAL_TX_MONITOR,
            Self::PEER_SHARING,
        ]
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin every named `MiniProtocolNum` constant against its canonical
    /// upstream wire ID. A typo in any single `pub const X: Self = Self(N)`
    /// would otherwise cause silent mux misrouting (e.g. peer's BlockFetch
    /// frames delivered to a TxSubmission handler) — the worst-case mux
    /// bug, since both sides could "succeed" at the SDU level while
    /// running entirely different conversations.
    ///
    /// References:
    ///   - `network-mux/src/Network/Mux/Types.hs` — `MiniProtocolNum`
    ///   - `Ouroboros.Network.NodeToNode.nodeToNodeProtocols` (NtN subset)
    ///   - `Ouroboros.Network.NodeToClient.nodeToClientProtocols` (NtC subset)
    #[test]
    fn mini_protocol_num_constants_match_upstream_wire_ids() {
        // NtN-side protocols (also includes the shared HANDSHAKE).
        assert_eq!(MiniProtocolNum::HANDSHAKE.0, 0, "Handshake wire ID");
        assert_eq!(MiniProtocolNum::CHAIN_SYNC.0, 2, "ChainSync wire ID");
        assert_eq!(MiniProtocolNum::BLOCK_FETCH.0, 3, "BlockFetch wire ID");
        assert_eq!(MiniProtocolNum::TX_SUBMISSION.0, 4, "TxSubmission2 wire ID");
        assert_eq!(MiniProtocolNum::KEEP_ALIVE.0, 8, "KeepAlive wire ID");
        assert_eq!(MiniProtocolNum::PEER_SHARING.0, 10, "PeerSharing wire ID");

        // NtC-side protocols.
        assert_eq!(
            MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION.0,
            5,
            "NtC LocalTxSubmission wire ID",
        );
        assert_eq!(
            MiniProtocolNum::NTC_LOCAL_STATE_QUERY.0,
            7,
            "NtC LocalStateQuery wire ID",
        );
        assert_eq!(
            MiniProtocolNum::NTC_LOCAL_TX_MONITOR.0,
            9,
            "NtC LocalTxMonitor wire ID",
        );
    }

    #[test]
    fn mini_protocol_num_all_named_is_strictly_ascending_and_complete() {
        let all = MiniProtocolNum::all_named();
        assert_eq!(
            all.len(),
            9,
            "MiniProtocolNum::all_named() must cover every named constant \
             (6 NtN + 3 NtC; HANDSHAKE is shared and counted once)",
        );

        // Strictly-ascending invariant: each entry's wire ID is greater
        // than the previous entry's. Catches a copy-paste reorder in
        // `all_named()` that happens to leave the *set* unchanged but
        // breaks any downstream code relying on ordered iteration.
        for window in all.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "all_named() must be strictly ascending: {} >= {}",
                window[0].0,
                window[1].0,
            );
        }

        // Every named `pub const` must appear in `all_named()` exactly
        // once. Pinning each entry by-value here turns a missing addition
        // (someone defines a new constant but forgets to extend the
        // slice) into a CI failure naming the offending position.
        let expected = [
            MiniProtocolNum::HANDSHAKE,
            MiniProtocolNum::CHAIN_SYNC,
            MiniProtocolNum::BLOCK_FETCH,
            MiniProtocolNum::TX_SUBMISSION,
            MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION,
            MiniProtocolNum::NTC_LOCAL_STATE_QUERY,
            MiniProtocolNum::KEEP_ALIVE,
            MiniProtocolNum::NTC_LOCAL_TX_MONITOR,
            MiniProtocolNum::PEER_SHARING,
        ];
        assert_eq!(all, &expected[..]);
    }
}
