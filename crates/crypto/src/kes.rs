use crate::CryptoError;

/// A KES evolution period.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KesPeriod(pub u32);

/// A placeholder byte-backed KES signature wrapper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KesSignature(pub Vec<u8>);

/// Evolves a KES period placeholder.
pub fn evolve_period(_period: KesPeriod) -> Result<KesPeriod, CryptoError> {
    Err(CryptoError::Unimplemented("KES period evolution"))
}
