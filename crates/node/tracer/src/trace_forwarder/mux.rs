//! Layer 3 of the cardano-tracer forwarder — `Network.Mux` SDU
//! framing.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side Rust port of the SDU
//! codec from upstream
//! `.reference-haskell-cardano-node/deps/ouroboros-network/network-mux/src/Network/Mux/Codec.hs`
//! (only the `encodeSDU` / `decodeSDU` pair; the surrounding
//! `Network.Mux.Types` `SDU` / `SDUHeader` records translate to the
//! Rust `Sdu` / `SduHeader` types). Yggdrasil keeps the upstream
//! field naming so the parity-comparison harness can pin byte
//! shapes against the Haskell encoder side-by-side. The remaining
//! Mux modules (`Network/Mux/Ingress.hs`, `Egress.hs`, `Bearer.hs`,
//! the typed-protocol driver, the Handshake mini-protocol) are
//! follow-ons gated on a live transport.
//!
//! This module currently ships the **SDU codec** — encode/decode of
//! the 8-byte SDU header. The full Mux state machine (ingress / egress
//! queues, per-mini-protocol scheduling, handshake driver) is **not**
//! implemented here yet; that lands once a binary actually consumes
//! the codec by speaking the protocol over an `AF_UNIX SOCK_STREAM`
//! bearer. Splitting the codec out lets us assert byte-equivalence
//! against the upstream wire format independently of the live
//! transport.
//!
//! ## Wire format
//!
//! The on-the-wire SDU is an 8-byte header followed by a variable-
//! length payload (≤ 65 535 bytes). All fields are big-endian.
//!
//! ```text
//! 0                   1                   2                   3
//! 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                        transmission time                      |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |d|    mini-protocol number     |             length            |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! Field semantics (per `ouroboros-network/network-mux/src/Network/Mux/Codec.hs`):
//!
//! - **bytes 0–3**: transmission timestamp (`Word32`). The unit is
//!   the `RemoteClockModel` tick, fixed at 1 µs per tick (1e-6 s).
//!   So an SDU written at time T (seconds since epoch / some
//!   reference) has timestamp `(T * 1_000_000) mod 2^32` — wraparound
//!   is implicit and explicit per the upstream `RemoteClockModel`.
//! - **bytes 4–5**: a 16-bit big-endian word with the direction bit
//!   in the high bit and the mini-protocol number in the low 15 bits.
//!   Per the **implementation** in `Network.Mux.Codec`:
//!   `InitiatorDir` is encoded as `n` (high bit **clear**) and
//!   `ResponderDir` as `n | 0x8000` (high bit **set**).
//!     - The block-comment diagram in `Codec.hs` reads the bit
//!       in the opposite direction (it labels `d = 1` as
//!       "initiator direction"). The Haskell implementation is the
//!       source of truth — the diagram is a documentation
//!       artifact. The Rust codec follows the implementation, so
//!       a real `cardano-tracer` decoder will accept Yggdrasil-
//!       emitted SDUs.
//! - **bytes 6–7**: payload length in bytes (`Word16`). A `0` here
//!   signals a malformed SDU per `decodeSDU` (`"short SDU"`).
//!
//! ## cardano-tracer mini-protocol numbers
//!
//! Per `cardano-tracer/src/Cardano/Tracer/Acceptors/{Client,Server}.hs`:
//!
//! - **1** — EKG metrics forwarding
//! - **2** — TraceObject forwarding (the one this crate emits)
//! - **3** — DataPoint forwarding
//!
//! The Handshake mini-protocol is **0** per upstream Network.Mux
//! convention.

/// Direction byte of an SDU. Encodes which side of the bearer
/// transmitted the SDU.
///
/// Per `Network.Mux.Codec.encodeSDU`:
///
/// - `Initiator` is the application that opened the bearer (typically
///   the node sending traces to the cardano-tracer collector). Its
///   high bit is **clear** on the wire (i.e. `n & 0x7fff`).
/// - `Responder` is the application that accepted the bearer (the
///   collector receiving traces). Its high bit is **set** on the
///   wire (i.e. `n | 0x8000`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MiniProtocolDir {
    /// The peer that opened the bearer. High direction bit ON.
    Initiator,
    /// The peer that accepted the bearer. High direction bit OFF.
    Responder,
}

/// Decoded SDU header (the 8-byte prefix on every SDU on the wire).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SduHeader {
    /// 32-bit big-endian transmission timestamp in
    /// `RemoteClockModel` ticks (1 µs per tick).
    pub timestamp: u32,
    /// Mini-protocol number, low 15 bits of the dir-and-num word.
    /// Range: 0..=0x7fff (the high bit is taken by the direction).
    pub mini_protocol_num: u16,
    /// Which direction (initiator vs responder) the SDU was sent in.
    pub direction: MiniProtocolDir,
    /// Length of the payload that follows this header, in bytes.
    /// Max representable: 65 535.
    pub length: u16,
}

/// Errors surfaced by `decode_sdu_header`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MuxError {
    /// The buffer was shorter than the 8 bytes required for an SDU
    /// header.
    ShortHeader { got: usize },
    /// Mini-protocol number is in the reserved 0x8000..=0xFFFF range
    /// (the high bit is reserved for direction). Decoded value is
    /// always 0..=0x7fff so this only fires on a user-constructed
    /// `SduHeader`; the decode path masks the high bit off
    /// unconditionally.
    ProtocolNumberOutOfRange { got: u16 },
}

impl core::fmt::Display for MuxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ShortHeader { got } => {
                write!(f, "SDU header is 8 bytes; got {got}")
            }
            Self::ProtocolNumberOutOfRange { got } => {
                write!(
                    f,
                    "mini-protocol number {got:#06x} is in the reserved 0x8000..=0xFFFF range"
                )
            }
        }
    }
}

impl std::error::Error for MuxError {}

/// Encode an SDU header to its 8-byte big-endian wire form.
///
/// Mirrors `Network.Mux.Codec.encodeSDU`. The output bytes match the
/// Haskell `runPut` byte-for-byte; a future `cardano-tracer` upgrade
/// that changes the field order would show up as a failing test in
/// `mux_tests`.
pub fn encode_sdu_header(header: &SduHeader) -> Result<[u8; 8], MuxError> {
    if header.mini_protocol_num & 0x8000 != 0 {
        return Err(MuxError::ProtocolNumberOutOfRange {
            got: header.mini_protocol_num,
        });
    }
    let dir_bit = match header.direction {
        MiniProtocolDir::Initiator => 0x0000_u16,
        MiniProtocolDir::Responder => 0x8000_u16,
    };
    let num_and_dir = (header.mini_protocol_num & 0x7fff) | dir_bit;
    let mut out = [0_u8; 8];
    out[0..4].copy_from_slice(&header.timestamp.to_be_bytes());
    out[4..6].copy_from_slice(&num_and_dir.to_be_bytes());
    out[6..8].copy_from_slice(&header.length.to_be_bytes());
    Ok(out)
}

/// Decode an 8-byte SDU header from the front of `buf`.
///
/// Mirrors `Network.Mux.Codec.decodeSDU`. The decoder consumes only
/// the first 8 bytes; callers slice off the payload separately
/// based on `header.length`.
pub fn decode_sdu_header(buf: &[u8]) -> Result<SduHeader, MuxError> {
    if buf.len() < 8 {
        return Err(MuxError::ShortHeader { got: buf.len() });
    }
    let timestamp = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let num_and_dir = u16::from_be_bytes([buf[4], buf[5]]);
    let length = u16::from_be_bytes([buf[6], buf[7]]);
    // Per `Network.Mux.Codec.decodeSDU`:
    //   getDir mid = if mid .&. 0x8000 == 0 then InitiatorDir else ResponderDir
    // The implementation is authoritative here — the diagram block-
    // comment at the top of `Codec.hs` labels `d = 1` as initiator
    // but the function above labels it as responder; interop with a
    // real cardano-tracer follows the function.
    let direction = if num_and_dir & 0x8000 == 0 {
        MiniProtocolDir::Initiator
    } else {
        MiniProtocolDir::Responder
    };
    let mini_protocol_num = num_and_dir & 0x7fff;
    Ok(SduHeader {
        timestamp,
        mini_protocol_num,
        direction,
        length,
    })
}

/// Mini-protocol number for the **TraceObject forwarding** sub-protocol
/// on the cardano-tracer bearer.
///
/// Per `cardano-tracer/src/Cardano/Tracer/Acceptors/{Client,Server}.hs`
/// the three live sub-protocols are:
///
/// | num | sub-protocol                       |
/// | --- | ---------------------------------- |
/// | 1   | EKG metrics forwarding             |
/// | 2   | TraceObject forwarding             |
/// | 3   | DataPoint forwarding               |
///
/// Yggdrasil only implements #2 today (the Layer 1 `TraceObject`
/// codec). #1 and #3 are out of scope for v1.0.
pub const TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM: u16 = 2;

/// Mini-protocol number reserved for the **Handshake** protocol on
/// every Mux bearer. Per `ouroboros-network/Network/Mux/Handshake`
/// convention this is **0**; the Handshake driver runs first on a
/// fresh connection and negotiates the per-bearer version data
/// (network magic, etc) before any TraceObject SDUs flow.
pub const HANDSHAKE_MINI_PROTOCOL_NUM: u16 = 0;

#[cfg(test)]
mod mux_tests {
    use super::*;

    /// Pin the SDU encoder against the Haskell `encodeSDU` byte
    /// layout for an Initiator-direction SDU on the TraceObject
    /// mini-protocol (num=2). A regression in either the timestamp
    /// field, the direction bit, or the length field surfaces here.
    #[test]
    fn encode_initiator_trace_object_sdu_header_byte_shape() {
        // Timestamp = 0x_DEAD_BEEF, num = 2 (TraceObject), Initiator
        // direction, payload length = 0x_0042 (66 bytes).
        //
        // Initiator → high bit CLEAR per the Haskell implementation
        // (`putNumAndMode (MiniProtocolNum n) InitiatorDir = n`),
        // so bytes 4–5 are `0x00 0x02`, not `0x80 0x02`.
        let h = SduHeader {
            timestamp: 0xDEAD_BEEF,
            mini_protocol_num: TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
            direction: MiniProtocolDir::Initiator,
            length: 0x0042,
        };
        let bytes = encode_sdu_header(&h).expect("encode");
        // Expected: 0xDE 0xAD 0xBE 0xEF  (timestamp BE)
        //           0x00 0x02            (direction=initiator high bit OFF, num=2)
        //           0x00 0x42            (length=66 BE)
        assert_eq!(
            bytes,
            [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x02, 0x00, 0x42],
            "Initiator/TraceObject/len=66 SDU header byte shape drifted"
        );
    }

    /// Responder direction: high bit sets. Pin the byte shape so a
    /// regression in the bit mask shows here.
    #[test]
    fn encode_responder_sdu_header_byte_shape() {
        let h = SduHeader {
            timestamp: 0x_0000_0001,
            mini_protocol_num: 1, // EKG forwarding
            direction: MiniProtocolDir::Responder,
            length: 0x_0010,
        };
        let bytes = encode_sdu_header(&h).expect("encode");
        // Responder → high bit SET, so bytes 4–5 = 0x80 0x01.
        assert_eq!(
            bytes,
            [0x00, 0x00, 0x00, 0x01, 0x80, 0x01, 0x00, 0x10],
            "Responder/EKG/len=16 SDU header byte shape drifted"
        );
    }

    /// Decode round-trip: every encodable header decodes back
    /// identically.
    #[test]
    fn encode_decode_sdu_header_round_trip() {
        for (timestamp, num, dir, length) in [
            (0_u32, 0_u16, MiniProtocolDir::Initiator, 0_u16),
            (1, 1, MiniProtocolDir::Responder, 1),
            (
                u32::MAX,
                0x7fff,
                MiniProtocolDir::Initiator,
                u16::MAX,
            ),
            (
                0x12_34_56_78,
                TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM,
                MiniProtocolDir::Initiator,
                512,
            ),
        ] {
            let original = SduHeader {
                timestamp,
                mini_protocol_num: num,
                direction: dir,
                length,
            };
            let bytes = encode_sdu_header(&original).expect("encode");
            let decoded = decode_sdu_header(&bytes).expect("decode");
            assert_eq!(decoded, original, "round-trip drift on {original:?}");
        }
    }

    /// Decoder rejects a buffer shorter than 8 bytes.
    #[test]
    fn decode_short_buffer_errors() {
        for n in 0..=7_usize {
            let buf = vec![0_u8; n];
            let result = decode_sdu_header(&buf);
            assert!(
                matches!(result, Err(MuxError::ShortHeader { got }) if got == n),
                "expected ShortHeader(got={n}); got {result:?}"
            );
        }
    }

    /// Encoder rejects a mini-protocol number with the high bit set
    /// (would alias with the direction bit on the wire).
    #[test]
    fn encode_rejects_out_of_range_protocol_number() {
        let bad = SduHeader {
            timestamp: 0,
            mini_protocol_num: 0x8000,
            direction: MiniProtocolDir::Initiator,
            length: 0,
        };
        assert!(matches!(
            encode_sdu_header(&bad),
            Err(MuxError::ProtocolNumberOutOfRange { got: 0x8000 })
        ));
    }

    /// `HANDSHAKE_MINI_PROTOCOL_NUM` is the upstream constant. Any
    /// drift here would silently break interop with a real
    /// `cardano-tracer` binary.
    #[test]
    fn handshake_protocol_num_is_zero() {
        assert_eq!(HANDSHAKE_MINI_PROTOCOL_NUM, 0);
    }

    /// `TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM` is the upstream
    /// constant.
    #[test]
    fn trace_object_protocol_num_is_two() {
        assert_eq!(TRACE_OBJECT_FORWARD_MINI_PROTOCOL_NUM, 2);
    }
}
