use crate::CryptoError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VrfSecretKey(pub [u8; 32]);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VrfVerificationKey(pub [u8; 32]);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VrfProof(pub Vec<u8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VrfOutput(pub [u8; 64]);

impl VrfSecretKey {
    pub fn prove(&self, _message: &[u8]) -> Result<(VrfOutput, VrfProof), CryptoError> {
        Err(CryptoError::Unimplemented("VRF proof generation"))
    }
}

impl VrfVerificationKey {
    pub fn verify(
        &self,
        _message: &[u8],
        _proof: &VrfProof,
    ) -> Result<VrfOutput, CryptoError> {
        Err(CryptoError::Unimplemented("VRF verification"))
    }
}
