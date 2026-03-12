use crate::{CryptoError, Signature, SigningKey, VerificationKey};
use std::fmt;
use subtle::ConstantTimeEq;

/// Serialized size of the single-period KES signing key.
pub const KES_SIGNING_KEY_SIZE: usize = 32;
/// Serialized size of the single-period KES verification key.
pub const KES_VERIFICATION_KEY_SIZE: usize = 32;
/// Serialized size of the single-period KES signature.
pub const KES_SIGNATURE_SIZE: usize = 64;
/// Serialized size of the compact single-period KES signature.
pub const COMPACT_KES_SIGNATURE_SIZE: usize = KES_SIGNATURE_SIZE + KES_VERIFICATION_KEY_SIZE;
/// Serialized size of the multi-period SimpleKES signature.
pub const SIMPLE_KES_SIGNATURE_SIZE: usize = 4 + KES_SIGNATURE_SIZE;
/// Serialized size of the compact multi-period SimpleKES signature.
pub const SIMPLE_COMPACT_KES_SIGNATURE_SIZE: usize = 4 + COMPACT_KES_SIGNATURE_SIZE;
/// Total supported periods for the current single-period KES baseline.
pub const KES_TOTAL_PERIODS: u32 = 1;

/// A KES evolution period.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KesPeriod(pub u32);

/// A byte-backed single-period KES signing key.
///
/// This is currently aligned with upstream `SingleKES Ed25519DSIGN` behavior:
/// the seed serialisation matches Ed25519 and only period `0` is valid.
#[derive(Clone)]
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

/// A byte-backed multi-period signing key for a `SimpleKES`-style baseline.
///
/// The key stores one 32-byte seed per period and signs by selecting the key
/// corresponding to the requested period index.
#[derive(Clone, Eq, PartialEq)]
pub struct SimpleKesSigningKey(pub Vec<KesSigningKey>);

/// A byte-backed multi-period verification key for a `SimpleKES`-style baseline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimpleKesVerificationKey(pub Vec<KesVerificationKey>);

/// A byte-backed multi-period signature for a `SimpleKES`-style baseline.
///
/// The serialized form is `period (u32 big-endian) || signature`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SimpleKesSignature(pub [u8; SIMPLE_KES_SIGNATURE_SIZE]);

/// A byte-backed compact multi-period signature for a `SimpleKES`-style baseline.
///
/// The serialized form is `period (u32 big-endian) || signature || verification key`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SimpleCompactKesSignature(pub [u8; SIMPLE_COMPACT_KES_SIGNATURE_SIZE]);

impl fmt::Debug for SimpleKesSigningKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SimpleKesSigningKey")
            .field("total_periods", &self.total_periods())
            .field("key_material", &"[REDACTED]")
            .finish()
    }
}

impl KesPeriod {
    /// Advances the period by one slot, returning an error on overflow.
    pub fn next(self) -> Result<Self, CryptoError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(CryptoError::KesPeriodOverflow)
    }
}

impl PartialEq for KesSigningKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for KesSigningKey {}

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

impl SimpleKesSigningKey {
    /// Constructs a multi-period signing key from one seed per period.
    pub fn from_seeds(seeds: Vec<[u8; KES_SIGNING_KEY_SIZE]>) -> Result<Self, CryptoError> {
        if seeds.is_empty() {
            return Err(CryptoError::InvalidKesDepth(0));
        }

        Ok(Self(
            seeds
                .into_iter()
                .map(KesSigningKey::from_bytes)
                .collect(),
        ))
    }

    /// Constructs a multi-period signing key from concatenated raw seed bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.is_empty() {
            return Err(CryptoError::InvalidKesDepth(0));
        }
        if bytes.len() % KES_SIGNING_KEY_SIZE != 0 {
            return Err(CryptoError::InvalidKesKeyMaterialLength(bytes.len()));
        }

        let seeds = bytes
            .chunks_exact(KES_SIGNING_KEY_SIZE)
            .map(|chunk| {
                chunk
                    .try_into()
                    .expect("KES signing key chunks should match fixed seed length")
            })
            .collect();
        Self::from_seeds(seeds)
    }

    /// Returns concatenated raw seed bytes in period order.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0
            .iter()
            .flat_map(|key| key.to_bytes())
            .collect()
    }

    /// Returns the number of supported periods.
    pub fn total_periods(&self) -> u32 {
        self.0
            .len()
            .try_into()
            .expect("SimpleKES depth should fit into u32")
    }

    /// Derives period-indexed verification keys.
    pub fn verification_key(&self) -> Result<SimpleKesVerificationKey, CryptoError> {
        Ok(SimpleKesVerificationKey(
            self.0
                .iter()
                .map(KesSigningKey::verification_key)
                .collect::<Result<Vec<_>, _>>()?,
        ))
    }

    /// Signs a message with the period-specific key.
    pub fn sign(&self, period: KesPeriod, message: &[u8]) -> Result<KesSignature, CryptoError> {
        let key = self
            .0
            .get(period.0 as usize)
            .ok_or(CryptoError::InvalidKesPeriod(period.0))?;
        key.sign(KesPeriod(0), message)
    }

    /// Signs a message and returns a typed multi-period signature that embeds the period.
    pub fn sign_indexed(
        &self,
        period: KesPeriod,
        message: &[u8],
    ) -> Result<SimpleKesSignature, CryptoError> {
        let signature = self.sign(period, message)?;
        let mut bytes = [0_u8; SIMPLE_KES_SIGNATURE_SIZE];
        bytes[..4].copy_from_slice(&period.0.to_be_bytes());
        bytes[4..].copy_from_slice(&signature.to_bytes());
        Ok(SimpleKesSignature(bytes))
    }

    /// Signs a message and returns a compact typed multi-period signature.
    pub fn sign_indexed_compact(
        &self,
        period: KesPeriod,
        message: &[u8],
    ) -> Result<SimpleCompactKesSignature, CryptoError> {
        let key = self
            .0
            .get(period.0 as usize)
            .ok_or(CryptoError::InvalidKesPeriod(period.0))?;
        let signature = key.sign_compact(KesPeriod(0), message)?;

        let mut bytes = [0_u8; SIMPLE_COMPACT_KES_SIGNATURE_SIZE];
        bytes[..4].copy_from_slice(&period.0.to_be_bytes());
        bytes[4..].copy_from_slice(&signature.to_bytes());
        Ok(SimpleCompactKesSignature(bytes))
    }
}

impl SimpleKesVerificationKey {
    /// Constructs a multi-period verification key from concatenated raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.is_empty() {
            return Err(CryptoError::InvalidKesDepth(0));
        }
        if bytes.len() % KES_VERIFICATION_KEY_SIZE != 0 {
            return Err(CryptoError::InvalidKesKeyMaterialLength(bytes.len()));
        }

        let keys = bytes
            .chunks_exact(KES_VERIFICATION_KEY_SIZE)
            .map(|chunk| {
                KesVerificationKey::from_bytes(
                    chunk
                        .try_into()
                        .expect("KES verification key chunks should match fixed key length"),
                )
            })
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return Err(CryptoError::InvalidKesDepth(0));
        }

        Ok(Self(keys))
    }

    /// Returns concatenated raw verification key bytes in period order.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0
            .iter()
            .flat_map(|key| key.to_bytes())
            .collect()
    }

    /// Returns the number of supported periods.
    pub fn total_periods(&self) -> u32 {
        self.0
            .len()
            .try_into()
            .expect("SimpleKES depth should fit into u32")
    }

    /// Verifies a message with the period-specific key.
    pub fn verify(
        &self,
        period: KesPeriod,
        message: &[u8],
        signature: &KesSignature,
    ) -> Result<(), CryptoError> {
        let key = self
            .0
            .get(period.0 as usize)
            .ok_or(CryptoError::InvalidKesPeriod(period.0))?;
        key.verify(KesPeriod(0), message, signature)
    }

    /// Verifies a typed multi-period signature by using its embedded period.
    pub fn verify_indexed(
        &self,
        message: &[u8],
        signature: &SimpleKesSignature,
    ) -> Result<(), CryptoError> {
        self.verify(signature.period(), message, &signature.signature())
    }

    /// Verifies a compact typed multi-period signature by using its embedded period.
    pub fn verify_indexed_compact(
        &self,
        message: &[u8],
        signature: &SimpleCompactKesSignature,
    ) -> Result<(), CryptoError> {
        let period = signature.period();
        let key = self
            .0
            .get(period.0 as usize)
            .ok_or(CryptoError::InvalidKesPeriod(period.0))?;

        if signature.verification_key() != *key {
            return Err(CryptoError::KesVerificationKeyMismatch);
        }

        key.verify(KesPeriod(0), message, &signature.signature())
    }
}

impl SimpleKesSignature {
    /// Constructs a multi-period signature from its 68-byte serialized form.
    pub fn from_bytes(bytes: [u8; SIMPLE_KES_SIGNATURE_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 68-byte serialized multi-period signature.
    pub fn to_bytes(&self) -> [u8; SIMPLE_KES_SIGNATURE_SIZE] {
        self.0
    }

    /// Returns the embedded signing period.
    pub fn period(&self) -> KesPeriod {
        KesPeriod(u32::from_be_bytes(
            self.0[..4]
                .try_into()
                .expect("SimpleKES signature prefix should match fixed period length"),
        ))
    }

    /// Returns the embedded Ed25519-like signature bytes.
    pub fn signature(&self) -> KesSignature {
        KesSignature(
            self.0[4..]
                .try_into()
                .expect("SimpleKES signature suffix should match fixed signature length"),
        )
    }
}

impl SimpleCompactKesSignature {
    /// Constructs a compact multi-period signature from its 100-byte serialized form.
    pub fn from_bytes(bytes: [u8; SIMPLE_COMPACT_KES_SIGNATURE_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 100-byte serialized compact multi-period signature.
    pub fn to_bytes(&self) -> [u8; SIMPLE_COMPACT_KES_SIGNATURE_SIZE] {
        self.0
    }

    /// Returns the embedded signing period.
    pub fn period(&self) -> KesPeriod {
        KesPeriod(u32::from_be_bytes(
            self.0[..4]
                .try_into()
                .expect("SimpleCompactKES signature prefix should match fixed period length"),
        ))
    }

    /// Returns the embedded Ed25519-like signature bytes.
    pub fn signature(&self) -> KesSignature {
        KesSignature(
            self.0[4..(4 + KES_SIGNATURE_SIZE)]
                .try_into()
                .expect("SimpleCompactKES signature middle should match fixed signature length"),
        )
    }

    /// Returns the embedded verification key bytes.
    pub fn verification_key(&self) -> KesVerificationKey {
        KesVerificationKey(
            self.0[(4 + KES_SIGNATURE_SIZE)..]
                .try_into()
                .expect("SimpleCompactKES signature suffix should match fixed key length"),
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
