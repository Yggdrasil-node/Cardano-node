//! Block producer credentials, text-envelope parsing, and leader check.
//!
//! This module implements the block-production side of the Praos protocol:
//! loading signing credentials from standard text-envelope files, checking
//! slot leadership using VRF proof generation, and forging block headers
//! signed with KES.
//!
//! ## Upstream Reference
//!
//! - `Cardano.Node.Run` — node startup and credential loading.
//! - `Ouroboros.Consensus.Protocol.Praos` — `forgePraosFields`,
//!   `PraosCanBeLeader`, `PraosIsLeader`, `praosCheckCanForge`.
//! - `Ouroboros.Consensus.Protocol.Ledger.HotKey` — evolving KES key
//!   management.

use std::path::Path;
use std::time::{Duration, Instant};

use yggdrasil_consensus::{
    ActiveSlotCoeff, HeaderBody, OpCert, VrfMode, check_is_leader, check_kes_period,
    kes_period_of_slot,
};
use yggdrasil_crypto::ed25519::{Signature, VerificationKey};
use yggdrasil_crypto::sum_kes::{
    SumKesSignature, SumKesSigningKey, SumKesVerificationKey, gen_sum_kes_signing_key,
    sign_sum_kes, update_sum_kes,
};
use yggdrasil_crypto::vrf::{VRF_SIGNING_KEY_SIZE, VrfOutput, VrfSecretKey, VrfVerificationKey};
use yggdrasil_ledger::{
    BlockNo, CborEncode, Encoder, Era, HeaderHash, Nonce, PraosHeader, PraosHeaderBody,
    ShelleyOpCert, ShelleyVrfCert, SlotNo, TxId,
};
use yggdrasil_mempool::MempoolEntry;

use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors related to block producer credential loading and usage.
#[derive(Debug, Error)]
pub enum BlockProducerError {
    /// A file could not be read.
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// Text envelope JSON parsing failed.
    #[error("invalid text envelope in {path}: {source}")]
    TextEnvelope {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    /// Hex decoding of the `cborHex` field failed.
    #[error("invalid hex in text envelope {path}: {source}")]
    Hex {
        path: String,
        #[source]
        source: hex::FromHexError,
    },
    /// The decoded CBOR payload has unexpected length.
    #[error("unexpected payload length in {path}: expected {expected}, got {actual}")]
    PayloadLength {
        path: String,
        expected: usize,
        actual: usize,
    },
    /// CBOR decoding of the operational certificate failed.
    #[error("failed to decode operational certificate from {path}: {detail}")]
    OpCertDecode { path: String, detail: String },
    /// The text envelope type tag does not match what was expected.
    #[error("unexpected text envelope type in {path}: expected {expected}, got {actual}")]
    TypeMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    /// Cryptographic error during signing or verification.
    #[error("crypto error: {0}")]
    Crypto(String),
    /// KES period check failed.
    #[error("KES period check failed: {0}")]
    KesPeriod(String),
    /// The node is not a leader for the requested slot.
    #[error("not elected leader for slot {0}")]
    NotLeader(u64),
    /// A secret-key file is readable by group or world.  Refuse to load
    /// it; cold/KES/VRF secrets must be `chmod 0400` (owner-read-only).
    /// Audit finding L-6.
    #[error(
        "insecure permissions on secret key file {path}: mode 0o{mode:04o} grants group/world access \
         (run `chmod 0400 {path}`)"
    )]
    InsecureKeyFileMode { path: String, mode: u32 },
}

// ---------------------------------------------------------------------------
// Text envelope serde type
// ---------------------------------------------------------------------------

/// Standard Cardano text-envelope format used for signing key and
/// certificate files.
///
/// Reference: `Cardano.Api.SerialiseTextEnvelope`.
#[derive(serde::Deserialize)]
struct TextEnvelope {
    #[serde(rename = "type")]
    type_tag: String,
    #[allow(dead_code)]
    description: String,
    #[serde(rename = "cborHex")]
    cbor_hex: String,
}

/// Parse a text envelope file from disk and return the decoded CBOR bytes.
fn read_text_envelope(path: &Path) -> Result<(String, Vec<u8>), BlockProducerError> {
    let contents = std::fs::read_to_string(path).map_err(|source| BlockProducerError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let envelope: TextEnvelope =
        serde_json::from_str(&contents).map_err(|source| BlockProducerError::TextEnvelope {
            path: path.display().to_string(),
            source,
        })?;
    let raw = hex::decode(&envelope.cbor_hex).map_err(|source| BlockProducerError::Hex {
        path: path.display().to_string(),
        source,
    })?;
    Ok((envelope.type_tag, raw))
}

/// Reject the secret-key file when group or world bits are set in its
/// mode. Mirrors the discipline upstream `cardano-cli` and the SPO
/// runbook documented in the Cardano Operations Book apply: cold, VRF
/// and KES signing keys must be `0o400`. Symbolic links to the secret
/// are also rejected (`metadata` follows the link, but the rejection
/// message names the configured path so the operator can audit it).
///
/// This is a no-op on non-Unix targets.  Audit finding L-6.
#[cfg(unix)]
fn ensure_secret_file_mode(path: &Path) -> Result<(), BlockProducerError> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).map_err(|source| BlockProducerError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mode = meta.mode() & 0o7777;
    if mode & 0o077 != 0 {
        return Err(BlockProducerError::InsecureKeyFileMode {
            path: path.display().to_string(),
            mode,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_secret_file_mode(_path: &Path) -> Result<(), BlockProducerError> {
    Ok(())
}

// ---------------------------------------------------------------------------
// VRF key loading
// ---------------------------------------------------------------------------

/// Expected text-envelope type tags for VRF signing keys.
const VRF_SIGNING_KEY_TYPE: &str = "VrfSigningKey_PraosVRF";

/// Load a VRF signing key from a text-envelope file.
///
/// The CBOR payload is expected to be a CBOR `bytes` item containing
/// the 64-byte VRF secret key (32-byte seed ‖ 32-byte verification key).
pub fn load_vrf_signing_key(path: &Path) -> Result<VrfSecretKey, BlockProducerError> {
    ensure_secret_file_mode(path)?;
    let (type_tag, cbor_bytes) = read_text_envelope(path)?;
    if type_tag != VRF_SIGNING_KEY_TYPE {
        return Err(BlockProducerError::TypeMismatch {
            path: path.display().to_string(),
            expected: VRF_SIGNING_KEY_TYPE.to_owned(),
            actual: type_tag,
        });
    }

    // The CBOR payload is `bstr .size 64`, which means the raw CBOR has a
    // 2-byte header (0x5840) followed by 64 bytes. Strip CBOR wrapper.
    let key_bytes = unwrap_cbor_bytes(&cbor_bytes, path)?;
    if key_bytes.len() != VRF_SIGNING_KEY_SIZE {
        return Err(BlockProducerError::PayloadLength {
            path: path.display().to_string(),
            expected: VRF_SIGNING_KEY_SIZE,
            actual: key_bytes.len(),
        });
    }
    let mut arr = [0u8; VRF_SIGNING_KEY_SIZE];
    arr.copy_from_slice(key_bytes);
    Ok(VrfSecretKey::from_bytes(arr))
}

// ---------------------------------------------------------------------------
// KES key loading
// ---------------------------------------------------------------------------

/// Expected text-envelope type prefix for KES signing keys.
const KES_SIGNING_KEY_TYPE_PREFIX: &str = "KesSigningKey_ed25519_kes_2^";

/// Expected text-envelope type tag for stake-pool cold verification keys.
///
/// This key is the block issuer key carried in Shelley-family headers.
const STAKE_POOL_VERIFICATION_KEY_TYPE: &str = "StakePoolVerificationKey_ed25519";

/// Load a SumKES signing key from a text-envelope file.
///
/// The text-envelope type string encodes the depth:
/// `KesSigningKey_ed25519_kes_2^6` means depth 6 (64 periods, mainnet).
///
/// The CBOR payload is a `bstr` containing the 32-byte seed from which
/// the full SumKES key tree is generated.
pub fn load_kes_signing_key(path: &Path) -> Result<SumKesSigningKey, BlockProducerError> {
    ensure_secret_file_mode(path)?;
    let (type_tag, cbor_bytes) = read_text_envelope(path)?;
    if !type_tag.starts_with(KES_SIGNING_KEY_TYPE_PREFIX) {
        return Err(BlockProducerError::TypeMismatch {
            path: path.display().to_string(),
            expected: format!("{KES_SIGNING_KEY_TYPE_PREFIX}<depth>"),
            actual: type_tag,
        });
    }
    let depth_str = &type_tag[KES_SIGNING_KEY_TYPE_PREFIX.len()..];
    let depth: u32 = depth_str
        .parse()
        .map_err(|_| BlockProducerError::OpCertDecode {
            path: path.display().to_string(),
            detail: format!("cannot parse KES depth from type tag suffix '{depth_str}'"),
        })?;

    let seed_bytes = unwrap_cbor_bytes(&cbor_bytes, path)?;
    if seed_bytes.len() != 32 {
        return Err(BlockProducerError::PayloadLength {
            path: path.display().to_string(),
            expected: 32,
            actual: seed_bytes.len(),
        });
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(seed_bytes);
    gen_sum_kes_signing_key(&seed, depth).map_err(|e| BlockProducerError::Crypto(e.to_string()))
}

/// Load the stake-pool cold verification key used as header `issuer_vkey`.
///
/// The CBOR payload is expected to be a CBOR `bytes` item containing the
/// 32-byte Ed25519 verification key.
pub fn load_issuer_verification_key(path: &Path) -> Result<VerificationKey, BlockProducerError> {
    let (type_tag, cbor_bytes) = read_text_envelope(path)?;
    if type_tag != STAKE_POOL_VERIFICATION_KEY_TYPE {
        return Err(BlockProducerError::TypeMismatch {
            path: path.display().to_string(),
            expected: STAKE_POOL_VERIFICATION_KEY_TYPE.to_owned(),
            actual: type_tag,
        });
    }

    let key_bytes = unwrap_cbor_bytes(&cbor_bytes, path)?;
    if key_bytes.len() != 32 {
        return Err(BlockProducerError::PayloadLength {
            path: path.display().to_string(),
            expected: 32,
            actual: key_bytes.len(),
        });
    }

    let mut arr = [0u8; 32];
    arr.copy_from_slice(key_bytes);
    Ok(VerificationKey::from_bytes(arr))
}

// ---------------------------------------------------------------------------
// Operational certificate loading
// ---------------------------------------------------------------------------

/// Expected text-envelope type tag for operational certificates.
const OPCERT_TYPE: &str = "NodeOperationalCertificate";

/// Load an operational certificate from a text-envelope file.
///
/// The CBOR payload is an array:
/// `[hot_vkey_bytes, sequence_number, kes_period, cold_sig_bytes]`
///
/// Reference: `OCert` in `Cardano.Protocol.TPraos.OCert`.
pub fn load_operational_certificate(path: &Path) -> Result<OpCert, BlockProducerError> {
    let (type_tag, cbor_bytes) = read_text_envelope(path)?;
    if type_tag != OPCERT_TYPE {
        return Err(BlockProducerError::TypeMismatch {
            path: path.display().to_string(),
            expected: OPCERT_TYPE.to_owned(),
            actual: type_tag,
        });
    }
    decode_opcert_cbor(&cbor_bytes, path)
}

/// Decode an operational certificate from raw CBOR bytes.
///
/// Expected encoding:
/// ```text
/// [hot_vkey_bytes(32), sequence_number(uint), kes_period(uint), sigma_bytes(64)]
/// ```
fn decode_opcert_cbor(data: &[u8], path: &Path) -> Result<OpCert, BlockProducerError> {
    use std::io::Cursor;

    let mut cursor = Cursor::new(data);
    let err = |detail: &str| BlockProducerError::OpCertDecode {
        path: path.display().to_string(),
        detail: detail.to_owned(),
    };

    // Expect a CBOR array of length 4.
    let first = read_cbor_byte(&mut cursor).map_err(|_| err("truncated CBOR"))?;
    if first != 0x84 {
        return Err(err(&format!("expected CBOR array(4), got 0x{first:02x}")));
    }

    // 1) hot_vkey: CBOR bytes(32)
    let hot_vkey_raw =
        read_cbor_bytes_field(&mut cursor).map_err(|_| err("failed to read hot_vkey bytes"))?;
    if hot_vkey_raw.len() != 32 {
        return Err(err(&format!(
            "hot_vkey length {}, expected 32",
            hot_vkey_raw.len()
        )));
    }
    let mut hot_arr = [0u8; 32];
    hot_arr.copy_from_slice(&hot_vkey_raw);
    let hot_vkey = SumKesVerificationKey::from_bytes(hot_arr);

    // 2) sequence_number: CBOR uint
    let sequence_number =
        read_cbor_uint(&mut cursor).map_err(|_| err("failed to read sequence_number"))?;

    // 3) kes_period: CBOR uint
    let kes_period = read_cbor_uint(&mut cursor).map_err(|_| err("failed to read kes_period"))?;

    // 4) sigma: CBOR bytes(64)
    let sigma_raw =
        read_cbor_bytes_field(&mut cursor).map_err(|_| err("failed to read sigma bytes"))?;
    if sigma_raw.len() != 64 {
        return Err(err(&format!(
            "sigma length {}, expected 64",
            sigma_raw.len()
        )));
    }
    let mut sigma_arr = [0u8; 64];
    sigma_arr.copy_from_slice(&sigma_raw);
    let sigma = Signature(sigma_arr);

    Ok(OpCert {
        hot_vkey,
        sequence_number,
        kes_period,
        sigma,
    })
}

// ---------------------------------------------------------------------------
// Minimal CBOR reading helpers (no dependency on a full CBOR crate)
// ---------------------------------------------------------------------------

fn read_cbor_byte(cursor: &mut std::io::Cursor<&[u8]>) -> Result<u8, ()> {
    use std::io::Read;
    let mut buf = [0u8; 1];
    cursor.read_exact(&mut buf).map_err(|_| ())?;
    Ok(buf[0])
}

fn read_cbor_uint(cursor: &mut std::io::Cursor<&[u8]>) -> Result<u64, ()> {
    let initial = read_cbor_byte(cursor)?;
    let major = initial >> 5;
    if major != 0 {
        return Err(());
    }
    let additional = initial & 0x1f;
    cbor_read_uint_value(cursor, additional)
}

fn read_cbor_bytes_field(cursor: &mut std::io::Cursor<&[u8]>) -> Result<Vec<u8>, ()> {
    let initial = read_cbor_byte(cursor)?;
    let major = initial >> 5;
    if major != 2 {
        return Err(());
    }
    let additional = initial & 0x1f;
    let len = cbor_read_uint_value(cursor, additional)? as usize;
    let mut buf = vec![0u8; len];
    use std::io::Read;
    cursor.read_exact(&mut buf).map_err(|_| ())?;
    Ok(buf)
}

fn cbor_read_uint_value(cursor: &mut std::io::Cursor<&[u8]>, additional: u8) -> Result<u64, ()> {
    use std::io::Read;
    match additional {
        v @ 0..=23 => Ok(v as u64),
        24 => {
            let mut buf = [0u8; 1];
            cursor.read_exact(&mut buf).map_err(|_| ())?;
            Ok(buf[0] as u64)
        }
        25 => {
            let mut buf = [0u8; 2];
            cursor.read_exact(&mut buf).map_err(|_| ())?;
            Ok(u16::from_be_bytes(buf) as u64)
        }
        26 => {
            let mut buf = [0u8; 4];
            cursor.read_exact(&mut buf).map_err(|_| ())?;
            Ok(u32::from_be_bytes(buf) as u64)
        }
        27 => {
            let mut buf = [0u8; 8];
            cursor.read_exact(&mut buf).map_err(|_| ())?;
            Ok(u64::from_be_bytes(buf))
        }
        _ => Err(()),
    }
}

/// Unwrap a CBOR `bstr` wrapper, returning the inner bytes.
///
/// Handles both short-form (`40`..`57` + bytes) and long-form
/// (`58` len8 + bytes, `59` len16 ...) CBOR byte-string headers.
fn unwrap_cbor_bytes<'a>(data: &'a [u8], path: &Path) -> Result<&'a [u8], BlockProducerError> {
    if data.is_empty() {
        return Err(BlockProducerError::PayloadLength {
            path: path.display().to_string(),
            expected: 1,
            actual: 0,
        });
    }
    let initial = data[0];
    let major = initial >> 5;
    if major != 2 {
        return Err(BlockProducerError::OpCertDecode {
            path: path.display().to_string(),
            detail: format!("expected CBOR bstr (major 2), got major {major}"),
        });
    }
    let additional = initial & 0x1f;
    match additional {
        n @ 0..=23 => {
            let len = n as usize;
            if data.len() < 1 + len {
                return Err(BlockProducerError::PayloadLength {
                    path: path.display().to_string(),
                    expected: 1 + len,
                    actual: data.len(),
                });
            }
            Ok(&data[1..1 + len])
        }
        24 => {
            if data.len() < 2 {
                return Err(BlockProducerError::PayloadLength {
                    path: path.display().to_string(),
                    expected: 2,
                    actual: data.len(),
                });
            }
            let len = data[1] as usize;
            if data.len() < 2 + len {
                return Err(BlockProducerError::PayloadLength {
                    path: path.display().to_string(),
                    expected: 2 + len,
                    actual: data.len(),
                });
            }
            Ok(&data[2..2 + len])
        }
        25 => {
            if data.len() < 3 {
                return Err(BlockProducerError::PayloadLength {
                    path: path.display().to_string(),
                    expected: 3,
                    actual: data.len(),
                });
            }
            let len = u16::from_be_bytes([data[1], data[2]]) as usize;
            if data.len() < 3 + len {
                return Err(BlockProducerError::PayloadLength {
                    path: path.display().to_string(),
                    expected: 3 + len,
                    actual: data.len(),
                });
            }
            Ok(&data[3..3 + len])
        }
        _ => Err(BlockProducerError::OpCertDecode {
            path: path.display().to_string(),
            detail: format!("unsupported CBOR bstr additional info {additional}"),
        }),
    }
}

// ---------------------------------------------------------------------------
// Block producer credentials
// ---------------------------------------------------------------------------

/// Loaded and validated block producer credentials.
///
/// Contains the VRF signing key for slot lottery participation, the KES
/// signing key for header signing, and the operational certificate binding
/// the cold key to the hot KES key.
///
/// Reference: `PraosCanBeLeader` in
/// `Ouroboros.Consensus.Protocol.Praos`.
#[derive(Debug)]
pub struct BlockProducerCredentials {
    /// VRF signing key used for leader slot lottery.
    pub vrf_signing_key: VrfSecretKey,
    /// VRF verification key (derived from signing key).
    pub vrf_verification_key: VrfVerificationKey,
    /// SumKES signing key used to sign block headers.
    pub kes_signing_key: SumKesSigningKey,
    /// Current KES period offset from the operational certificate start.
    pub kes_current_period: u32,
    /// Operational certificate binding cold key to hot KES key.
    pub operational_cert: OpCert,
    /// Cold verification key used as block header `issuer_vkey`.
    pub issuer_vkey: VerificationKey,
    /// Slots per KES period (from config).
    pub slots_per_kes_period: u64,
    /// Maximum KES evolutions (from config).
    pub max_kes_evolutions: u64,
}

/// Load block producer credentials from text-envelope files.
///
/// Validates that the KES key depth is consistent with the operational
/// certificate and config parameters, and verifies that the operational
/// certificate signature matches the configured issuer cold verification key.
///
/// Reference: upstream node startup in `Cardano.Node.Run.handleSimpleNode`.
pub fn load_block_producer_credentials(
    kes_key_path: &Path,
    vrf_key_path: &Path,
    opcert_path: &Path,
    issuer_vkey_path: &Path,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<BlockProducerCredentials, BlockProducerError> {
    let vrf_signing_key = load_vrf_signing_key(vrf_key_path)?;
    let vrf_verification_key = vrf_signing_key.verification_key();
    let kes_signing_key = load_kes_signing_key(kes_key_path)?;
    let operational_cert = load_operational_certificate(opcert_path)?;
    let issuer_vkey = load_issuer_verification_key(issuer_vkey_path)?;

    operational_cert.verify(&issuer_vkey).map_err(|e| {
        BlockProducerError::Crypto(format!(
            "operational certificate does not verify against issuer key: {e}"
        ))
    })?;

    Ok(BlockProducerCredentials {
        vrf_signing_key,
        vrf_verification_key,
        kes_signing_key,
        kes_current_period: 0,
        operational_cert,
        issuer_vkey,
        slots_per_kes_period,
        max_kes_evolutions,
    })
}

// ---------------------------------------------------------------------------
// Leader check
// ---------------------------------------------------------------------------

/// Result of a successful leader election check.
///
/// Carries the VRF evidence that proves leadership, needed for block header
/// construction.
///
/// Reference: `PraosIsLeader` in
/// `Ouroboros.Consensus.Protocol.Praos`.
#[derive(Clone, Debug)]
pub struct LeaderElectionResult {
    /// VRF output hash used for both leadership proof and nonce contribution.
    pub vrf_output: VrfOutput,
    /// 80-byte VRF proof that can be verified by any node.
    pub vrf_proof: Vec<u8>,
}

/// Check whether the block producer's credentials win the slot lottery for
/// the given slot.
///
/// # Arguments
///
/// * `creds` — Block producer credentials.
/// * `slot` — Slot number to check.
/// * `epoch_nonce` — Current epoch nonce.
/// * `sigma_num` / `sigma_den` — Pool relative stake as a fraction.
/// * `active_slot_coeff` — Network active slot coefficient.
///
/// Returns `Ok(Some(result))` if elected leader, `Ok(None)` if not.
///
/// Reference: `checkIsLeader` in
/// `Ouroboros.Consensus.Protocol.Praos`.
pub fn check_slot_leadership(
    creds: &BlockProducerCredentials,
    slot: SlotNo,
    epoch_nonce: Nonce,
    sigma_num: u64,
    sigma_den: u64,
    active_slot_coeff: &ActiveSlotCoeff,
) -> Result<Option<LeaderElectionResult>, BlockProducerError> {
    let result = check_is_leader(
        &creds.vrf_signing_key,
        slot,
        epoch_nonce,
        sigma_num,
        sigma_den,
        active_slot_coeff,
        VrfMode::Praos, // Block production is always Praos-era (Babbage/Conway).
    )
    .map_err(|e| BlockProducerError::Crypto(e.to_string()))?;

    Ok(result.map(|(vrf_output, vrf_proof)| LeaderElectionResult {
        vrf_output,
        vrf_proof,
    }))
}

// ---------------------------------------------------------------------------
// KES key management
// ---------------------------------------------------------------------------

/// Check that the KES key is valid for the given slot and return the
/// relative period offset needed for signing.
///
/// Reference: `praosCheckCanForge` in
/// `Ouroboros.Consensus.Protocol.Praos`.
pub fn check_can_forge(
    creds: &BlockProducerCredentials,
    slot: SlotNo,
) -> Result<u32, BlockProducerError> {
    let current_kes_period = kes_period_of_slot(slot.0, creds.slots_per_kes_period)
        .map_err(|e| BlockProducerError::KesPeriod(e.to_string()))?;

    check_kes_period(
        &creds.operational_cert,
        current_kes_period,
        creds.max_kes_evolutions,
    )
    .map_err(|e| BlockProducerError::KesPeriod(e.to_string()))?;

    let offset = current_kes_period
        .checked_sub(creds.operational_cert.kes_period)
        .ok_or_else(|| {
            BlockProducerError::KesPeriod(format!(
                "current KES period {current_kes_period} < opcert start {}",
                creds.operational_cert.kes_period
            ))
        })?;

    u32::try_from(offset)
        .map_err(|_| BlockProducerError::KesPeriod("KES period offset overflows u32".to_owned()))
}

// ---------------------------------------------------------------------------
// Block forging
// ---------------------------------------------------------------------------

/// Configuration for block forging.
pub struct ForgeBlockConfig {
    /// Protocol version to embed in the header.
    pub protocol_version: (u64, u64),
}

/// A freshly forged block header, ready for network announcement.
///
/// Reference: `PraosFields` in
/// `Ouroboros.Consensus.Protocol.Praos`.
#[derive(Clone, Debug)]
pub struct ForgedBlockHeader {
    /// The complete header body including VRF evidence and opcert.
    pub header_body: HeaderBody,
    /// KES signature over the header body signable bytes.
    pub kes_signature: SumKesSignature,
}

/// Forge a block header from leadership evidence and block body metadata.
///
/// This implements the core `forgePraosFields` pipeline:
/// 1. Construct the `HeaderBody` (Praos 14-element format).
/// 2. Produce signable bytes from the header body.
/// 3. Sign with SumKES at the correct period offset.
///
/// # Arguments
///
/// * `creds` — Block producer credentials (VRF key, KES key, opcert).
/// * `election` — VRF leadership evidence from `check_slot_leadership`.
/// * `slot` — Slot being forged.
/// * `block_number` — New block height.
/// * `prev_hash` — Hash of the previous block header.
/// * `block_body_hash` — Blake2b-256 hash of the serialized block body.
/// * `block_body_size` — Byte size of the serialized block body.
/// * `config` — Protocol version and other forging parameters.
///
/// Reference: `forgePraosFields` in
/// `Ouroboros.Consensus.Protocol.Praos`.
#[allow(clippy::too_many_arguments)]
pub fn forge_block_header(
    creds: &BlockProducerCredentials,
    election: &LeaderElectionResult,
    slot: SlotNo,
    block_number: BlockNo,
    prev_hash: Option<HeaderHash>,
    block_body_hash: [u8; 32],
    block_body_size: u32,
    issuer_vkey: VerificationKey,
    config: &ForgeBlockConfig,
) -> Result<ForgedBlockHeader, BlockProducerError> {
    // Compute KES period offset for signing.
    let kes_offset = check_can_forge(creds, slot)?;

    // Build the Praos-era header body (14 elements — single VRF result,
    // no separate nonce VRF fields).
    let header_body = HeaderBody {
        block_number,
        slot,
        prev_hash,
        issuer_vkey,
        vrf_vkey: creds.vrf_verification_key,
        leader_vrf_output: election.vrf_output.0.to_vec(),
        leader_vrf_proof: {
            let mut proof = [0u8; 80];
            let len = election.vrf_proof.len().min(80);
            proof[..len].copy_from_slice(&election.vrf_proof[..len]);
            proof
        },
        nonce_vrf_output: None, // Praos era — single VRF result
        nonce_vrf_proof: None,
        block_body_size,
        block_body_hash,
        operational_cert: creds.operational_cert.clone(),
        protocol_version: config.protocol_version,
    };

    // Sign the header body with SumKES.
    let signable = header_body.to_signable_bytes();
    let kes_signature = sign_sum_kes(&creds.kes_signing_key, kes_offset, &signable)
        .map_err(|e| BlockProducerError::Crypto(e.to_string()))?;

    Ok(ForgedBlockHeader {
        header_body,
        kes_signature,
    })
}

/// Evolve the KES signing key to the next period.
///
/// Returns `true` if evolution succeeded, `false` if the key has reached
/// its final period.
///
/// Reference: `updateKES` / `HotKey.evolve` in upstream.
pub fn evolve_kes_key(creds: &mut BlockProducerCredentials) -> Result<bool, BlockProducerError> {
    let result = update_sum_kes(&creds.kes_signing_key, creds.kes_current_period)
        .map_err(|e| BlockProducerError::Crypto(e.to_string()))?;
    match result {
        Some(new_key) => {
            creds.kes_signing_key = new_key;
            creds.kes_current_period += 1;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Evolve the KES key to the target period, performing multiple evolutions
/// if necessary.
///
/// Returns the number of evolutions performed.
pub fn evolve_kes_key_to_period(
    creds: &mut BlockProducerCredentials,
    target_period: u32,
) -> Result<u32, BlockProducerError> {
    let mut evolutions = 0u32;
    while creds.kes_current_period < target_period {
        if !evolve_kes_key(creds)? {
            return Err(BlockProducerError::KesPeriod(format!(
                "KES key exhausted at period {} before reaching target {}",
                creds.kes_current_period, target_period
            )));
        }
        evolutions += 1;
    }
    Ok(evolutions)
}

// ---------------------------------------------------------------------------
// ShouldForge decision (upstream CheckShouldForge)
// ---------------------------------------------------------------------------

/// Decision about whether to forge a block for a given slot.
///
/// Mirrors the upstream `ShouldForge` type from
/// `Ouroboros.Consensus.Block.Forging`.
#[derive(Clone, Debug)]
pub enum ShouldForge {
    /// KES key evolution failed — cannot even attempt leader check.
    ForgeStateUpdateError(String),
    /// Won the VRF lottery but a gating condition prevents forging
    /// (e.g. KES period expired relative to opcert).
    CannotForge(String),
    /// VRF check returned not-leader for this slot.
    NotLeader,
    /// All checks passed — proceed to forge.
    ShouldForge(LeaderElectionResult),
}

/// Block context derived from the current chain tip, used to determine
/// the block number and previous header hash for a new block.
///
/// Reference: `BlockContext` in
/// `Ouroboros.Consensus.NodeKernel` (`mkCurrentBlockContext`).
#[derive(Clone, Debug)]
pub struct BlockContext {
    /// The block number of the new block to be forged.
    pub block_number: BlockNo,
    /// The header hash of the predecessor block (None for genesis successor).
    pub prev_hash: Option<HeaderHash>,
    /// The slot of the predecessor block (None for genesis successor).
    pub prev_slot: Option<SlotNo>,
}

/// Determine the block context from the current chain tip.
///
/// Returns `None` if the current slot is not ahead of the tip slot
/// (the slot is immutable or already occupied by a non-EBB block).
///
/// Reference: `mkCurrentBlockContext` in
/// `Ouroboros.Consensus.NodeKernel`.
pub fn make_block_context(
    current_slot: SlotNo,
    tip_slot: Option<SlotNo>,
    tip_block_no: Option<BlockNo>,
    tip_hash: Option<HeaderHash>,
) -> Option<BlockContext> {
    match (tip_slot, tip_block_no, tip_hash) {
        (Some(ts), Some(bn), Some(hash)) => {
            if current_slot <= ts {
                // Slot is occupied or in the past — cannot forge.
                None
            } else {
                Some(BlockContext {
                    block_number: BlockNo(bn.0 + 1),
                    prev_hash: Some(hash),
                    prev_slot: Some(ts),
                })
            }
        }
        // At genesis — first block ever.
        _ => Some(BlockContext {
            block_number: BlockNo(0),
            prev_hash: None,
            prev_slot: None,
        }),
    }
}

/// Runtime slot clock anchored to an initial slot/time pair.
///
/// The clock advances by one slot per `slot_length` elapsed wall-clock
/// duration from `anchor_instant`.
#[derive(Clone, Debug)]
pub struct SlotClock {
    anchor_slot: SlotNo,
    anchor_instant: Instant,
    slot_length: Duration,
}

impl SlotClock {
    /// Construct a slot clock anchored at the current instant.
    pub fn new(anchor_slot: SlotNo, slot_length: Duration) -> Self {
        let slot_length = if slot_length.is_zero() {
            Duration::from_secs(1)
        } else {
            slot_length
        };
        Self {
            anchor_slot,
            anchor_instant: Instant::now(),
            slot_length,
        }
    }

    /// Derive the slot number corresponding to `now`.
    pub fn slot_at(&self, now: Instant) -> SlotNo {
        let elapsed = now
            .checked_duration_since(self.anchor_instant)
            .unwrap_or_default();
        let slots_elapsed = (elapsed.as_secs_f64() / self.slot_length.as_secs_f64()).floor() as u64;
        SlotNo(self.anchor_slot.0.saturating_add(slots_elapsed))
    }

    /// Return the current slot according to the local wall-clock.
    pub fn current_slot(&self) -> SlotNo {
        self.slot_at(Instant::now())
    }
}

/// Run the upstream `checkShouldForge` sequence for a single slot:
///
/// 1. Evolve KES key to the current period (`updateForgeState`).
/// 2. Check VRF slot leadership (`checkIsLeader`).
/// 3. Validate that the KES period is within bounds (`checkCanForge`).
///
/// Reference: `checkShouldForge` in
/// `Ouroboros.Consensus.Block.Forging`.
pub fn check_should_forge(
    creds: &mut BlockProducerCredentials,
    slot: SlotNo,
    epoch_nonce: Nonce,
    sigma_num: u64,
    sigma_den: u64,
    active_slot_coeff: &ActiveSlotCoeff,
) -> ShouldForge {
    // Step 1: updateForgeState — evolve KES key to the current period.
    let target_period = match kes_period_of_slot(slot.0, creds.slots_per_kes_period) {
        Ok(p) => p,
        Err(e) => return ShouldForge::ForgeStateUpdateError(e.to_string()),
    };
    let target_u32 = match u32::try_from(target_period) {
        Ok(p) => p,
        Err(_) => return ShouldForge::ForgeStateUpdateError("KES period exceeds u32".to_owned()),
    };
    if target_u32 > creds.kes_current_period {
        match evolve_kes_key_to_period(creds, target_u32) {
            Ok(_) => {}
            Err(e) => return ShouldForge::ForgeStateUpdateError(e.to_string()),
        }
    }

    // Step 2: checkIsLeader — VRF slot lottery.
    let election = match check_slot_leadership(
        creds,
        slot,
        epoch_nonce,
        sigma_num,
        sigma_den,
        active_slot_coeff,
    ) {
        Ok(Some(result)) => result,
        Ok(None) => return ShouldForge::NotLeader,
        Err(e) => return ShouldForge::ForgeStateUpdateError(e.to_string()),
    };

    // Step 3: checkCanForge — KES period bounds.
    if let Err(e) = check_can_forge(creds, slot) {
        return ShouldForge::CannotForge(e.to_string());
    }

    ShouldForge::ShouldForge(election)
}

/// Transaction selected from the mempool for block body assembly.
///
/// Reference: upstream `Validated (GenTx blk)` from `getSnapshotFor`.
#[derive(Clone, Debug)]
pub struct ForgeBodyTx {
    /// Transaction identifier.
    pub tx_id: TxId,
    /// Era of the transaction.
    pub era: Era,
    /// Raw CBOR-encoded transaction body bytes.
    pub body: Vec<u8>,
    /// Raw CBOR-encoded submitted transaction bytes (full tx for the wire).
    pub raw_tx: Vec<u8>,
    /// Transaction size in bytes.
    pub size_bytes: usize,
    /// Transaction fee in lovelace.
    pub fee: u64,
}

/// Assemble the block body by taking a prefix of mempool entries that
/// fits within the maximum block body size.
///
/// Returns the selected transactions and their aggregate CBOR body size.
///
/// # Arguments
///
/// * `entries` — Fee-ordered mempool entries (highest fee first).
/// * `max_block_body_size` — Maximum permitted aggregate Tx body size.
///
/// Reference: `snapshotTake` with `blockCapacityTxMeasure` in
/// `Ouroboros.Consensus.Mempool.Impl.Common`.
pub fn assemble_block_body<'a>(
    entries: impl Iterator<Item = &'a MempoolEntry>,
    max_block_body_size: u32,
) -> (Vec<ForgeBodyTx>, u32) {
    let mut selected = Vec::new();
    let mut total_size: u32 = 0;

    for entry in entries {
        let entry_size = entry.size_bytes as u32;
        if total_size + entry_size > max_block_body_size {
            // Block capacity reached — stop.
            break;
        }
        selected.push(ForgeBodyTx {
            tx_id: entry.tx_id,
            era: entry.era,
            body: entry.body.clone(),
            raw_tx: entry.raw_tx.clone(),
            size_bytes: entry.size_bytes,
            fee: entry.fee,
        });
        total_size += entry_size;
    }

    (selected, total_size)
}

/// Outcome of a successful block forging operation.
///
/// Contains all the information needed to add the block to the ChainDB
/// and announce it to peers.
#[derive(Clone, Debug)]
pub struct ForgedBlock {
    /// The forged block header.
    pub header: ForgedBlockHeader,
    /// Transactions included in the block body.
    pub transactions: Vec<ForgeBodyTx>,
    /// Blake2b-256 hash of the serialized header (the block's identity).
    pub header_hash: HeaderHash,
    /// Slot in which the block was forged.
    pub slot: SlotNo,
    /// Block height.
    pub block_number: BlockNo,
    /// Aggregate block body size in bytes.
    pub body_size: u32,
    /// Total fees collected from the included transactions.
    pub total_fees: u64,
}

/// Forge a complete block: assemble body from mempool, compute body hash,
/// construct and sign the header, and return the fully-formed block.
///
/// This composites the upstream `forgeBlock` call from
/// `Ouroboros.Consensus.Node.BlockForging`:
/// 1. `assemble_block_body` (mempool prefix selection)
/// 2. Compute body hash (Blake2b-256 over concatenated tx bodies)
/// 3. `forge_block_header` (header construction + KES signing)
///
/// Reference: `forgeShelleyBlock` in
/// `Ouroboros.Consensus.Shelley.Node.Forging`.
#[allow(clippy::too_many_arguments)]
pub fn forge_block(
    creds: &BlockProducerCredentials,
    election: &LeaderElectionResult,
    context: &BlockContext,
    slot: SlotNo,
    entries: &[MempoolEntry],
    max_block_body_size: u32,
    issuer_vkey: VerificationKey,
    protocol_version: (u64, u64),
) -> Result<ForgedBlock, BlockProducerError> {
    // Step 1: Assemble the block body from mempool entries.
    let (body_txs, _selected_tx_body_size) =
        assemble_block_body(entries.iter(), max_block_body_size);

    // Step 2: Canonical body payload bytes = all block elements after the
    // header in Conway block encoding.
    let body_payload = encode_forged_body_payload(&body_txs);
    let body_size = u32::try_from(body_payload.len()).map_err(|_| {
        BlockProducerError::Crypto("forged block body size overflows u32".to_owned())
    })?;

    // Step 3: Compute the canonical block body hash.
    //
    // Upstream `bbHash` / `hashTxSeq` composes Blake2b-256 over the
    // per-segment hashes of the post-header block elements:
    //
    //     H( H(transaction_bodies) || H(witness_sets)
    //        || H(auxiliary_data_set) || H(invalid_transactions) )
    //
    // We replicate that here by hashing each top-level element of
    // `body_payload` (which is the four-element CBOR sequence emitted by
    // [`encode_forged_body_payload`]) and then hashing the concatenation
    // of those segment hashes.
    let body_hash = {
        use yggdrasil_crypto::blake2b::hash_bytes_256;
        use yggdrasil_ledger::cbor::Decoder;

        let mut dec = Decoder::new(&body_payload);
        let mut combined = Vec::with_capacity(32 * 4);
        // Body payload is a flat sequence of 4 top-level CBOR items
        // (tx_bodies, witness_sets, aux_data_map, invalid_txs).
        for _ in 0..4 {
            let seg_start = dec.position();
            dec.skip().map_err(|err| {
                BlockProducerError::Crypto(format!("forged body payload skip failed: {err}"))
            })?;
            let seg_end = dec.position();
            let seg_hash = hash_bytes_256(&body_payload[seg_start..seg_end]).0;
            combined.extend_from_slice(&seg_hash);
        }
        hash_bytes_256(&combined).0
    };

    let total_fees: u64 = body_txs.iter().map(|tx| tx.fee).sum();

    // Step 4: Forge the header.
    let config = ForgeBlockConfig { protocol_version };
    let forged_header = forge_block_header(
        creds,
        election,
        slot,
        context.block_number,
        context.prev_hash,
        body_hash,
        body_size,
        issuer_vkey,
        &config,
    )?;

    // Step 5: Compute the header hash from the on-wire Praos header CBOR.
    let header_hash = forged_header_to_praos_header(&forged_header).header_hash();

    Ok(ForgedBlock {
        header: forged_header,
        transactions: body_txs,
        header_hash,
        slot,
        block_number: context.block_number,
        body_size,
        total_fees,
    })
}

fn encode_forged_body_payload(transactions: &[ForgeBodyTx]) -> Vec<u8> {
    let mut raw_bodies: Vec<&[u8]> = Vec::with_capacity(transactions.len());
    let mut raw_witnesses: Vec<&[u8]> = Vec::with_capacity(transactions.len());
    let mut aux_entries: Vec<(u64, &[u8])> = Vec::new();

    for (idx, tx) in transactions.iter().enumerate() {
        if let Ok((body_slice, wit_slice, aux_slice)) = split_submitted_tx(&tx.raw_tx) {
            raw_bodies.push(body_slice);
            raw_witnesses.push(wit_slice);
            if let Some(aux) = aux_slice {
                aux_entries.push((idx as u64, aux));
            }
        } else {
            // Fallback: preserve deterministic encoding even if submitted-tx
            // framing is unavailable.
            raw_bodies.push(&tx.body);
            raw_witnesses.push(&[0xa0]);
        }
    }

    let mut enc = Encoder::new();

    // transaction_bodies
    enc.array(raw_bodies.len() as u64);
    for body in &raw_bodies {
        enc.raw(body);
    }

    // transaction_witness_sets
    enc.array(raw_witnesses.len() as u64);
    for wit in &raw_witnesses {
        enc.raw(wit);
    }

    // auxiliary_data_set
    enc.map(aux_entries.len() as u64);
    for &(idx, aux) in &aux_entries {
        enc.unsigned(idx);
        enc.raw(aux);
    }

    // invalid_transactions
    enc.array(0);

    enc.into_bytes()
}

/// Serialize a forged block into the multi-era CBOR envelope.
///
/// Produces `[era_tag, ConwayBlock_CBOR]` where the Conway block is
/// `[header, tx_bodies, witness_sets, aux_data_map, invalid_txs]`.
///
/// The header is encoded as a `PraosHeader` with a 14-element
/// `PraosHeaderBody` matching the on-chain Babbage/Conway wire format.
///
/// Transaction bodies and witness sets are extracted from each
/// `ForgeBodyTx.raw_tx` which carries the submitted-tx CBOR
/// `[body, witnesses, is_valid, aux_data]`.
///
/// Reference: `Cardano.Ledger.Block`, `forgeShelleyBlock`.
pub fn serialize_forged_block_cbor(forged: &ForgedBlock) -> Vec<u8> {
    // Conway era tag in the multi-era envelope.
    const CONWAY_ERA_TAG: u64 = 7;

    let praos_hdr = forged_header_to_praos_header(&forged.header);

    // Parse each raw_tx to split tx body and witness set CBOR.
    // Submitted-tx format: [body, witnesses, is_valid, aux_data / null]
    let mut raw_bodies: Vec<&[u8]> = Vec::with_capacity(forged.transactions.len());
    let mut raw_witnesses: Vec<&[u8]> = Vec::with_capacity(forged.transactions.len());
    let mut aux_entries: Vec<(u64, &[u8])> = Vec::new();

    for (idx, btx) in forged.transactions.iter().enumerate() {
        let data = &btx.raw_tx;
        if let Ok((body_slice, wit_slice, aux_slice)) = split_submitted_tx(data) {
            raw_bodies.push(body_slice);
            raw_witnesses.push(wit_slice);
            if let Some(aux) = aux_slice {
                aux_entries.push((idx as u64, aux));
            }
        } else {
            // Fallback: use the stored body bytes and an empty witness set.
            raw_bodies.push(&btx.body);
            raw_witnesses.push(&[0xa0]); // empty CBOR map
        }
    }

    // Encode the multi-era envelope: [7, ConwayBlock]
    let mut enc = Encoder::with_capacity(
        2 + praos_hdr.to_cbor_bytes().len()
            + forged
                .transactions
                .iter()
                .map(|t| t.raw_tx.len())
                .sum::<usize>()
            + 64,
    );

    // Outer envelope
    enc.array(2);
    enc.unsigned(CONWAY_ERA_TAG);

    // ConwayBlock: [header, tx_bodies, witnesses, aux_map, invalid_txs]
    enc.array(5);
    praos_hdr.encode_cbor(&mut enc);

    // transaction_bodies
    enc.array(raw_bodies.len() as u64);
    for body in &raw_bodies {
        enc.raw(body);
    }

    // transaction_witness_sets
    enc.array(raw_witnesses.len() as u64);
    for wit in &raw_witnesses {
        enc.raw(wit);
    }

    // auxiliary_data_set (map of tx_index → raw aux bytes)
    enc.map(aux_entries.len() as u64);
    for &(idx, aux) in &aux_entries {
        enc.unsigned(idx);
        enc.raw(aux);
    }

    // invalid_transactions (empty for locally forged blocks)
    enc.array(0);

    enc.into_bytes()
}

fn forged_header_to_praos_header(header: &ForgedBlockHeader) -> PraosHeader {
    let hb = &header.header_body;
    PraosHeader {
        body: PraosHeaderBody {
            block_number: hb.block_number.0,
            slot: hb.slot.0,
            prev_hash: hb.prev_hash.map(|h| h.0),
            issuer_vkey: hb.issuer_vkey.0,
            vrf_vkey: hb.vrf_vkey.to_bytes(),
            vrf_result: ShelleyVrfCert {
                output: hb.leader_vrf_output.clone(),
                proof: hb.leader_vrf_proof,
            },
            block_body_size: hb.block_body_size,
            block_body_hash: hb.block_body_hash,
            operational_cert: ShelleyOpCert {
                hot_vkey: hb.operational_cert.hot_vkey.to_bytes(),
                sequence_number: hb.operational_cert.sequence_number,
                kes_period: hb.operational_cert.kes_period,
                sigma: hb.operational_cert.sigma.0,
            },
            protocol_version: hb.protocol_version,
        },
        signature: header.kes_signature.to_bytes().to_vec(),
    }
}

/// Split a submitted-transaction CBOR `[body, witnesses, is_valid, aux]`
/// into references to the body, witness, and optional auxiliary slices
/// within the original buffer.
///
/// Returns `(body_slice, witness_slice, Option<aux_slice>)`.
type SubmittedTxSlices<'a> = (&'a [u8], &'a [u8], Option<&'a [u8]>);

fn split_submitted_tx(data: &[u8]) -> Result<SubmittedTxSlices<'_>, ()> {
    use yggdrasil_ledger::Decoder;
    let mut dec = Decoder::new(data);

    let arr_len = dec.array().map_err(|_| ())?;
    if arr_len < 3 {
        return Err(());
    }

    // Element 0: tx body
    let body_start = dec.position();
    dec.skip().map_err(|_| ())?;
    let body_end = dec.position();
    let body_slice = dec.slice(body_start, body_end).map_err(|_| ())?;

    // Element 1: witness set
    let wit_start = dec.position();
    dec.skip().map_err(|_| ())?;
    let wit_end = dec.position();
    let wit_slice = dec.slice(wit_start, wit_end).map_err(|_| ())?;

    // Element 2: is_valid (skip)
    dec.skip().map_err(|_| ())?;

    // Element 3 (optional): auxiliary data
    let aux_slice = if arr_len >= 4 {
        let aux_start = dec.position();
        dec.skip().map_err(|_| ())?;
        let aux_end = dec.position();
        let raw = dec.slice(aux_start, aux_end).map_err(|_| ())?;
        // Check for CBOR null (0xf6)
        if raw == [0xf6] { None } else { Some(raw) }
    } else {
        None
    };

    Ok((body_slice, wit_slice, aux_slice))
}

/// Convert a `ForgedBlock` into the storage-facing `Block` type suitable
/// for insertion into the volatile ChainDB.
///
/// The block is fully serialized to its multi-era CBOR envelope so that
/// ChainSync servers can serve its header and BlockFetch servers can
/// relay the complete block to downstream peers.
///
/// Reference: upstream `forgeShelleyBlock` → `ShelleyBlock` with
/// serialized CBOR for wire relay.
pub fn forged_block_to_storage_block(forged: &ForgedBlock) -> yggdrasil_ledger::Block {
    use yggdrasil_ledger::{Block, BlockHeader, Era, Tx};

    let raw_cbor = serialize_forged_block_cbor(forged);

    let txs: Vec<Tx> = forged
        .transactions
        .iter()
        .map(|btx| {
            let tx_id = btx.tx_id;
            Tx {
                id: tx_id,
                body: btx.body.clone(),
                witnesses: None,
                auxiliary_data: None,
                is_valid: None,
            }
        })
        .collect();

    let prev_hash = forged
        .header
        .header_body
        .prev_hash
        .unwrap_or(HeaderHash([0u8; 32]));

    Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: forged.header_hash,
            prev_hash,
            slot_no: forged.slot,
            block_no: forged.block_number,
            issuer_vkey: forged.header.header_body.issuer_vkey.0,
        },
        transactions: txs,
        raw_cbor: Some(std::sync::Arc::from(raw_cbor)),
        header_cbor_size: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use yggdrasil_crypto::sum_kes::derive_sum_kes_vk;

    /// Create a minimal text-envelope JSON string.
    fn make_text_envelope(type_tag: &str, description: &str, payload_hex: &str) -> String {
        serde_json::json!({
            "type": type_tag,
            "description": description,
            "cborHex": payload_hex,
        })
        .to_string()
    }

    /// CBOR-encode a byte string with a 2-byte header (0x58 len).
    fn cbor_bstr(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        if data.len() < 24 {
            out.push(0x40 | data.len() as u8);
        } else if data.len() < 256 {
            out.push(0x58);
            out.push(data.len() as u8);
        } else {
            out.push(0x59);
            out.push((data.len() >> 8) as u8);
            out.push(data.len() as u8);
        }
        out.extend_from_slice(data);
        out
    }

    fn write_temp_file(dir: &tempfile::TempDir, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        // Match the production-mode requirement (0o400) so the
        // ensure_secret_file_mode gate is satisfied for `.skey` fixtures.
        // Audit finding L-6.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
        }
        path
    }

    #[test]
    fn load_vrf_key_from_text_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let key_bytes = [0xABu8; 64];
        let cbor = cbor_bstr(&key_bytes);
        let json = make_text_envelope(
            "VrfSigningKey_PraosVRF",
            "VRF Signing Key",
            &hex::encode(&cbor),
        );
        let path = write_temp_file(&dir, "vrf.skey", &json);
        let sk = load_vrf_signing_key(&path).unwrap();
        assert_eq!(sk.0, key_bytes);
    }

    #[test]
    fn load_vrf_key_rejects_wrong_type() {
        let dir = tempfile::tempdir().unwrap();
        let key_bytes = [0xABu8; 64];
        let cbor = cbor_bstr(&key_bytes);
        let json = make_text_envelope("WrongType", "", &hex::encode(&cbor));
        let path = write_temp_file(&dir, "vrf_bad.skey", &json);
        let result = load_vrf_signing_key(&path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unexpected text envelope type"),
            "{err_msg}"
        );
    }

    #[test]
    fn load_kes_key_from_text_envelope() {
        let dir = tempfile::tempdir().unwrap();
        // Depth-0 KES key = just a 32-byte seed
        let seed = [0xCDu8; 32];
        let cbor = cbor_bstr(&seed);
        let json = make_text_envelope("KesSigningKey_ed25519_kes_2^0", "", &hex::encode(&cbor));
        let path = write_temp_file(&dir, "kes.skey", &json);
        let sk = load_kes_signing_key(&path).unwrap();
        assert_eq!(sk.depth(), 0);
        assert_eq!(sk.total_periods(), 1);
    }

    #[test]
    fn load_issuer_verification_key_from_text_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let issuer_bytes = [0xA5u8; 32];
        let cbor = cbor_bstr(&issuer_bytes);
        let json = make_text_envelope("StakePoolVerificationKey_ed25519", "", &hex::encode(&cbor));
        let path = write_temp_file(&dir, "cold.vkey", &json);

        let issuer = load_issuer_verification_key(&path).expect("load issuer vkey");
        assert_eq!(issuer.to_bytes(), issuer_bytes);
    }

    #[test]
    fn load_kes_key_depth_6() {
        let dir = tempfile::tempdir().unwrap();
        let seed = [0x42u8; 32];
        let cbor = cbor_bstr(&seed);
        let json = make_text_envelope("KesSigningKey_ed25519_kes_2^6", "", &hex::encode(&cbor));
        let path = write_temp_file(&dir, "kes6.skey", &json);
        let sk = load_kes_signing_key(&path).unwrap();
        assert_eq!(sk.depth(), 6);
        assert_eq!(sk.total_periods(), 64);
    }

    #[test]
    fn load_opcert_from_text_envelope() {
        let dir = tempfile::tempdir().unwrap();

        // Build a minimal CBOR array(4): [hot_vkey(32), seq_no, kes_period, sigma(64)]
        let hot_vkey = [0x11u8; 32];
        let sigma = [0x22u8; 64];

        let mut cbor = Vec::new();
        cbor.push(0x84); // array(4)
        cbor.extend_from_slice(&cbor_bstr(&hot_vkey)); // bytes(32)
        cbor.push(0x05); // uint 5 (sequence_number)
        cbor.push(0x0A); // uint 10 (kes_period)
        cbor.extend_from_slice(&cbor_bstr(&sigma)); // bytes(64)

        let json = make_text_envelope("NodeOperationalCertificate", "", &hex::encode(&cbor));
        let path = write_temp_file(&dir, "node.cert", &json);
        let opcert = load_operational_certificate(&path).unwrap();

        assert_eq!(opcert.hot_vkey.to_bytes(), hot_vkey);
        assert_eq!(opcert.sequence_number, 5);
        assert_eq!(opcert.kes_period, 10);
        assert_eq!(opcert.sigma.0, sigma);
    }

    #[test]
    fn load_block_producer_credentials_rejects_mismatched_issuer_key() {
        use yggdrasil_crypto::ed25519::SigningKey;

        let dir = tempfile::tempdir().unwrap();

        // VRF signing key file.
        let vrf_key_bytes = [0xABu8; 64];
        let vrf_json = make_text_envelope(
            "VrfSigningKey_PraosVRF",
            "",
            &hex::encode(cbor_bstr(&vrf_key_bytes)),
        );
        let vrf_path = write_temp_file(&dir, "vrf.skey", &vrf_json);

        // KES signing key file (depth 0, 32-byte seed).
        let kes_seed = [0xCDu8; 32];
        let kes_json = make_text_envelope(
            "KesSigningKey_ed25519_kes_2^0",
            "",
            &hex::encode(cbor_bstr(&kes_seed)),
        );
        let kes_path = write_temp_file(&dir, "kes.skey", &kes_json);

        // Build an opcert signed by cold key A.
        let cold_sk_a = SigningKey::from_bytes([0x11u8; 32]);
        let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("kes sk");
        let kes_vk = derive_sum_kes_vk(&kes_sk).expect("kes vk");

        let mut signable = [0u8; 48];
        signable[..32].copy_from_slice(&kes_vk.to_bytes());
        signable[32..40].copy_from_slice(&0u64.to_be_bytes());
        signable[40..48].copy_from_slice(&0u64.to_be_bytes());
        let sigma = cold_sk_a.sign(&signable).expect("opcert signature");

        let mut opcert_cbor = Vec::new();
        opcert_cbor.push(0x84); // array(4)
        opcert_cbor.extend_from_slice(&cbor_bstr(&kes_vk.to_bytes()));
        opcert_cbor.push(0x00); // sequence_number = 0
        opcert_cbor.push(0x00); // kes_period = 0
        opcert_cbor.extend_from_slice(&cbor_bstr(&sigma.0));
        let opcert_json =
            make_text_envelope("NodeOperationalCertificate", "", &hex::encode(opcert_cbor));
        let opcert_path = write_temp_file(&dir, "node.cert", &opcert_json);

        // Issuer key file uses different cold key B.
        let cold_vk_b = SigningKey::from_bytes([0x22u8; 32])
            .verification_key()
            .expect("derive verification key");
        let issuer_json = make_text_envelope(
            "StakePoolVerificationKey_ed25519",
            "",
            &hex::encode(cbor_bstr(&cold_vk_b.to_bytes())),
        );
        let issuer_path = write_temp_file(&dir, "cold.vkey", &issuer_json);

        let err = load_block_producer_credentials(
            &kes_path,
            &vrf_path,
            &opcert_path,
            &issuer_path,
            129_600,
            62,
        )
        .expect_err("mismatched issuer key must fail credential loading");

        assert!(
            err.to_string()
                .contains("operational certificate does not verify against issuer key"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn leader_election_with_synthetic_credentials() {
        // Build minimal credentials: depth-0 KES, arbitrary VRF key.
        let vrf_sk = VrfSecretKey::from_seed([0x99u8; 32]);
        let vrf_vk = vrf_sk.verification_key();
        let kes_sk = gen_sum_kes_signing_key(&[0xAAu8; 32], 0).unwrap();
        let opcert = OpCert {
            hot_vkey: derive_sum_kes_vk(&kes_sk).unwrap(),
            sequence_number: 0,
            kes_period: 0,
            sigma: Signature([0u8; 64]),
        };

        let creds = BlockProducerCredentials {
            vrf_signing_key: vrf_sk,
            vrf_verification_key: vrf_vk,
            kes_signing_key: kes_sk,
            kes_current_period: 0,
            operational_cert: opcert,
            issuer_vkey: VerificationKey::from_bytes([0x10; 32]),
            slots_per_kes_period: 129600,
            max_kes_evolutions: 62,
        };

        let active_slot_coeff = ActiveSlotCoeff::new(1.0).unwrap();
        let nonce = Nonce::Hash([0u8; 32]);

        // With f=1.0 and sigma=1/1, every slot should be a leader.
        let result = check_slot_leadership(
            &creds,
            SlotNo(100),
            nonce,
            1, // sigma_num
            1, // sigma_den
            &active_slot_coeff,
        )
        .unwrap();
        assert!(result.is_some(), "should be elected leader with f=1.0");
    }

    #[test]
    fn check_can_forge_rejects_expired_kes() {
        let kes_sk = gen_sum_kes_signing_key(&[0xBBu8; 32], 0).unwrap();
        let opcert = OpCert {
            hot_vkey: derive_sum_kes_vk(&kes_sk).unwrap(),
            sequence_number: 0,
            kes_period: 0,
            sigma: Signature([0u8; 64]),
        };

        let creds = BlockProducerCredentials {
            vrf_signing_key: VrfSecretKey::from_seed([0u8; 32]),
            vrf_verification_key: VrfSecretKey::from_seed([0u8; 32]).verification_key(),
            kes_signing_key: kes_sk,
            kes_current_period: 0,
            operational_cert: opcert,
            issuer_vkey: VerificationKey::from_bytes([0x20; 32]),
            slots_per_kes_period: 10,
            max_kes_evolutions: 1, // only 1 evolution allowed
        };

        // Slot 10 → KES period 1, which is beyond the max (0 + 1 = 1 → expired).
        let result = check_can_forge(&creds, SlotNo(10));
        assert!(result.is_err(), "should reject expired KES");
    }

    #[test]
    fn forge_block_header_roundtrip() {
        use yggdrasil_consensus::header::verify_header;
        use yggdrasil_crypto::ed25519::SigningKey;

        let cold_sk = SigningKey::from_bytes([0xEEu8; 32]);
        let cold_vk = cold_sk
            .verification_key()
            .expect("derive cold verification key");

        let kes_seed = [0xFFu8; 32];
        let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).unwrap();
        let kes_vk = derive_sum_kes_vk(&kes_sk).unwrap();

        // Produce a valid opcert signed by the cold key.
        let opcert_signable = {
            let mut buf = [0u8; 48];
            buf[..32].copy_from_slice(&kes_vk.to_bytes());
            buf[32..40].copy_from_slice(&0u64.to_be_bytes()); // sequence_number
            buf[40..48].copy_from_slice(&0u64.to_be_bytes()); // kes_period
            buf
        };
        let opcert_sigma = cold_sk.sign(&opcert_signable).unwrap();
        let opcert = OpCert {
            hot_vkey: kes_vk,
            sequence_number: 0,
            kes_period: 0,
            sigma: opcert_sigma,
        };

        let vrf_sk = VrfSecretKey::from_seed([0x77u8; 32]);
        let vrf_vk = vrf_sk.verification_key();

        let creds = BlockProducerCredentials {
            vrf_signing_key: vrf_sk,
            vrf_verification_key: vrf_vk,
            kes_signing_key: kes_sk,
            kes_current_period: 0,
            operational_cert: opcert,
            issuer_vkey: cold_vk.clone(),
            slots_per_kes_period: 129600,
            max_kes_evolutions: 62,
        };

        let active_slot_coeff = ActiveSlotCoeff::new(1.0).unwrap();
        let nonce = Nonce::Hash([1u8; 32]);
        let slot = SlotNo(42);

        let election = check_slot_leadership(&creds, slot, nonce, 1, 1, &active_slot_coeff)
            .unwrap()
            .expect("should be leader with f=1.0");

        let forged = forge_block_header(
            &creds,
            &election,
            slot,
            BlockNo(1),
            None,
            [0xAA; 32], // body hash
            100,        // body size
            cold_vk,
            &ForgeBlockConfig {
                protocol_version: (9, 0),
            },
        )
        .unwrap();

        // Verify the forged header.
        let header = yggdrasil_consensus::Header {
            body: forged.header_body,
            kes_signature: forged.kes_signature,
        };
        let verify_result = verify_header(&header, 129600, 62);
        assert!(
            verify_result.is_ok(),
            "forged header should verify: {:?}",
            verify_result.err()
        );
    }

    #[test]
    fn evolve_kes_key_advances_period() {
        let kes_sk = gen_sum_kes_signing_key(&[0xCCu8; 32], 2).unwrap();
        let opcert = OpCert {
            hot_vkey: derive_sum_kes_vk(&kes_sk).unwrap(),
            sequence_number: 0,
            kes_period: 0,
            sigma: Signature([0u8; 64]),
        };

        let mut creds = BlockProducerCredentials {
            vrf_signing_key: VrfSecretKey::from_seed([0u8; 32]),
            vrf_verification_key: VrfSecretKey::from_seed([0u8; 32]).verification_key(),
            kes_signing_key: kes_sk,
            kes_current_period: 0,
            operational_cert: opcert,
            issuer_vkey: VerificationKey::from_bytes([0x30; 32]),
            slots_per_kes_period: 100,
            max_kes_evolutions: 4,
        };

        // Depth 2 → 4 periods (0, 1, 2, 3).
        assert!(evolve_kes_key(&mut creds).unwrap());
        assert_eq!(creds.kes_current_period, 1);
        assert!(evolve_kes_key(&mut creds).unwrap());
        assert_eq!(creds.kes_current_period, 2);
        assert!(evolve_kes_key(&mut creds).unwrap());
        assert_eq!(creds.kes_current_period, 3);
        // Last period — evolution should return false.
        assert!(!evolve_kes_key(&mut creds).unwrap());
    }

    #[test]
    fn evolve_kes_key_to_target_period_works() {
        let kes_sk = gen_sum_kes_signing_key(&[0xDDu8; 32], 2).unwrap();
        let opcert = OpCert {
            hot_vkey: derive_sum_kes_vk(&kes_sk).unwrap(),
            sequence_number: 0,
            kes_period: 0,
            sigma: Signature([0u8; 64]),
        };

        let mut creds = BlockProducerCredentials {
            vrf_signing_key: VrfSecretKey::from_seed([0u8; 32]),
            vrf_verification_key: VrfSecretKey::from_seed([0u8; 32]).verification_key(),
            kes_signing_key: kes_sk,
            kes_current_period: 0,
            operational_cert: opcert,
            issuer_vkey: VerificationKey::from_bytes([0x40; 32]),
            slots_per_kes_period: 100,
            max_kes_evolutions: 4,
        };

        let evolutions = evolve_kes_key_to_period(&mut creds, 3).unwrap();
        assert_eq!(evolutions, 3);
        assert_eq!(creds.kes_current_period, 3);
    }

    #[test]
    fn forged_storage_block_carries_multi_era_raw_cbor() {
        use yggdrasil_crypto::ed25519::SigningKey;
        use yggdrasil_ledger::{CborDecode, ConwayBlock, Decoder};

        let cold_sk = SigningKey::from_bytes([0x9Au8; 32]);
        let cold_vk = cold_sk
            .verification_key()
            .expect("derive cold verification key");

        let kes_sk = gen_sum_kes_signing_key(&[0x8Bu8; 32], 0).unwrap();
        let kes_vk = derive_sum_kes_vk(&kes_sk).unwrap();
        let opcert_signable = {
            let mut buf = [0u8; 48];
            buf[..32].copy_from_slice(&kes_vk.to_bytes());
            buf[32..40].copy_from_slice(&0u64.to_be_bytes());
            buf[40..48].copy_from_slice(&0u64.to_be_bytes());
            buf
        };
        let opcert_sigma = cold_sk.sign(&opcert_signable).unwrap();

        let creds = BlockProducerCredentials {
            vrf_signing_key: VrfSecretKey::from_seed([0x7Cu8; 32]),
            vrf_verification_key: VrfSecretKey::from_seed([0x7Cu8; 32]).verification_key(),
            kes_signing_key: kes_sk,
            kes_current_period: 0,
            operational_cert: OpCert {
                hot_vkey: kes_vk,
                sequence_number: 0,
                kes_period: 0,
                sigma: opcert_sigma,
            },
            issuer_vkey: cold_vk.clone(),
            slots_per_kes_period: 129600,
            max_kes_evolutions: 62,
        };

        let election = check_slot_leadership(
            &creds,
            SlotNo(123),
            Nonce::Hash([0x55; 32]),
            1,
            1,
            &ActiveSlotCoeff::new(1.0).unwrap(),
        )
        .unwrap()
        .expect("leader election");

        let forged = forge_block(
            &creds,
            &election,
            &BlockContext {
                block_number: BlockNo(1),
                prev_hash: None,
                prev_slot: None,
            },
            SlotNo(123),
            &[],
            1024,
            cold_vk,
            (9, 0),
        )
        .expect("forge block");

        let storage_block = forged_block_to_storage_block(&forged);
        assert_eq!(storage_block.era, Era::Conway);
        let raw = storage_block.raw_cbor.expect("raw cbor must be present");

        let mut dec = Decoder::new(&raw);
        assert_eq!(dec.array().expect("envelope array"), 2);
        assert_eq!(dec.unsigned().expect("era tag"), 7);

        let body_start = dec.position();
        dec.skip().expect("skip envelope body");
        let body_end = dec.position();
        let conway =
            ConwayBlock::from_cbor_bytes(&raw[body_start..body_end]).expect("decode conway block");

        assert_eq!(conway.header.body.slot, 123);
        assert!(conway.transaction_bodies.is_empty());
    }

    #[test]
    fn unwrap_cbor_bytes_handles_short_form() {
        // 0x45 = bytes(5)
        let data = [0x45, 0x01, 0x02, 0x03, 0x04, 0x05];
        let result = unwrap_cbor_bytes(&data, Path::new("test")).unwrap();
        assert_eq!(result, &[0x01, 0x02, 0x03, 0x04, 0x05]);
    }

    #[test]
    fn unwrap_cbor_bytes_handles_1byte_length() {
        // 0x58 0x03 = bytes(3)
        let data = [0x58, 0x03, 0xAA, 0xBB, 0xCC];
        let result = unwrap_cbor_bytes(&data, Path::new("test")).unwrap();
        assert_eq!(result, &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn text_envelope_rejects_non_bstr_cbor() {
        // 0x84 = array(4), not a bstr
        let data = [0x84, 0x01, 0x02, 0x03, 0x04];
        let result = unwrap_cbor_bytes(&data, Path::new("test"));
        assert!(result.is_err());
    }

    #[test]
    fn slot_clock_advances_slots_by_elapsed_time() {
        let clock = SlotClock::new(SlotNo(10), Duration::from_secs(1));
        let now = Instant::now();
        let slot = clock.slot_at(now + Duration::from_secs(3));
        assert_eq!(slot, SlotNo(13));
    }

    #[test]
    fn slot_clock_zero_length_falls_back_to_one_second() {
        let clock = SlotClock::new(SlotNo(7), Duration::ZERO);
        let now = Instant::now();
        let slot = clock.slot_at(now + Duration::from_secs(2));
        assert_eq!(slot, SlotNo(9));
    }
}
