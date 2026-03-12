use crate::CryptoError;

/// A placeholder byte-backed VRF secret key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VrfSecretKey(pub [u8; 32]);

/// A placeholder byte-backed VRF verification key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VrfVerificationKey(pub [u8; 32]);

/// A placeholder byte-backed VRF proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VrfProof(pub Vec<u8>);

/// A placeholder VRF output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VrfOutput(pub [u8; 64]);

impl VrfSecretKey {
    /// Produces a placeholder VRF proof for a message.
    pub fn prove(&self, _message: &[u8]) -> Result<(VrfOutput, VrfProof), CryptoError> {
        Err(CryptoError::Unimplemented("VRF proof generation"))
    }
}

impl VrfVerificationKey {
    /// Verifies a placeholder VRF proof for a message.
    pub fn verify(
        &self,
        _message: &[u8],
        _proof: &VrfProof,
    ) -> Result<VrfOutput, CryptoError> {
        Err(CryptoError::Unimplemented("VRF verification"))
    }
}
