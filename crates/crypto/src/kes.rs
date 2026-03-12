use crate::CryptoError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KesPeriod(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KesSignature(pub Vec<u8>);

pub fn evolve_period(_period: KesPeriod) -> Result<KesPeriod, CryptoError> {
    Err(CryptoError::Unimplemented("KES period evolution"))
}
