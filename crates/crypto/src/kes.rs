use crate::{CryptoError, Signature, SigningKey, VerificationKey};
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

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
///
/// Secret key material is zeroized on drop.
#[derive(Clone)]
pub struct KesSigningKey(pub [u8; KES_SIGNING_KEY_SIZE]);

impl Zeroize for KesSigningKey {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

impl Drop for KesSigningKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

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
///
/// Secret key material is zeroized on drop.
#[derive(Clone, Eq, PartialEq)]
pub struct SimpleKesSigningKey(pub Vec<KesSigningKey>);

impl Zeroize for SimpleKesSigningKey {
    fn zeroize(&mut self) {
        for key in &mut self.0 {
            key.zeroize();
        }
    }
}

impl Drop for SimpleKesSigningKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

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
            SigningKey::from_bytes(self.0)
                .verification_key()?
                .to_bytes(),
        ))
    }

    /// Signs a message for period `0`.
    pub fn sign(&self, period: KesPeriod, message: &[u8]) -> Result<KesSignature, CryptoError> {
        ensure_supported_period(period)?;
        Ok(KesSignature(
            SigningKey::from_bytes(self.0).sign(message)?.to_bytes(),
        ))
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
            seeds.into_iter().map(KesSigningKey::from_bytes).collect(),
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
        self.0.iter().flat_map(|key| key.to_bytes()).collect()
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
        self.0.iter().flat_map(|key| key.to_bytes()).collect()
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
        KesPeriod(u32::from_be_bytes(self.0[..4].try_into().expect(
            "SimpleKES signature prefix should match fixed period length",
        )))
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
        KesPeriod(u32::from_be_bytes(self.0[..4].try_into().expect(
            "SimpleCompactKES signature prefix should match fixed period length",
        )))
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::test_vectors::simple_kes_two_period_test_vectors;

    // ── KesPeriod ────────────────────────────────────────────────────────

    #[test]
    fn kes_period_next_increments() {
        let p = KesPeriod(0).next().unwrap();
        assert_eq!(p, KesPeriod(1));
    }

    #[test]
    fn kes_period_next_overflow() {
        let result = KesPeriod(u32::MAX).next();
        assert_eq!(result, Err(CryptoError::KesPeriodOverflow));
    }

    #[test]
    fn kes_period_equality() {
        assert_eq!(KesPeriod(5), KesPeriod(5));
        assert_ne!(KesPeriod(0), KesPeriod(1));
    }

    #[test]
    fn evolve_period_wraps_next() {
        assert_eq!(evolve_period(KesPeriod(0)).unwrap(), KesPeriod(1));
        assert_eq!(
            evolve_period(KesPeriod(u32::MAX)),
            Err(CryptoError::KesPeriodOverflow)
        );
    }

    // ── KesSigningKey ────────────────────────────────────────────────────

    #[test]
    fn kes_signing_key_from_bytes_roundtrip() {
        let seed = [0x42; KES_SIGNING_KEY_SIZE];
        let sk = KesSigningKey::from_bytes(seed);
        assert_eq!(sk.to_bytes(), seed);
    }

    #[test]
    fn kes_signing_key_verification_key_deterministic() {
        let sk = KesSigningKey::from_bytes([7u8; 32]);
        let vk1 = sk.verification_key().unwrap();
        let vk2 = sk.verification_key().unwrap();
        assert_eq!(vk1, vk2);
    }

    #[test]
    fn kes_signing_key_constant_time_eq() {
        let sk1 = KesSigningKey::from_bytes([1u8; 32]);
        let sk2 = KesSigningKey::from_bytes([1u8; 32]);
        let sk3 = KesSigningKey::from_bytes([2u8; 32]);
        assert!(sk1 == sk2, "same bytes should be equal");
        assert!(sk1 != sk3, "different bytes should not be equal");
    }

    // ── Single-period sign / verify ──────────────────────────────────────

    #[test]
    fn kes_sign_verify_period_zero() {
        let sk = KesSigningKey::from_bytes([0x0A; 32]);
        let vk = sk.verification_key().unwrap();
        let msg = b"test message";
        let sig = sk.sign(KesPeriod(0), msg).unwrap();
        vk.verify(KesPeriod(0), msg, &sig)
            .expect("valid single-period KES signature should verify");
    }

    #[test]
    fn kes_sign_rejects_period_one() {
        let sk = KesSigningKey::from_bytes([0x0B; 32]);
        let result = sk.sign(KesPeriod(1), b"msg");
        assert_eq!(result, Err(CryptoError::InvalidKesPeriod(1)));
    }

    #[test]
    fn kes_verify_rejects_period_above_zero() {
        let sk = KesSigningKey::from_bytes([0x0C; 32]);
        let vk = sk.verification_key().unwrap();
        let sig = sk.sign(KesPeriod(0), b"msg").unwrap();
        assert_eq!(
            vk.verify(KesPeriod(1), b"msg", &sig),
            Err(CryptoError::InvalidKesPeriod(1))
        );
    }

    #[test]
    fn kes_sign_wrong_message_fails() {
        let sk = KesSigningKey::from_bytes([0x0D; 32]);
        let vk = sk.verification_key().unwrap();
        let sig = sk.sign(KesPeriod(0), b"good").unwrap();
        assert!(vk.verify(KesPeriod(0), b"bad", &sig).is_err());
    }

    #[test]
    fn kes_sign_wrong_key_fails() {
        let sk1 = KesSigningKey::from_bytes([1u8; 32]);
        let vk2 = KesSigningKey::from_bytes([2u8; 32])
            .verification_key()
            .unwrap();
        let sig = sk1.sign(KesPeriod(0), b"msg").unwrap();
        assert!(vk2.verify(KesPeriod(0), b"msg", &sig).is_err());
    }

    // ── Compact KES ──────────────────────────────────────────────────────

    #[test]
    fn compact_kes_sign_verify() {
        let sk = KesSigningKey::from_bytes([0x10; 32]);
        let vk = sk.verification_key().unwrap();
        let msg = b"compact test";
        let csig = sk.sign_compact(KesPeriod(0), msg).unwrap();
        vk.verify_compact(KesPeriod(0), msg, &csig)
            .expect("compact KES signature should verify");
    }

    #[test]
    fn compact_kes_embeds_correct_vk() {
        let sk = KesSigningKey::from_bytes([0x11; 32]);
        let vk = sk.verification_key().unwrap();
        let csig = sk.sign_compact(KesPeriod(0), b"msg").unwrap();
        assert_eq!(csig.verification_key(), vk);
    }

    #[test]
    fn compact_kes_rejects_vk_mismatch() {
        let sk1 = KesSigningKey::from_bytes([1u8; 32]);
        let vk2 = KesSigningKey::from_bytes([2u8; 32])
            .verification_key()
            .unwrap();
        let csig = sk1.sign_compact(KesPeriod(0), b"msg").unwrap();
        let result = vk2.verify_compact(KesPeriod(0), b"msg", &csig);
        assert_eq!(result, Err(CryptoError::KesVerificationKeyMismatch));
    }

    #[test]
    fn compact_kes_signature_extracts_inner_sig() {
        let sk = KesSigningKey::from_bytes([0x12; 32]);
        let sig = sk.sign(KesPeriod(0), b"msg").unwrap();
        let csig = sk.sign_compact(KesPeriod(0), b"msg").unwrap();
        assert_eq!(csig.signature(), sig);
    }

    #[test]
    fn compact_kes_from_bytes_roundtrip() {
        let sk = KesSigningKey::from_bytes([0x13; 32]);
        let csig = sk.sign_compact(KesPeriod(0), b"x").unwrap();
        let bytes = csig.to_bytes();
        let csig2 = CompactKesSignature::from_bytes(bytes);
        assert_eq!(csig, csig2);
    }

    // ── KesSignature / KesVerificationKey serialization ──────────────────

    #[test]
    fn kes_signature_from_bytes_roundtrip() {
        let bytes = [0xAA; KES_SIGNATURE_SIZE];
        let sig = KesSignature::from_bytes(bytes);
        assert_eq!(sig.to_bytes(), bytes);
    }

    #[test]
    fn kes_verification_key_from_bytes_roundtrip() {
        let bytes = [0xBB; KES_VERIFICATION_KEY_SIZE];
        let vk = KesVerificationKey::from_bytes(bytes);
        assert_eq!(vk.to_bytes(), bytes);
    }

    // ── SimpleKesSigningKey ──────────────────────────────────────────────

    #[test]
    fn simple_kes_from_seeds() {
        let seeds = vec![[1u8; 32], [2u8; 32]];
        let sk = SimpleKesSigningKey::from_seeds(seeds).unwrap();
        assert_eq!(sk.total_periods(), 2);
    }

    #[test]
    fn simple_kes_from_seeds_empty_rejected() {
        let result = SimpleKesSigningKey::from_seeds(vec![]);
        assert_eq!(result, Err(CryptoError::InvalidKesDepth(0)));
    }

    #[test]
    fn simple_kes_from_bytes_roundtrip() {
        let mut bytes = vec![0u8; 64];
        bytes[..32].copy_from_slice(&[1u8; 32]);
        bytes[32..].copy_from_slice(&[2u8; 32]);
        let sk = SimpleKesSigningKey::from_bytes(&bytes).unwrap();
        assert_eq!(sk.to_bytes(), bytes);
        assert_eq!(sk.total_periods(), 2);
    }

    #[test]
    fn simple_kes_from_bytes_empty_rejected() {
        assert_eq!(
            SimpleKesSigningKey::from_bytes(&[]),
            Err(CryptoError::InvalidKesDepth(0))
        );
    }

    #[test]
    fn simple_kes_from_bytes_non_multiple_rejected() {
        assert_eq!(
            SimpleKesSigningKey::from_bytes(&[0u8; 33]),
            Err(CryptoError::InvalidKesKeyMaterialLength(33))
        );
    }

    #[test]
    fn simple_kes_sign_verify_each_period() {
        let seeds = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let sk = SimpleKesSigningKey::from_seeds(seeds).unwrap();
        let vk = sk.verification_key().unwrap();
        let msg = b"multi-period test";

        for period in 0..3 {
            let sig = sk.sign(KesPeriod(period), msg).unwrap();
            vk.verify(KesPeriod(period), msg, &sig)
                .unwrap_or_else(|e| panic!("period {period} should verify: {e}"));
        }
    }

    #[test]
    fn simple_kes_sign_out_of_range_rejected() {
        let sk = SimpleKesSigningKey::from_seeds(vec![[1u8; 32]]).unwrap();
        assert_eq!(
            sk.sign(KesPeriod(1), b"msg"),
            Err(CryptoError::InvalidKesPeriod(1))
        );
    }

    #[test]
    fn simple_kes_cross_period_verification_fails() {
        let seeds = vec![[1u8; 32], [2u8; 32]];
        let sk = SimpleKesSigningKey::from_seeds(seeds).unwrap();
        let vk = sk.verification_key().unwrap();
        let sig_p0 = sk.sign(KesPeriod(0), b"msg").unwrap();
        // Signature from period 0 must not verify under period 1's key.
        assert!(vk.verify(KesPeriod(1), b"msg", &sig_p0).is_err());
    }

    // ── SimpleKES indexed signatures ─────────────────────────────────────

    #[test]
    fn simple_kes_sign_indexed_embeds_period() {
        let seeds = vec![[1u8; 32], [2u8; 32]];
        let sk = SimpleKesSigningKey::from_seeds(seeds).unwrap();
        let sig = sk.sign_indexed(KesPeriod(1), b"msg").unwrap();
        assert_eq!(sig.period(), KesPeriod(1));
    }

    #[test]
    fn simple_kes_verify_indexed_roundtrip() {
        let seeds = vec![[1u8; 32], [2u8; 32]];
        let sk = SimpleKesSigningKey::from_seeds(seeds).unwrap();
        let vk = sk.verification_key().unwrap();
        let sig = sk.sign_indexed(KesPeriod(1), b"msg").unwrap();
        vk.verify_indexed(b"msg", &sig)
            .expect("indexed signature should verify");
    }

    #[test]
    fn simple_kes_indexed_signature_from_bytes_roundtrip() {
        let seeds = vec![[1u8; 32], [2u8; 32]];
        let sk = SimpleKesSigningKey::from_seeds(seeds).unwrap();
        let sig = sk.sign_indexed(KesPeriod(0), b"msg").unwrap();
        let bytes = sig.to_bytes();
        let sig2 = SimpleKesSignature::from_bytes(bytes);
        assert_eq!(sig, sig2);
    }

    // ── SimpleKES compact indexed signatures ─────────────────────────────

    #[test]
    fn simple_kes_sign_indexed_compact_roundtrip() {
        let seeds = vec![[1u8; 32], [2u8; 32]];
        let sk = SimpleKesSigningKey::from_seeds(seeds).unwrap();
        let vk = sk.verification_key().unwrap();
        let csig = sk.sign_indexed_compact(KesPeriod(1), b"msg").unwrap();
        vk.verify_indexed_compact(b"msg", &csig)
            .expect("compact indexed signature should verify");
    }

    #[test]
    fn simple_compact_kes_rejects_vk_mismatch() {
        let sk = SimpleKesSigningKey::from_seeds(vec![[1u8; 32], [2u8; 32]]).unwrap();
        let vk_other = SimpleKesSigningKey::from_seeds(vec![[3u8; 32], [4u8; 32]])
            .unwrap()
            .verification_key()
            .unwrap();
        let csig = sk.sign_indexed_compact(KesPeriod(0), b"msg").unwrap();
        let result = vk_other.verify_indexed_compact(b"msg", &csig);
        assert_eq!(result, Err(CryptoError::KesVerificationKeyMismatch));
    }

    #[test]
    fn simple_compact_kes_from_bytes_roundtrip() {
        let sk = SimpleKesSigningKey::from_seeds(vec![[1u8; 32]]).unwrap();
        let csig = sk.sign_indexed_compact(KesPeriod(0), b"msg").unwrap();
        let bytes = csig.to_bytes();
        let csig2 = SimpleCompactKesSignature::from_bytes(bytes);
        assert_eq!(csig, csig2);
    }

    #[test]
    fn simple_compact_kes_accessors() {
        let sk = SimpleKesSigningKey::from_seeds(vec![[1u8; 32], [2u8; 32]]).unwrap();
        let csig = sk.sign_indexed_compact(KesPeriod(1), b"msg").unwrap();
        assert_eq!(csig.period(), KesPeriod(1));
        let inner_sig = csig.signature();
        assert_eq!(inner_sig.0.len(), KES_SIGNATURE_SIZE);
        let inner_vk = csig.verification_key();
        assert_eq!(inner_vk.0.len(), KES_VERIFICATION_KEY_SIZE);
    }

    // ── SimpleKesVerificationKey ─────────────────────────────────────────

    #[test]
    fn simple_kes_vk_from_bytes_roundtrip() {
        let seeds = vec![[1u8; 32], [2u8; 32]];
        let sk = SimpleKesSigningKey::from_seeds(seeds).unwrap();
        let vk = sk.verification_key().unwrap();
        let bytes = vk.to_bytes();
        let vk2 = SimpleKesVerificationKey::from_bytes(&bytes).unwrap();
        assert_eq!(vk, vk2);
    }

    #[test]
    fn simple_kes_vk_from_bytes_empty_rejected() {
        assert_eq!(
            SimpleKesVerificationKey::from_bytes(&[]),
            Err(CryptoError::InvalidKesDepth(0))
        );
    }

    #[test]
    fn simple_kes_vk_from_bytes_non_multiple_rejected() {
        assert_eq!(
            SimpleKesVerificationKey::from_bytes(&[0u8; 33]),
            Err(CryptoError::InvalidKesKeyMaterialLength(33))
        );
    }

    #[test]
    fn simple_kes_vk_total_periods() {
        let seeds = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let sk = SimpleKesSigningKey::from_seeds(seeds).unwrap();
        let vk = sk.verification_key().unwrap();
        assert_eq!(vk.total_periods(), 3);
    }

    // ── SimpleKesSigningKey debug ────────────────────────────────────────

    #[test]
    fn simple_kes_signing_key_debug_redacted() {
        let sk = SimpleKesSigningKey::from_seeds(vec![[1u8; 32]]).unwrap();
        let dbg = format!("{:?}", sk);
        assert!(dbg.contains("REDACTED"));
        assert!(dbg.contains("total_periods"));
    }

    // ── Test vector: two-period SimpleKES ────────────────────────────────

    #[test]
    fn simple_kes_two_period_test_vector_sign_indexed() {
        let v = &simple_kes_two_period_test_vectors()[0];
        let sk = SimpleKesSigningKey::from_seeds(v.seeds.to_vec()).unwrap();
        let sig = sk.sign_indexed(KesPeriod(v.period), &v.message).unwrap();
        assert_eq!(
            sig.to_bytes(),
            v.indexed_signature,
            "indexed signature mismatch for vector: {}",
            v.name
        );
    }

    #[test]
    fn simple_kes_two_period_test_vector_verify_indexed() {
        let v = &simple_kes_two_period_test_vectors()[0];
        let sk = SimpleKesSigningKey::from_seeds(v.seeds.to_vec()).unwrap();
        let vk = sk.verification_key().unwrap();
        let sig = SimpleKesSignature::from_bytes(v.indexed_signature);
        vk.verify_indexed(&v.message, &sig)
            .expect("test vector indexed signature should verify");
    }

    #[test]
    fn simple_kes_two_period_test_vector_verify_compact() {
        let v = &simple_kes_two_period_test_vectors()[0];
        let sk = SimpleKesSigningKey::from_seeds(v.seeds.to_vec()).unwrap();
        let vk = sk.verification_key().unwrap();
        let csig = SimpleCompactKesSignature::from_bytes(v.compact_indexed_signature);
        vk.verify_indexed_compact(&v.message, &csig)
            .expect("test vector compact indexed signature should verify");
    }

    #[test]
    fn simple_kes_two_period_test_vector_key_derivation() {
        let v = &simple_kes_two_period_test_vectors()[0];
        let sk = SimpleKesSigningKey::from_seeds(v.seeds.to_vec()).unwrap();
        let vk = sk.verification_key().unwrap();
        for (i, expected_vk_bytes) in v.verification_keys.iter().enumerate() {
            assert_eq!(
                vk.0[i].to_bytes(),
                *expected_vk_bytes,
                "VK mismatch at period {i} for vector: {}",
                v.name
            );
        }
    }

    #[test]
    fn simple_kes_two_period_test_vector_raw_signature() {
        let v = &simple_kes_two_period_test_vectors()[0];
        let sk = SimpleKesSigningKey::from_seeds(v.seeds.to_vec()).unwrap();
        let sig = sk.sign(KesPeriod(v.period), &v.message).unwrap();
        assert_eq!(
            sig.to_bytes(),
            v.signature,
            "raw signature mismatch for vector: {}",
            v.name
        );
    }
}
