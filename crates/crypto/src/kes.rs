use crate::{CryptoError, Signature, SigningKey, VerificationKey};

/// Serialized size of the single-period KES signing key.
pub const KES_SIGNING_KEY_SIZE: usize = 32;
/// Serialized size of the single-period KES verification key.
pub const KES_VERIFICATION_KEY_SIZE: usize = 32;
/// Serialized size of the single-period KES signature.
pub const KES_SIGNATURE_SIZE: usize = 64;
/// Serialized size of the compact single-period KES signature.
pub const COMPACT_KES_SIGNATURE_SIZE: usize = KES_SIGNATURE_SIZE + KES_VERIFICATION_KEY_SIZE;
/// Total supported periods for the current single-period KES baseline.
pub const KES_TOTAL_PERIODS: u32 = 1;

/// A KES evolution period.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KesPeriod(pub u32);

/// A byte-backed single-period KES signing key.
///
/// This is currently aligned with upstream `SingleKES Ed25519DSIGN` behavior:
/// the seed serialisation matches Ed25519 and only period `0` is valid.
#[derive(Clone, Eq, PartialEq)]
pub struct KesSigningKey(pub [u8; KES_SIGNING_KEY_SIZE]);

/// A byte-backed single-period KES verification key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KesVerificationKey(pub [u8; KES_VERIFICATION_KEY_SIZE]);

/// A byte-backed single-period KES signature wrapper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KesSignature(pub [u8; KES_SIGNATURE_SIZE]);

/// A byte-backed compact single-period KES signature.
///
/// This follows the upstream `CompactSingleKES` shape by storing the Ed25519
/// signature followed by the verification key needed to validate it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompactKesSignature(pub [u8; COMPACT_KES_SIGNATURE_SIZE]);

impl KesPeriod {
    /// Advances the period by one slot, returning an error on overflow.
    pub fn next(self) -> Result<Self, CryptoError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(CryptoError::KesPeriodOverflow)
    }
}

impl KesSigningKey {
    /// Constructs a KES signing key from its 32-byte seed encoding.
    pub fn from_bytes(bytes: [u8; KES_SIGNING_KEY_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 32-byte seed backing this KES signing key.
    pub fn to_bytes(&self) -> [u8; KES_SIGNING_KEY_SIZE] {
        self.0
    }

    /// Derives the verification key for this single-period KES key.
    pub fn verification_key(&self) -> Result<KesVerificationKey, CryptoError> {
        Ok(KesVerificationKey(
            SigningKey::from_bytes(self.0).verification_key()?.to_bytes(),
        ))
    }

    /// Signs a message for period `0`.
    pub fn sign(&self, period: KesPeriod, message: &[u8]) -> Result<KesSignature, CryptoError> {
        ensure_supported_period(period)?;
        Ok(KesSignature(SigningKey::from_bytes(self.0).sign(message)?.to_bytes()))
    }

    /// Signs a message using the compact single-period KES encoding.
    pub fn sign_compact(
        &self,
        period: KesPeriod,
        message: &[u8],
    ) -> Result<CompactKesSignature, CryptoError> {
        ensure_supported_period(period)?;
        let verification_key = self.verification_key()?;
        let signature = self.sign(period, message)?;
        let mut bytes = [0_u8; COMPACT_KES_SIGNATURE_SIZE];
        bytes[..KES_SIGNATURE_SIZE].copy_from_slice(&signature.to_bytes());
        bytes[KES_SIGNATURE_SIZE..].copy_from_slice(&verification_key.to_bytes());
        Ok(CompactKesSignature(bytes))
    }
}

impl KesVerificationKey {
    /// Constructs a KES verification key from its 32-byte encoding.
    pub fn from_bytes(bytes: [u8; KES_VERIFICATION_KEY_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 32-byte encoded KES verification key.
    pub fn to_bytes(&self) -> [u8; KES_VERIFICATION_KEY_SIZE] {
        self.0
    }

    /// Verifies a message and signature pair for period `0`.
    pub fn verify(
        &self,
        period: KesPeriod,
        message: &[u8],
        signature: &KesSignature,
    ) -> Result<(), CryptoError> {
        ensure_supported_period(period)?;
        VerificationKey::from_bytes(self.0).verify(message, &Signature::from_bytes(signature.0))
    }

    /// Verifies a compact single-period KES signature.
    pub fn verify_compact(
        &self,
        period: KesPeriod,
        message: &[u8],
        signature: &CompactKesSignature,
    ) -> Result<(), CryptoError> {
        ensure_supported_period(period)?;

        if signature.verification_key() != *self {
            return Err(CryptoError::KesVerificationKeyMismatch);
        }

        self.verify(period, message, &signature.signature())
    }
}

impl KesSignature {
    /// Constructs a KES signature from its 64-byte encoding.
    pub fn from_bytes(bytes: [u8; KES_SIGNATURE_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 64-byte encoded KES signature.
    pub fn to_bytes(&self) -> [u8; KES_SIGNATURE_SIZE] {
        self.0
    }
}

impl CompactKesSignature {
    /// Constructs a compact KES signature from its 96-byte encoding.
    pub fn from_bytes(bytes: [u8; COMPACT_KES_SIGNATURE_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 96-byte encoded compact KES signature.
    pub fn to_bytes(&self) -> [u8; COMPACT_KES_SIGNATURE_SIZE] {
        self.0
    }

    /// Extracts the embedded Ed25519-like signature bytes.
    pub fn signature(&self) -> KesSignature {
        KesSignature(
            self.0[..KES_SIGNATURE_SIZE]
                .try_into()
                .expect("compact KES signature prefix should match the fixed signature length"),
        )
    }

    /// Extracts the verification key embedded in the compact signature.
    pub fn verification_key(&self) -> KesVerificationKey {
        KesVerificationKey(
            self.0[KES_SIGNATURE_SIZE..]
                .try_into()
                .expect("compact KES signature suffix should match the fixed key length"),
        )
    }
}

/// Evolves a KES period placeholder.
pub fn evolve_period(period: KesPeriod) -> Result<KesPeriod, CryptoError> {
    period.next()
}

fn ensure_supported_period(period: KesPeriod) -> Result<(), CryptoError> {
    if period.0 < KES_TOTAL_PERIODS {
        Ok(())
    } else {
        Err(CryptoError::InvalidKesPeriod(period.0))
    }
}
