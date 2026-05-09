//! Handshake version-number type and per-version negotiation data.
//!
//! ## Naming parity
//!
//! **Strict mirror:** Ouroboros/Network/Protocol/Handshake/Version.hs.
//! Filename flattens the upstream directory; the file carries the
//! version-number `pub const` table plus the `NodeToNodeVersionData`
//! payload exchanged alongside each version proposal.

/// A network protocol version number used during handshake negotiation.
///
/// Node-to-node versions 14 and 15 are currently defined.
///
/// Reference: `handshake-node-to-node-v14.cddl` — `versionNumber_v14 = 14 / 15`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HandshakeVersion(pub u16);

impl HandshakeVersion {
    /// Node-to-node protocol version 13 (Conway / PeerSharing).
    pub const V13: Self = Self(13);
    /// Node-to-node protocol version 14.
    pub const V14: Self = Self(14);
    /// Node-to-node protocol version 15.
    pub const V15: Self = Self(15);
}

// ---------------------------------------------------------------------------
// Version data negotiated alongside the version number
// ---------------------------------------------------------------------------

/// Per-version parameters exchanged during the node-to-node handshake.
///
/// Reference: `node-to-node-version-data-v14.cddl` —
/// `[networkMagic, initiatorOnlyDiffusionMode, peerSharing, query]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeToNodeVersionData {
    /// Network discriminator (e.g. `764824073` for mainnet).
    pub network_magic: u32,
    /// When `true` the initiator will not act as a responder on this
    /// connection.
    pub initiator_only_diffusion_mode: bool,
    /// Peer-sharing willingness indicator: `0` = disabled, `1` = enabled.
    pub peer_sharing: u8,
    /// When `true` the handshake is a version query only; the connection will
    /// be closed after the server replies.
    pub query: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntn_handshake_version_constants_are_sequential() {
        // Mirror of slice-87's NtC drift guard for the NtN side: pin
        // that `V13 / V14 / V15` map to literal u16 values `13 / 14 / 15`.
        // A copy-paste typo in ONE constant (e.g. `V14: Self(15)`) would
        // silently misnegotiate client connections onto the wrong NtN
        // protocol semantics — catastrophic for mux-mini-protocol
        // behaviour while the handshake itself succeeds.
        assert_eq!(HandshakeVersion::V13.0, 13);
        assert_eq!(HandshakeVersion::V14.0, 14);
        assert_eq!(HandshakeVersion::V15.0, 15);
    }
}
