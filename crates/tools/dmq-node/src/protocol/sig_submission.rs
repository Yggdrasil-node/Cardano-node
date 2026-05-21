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
use std::time::Duration;

use crate::diffusion::{PoolId, PoolValidationCtx};
use yggdrasil_consensus::OpCert;
use yggdrasil_crypto::{KesSignature, Signature, SumKesVerificationKey, VerificationKey};
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder, vec_with_strict_capacity};

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

impl SigValidationError {
    /// Render this error as JSON.
    ///
    /// Mirror of upstream `instance ToJSON SigValidationError`:
    /// `SigDuplicate` / `SigExpired` render as the bare strings
    /// `"duplicate"` / `"expired"`; `SigResultOther` as
    /// `{"type":"other","reason":<text>}`; every other variant as
    /// `{"type":"invalid","reason":<rendered error>}`.
    ///
    /// Upstream's `"invalid"` `reason` uses Haskell `show`; the Rust
    /// port uses the variant's `Debug` rendering — the JSON
    /// *structure* is byte-exact, the human-readable `reason` text is
    /// the Rust formatting of the same fields.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            SigValidationError::SigDuplicate => serde_json::Value::String("duplicate".to_string()),
            SigValidationError::SigExpired => serde_json::Value::String("expired".to_string()),
            SigValidationError::SigResultOther(reason) => {
                serde_json::json!({ "type": "other", "reason": reason })
            }
            other => serde_json::json!({ "type": "invalid", "reason": format!("{other:?}") }),
        }
    }
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

// ---------------------------------------------------------------------------
// CBOR codec (upstream `SigSubmission/Codec.hs`)
// ---------------------------------------------------------------------------

/// Encode a [`SigId`] as a CBOR byte string.
///
/// Mirror of upstream `encodeSigId SigId{getSigId} =
/// encodeBytes (getSigHash getSigId)` (`SigSubmission/Codec.hs`).
pub fn encode_sig_id(sig_id: &SigId, enc: &mut Encoder) {
    enc.bytes(sig_id.get().get());
}

/// Decode a [`SigId`] from a CBOR byte string.
///
/// Mirror of upstream `decodeSigId = SigId . SigHash <$> decodeBytes`.
pub fn decode_sig_id(dec: &mut Decoder) -> Result<SigId, LedgerError> {
    Ok(SigId(SigHash(dec.bytes_owned()?)))
}

/// Decode a CBOR byte string of exactly `N` bytes into a fixed array.
fn decode_fixed_bytes<const N: usize>(dec: &mut Decoder) -> Result<[u8; N], LedgerError> {
    let bytes = dec.bytes_owned()?;
    <[u8; N]>::try_from(bytes.as_slice()).map_err(|_| LedgerError::CborInvalidLength {
        expected: N,
        actual: bytes.len(),
    })
}

/// Decode a CBOR array header and require it to declare exactly `n`
/// elements.
fn expect_array_len(dec: &mut Decoder, n: u64) -> Result<(), LedgerError> {
    let len = dec.array()?;
    if len != n {
        return Err(LedgerError::CborInvalidLength {
            expected: n as usize,
            actual: len as usize,
        });
    }
    Ok(())
}

/// Encode a [`SigOpCertificate`] as a CBOR 4-element array.
///
/// Mirror of upstream `encodeSigOpCertificate`:
/// `encodeListLen 4 <> encodeVerKeyKES (ocertVkHot) <> toCBOR (ocertN)
/// <> toCBOR (ocertKESPeriod) <> encodeSignedDSIGN (ocertSigma)`.
/// `encodeVerKeyKES` and `encodeSignedDSIGN` are CBOR byte strings of
/// the raw key / signature bytes; `ocertN` / `ocertKESPeriod` are CBOR
/// unsigned integers.
pub fn encode_sig_op_certificate(cert: &SigOpCertificate, enc: &mut Encoder) {
    let ocert = cert.get();
    enc.array(4);
    enc.bytes(&ocert.hot_vkey.0);
    enc.unsigned(ocert.sequence_number);
    enc.unsigned(ocert.kes_period);
    enc.bytes(&ocert.sigma.0);
}

/// Decode a [`SigOpCertificate`] from a CBOR 4-element array.
///
/// Mirror of upstream `decodeSigOpCertificate` — rejects any list
/// length other than 4.
pub fn decode_sig_op_certificate(dec: &mut Decoder) -> Result<SigOpCertificate, LedgerError> {
    let len = dec.array()?;
    if len != 4 {
        return Err(LedgerError::CborInvalidLength {
            expected: 4,
            actual: len as usize,
        });
    }
    let hot_vkey = decode_fixed_bytes::<32>(dec)?;
    let sequence_number = dec.unsigned()?;
    let kes_period = dec.unsigned()?;
    let sigma = decode_fixed_bytes::<64>(dec)?;
    Ok(SigOpCertificate(OpCert {
        hot_vkey: SumKesVerificationKey(hot_vkey),
        sequence_number,
        kes_period,
        sigma: Signature(sigma),
    }))
}

/// Encode a [`Sig`] as a CBOR byte string of its cached `SigRaw`
/// bytes.
///
/// Mirror of upstream `encodeSig = encodeBytes . sigRawBytes` — a
/// `Sig` carries the already-encoded `SigRaw`; encoding wraps those
/// bytes in a CBOR byte string.
pub fn encode_sig(sig: &Sig, enc: &mut Encoder) {
    enc.bytes(&sig.sig_raw_bytes);
}

/// Encode a [`SigRaw`] as the CBOR 4-element array `decodeSig` parses.
///
/// Mirror of the structure upstream `decodeSig` decodes: a 4-element
/// array `[payload, kesSignature, opCertificate, coldKey]` where
/// `payload` is itself the 4-element array
/// `[sigId, sigBody, kesPeriod, expiresAt]` — the bytes the KES key
/// signs. The KES signature and cold key encode as CBOR byte strings
/// (`encodeSigKES` / `encodeVerKeyDSIGN`).
pub fn encode_sig_raw(raw: &SigRaw, enc: &mut Encoder) {
    enc.array(4);
    // [0] payload — the signed sub-array.
    enc.array(4);
    encode_sig_id(&raw.sig_raw_id, enc);
    enc.bytes(raw.sig_raw_body.get());
    enc.unsigned(raw.sig_raw_kes_period);
    enc.unsigned(u64::from(raw.sig_raw_expires_at.0));
    // [1] KES signature.
    enc.bytes(&raw.sig_raw_kes_signature.0.0);
    // [2] operational certificate.
    encode_sig_op_certificate(&raw.sig_raw_op_certificate, enc);
    // [3] cold key.
    enc.bytes(&raw.sig_raw_cold_key.0.0);
}

/// Decode the `SigRaw` payload sub-array — the 4-element CBOR array
/// `[sigId, sigBody, kesPeriod, expiresAt]` that the KES key signs.
///
/// Mirror of upstream `decodeSig`'s `decodePayload` `where`-clause.
fn decode_sig_payload(dec: &mut Decoder) -> Result<(SigId, SigBody, u64, PosixTime), LedgerError> {
    expect_array_len(dec, 4)?;
    let sig_id = decode_sig_id(dec)?;
    let sig_body = SigBody(dec.bytes_owned()?);
    let kes_period = dec.unsigned()?;
    let expires = dec.unsigned()?;
    let expires_at = PosixTime(
        u32::try_from(expires).map_err(|_| LedgerError::ValueOverflow {
            site: "SigRaw.expiresAt",
        })?,
    );
    Ok((sig_id, sig_body, kes_period, expires_at))
}

/// Decode a [`SigRaw`] from the CBOR 4-element array.
///
/// Mirror of the structure upstream `decodeSig` parses — without the
/// `sigRawSignedBytes` byte-offset capture, which [`decode_sig`] adds.
pub fn decode_sig_raw(dec: &mut Decoder) -> Result<SigRaw, LedgerError> {
    expect_array_len(dec, 4)?;
    // [0] payload — the signed sub-array.
    let (sig_raw_id, sig_raw_body, sig_raw_kes_period, sig_raw_expires_at) =
        decode_sig_payload(dec)?;
    // [1] KES signature.
    let sig_raw_kes_signature = SigKesSignature(KesSignature(decode_fixed_bytes::<64>(dec)?));
    // [2] operational certificate.
    let sig_raw_op_certificate = decode_sig_op_certificate(dec)?;
    // [3] cold key.
    let sig_raw_cold_key = SigColdKey(VerificationKey(decode_fixed_bytes::<32>(dec)?));
    Ok(SigRaw {
        sig_raw_id,
        sig_raw_body,
        sig_raw_kes_period,
        sig_raw_op_certificate,
        sig_raw_cold_key,
        sig_raw_expires_at,
        sig_raw_kes_signature,
    })
}

/// Decode a [`Sig`] from the full CBOR message bytes.
///
/// Mirror of upstream `decodeSig` — decodes the `SigRaw` 4-element
/// array and additionally captures `sigRawSignedBytes`: the exact
/// bytes of the payload sub-array (element 0), the bytes the KES key
/// signed. Upstream uses `peekByteOffset` / `bytesBetweenOffsets`;
/// the Rust port uses `Decoder::position()` to bracket the payload.
///
/// `input` must be exactly the bytes the returned `Sig`'s
/// `sig_raw_bytes` should hold — the full encoded `SigRaw` message.
pub fn decode_sig(input: &[u8]) -> Result<Sig, LedgerError> {
    let mut dec = Decoder::new(input);
    expect_array_len(&mut dec, 4)?;
    // Bracket the payload sub-array to recover the signed bytes.
    let start = dec.position();
    let (sig_raw_id, sig_raw_body, sig_raw_kes_period, sig_raw_expires_at) =
        decode_sig_payload(&mut dec)?;
    let end = dec.position();
    let sig_raw_kes_signature = SigKesSignature(KesSignature(decode_fixed_bytes::<64>(&mut dec)?));
    let sig_raw_op_certificate = decode_sig_op_certificate(&mut dec)?;
    let sig_raw_cold_key = SigColdKey(VerificationKey(decode_fixed_bytes::<32>(&mut dec)?));
    let sig_raw = SigRaw {
        sig_raw_id,
        sig_raw_body,
        sig_raw_kes_period,
        sig_raw_op_certificate,
        sig_raw_cold_key,
        sig_raw_expires_at,
        sig_raw_kes_signature,
    };
    Ok(Sig {
        sig_raw_bytes: input.to_vec(),
        sig_raw_with_signed_bytes: SigRawWithSignedBytes {
            sig_raw_signed_bytes: input[start..end].to_vec(),
            sig_raw,
        },
    })
}

// ---------------------------------------------------------------------------
// Signature validation (upstream `SigSubmission/Validate.hs`)
// ---------------------------------------------------------------------------

/// Maximum tolerated clock skew, in seconds, for a signature's
/// pool-eligibility window check.
///
/// Mirror of upstream `c_MAX_CLOCK_SKEW_SEC :: NominalDiffTime = 5`
/// (`SigSubmission/Validate.hs`).
pub const MAX_CLOCK_SKEW_SEC: u64 = 5;

/// Verify a signature's KES period lies within its operational
/// certificate's validity window `[ocert_kes_period, ocert_kes_period
/// + total_kes_periods)`.
///
/// Mirror of upstream `validateSig`'s KES-period checks —
/// `sigKESPeriod < endKESPeriod ?! KESAfterEndOCERT …` then
/// `sigKESPeriod >= startKESPeriod ?! KESBeforeStartOCERT …`, where
/// `startKESPeriod = ocertKESPeriod` and `endKESPeriod` is
/// `startKESPeriod` plus `totalPeriodsKES`. `total_kes_periods` is the
/// KES algorithm's total period count (upstream's `totalPeriodsKES`),
/// supplied by the caller.
pub fn validate_kes_period(
    sig_kes_period: u64,
    ocert_kes_period: u64,
    total_kes_periods: u64,
) -> Result<(), SigValidationError> {
    let start_period = ocert_kes_period;
    let end_period = ocert_kes_period.saturating_add(total_kes_periods);
    // After-end check first, mirroring upstream's check order.
    if sig_kes_period >= end_period {
        return Err(SigValidationError::KesAfterEndOcert {
            kes_period: sig_kes_period,
            end_period,
        });
    }
    if sig_kes_period < start_period {
        return Err(SigValidationError::KesBeforeStartOcert {
            kes_period: sig_kes_period,
            start_period,
        });
    }
    Ok(())
}

/// Verify the operational-certificate counter is monotonic for the
/// issuing pool, recording the observed counter in the context's
/// `ocert_map`.
///
/// Mirror of upstream `validateSig`'s ocert-counter check: an absent
/// counter, or one not below the last seen value, is accepted and
/// recorded; a counter below the last seen one fails with
/// `InvalidOCertCounter`.
pub fn validate_ocert_counter(
    ctx: &mut PoolValidationCtx,
    pool: &PoolId,
    ocert_n: u64,
) -> Result<(), SigValidationError> {
    if let Some(prev) = ctx.ocert_map.get(pool).copied() {
        if prev > ocert_n {
            return Err(SigValidationError::InvalidOcertCounter {
                last_seen: prev,
                received: ocert_n,
            });
        }
    }
    ctx.ocert_map.insert(pool.clone(), ocert_n);
    Ok(())
}

/// Verify the signature's issuing pool is registered and eligible to
/// mint, given the validation context and the current POSIX time
/// (seconds).
///
/// Mirror of upstream `validateSig`'s pool-eligibility check
/// (`Validate.hs`): the `vctxStakeMap` lookup, then the
/// `NotZeroSetSnapshot` / `NotZeroMarkSnapshot` / `ZeroSetSnapshot`
/// stake-snapshot branching with the `MAX_CLOCK_SKEW_SEC` window
/// around the next epoch boundary (`vctxEpoch`).
pub fn validate_pool_eligibility(
    ctx: &PoolValidationCtx,
    pool: &PoolId,
    now: u64,
) -> Result<(), SigValidationError> {
    let Some(snapshot) = ctx.stake_map.get(pool) else {
        // An unknown pool: `NotInitialized` if the context has not
        // been populated yet (no epoch), otherwise `UnrecognizedPool`.
        return Err(if ctx.epoch.is_none() {
            SigValidationError::NotInitialized
        } else {
            SigValidationError::UnrecognizedPool
        });
    };
    // Upstream `fromJust vctxEpoch` is safe because the epoch and the
    // stake map are populated together; `now` is a benign fallback.
    let next_epoch = ctx.epoch.unwrap_or(now);
    let skew = MAX_CLOCK_SKEW_SEC as i64;
    // `diffUTCTime nextEpoch now`.
    let delta = next_epoch as i64 - now as i64;

    if snapshot.set_pool != 0 {
        // NotZeroSetSnapshot.
        if now <= next_epoch.saturating_add(MAX_CLOCK_SKEW_SEC) {
            Ok(())
        } else if snapshot.mark_pool == 0 {
            Err(SigValidationError::SigExpired)
        } else {
            Err(SigValidationError::ClockSkew)
        }
    } else if snapshot.mark_pool != 0 {
        // NotZeroMarkSnapshot (the set snapshot is zero).
        if delta.abs() <= skew {
            Ok(())
        } else if delta > skew {
            Err(SigValidationError::PoolNotEligible)
        } else {
            Err(SigValidationError::ClockSkew)
        }
    } else {
        // ZeroSetSnapshot — the pool is unregistered / ineligible.
        Err(SigValidationError::SigExpired)
    }
}

// ---------------------------------------------------------------------------
// SigSubmission mini-protocol
// (upstream `SigSubmission crypto = TxSubmission2 SigId (Sig crypto)`)
// ---------------------------------------------------------------------------

/// States of the `SigSubmission` mini-protocol state machine.
///
/// Upstream `SigSubmission crypto = TxSubmission2 SigId (Sig crypto)` —
/// the protocol *is* `TxSubmission2`, so the states are
/// `TxSubmission2`'s. This mirrors `crates/network`'s
/// `TxSubmissionState`; dmq-node carries its own copy because
/// `crates/network`'s `TxSubmission2` is concrete over the ledger
/// `TxId` and not generic over the id / tx types (R731 / R732
/// decision).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SigSubmissionState {
    /// Client agency — must send `MsgInit`.
    StInit,
    /// Server agency — may send `MsgRequestTxIds` or `MsgRequestTxs`.
    StIdle,
    /// Client agency — must reply with `MsgReplyTxIds` (or, if
    /// blocking, `MsgDone`).
    StTxIds {
        /// Whether this is a blocking request.
        blocking: bool,
    },
    /// Client agency — must reply with `MsgReplyTxs`.
    StTxs,
    /// Terminal state — no further messages.
    StDone,
}

/// A [`SigId`] paired with the serialized size of its [`Sig`].
///
/// Mirror of `crates/network`'s `TxIdAndSize` with a DMQ `SigId` —
/// the `(txid, SizeInBytes)` pair of the `TxSubmission2` protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigIdAndSize {
    /// The signature identifier.
    pub sig_id: SigId,
    /// Size of the serialized `Sig` in bytes.
    pub size: u32,
}

/// Messages of the `SigSubmission` mini-protocol.
///
/// Upstream `SigSubmission = TxSubmission2 SigId (Sig crypto)`, so the
/// messages are `TxSubmission2`'s `Message` constructors with `SigId`
/// identifiers and `Sig` payloads (the variant names keep upstream's
/// `Tx` spelling — the protocol *is* `TxSubmission2`). The CBOR
/// message-envelope tags (`6`/`0`/`1`/`2`/`3`/`4`) are byte-identical
/// to `crates/network`'s `TxSubmissionMessage` — the wire-equivalence
/// guarantee for the dmq-node-local protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SigSubmissionMessage {
    /// `[6]` — client sends the initial message. `StInit → StIdle`.
    MsgInit,
    /// `[0, blocking, ack, req]` — server requests signature
    /// identifiers. `StIdle → StTxIds(blocking)`.
    MsgRequestTxIds {
        /// `true` blocking (a non-empty reply is required), `false`
        /// non-blocking (an empty reply is permitted).
        blocking: bool,
        /// Number of outstanding identifiers to acknowledge.
        ack: u16,
        /// Maximum number of new identifiers to request.
        req: u16,
    },
    /// `[1, [*[sigId, size]]]` — client replies with signature
    /// identifiers. `StTxIds → StIdle`.
    MsgReplyTxIds {
        /// The signature identifiers and their sizes.
        sig_ids: Vec<SigIdAndSize>,
    },
    /// `[2, [*sigId]]` — server requests specific signatures by id.
    /// `StIdle → StTxs`.
    MsgRequestTxs {
        /// Signature identifiers to fetch.
        sig_ids: Vec<SigId>,
    },
    /// `[3, [*sig]]` — client replies with the requested signatures.
    /// `StTxs → StIdle`.
    MsgReplyTxs {
        /// The requested signatures (an invalid one may be omitted).
        sigs: Vec<Sig>,
    },
    /// `[4]` — client terminates the protocol (only from a blocking
    /// `StTxIds`). `StTxIds(blocking) → StDone`.
    MsgDone,
}

impl SigSubmissionMessage {
    /// The CBOR message-envelope tag — byte-identical to
    /// `crates/network`'s `TxSubmissionMessage::wire_tag`.
    pub fn wire_tag(&self) -> u8 {
        match self {
            SigSubmissionMessage::MsgRequestTxIds { .. } => 0,
            SigSubmissionMessage::MsgReplyTxIds { .. } => 1,
            SigSubmissionMessage::MsgRequestTxs { .. } => 2,
            SigSubmissionMessage::MsgReplyTxs { .. } => 3,
            SigSubmissionMessage::MsgDone => 4,
            SigSubmissionMessage::MsgInit => 6,
        }
    }

    /// Human-readable tag name, used in transition-error messages.
    pub fn tag_name(&self) -> &'static str {
        match self {
            SigSubmissionMessage::MsgInit => "MsgInit",
            SigSubmissionMessage::MsgRequestTxIds { .. } => "MsgRequestTxIds",
            SigSubmissionMessage::MsgReplyTxIds { .. } => "MsgReplyTxIds",
            SigSubmissionMessage::MsgRequestTxs { .. } => "MsgRequestTxs",
            SigSubmissionMessage::MsgReplyTxs { .. } => "MsgReplyTxs",
            SigSubmissionMessage::MsgDone => "MsgDone",
        }
    }

    /// Encode this message to CBOR.
    ///
    /// Wire format — byte-identical to `crates/network`'s
    /// `TxSubmissionMessage::to_cbor` for the message envelope, with
    /// `SigId` / `Sig` payloads (mirror of upstream
    /// `encodeTxSubmission2`):
    /// - `MsgInit`         is `[6]`
    /// - `MsgRequestTxIds` is `[0, blocking, ack, req]`
    /// - `MsgReplyTxIds`   is `[1, [[sigId, size], ...]]`
    /// - `MsgRequestTxs`   is `[2, [sigId, ...]]`
    /// - `MsgReplyTxs`     is `[3, [sig, ...]]`
    /// - `MsgDone`         is `[4]`
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            SigSubmissionMessage::MsgInit => {
                enc.array(1).unsigned(6);
            }
            SigSubmissionMessage::MsgRequestTxIds { blocking, ack, req } => {
                enc.array(4)
                    .unsigned(0)
                    .bool(*blocking)
                    .unsigned(u64::from(*ack))
                    .unsigned(u64::from(*req));
            }
            SigSubmissionMessage::MsgReplyTxIds { sig_ids } => {
                enc.array(2).unsigned(1);
                enc.array(sig_ids.len() as u64);
                for item in sig_ids {
                    enc.array(2);
                    encode_sig_id(&item.sig_id, &mut enc);
                    enc.unsigned(u64::from(item.size));
                }
            }
            SigSubmissionMessage::MsgRequestTxs { sig_ids } => {
                enc.array(2).unsigned(2);
                enc.array(sig_ids.len() as u64);
                for sig_id in sig_ids {
                    encode_sig_id(sig_id, &mut enc);
                }
            }
            SigSubmissionMessage::MsgReplyTxs { sigs } => {
                enc.array(2).unsigned(3);
                enc.array(sigs.len() as u64);
                for sig in sigs {
                    encode_sig(sig, &mut enc);
                }
            }
            SigSubmissionMessage::MsgDone => {
                enc.array(1).unsigned(4);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    ///
    /// Inverse of [`Self::to_cbor`]; rejects an unknown tag, a
    /// wrong-arity envelope, or trailing bytes.
    pub fn from_cbor(data: &[u8]) -> Result<SigSubmissionMessage, LedgerError> {
        let mut dec = Decoder::new(data);
        let arr_len = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, arr_len) {
            (6, 1) => SigSubmissionMessage::MsgInit,
            (0, 4) => {
                let blocking = dec.bool()?;
                let ack = dec.unsigned()? as u16;
                let req = dec.unsigned()? as u16;
                SigSubmissionMessage::MsgRequestTxIds { blocking, ack, req }
            }
            (1, 2) => {
                let count = dec.array()?;
                let mut sig_ids = vec_with_strict_capacity(count, SIG_SUBMISSION_LIST_MAX)?;
                for _ in 0..count {
                    let inner = dec.array()?;
                    if inner != 2 {
                        return Err(LedgerError::CborInvalidLength {
                            expected: 2,
                            actual: inner as usize,
                        });
                    }
                    let sig_id = decode_sig_id(&mut dec)?;
                    let size = dec.unsigned()? as u32;
                    sig_ids.push(SigIdAndSize { sig_id, size });
                }
                SigSubmissionMessage::MsgReplyTxIds { sig_ids }
            }
            (2, 2) => {
                let count = dec.array()?;
                let mut sig_ids = vec_with_strict_capacity(count, SIG_SUBMISSION_LIST_MAX)?;
                for _ in 0..count {
                    sig_ids.push(decode_sig_id(&mut dec)?);
                }
                SigSubmissionMessage::MsgRequestTxs { sig_ids }
            }
            (3, 2) => {
                let count = dec.array()?;
                let mut sigs = vec_with_strict_capacity(count, SIG_SUBMISSION_LIST_MAX)?;
                for _ in 0..count {
                    let raw = dec.bytes_owned()?;
                    sigs.push(decode_sig(&raw)?);
                }
                SigSubmissionMessage::MsgReplyTxs { sigs }
            }
            (4, 1) => SigSubmissionMessage::MsgDone,
            _ => {
                return Err(LedgerError::CborTypeMismatch {
                    expected: 0,
                    actual: tag as u8,
                });
            }
        };
        if !dec.is_empty() {
            return Err(LedgerError::CborTrailingBytes(dec.remaining()));
        }
        Ok(msg)
    }
}

/// Anti-DoS pre-allocation cap for `SigSubmissionMessage` list
/// decoding — far above any legitimate message. The protocol-level
/// in-flight limits (upstream `Policy.hs`) are enforced separately by
/// the inbound governor.
const SIG_SUBMISSION_LIST_MAX: usize = 4_096;

/// `shortWait` — upstream `Ouroboros.Network.Protocol.Limits.shortWait`
/// (`Just 10`).
const SHORT_WAIT: Option<Duration> = Some(Duration::from_secs(10));

/// `smallByteLimit` — upstream
/// `Ouroboros.Network.Protocol.Limits.smallByteLimit` (`0xffff`).
const SMALL_BYTE_LIMIT: u64 = 0xffff;

/// An illegal `SigSubmission` state transition.
///
/// Mirror of `crates/network`'s `TxSubmissionTransitionError`.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("illegal SigSubmission transition: {message} not allowed in {state:?}")]
pub struct SigSubmissionTransitionError {
    /// The state the message arrived in.
    pub state: SigSubmissionState,
    /// The offending message's tag name.
    pub message: &'static str,
}

impl SigSubmissionState {
    /// The next state after an incoming message, or an error if the
    /// transition is illegal.
    ///
    /// Mirror of `crates/network`'s `TxSubmissionState::transition` —
    /// the `SigSubmission` protocol is `TxSubmission2`, so the
    /// transition table is identical.
    pub fn transition(
        self,
        msg: &SigSubmissionMessage,
    ) -> Result<SigSubmissionState, SigSubmissionTransitionError> {
        match (self, msg) {
            // Client agency — StInit.
            (SigSubmissionState::StInit, SigSubmissionMessage::MsgInit) => {
                Ok(SigSubmissionState::StIdle)
            }
            // Server agency — StIdle.
            (
                SigSubmissionState::StIdle,
                SigSubmissionMessage::MsgRequestTxIds { blocking, .. },
            ) => Ok(SigSubmissionState::StTxIds {
                blocking: *blocking,
            }),
            (SigSubmissionState::StIdle, SigSubmissionMessage::MsgRequestTxs { .. }) => {
                Ok(SigSubmissionState::StTxs)
            }
            // Client agency — StTxIds.
            (SigSubmissionState::StTxIds { .. }, SigSubmissionMessage::MsgReplyTxIds { .. }) => {
                Ok(SigSubmissionState::StIdle)
            }
            // MsgDone only from a blocking StTxIds.
            (SigSubmissionState::StTxIds { blocking: true }, SigSubmissionMessage::MsgDone) => {
                Ok(SigSubmissionState::StDone)
            }
            // Client agency — StTxs.
            (SigSubmissionState::StTxs, SigSubmissionMessage::MsgReplyTxs { .. }) => {
                Ok(SigSubmissionState::StIdle)
            }
            (state, msg) => Err(SigSubmissionTransitionError {
                state,
                message: msg.tag_name(),
            }),
        }
    }

    /// The inactivity timeout for this protocol state — `None` is
    /// upstream `waitForever`.
    ///
    /// Mirror of upstream `Codec.hs::timeLimitsSigSubmission`:
    /// `StInit` / `StIdle` / blocking `StTxIds` wait forever;
    /// non-blocking `StTxIds` and `StTxs` use `shortWait`. The
    /// terminal `StDone` has no active timeout.
    pub fn time_limit(self) -> Option<Duration> {
        match self {
            SigSubmissionState::StInit
            | SigSubmissionState::StIdle
            | SigSubmissionState::StTxIds { blocking: true }
            | SigSubmissionState::StDone => None,
            SigSubmissionState::StTxIds { blocking: false } | SigSubmissionState::StTxs => {
                SHORT_WAIT
            }
        }
    }

    /// The maximum inbound-message size for this protocol state.
    ///
    /// Mirror of upstream `Codec.hs::byteLimitsSigSubmission` —
    /// `smallByteLimit` for every state.
    pub fn byte_limit(self) -> u64 {
        SMALL_BYTE_LIMIT
    }
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
    fn sig_validation_error_to_json_matches_upstream_shapes() {
        assert_eq!(
            SigValidationError::SigDuplicate.to_json(),
            serde_json::json!("duplicate")
        );
        assert_eq!(
            SigValidationError::SigExpired.to_json(),
            serde_json::json!("expired")
        );
        assert_eq!(
            SigValidationError::SigResultOther("boom".to_string()).to_json(),
            serde_json::json!({ "type": "other", "reason": "boom" })
        );
        // Every other variant → {"type":"invalid","reason":<rendered>}.
        let invalid = SigValidationError::ClockSkew.to_json();
        assert_eq!(invalid["type"], serde_json::json!("invalid"));
        assert_eq!(invalid["reason"], serde_json::json!("ClockSkew"));
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

    #[test]
    fn encode_sig_id_produces_cbor_byte_string() {
        let mut enc = Encoder::new();
        encode_sig_id(&SigId(SigHash(vec![0xAA, 0xBB])), &mut enc);
        // CBOR major type 2, length 2 → 0x42, then the raw bytes.
        assert_eq!(enc.into_bytes(), vec![0x42, 0xAA, 0xBB]);
    }

    #[test]
    fn sig_id_codec_round_trips() {
        let original = SigId(SigHash(vec![0x01, 0x02, 0x03, 0x04]));
        let mut enc = Encoder::new();
        encode_sig_id(&original, &mut enc);
        let encoded = enc.into_bytes();
        let mut dec = Decoder::new(&encoded);
        let decoded = decode_sig_id(&mut dec).expect("decodes");
        assert_eq!(decoded, original);
    }

    #[test]
    fn sig_op_certificate_codec_round_trips() {
        let cert = SigOpCertificate(OpCert {
            hot_vkey: SumKesVerificationKey([0x33; 32]),
            sequence_number: 42,
            kes_period: 7,
            sigma: Signature([0x44; 64]),
        });
        let mut enc = Encoder::new();
        encode_sig_op_certificate(&cert, &mut enc);
        let encoded = enc.into_bytes();
        // A CBOR definite-length array of 4 elements.
        assert_eq!(encoded[0], 0x84);
        let mut dec = Decoder::new(&encoded);
        let decoded = decode_sig_op_certificate(&mut dec).expect("decodes");
        assert_eq!(decoded, cert);
    }

    #[test]
    fn sig_raw_codec_round_trips() {
        let raw = sample_sig_raw();
        let mut enc = Encoder::new();
        encode_sig_raw(&raw, &mut enc);
        let encoded = enc.into_bytes();
        // Outer CBOR definite-length array of 4.
        assert_eq!(encoded[0], 0x84);
        let mut dec = Decoder::new(&encoded);
        let decoded = decode_sig_raw(&mut dec).expect("decodes");
        assert_eq!(decoded, raw);
    }

    #[test]
    fn encode_sig_emits_cached_sig_raw_bytes_as_byte_string() {
        let sig = Sig {
            sig_raw_bytes: vec![0x01, 0x02, 0x03],
            sig_raw_with_signed_bytes: SigRawWithSignedBytes {
                sig_raw_signed_bytes: vec![0xDE],
                sig_raw: sample_sig_raw(),
            },
        };
        let mut enc = Encoder::new();
        encode_sig(&sig, &mut enc);
        // CBOR byte string of length 3 → 0x43, then the cached bytes.
        assert_eq!(enc.into_bytes(), vec![0x43, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn sig_submission_message_wire_tags_match_tx_submission2() {
        // The envelope tags must equal crates/network's
        // TxSubmissionMessage tags (6/0/1/2/3/4).
        assert_eq!(SigSubmissionMessage::MsgInit.wire_tag(), 6);
        assert_eq!(
            SigSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: 1,
            }
            .wire_tag(),
            0
        );
        assert_eq!(
            SigSubmissionMessage::MsgReplyTxIds { sig_ids: vec![] }.wire_tag(),
            1
        );
        assert_eq!(
            SigSubmissionMessage::MsgRequestTxs { sig_ids: vec![] }.wire_tag(),
            2
        );
        assert_eq!(
            SigSubmissionMessage::MsgReplyTxs { sigs: vec![] }.wire_tag(),
            3
        );
        assert_eq!(SigSubmissionMessage::MsgDone.wire_tag(), 4);
    }

    #[test]
    fn sig_submission_transition_follows_the_protocol() {
        // The legal happy-path walk: Init → Idle → TxIds → Idle → Txs
        // → Idle, then a blocking TxIds → Done.
        let s = SigSubmissionState::StInit
            .transition(&SigSubmissionMessage::MsgInit)
            .expect("init");
        assert_eq!(s, SigSubmissionState::StIdle);
        let s = s
            .transition(&SigSubmissionMessage::MsgRequestTxIds {
                blocking: false,
                ack: 0,
                req: 2,
            })
            .expect("request ids");
        assert_eq!(s, SigSubmissionState::StTxIds { blocking: false });
        let s = s
            .transition(&SigSubmissionMessage::MsgReplyTxIds { sig_ids: vec![] })
            .expect("reply ids");
        assert_eq!(s, SigSubmissionState::StIdle);
        let s = s
            .transition(&SigSubmissionMessage::MsgRequestTxs { sig_ids: vec![] })
            .expect("request txs");
        assert_eq!(s, SigSubmissionState::StTxs);
        let s = s
            .transition(&SigSubmissionMessage::MsgReplyTxs { sigs: vec![] })
            .expect("reply txs");
        assert_eq!(s, SigSubmissionState::StIdle);
        // MsgDone is legal only from a blocking StTxIds.
        let done = SigSubmissionState::StTxIds { blocking: true }
            .transition(&SigSubmissionMessage::MsgDone)
            .expect("done");
        assert_eq!(done, SigSubmissionState::StDone);
    }

    #[test]
    fn sig_submission_transition_rejects_illegal_messages() {
        // MsgDone from a non-blocking StTxIds is illegal.
        let err = SigSubmissionState::StTxIds { blocking: false }
            .transition(&SigSubmissionMessage::MsgDone)
            .expect_err("rejects");
        assert_eq!(err.message, "MsgDone");
        assert_eq!(err.state, SigSubmissionState::StTxIds { blocking: false });
        // A reply in StIdle is illegal.
        assert!(
            SigSubmissionState::StIdle
                .transition(&SigSubmissionMessage::MsgReplyTxs { sigs: vec![] })
                .is_err()
        );
    }

    #[test]
    fn sig_submission_state_and_message_construct() {
        assert_eq!(
            SigSubmissionState::StTxIds { blocking: true },
            SigSubmissionState::StTxIds { blocking: true }
        );
        assert_ne!(SigSubmissionState::StIdle, SigSubmissionState::StDone);
        let reply = SigSubmissionMessage::MsgReplyTxIds {
            sig_ids: vec![SigIdAndSize {
                sig_id: SigId(SigHash(vec![0x01])),
                size: 42,
            }],
        };
        match reply {
            SigSubmissionMessage::MsgReplyTxIds { sig_ids } => {
                assert_eq!(sig_ids.len(), 1);
                assert_eq!(sig_ids[0].size, 42);
            }
            _ => panic!("wrong variant"),
        }
    }

    fn sample_sig() -> Sig {
        let raw = sample_sig_raw();
        let mut enc = Encoder::new();
        encode_sig_raw(&raw, &mut enc);
        decode_sig(&enc.into_bytes()).expect("sample sig")
    }

    #[test]
    fn sig_submission_message_codec_round_trips() {
        let sig_id = SigId(SigHash(vec![0xAB, 0xCD]));
        let messages = vec![
            SigSubmissionMessage::MsgInit,
            SigSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 3,
                req: 7,
            },
            SigSubmissionMessage::MsgReplyTxIds {
                sig_ids: vec![SigIdAndSize {
                    sig_id: sig_id.clone(),
                    size: 99,
                }],
            },
            SigSubmissionMessage::MsgRequestTxs {
                sig_ids: vec![sig_id.clone()],
            },
            SigSubmissionMessage::MsgReplyTxs {
                sigs: vec![sample_sig()],
            },
            SigSubmissionMessage::MsgDone,
        ];
        for msg in messages {
            let encoded = msg.to_cbor();
            let decoded = SigSubmissionMessage::from_cbor(&encoded).expect("decodes");
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn sig_submission_message_envelope_bytes() {
        // `[6]` and `[4]` — a CBOR array of one unsigned integer.
        assert_eq!(SigSubmissionMessage::MsgInit.to_cbor(), vec![0x81, 0x06]);
        assert_eq!(SigSubmissionMessage::MsgDone.to_cbor(), vec![0x81, 0x04]);
    }

    #[test]
    fn from_cbor_rejects_unknown_tag() {
        let mut enc = Encoder::new();
        enc.array(1).unsigned(99);
        let err = SigSubmissionMessage::from_cbor(&enc.into_bytes()).expect_err("rejects");
        assert!(
            matches!(err, LedgerError::CborTypeMismatch { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn sig_submission_time_limits_match_upstream() {
        // waitForever (None) for Init / Idle / blocking TxIds / Done.
        assert_eq!(SigSubmissionState::StInit.time_limit(), None);
        assert_eq!(SigSubmissionState::StIdle.time_limit(), None);
        assert_eq!(
            SigSubmissionState::StTxIds { blocking: true }.time_limit(),
            None
        );
        assert_eq!(SigSubmissionState::StDone.time_limit(), None);
        // shortWait (10s) for non-blocking TxIds and Txs.
        let short = Some(std::time::Duration::from_secs(10));
        assert_eq!(
            SigSubmissionState::StTxIds { blocking: false }.time_limit(),
            short
        );
        assert_eq!(SigSubmissionState::StTxs.time_limit(), short);
    }

    #[test]
    fn sig_submission_byte_limit_is_small_byte_limit() {
        for state in [
            SigSubmissionState::StInit,
            SigSubmissionState::StIdle,
            SigSubmissionState::StTxIds { blocking: true },
            SigSubmissionState::StTxs,
            SigSubmissionState::StDone,
        ] {
            assert_eq!(state.byte_limit(), 0xffff);
        }
    }

    #[test]
    fn validate_ocert_counter_accepts_and_records() {
        use crate::diffusion::PoolId;
        let pool = PoolId([0x07; 28]);
        let mut ctx = PoolValidationCtx::default();
        // First sighting of a pool — accepted and recorded.
        assert!(validate_ocert_counter(&mut ctx, &pool, 5).is_ok());
        assert_eq!(ctx.ocert_map.get(&pool).copied(), Some(5));
        // A non-decreasing counter is accepted and updates the map.
        assert!(validate_ocert_counter(&mut ctx, &pool, 5).is_ok());
        assert!(validate_ocert_counter(&mut ctx, &pool, 9).is_ok());
        assert_eq!(ctx.ocert_map.get(&pool).copied(), Some(9));
    }

    #[test]
    fn validate_ocert_counter_rejects_a_regression() {
        use crate::diffusion::PoolId;
        let pool = PoolId([0x07; 28]);
        let mut ctx = PoolValidationCtx::default();
        validate_ocert_counter(&mut ctx, &pool, 9).expect("first");
        let err = validate_ocert_counter(&mut ctx, &pool, 4).expect_err("regresses");
        assert_eq!(
            err,
            SigValidationError::InvalidOcertCounter {
                last_seen: 9,
                received: 4,
            }
        );
        // The rejected counter must not overwrite the recorded value.
        assert_eq!(ctx.ocert_map.get(&pool).copied(), Some(9));
    }

    #[test]
    fn validate_pool_eligibility_unknown_pool() {
        use crate::diffusion::PoolId;
        let pool = PoolId([0x01; 28]);
        // No epoch yet → NotInitialized.
        assert_eq!(
            validate_pool_eligibility(&PoolValidationCtx::default(), &pool, 100),
            Err(SigValidationError::NotInitialized)
        );
        // Epoch set but the pool is absent → UnrecognizedPool.
        let ctx = PoolValidationCtx {
            epoch: Some(100),
            ..PoolValidationCtx::default()
        };
        assert_eq!(
            validate_pool_eligibility(&ctx, &pool, 100),
            Err(SigValidationError::UnrecognizedPool)
        );
    }

    #[test]
    fn validate_pool_eligibility_set_snapshot() {
        use crate::diffusion::{PoolId, StakeSnapshot};
        let pool = PoolId([0x02; 28]);
        let ctx = |mark: u64| {
            let mut c = PoolValidationCtx {
                epoch: Some(1_000),
                ..PoolValidationCtx::default()
            };
            c.stake_map.insert(
                pool.clone(),
                StakeSnapshot {
                    mark_pool: mark,
                    set_pool: 50,
                    go_pool: 0,
                },
            );
            c
        };
        // set != 0, now within [.., nextEpoch + skew] → eligible.
        assert!(validate_pool_eligibility(&ctx(10), &pool, 1_003).is_ok());
        // now past the window, mark zero → SigExpired.
        assert_eq!(
            validate_pool_eligibility(&ctx(0), &pool, 2_000),
            Err(SigValidationError::SigExpired)
        );
        // now past the window, mark non-zero → ClockSkew.
        assert_eq!(
            validate_pool_eligibility(&ctx(10), &pool, 2_000),
            Err(SigValidationError::ClockSkew)
        );
    }

    #[test]
    fn validate_pool_eligibility_mark_snapshot_and_zero() {
        use crate::diffusion::{PoolId, StakeSnapshot};
        let pool = PoolId([0x03; 28]);
        let with = |mark: u64, set: u64| {
            let mut c = PoolValidationCtx {
                epoch: Some(1_000),
                ..PoolValidationCtx::default()
            };
            c.stake_map.insert(
                pool.clone(),
                StakeSnapshot {
                    mark_pool: mark,
                    set_pool: set,
                    go_pool: 0,
                },
            );
            c
        };
        // set == 0, mark != 0, within skew of the epoch → eligible.
        assert!(validate_pool_eligibility(&with(10, 0), &pool, 1_002).is_ok());
        // epoch is well ahead of now → PoolNotEligible.
        assert_eq!(
            validate_pool_eligibility(&with(10, 0), &pool, 100),
            Err(SigValidationError::PoolNotEligible)
        );
        // now well past the epoch → ClockSkew.
        assert_eq!(
            validate_pool_eligibility(&with(10, 0), &pool, 5_000),
            Err(SigValidationError::ClockSkew)
        );
        // set == 0 and mark == 0 → SigExpired.
        assert_eq!(
            validate_pool_eligibility(&with(0, 0), &pool, 1_000),
            Err(SigValidationError::SigExpired)
        );
    }

    #[test]
    fn validate_kes_period_accepts_in_window() {
        // Window [5, 69): 5 (start, inclusive), 10, 68 (end-1) all pass.
        assert!(validate_kes_period(5, 5, 64).is_ok());
        assert!(validate_kes_period(10, 5, 64).is_ok());
        assert!(validate_kes_period(68, 5, 64).is_ok());
    }

    #[test]
    fn validate_kes_period_rejects_before_start_and_after_end() {
        // Before the opcert's start period.
        assert_eq!(
            validate_kes_period(3, 5, 64),
            Err(SigValidationError::KesBeforeStartOcert {
                kes_period: 3,
                start_period: 5,
            })
        );
        // At/after the window end (5 + 64 = 69, exclusive).
        assert_eq!(
            validate_kes_period(69, 5, 64),
            Err(SigValidationError::KesAfterEndOcert {
                kes_period: 69,
                end_period: 69,
            })
        );
        assert_eq!(
            validate_kes_period(70, 5, 64),
            Err(SigValidationError::KesAfterEndOcert {
                kes_period: 70,
                end_period: 69,
            })
        );
    }

    #[test]
    fn decode_sig_captures_signed_payload_bytes() {
        let raw = sample_sig_raw();
        let mut enc = Encoder::new();
        encode_sig_raw(&raw, &mut enc);
        let encoded = enc.into_bytes();

        let sig = decode_sig(&encoded).expect("decodes");
        // The decoded SigRaw round-trips.
        assert_eq!(sig.sig_raw_with_signed_bytes.sig_raw, raw);
        // `sigBytes` is the full encoded message.
        assert_eq!(sig.sig_bytes(), encoded.as_slice());

        // `sigSignedBytes` is exactly the payload sub-array: re-decoding
        // it as a payload yields the original fields and consumes every
        // byte (no trailing data, nothing missing).
        let signed = sig.sig_signed_bytes().to_vec();
        let mut dec = Decoder::new(&signed);
        let (id, body, kp, exp) = decode_sig_payload(&mut dec).expect("payload decodes");
        assert_eq!(id, raw.sig_raw_id);
        assert_eq!(body, raw.sig_raw_body);
        assert_eq!(kp, raw.sig_raw_kes_period);
        assert_eq!(exp, raw.sig_raw_expires_at);
        assert_eq!(
            dec.remaining(),
            0,
            "signed bytes must be exactly the payload"
        );
    }

    #[test]
    fn decode_sig_op_certificate_rejects_wrong_list_length() {
        // A 3-element CBOR array — not a valid SigOpCertificate.
        let mut enc = Encoder::new();
        enc.array(3);
        enc.unsigned(1);
        enc.unsigned(2);
        enc.unsigned(3);
        let encoded = enc.into_bytes();
        let mut dec = Decoder::new(&encoded);
        let err = decode_sig_op_certificate(&mut dec).expect_err("rejects");
        assert!(
            matches!(
                err,
                LedgerError::CborInvalidLength {
                    expected: 4,
                    actual: 3
                }
            ),
            "unexpected error: {err:?}"
        );
    }
}
