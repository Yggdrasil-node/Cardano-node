/// A network protocol version used during handshake negotiation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandshakeVersion(pub u16);

/// A minimal handshake request carrying network magic and version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandshakeRequest {
    pub network_magic: u32,
    pub version: HandshakeVersion,
}
