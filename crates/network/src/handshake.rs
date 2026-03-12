#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandshakeVersion(pub u16);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandshakeRequest {
    pub network_magic: u32,
    pub version: HandshakeVersion,
}
