//! DMQ `SigSubmission` mini-protocol — signature diffusion.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Collapses the upstream
//! `DMQ/Protocol/SigSubmission/{Type,Codec,Validate}.hs` trio into one
//! Rust file, mirroring the `crates/network/src/protocols/`
//! one-file-per-mini-protocol pattern. `SigSubmission` is upstream
//! `type SigSubmission crypto = TxSubmission2 SigId (Sig crypto)` — DMQ
//! reuses the `TxSubmission2` mini-protocol to diffuse signatures
//! (e.g. Mithril signatures) across the network.
//!
//! This slice ports the `Type.hs` byte-wrapper newtypes — [`SigHash`],
//! [`SigId`], [`SigBody`], [`CborBytes`]. The crypto-parameterized
//! `SigRaw` / `Sig` payload types, the `SigValidationError` tree, the
//! CBOR codec, and the validator land in subsequent dmq-node-arc
//! rounds, appended to this file.

use std::fmt;

use yggdrasil_consensus::OpCert;
use yggdrasil_crypto::{KesSignature, VerificationKey};

/// The hash identifying a DMQ signature.
///
/// Upstream `newtype SigHash = SigHash { getSigHash :: ByteString }`.
/// Upstream's `Show` instance renders the first 10 bytes as hex (20
/// hex chars): `take 20 . decodeUtf8 . Base16.encode`. The Rust
/// [`fmt::Debug`] impl (the `Show` analog) reproduces that.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SigHash(pub Vec<u8>);

impl SigHash {
    /// The raw hash bytes — upstream `getSigHash`.
    pub fn get(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SigHash {
    /// Mirror of upstream `instance Show SigHash` — the first 10 bytes
    /// rendered as lowercase hex (at most 20 characters).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0.iter().take(10) {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Identifier of a DMQ signature — a newtype over [`SigHash`].
///
/// Upstream `newtype SigId = SigId { getSigId :: SigHash }`. This is
/// the `txid`-analog in the `TxSubmission2`-based `SigSubmission`
/// mini-protocol.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SigId(pub SigHash);

impl SigId {
    /// The underlying [`SigHash`] — upstream `getSigId`.
    pub fn get(&self) -> &SigHash {
        &self.0
    }
}

/// The opaque body/payload of a DMQ signature.
///
/// Upstream `newtype SigBody = SigBody { getSigBody :: ByteString }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigBody(pub Vec<u8>);

impl SigBody {
    /// The raw body bytes — upstream `getSigBody`.
    pub fn get(&self) -> &[u8] {
        &self.0
    }
}

/// A wrapper that renders CBOR bytes as hex.
///
/// Upstream `newtype CBORBytes = CBORBytes { getCBORBytes :: LBS.ByteString }`
/// with `Show = base16-encode` (the full byte string, unlike
/// [`SigHash`]'s 10-byte truncation).
#[derive(Clone, Eq, PartialEq)]
pub struct CborBytes(pub Vec<u8>);

impl CborBytes {
    /// The wrapped CBOR bytes — upstream `getCBORBytes`.
    pub fn get(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for CborBytes {
    /// Mirror of upstream `instance Show CBORBytes` — the full byte
    /// string rendered as lowercase hex.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A DMQ signature's KES signature.
///
/// Upstream `newtype SigKESSignature crypto =
/// SigKESSignature { getSigKESSignature :: SigKES (KES crypto) }`. The
/// `crypto` type parameter collapses to yggdrasil's concrete
/// [`KesSignature`] — yggdrasil is not generic over the crypto suite.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigKesSignature(pub KesSignature);

impl SigKesSignature {
    /// The wrapped KES signature — upstream `getSigKESSignature`.
    pub fn get(&self) -> &KesSignature {
        &self.0
    }
}

/// A DMQ signature's cold (DSIGN) verification key.
///
/// Upstream `newtype SigColdKey crypto =
/// SigColdKey { getSigColdKey :: VerKeyDSIGN (KES.DSIGN crypto) }`.
/// Collapses to yggdrasil's concrete Ed25519 [`VerificationKey`] — the
/// cold key that issues the signature's operational certificate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigColdKey(pub VerificationKey);

impl SigColdKey {
    /// The wrapped Ed25519 verification key — upstream `getSigColdKey`.
    pub fn get(&self) -> &VerificationKey {
        &self.0
    }
}

/// A DMQ signature's operational certificate.
///
/// Upstream `newtype SigOpCertificate crypto =
/// SigOpCertificate { getSigOpCertificate :: OCert crypto }`. The
/// `crypto` parameter collapses to yggdrasil's concrete consensus
/// [`OpCert`] (the hot-KES-verkey / counter / KES-period / cold
/// signature record shared with block-header validation).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigOpCertificate(pub OpCert);

impl SigOpCertificate {
    /// The wrapped operational certificate — upstream `getSigOpCertificate`.
    pub fn get(&self) -> &OpCert {
        &self.0
    }
}

/// A POSIX timestamp — whole seconds since the Unix epoch.
///
/// Upstream `sigRawExpiresAt :: POSIXTime` (`Data.Time.Clock.POSIX`).
/// The `SigSubmission` codec decodes it as a bare `Word32`
/// (`realToFrac <$> CBOR.decodeWord32`), so the wire/value
/// representation is `u32` whole seconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PosixTime(pub u32);

/// The payload of a DMQ signature.
///
/// Upstream `data SigRaw crypto = SigRaw { sigRawId, sigRawBody,
/// sigRawKESPeriod, sigRawOpCertificate, sigRawColdKey,
/// sigRawExpiresAt, sigRawKESSignature }`. `sig_raw_kes_period` is
/// `u64` — upstream's `KESPeriod` is a `Word` newtype and CIP-137
/// mandates `Word64`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigRaw {
    /// `sigRawId`.
    pub sig_raw_id: SigId,
    /// `sigRawBody`.
    pub sig_raw_body: SigBody,
    /// `sigRawKESPeriod` — the KES period the signature was created in.
    pub sig_raw_kes_period: u64,
    /// `sigRawOpCertificate`.
    pub sig_raw_op_certificate: SigOpCertificate,
    /// `sigRawColdKey`.
    pub sig_raw_cold_key: SigColdKey,
    /// `sigRawExpiresAt`.
    pub sig_raw_expires_at: PosixTime,
    /// `sigRawKESSignature` — KES signature over the preceding fields.
    pub sig_raw_kes_signature: SigKesSignature,
}

/// A [`SigRaw`] paired with the exact bytes the KES key signed.
///
/// Upstream `data SigRawWithSignedBytes crypto = SigRawWithSignedBytes
/// { sigRawSignedBytes :: LBS.ByteString, sigRaw :: SigRaw crypto }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigRawWithSignedBytes {
    /// `sigRawSignedBytes` — the bytes signed by the KES key.
    pub sig_raw_signed_bytes: Vec<u8>,
    /// `sigRaw` — the decoded payload.
    pub sig_raw: SigRaw,
}

/// A wire DMQ signature: the encoded `SigRaw` bytes plus the decoded
/// payload-with-signed-bytes.
///
/// Upstream `data Sig crypto = SigWithBytes { sigRawBytes ::
/// LBS.ByteString, sigRawWithSignedBytes :: SigRawWithSignedBytes
/// crypto }`. Upstream additionally exposes a bidirectional `Sig`
/// pattern synonym with flat accessors; the Rust port provides those
/// as methods.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sig {
    /// `sigRawBytes` / `sigBytes` — the full encoded `SigRaw`.
    pub sig_raw_bytes: Vec<u8>,
    /// `sigRawWithSignedBytes`.
    pub sig_raw_with_signed_bytes: SigRawWithSignedBytes,
}

impl Sig {
    /// `sigId` — the signature's id.
    pub fn sig_id(&self) -> &SigId {
        &self.sig_raw_with_signed_bytes.sig_raw.sig_raw_id
    }
    /// `sigBody` — the signature body.
    pub fn sig_body(&self) -> &SigBody {
        &self.sig_raw_with_signed_bytes.sig_raw.sig_raw_body
    }
    /// `sigKESPeriod` — the KES period the signature was created in.
    pub fn sig_kes_period(&self) -> u64 {
        self.sig_raw_with_signed_bytes.sig_raw.sig_raw_kes_period
    }
    /// `sigOpCertificate` — the operational certificate.
    pub fn sig_op_certificate(&self) -> &SigOpCertificate {
        &self
            .sig_raw_with_signed_bytes
            .sig_raw
            .sig_raw_op_certificate
    }
    /// `sigColdKey` — the issuing cold key.
    pub fn sig_cold_key(&self) -> &SigColdKey {
        &self.sig_raw_with_signed_bytes.sig_raw.sig_raw_cold_key
    }
    /// `sigExpiresAt` — the signature's expiry timestamp.
    pub fn sig_expires_at(&self) -> PosixTime {
        self.sig_raw_with_signed_bytes.sig_raw.sig_raw_expires_at
    }
    /// `sigKESSignature` — the KES signature.
    pub fn sig_kes_signature(&self) -> &SigKesSignature {
        &self.sig_raw_with_signed_bytes.sig_raw.sig_raw_kes_signature
    }
    /// `sigSignedBytes` — the bytes the KES key signed.
    pub fn sig_signed_bytes(&self) -> &[u8] {
        &self.sig_raw_with_signed_bytes.sig_raw_signed_bytes
    }
    /// `sigBytes` — the full encoded `SigRaw`.
    pub fn sig_bytes(&self) -> &[u8] {
        &self.sig_raw_bytes
    }
}

/// A reason a DMQ signature failed validation.
///
/// Upstream `data SigValidationError` (`SigSubmission/Type.hs`,
/// `deriving (Eq, Show)`). KES periods are `u64`: upstream's
/// `KESPeriod` is a `Word` newtype, and CIP-137 mandates `Word64` for
/// DMQ KES periods (the `Type.hs` `sigRawKESPeriod` note). `Word64`
/// counters map to `u64`; `String` / `Text` diagnostics to `String`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SigValidationError {
    /// `InvalidKESSignature KESPeriod KESPeriod String` — the KES
    /// signature did not verify.
    InvalidKesSignature {
        /// Current KES period.
        current_period: u64,
        /// The operational certificate's KES period.
        opcert_period: u64,
        /// Verification-failure reason.
        reason: String,
    },
    /// `InvalidSignatureOCERT Word64 KESPeriod String` — the
    /// operational certificate's own DSIGN signature did not verify.
    InvalidSignatureOcert {
        /// Operational-certificate counter.
        ocert_counter: u64,
        /// Operational-certificate KES period.
        ocert_kes_period: u64,
        /// DSIGN-failure reason.
        reason: String,
    },
    /// `InvalidOCertCounter Word64 Word64` — the operational
    /// certificate counter regressed.
    InvalidOcertCounter {
        /// Last counter the validator saw for this pool.
        last_seen: u64,
        /// Counter received in this signature.
        received: u64,
    },
    /// `KESBeforeStartOCERT KESPeriod KESPeriod` — the KES period
    /// precedes the operational certificate's start period.
    KesBeforeStartOcert {
        /// The signature's KES period.
        kes_period: u64,
        /// The operational certificate's start period.
        start_period: u64,
    },
    /// `KESAfterEndOCERT KESPeriod KESPeriod` — the KES period is past
    /// the operational certificate's end period.
    KesAfterEndOcert {
        /// The signature's KES period.
        kes_period: u64,
        /// The operational certificate's end period.
        end_period: u64,
    },
    /// `PoolNotEligible` — the issuing pool is not eligible.
    PoolNotEligible,
    /// `UnrecognizedPool` — the issuing pool is unknown.
    UnrecognizedPool,
    /// `NotInitialized` — the validator has no ledger state yet.
    NotInitialized,
    /// `ClockSkew` — the signature's timestamp is outside tolerance.
    ClockSkew,
    /// `SigDuplicate` — the signature was already seen.
    SigDuplicate,
    /// `SigExpired` — the signature is past its `expiresAt` time.
    SigExpired,
    /// `SigResultOther Text` — any other validation failure.
    SigResultOther(String),
}

/// A trace event emitted when a signature fails validation.
///
/// Upstream `data SigValidationTrace = InvalidSignature SigId
/// SigValidationError` (`deriving Show`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SigValidationTrace {
    /// `InvalidSignature SigId SigValidationError`.
    InvalidSignature {
        /// The offending signature's id.
        sig_id: SigId,
        /// Why it failed validation.
        error: SigValidationError,
    },
}

/// The exception thrown when signature validation fails.
///
/// Upstream `data SigValidationException = SigValidationException SigId
/// SigValidationError` with `instance Exception`. The Rust port is a
/// `thiserror::Error` carrying the same two fields.
#[derive(Clone, Debug, thiserror::Error)]
#[error("DMQ signature {sig_id:?} failed validation: {error:?}")]
pub struct SigValidationException {
    /// The offending signature's id.
    pub sig_id: SigId,
    /// Why it failed validation.
    pub error: SigValidationError,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sig_hash_debug_renders_first_10_bytes_as_hex() {
        // 12 bytes — only the first 10 (20 hex chars) are shown.
        let hash = SigHash(vec![
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB,
        ]);
        assert_eq!(format!("{hash:?}"), "00112233445566778899");
    }

    #[test]
    fn sig_hash_debug_renders_short_hash_in_full() {
        let hash = SigHash(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(format!("{hash:?}"), "deadbeef");
        assert_eq!(hash.get(), &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn sig_id_wraps_a_sig_hash() {
        let id = SigId(SigHash(vec![0x01, 0x02, 0x03]));
        assert_eq!(id.get().get(), &[0x01, 0x02, 0x03]);
        assert_eq!(format!("{id:?}"), "SigId(010203)");
    }

    #[test]
    fn sig_id_ordering_follows_the_hash() {
        let a = SigId(SigHash(vec![0x01]));
        let b = SigId(SigHash(vec![0x02]));
        assert!(a < b);
    }

    #[test]
    fn sig_body_round_trips() {
        let body = SigBody(vec![0xCA, 0xFE]);
        assert_eq!(body.get(), &[0xCA, 0xFE]);
        assert_eq!(body, SigBody(vec![0xCA, 0xFE]));
    }

    #[test]
    fn cbor_bytes_debug_renders_full_hex() {
        let bytes = CborBytes(vec![0x82, 0x01, 0x02]);
        assert_eq!(format!("{bytes:?}"), "820102");
        assert_eq!(bytes.get(), &[0x82, 0x01, 0x02]);
    }

    #[test]
    fn cbor_bytes_empty_renders_empty() {
        assert_eq!(format!("{:?}", CborBytes(Vec::new())), "");
    }

    #[test]
    fn sig_kes_signature_wraps_and_compares() {
        let sig = SigKesSignature(KesSignature([0x22; 64]));
        assert_eq!(sig, SigKesSignature(KesSignature([0x22; 64])));
        assert_ne!(sig, SigKesSignature(KesSignature([0x00; 64])));
        assert_eq!(sig.get(), &KesSignature([0x22; 64]));
    }

    #[test]
    fn sig_cold_key_wraps_and_compares() {
        let key = SigColdKey(VerificationKey([0x11; 32]));
        assert_eq!(key, SigColdKey(VerificationKey([0x11; 32])));
        assert_ne!(key, SigColdKey(VerificationKey([0xFF; 32])));
        assert_eq!(key.get(), &VerificationKey([0x11; 32]));
    }

    #[test]
    fn sig_op_certificate_wraps_and_compares() {
        use yggdrasil_crypto::{Signature, SumKesVerificationKey};
        let ocert = |seq: u64| OpCert {
            hot_vkey: SumKesVerificationKey([0x33; 32]),
            sequence_number: seq,
            kes_period: 5,
            sigma: Signature([0x44; 64]),
        };
        let cert = SigOpCertificate(ocert(1));
        assert_eq!(cert, SigOpCertificate(ocert(1)));
        assert_ne!(cert, SigOpCertificate(ocert(2)));
        assert_eq!(cert.get().sequence_number, 1);
    }

    fn sample_sig_raw() -> SigRaw {
        use yggdrasil_crypto::{Signature, SumKesVerificationKey};
        SigRaw {
            sig_raw_id: SigId(SigHash(vec![0xAA])),
            sig_raw_body: SigBody(vec![0xBB, 0xCC]),
            sig_raw_kes_period: 7,
            sig_raw_op_certificate: SigOpCertificate(OpCert {
                hot_vkey: SumKesVerificationKey([0x33; 32]),
                sequence_number: 1,
                kes_period: 7,
                sigma: Signature([0x44; 64]),
            }),
            sig_raw_cold_key: SigColdKey(VerificationKey([0x11; 32])),
            sig_raw_expires_at: PosixTime(1_700_000_000),
            sig_raw_kes_signature: SigKesSignature(KesSignature([0x22; 64])),
        }
    }

    #[test]
    fn sig_raw_round_trips() {
        let raw = sample_sig_raw();
        assert_eq!(raw.sig_raw_id, SigId(SigHash(vec![0xAA])));
        assert_eq!(raw.sig_raw_kes_period, 7);
        assert_eq!(raw.sig_raw_expires_at, PosixTime(1_700_000_000));
        assert_eq!(raw, sample_sig_raw());
    }

    #[test]
    fn sig_flat_accessors_reach_through_to_sig_raw() {
        let sig = Sig {
            sig_raw_bytes: vec![0x84, 0x01, 0x02],
            sig_raw_with_signed_bytes: SigRawWithSignedBytes {
                sig_raw_signed_bytes: vec![0xDE, 0xAD],
                sig_raw: sample_sig_raw(),
            },
        };
        // The `Sig` pattern-synonym accessors reach into `SigRaw`.
        assert_eq!(sig.sig_id(), &SigId(SigHash(vec![0xAA])));
        assert_eq!(sig.sig_body(), &SigBody(vec![0xBB, 0xCC]));
        assert_eq!(sig.sig_kes_period(), 7);
        assert_eq!(sig.sig_expires_at(), PosixTime(1_700_000_000));
        // ... and `sigSignedBytes` / `sigBytes` to the wrapper bytes.
        assert_eq!(sig.sig_signed_bytes(), &[0xDE, 0xAD]);
        assert_eq!(sig.sig_bytes(), &[0x84, 0x01, 0x02]);
    }

    #[test]
    fn sig_validation_error_variants_construct_and_compare() {
        let kes = SigValidationError::InvalidKesSignature {
            current_period: 7,
            opcert_period: 5,
            reason: "bad".to_string(),
        };
        assert_eq!(
            kes,
            SigValidationError::InvalidKesSignature {
                current_period: 7,
                opcert_period: 5,
                reason: "bad".to_string(),
            }
        );
        assert_ne!(kes, SigValidationError::SigExpired);
        // Field-less variants compare by discriminant.
        assert_eq!(
            SigValidationError::SigDuplicate,
            SigValidationError::SigDuplicate
        );
        assert_ne!(
            SigValidationError::SigResultOther("a".to_string()),
            SigValidationError::SigResultOther("b".to_string())
        );
    }

    #[test]
    fn sig_validation_trace_carries_id_and_error() {
        let trace = SigValidationTrace::InvalidSignature {
            sig_id: SigId(SigHash(vec![0xAB])),
            error: SigValidationError::ClockSkew,
        };
        let SigValidationTrace::InvalidSignature { sig_id, error } = trace;
        assert_eq!(sig_id, SigId(SigHash(vec![0xAB])));
        assert_eq!(error, SigValidationError::ClockSkew);
    }

    #[test]
    fn sig_validation_exception_display_names_sig_and_reason() {
        let exc = SigValidationException {
            sig_id: SigId(SigHash(vec![0xDE, 0xAD])),
            error: SigValidationError::SigExpired,
        };
        let rendered = format!("{exc}");
        assert!(rendered.contains("dead"), "got: {rendered}");
        assert!(rendered.contains("SigExpired"), "got: {rendered}");
    }
}
