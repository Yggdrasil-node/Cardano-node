use crate::CryptoError;

/// A KES evolution period.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KesPeriod(pub u32);

/// A placeholder byte-backed KES signature wrapper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KesSignature(pub Vec<u8>);

impl KesPeriod {
    /// Advances the period by one slot, returning an error on overflow.
    pub fn next(self) -> Result<Self, CryptoError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(CryptoError::KesPeriodOverflow)
    }
}

/// Evolves a KES period placeholder.
pub fn evolve_period(period: KesPeriod) -> Result<KesPeriod, CryptoError> {
    period.next()
}
