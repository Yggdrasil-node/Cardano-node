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

use yggdrasil_consensus::{
    ActiveSlotCoeff, HeaderBody, OpCert,
    check_is_leader, kes_period_of_slot, check_kes_period,
};
use yggdrasil_crypto::ed25519::{Signature, VerificationKey};
use yggdrasil_crypto::sum_kes::{
    SumKesSigningKey, SumKesSignature, SumKesVerificationKey,
    gen_sum_kes_signing_key, sign_sum_kes, update_sum_kes,
};
use yggdrasil_crypto::vrf::{
    VrfOutput, VrfSecretKey, VrfVerificationKey,
    VRF_SIGNING_KEY_SIZE,
};
use yggdrasil_ledger::{BlockNo, HeaderHash, Nonce, SlotNo};

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

/// Load a SumKES signing key from a text-envelope file.
///
/// The text-envelope type string encodes the depth:
/// `KesSigningKey_ed25519_kes_2^6` means depth 6 (64 periods, mainnet).
///
/// The CBOR payload is a `bstr` containing the 32-byte seed from which
/// the full SumKES key tree is generated.
pub fn load_kes_signing_key(path: &Path) -> Result<SumKesSigningKey, BlockProducerError> {
    let (type_tag, cbor_bytes) = read_text_envelope(path)?;
    if !type_tag.starts_with(KES_SIGNING_KEY_TYPE_PREFIX) {
        return Err(BlockProducerError::TypeMismatch {
            path: path.display().to_string(),
            expected: format!("{KES_SIGNING_KEY_TYPE_PREFIX}<depth>"),
            actual: type_tag,
        });
    }
    let depth_str = &type_tag[KES_SIGNING_KEY_TYPE_PREFIX.len()..];
    let depth: u32 = depth_str.parse().map_err(|_| BlockProducerError::OpCertDecode {
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
    let hot_vkey_raw = read_cbor_bytes_field(&mut cursor)
        .map_err(|_| err("failed to read hot_vkey bytes"))?;
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
    let kes_period =
        read_cbor_uint(&mut cursor).map_err(|_| err("failed to read kes_period"))?;

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
    /// Cold (block issuer) verification key hash — derived from the
    /// operational certificate's cold-key signature.
    ///
    /// In full production use this would be loaded from a separate cold
    /// verification key file; for now we carry the KES hot VK as the
    /// issuer proxy since the OpCert binds them.
    ///
    /// Slots per KES period (from config).
    pub slots_per_kes_period: u64,
    /// Maximum KES evolutions (from config).
    pub max_kes_evolutions: u64,
}

/// Load block producer credentials from text-envelope files.
///
/// Validates that the KES key depth is consistent with the operational
/// certificate and config parameters.
///
/// Reference: upstream node startup in `Cardano.Node.Run.handleSimpleNode`.
pub fn load_block_producer_credentials(
    kes_key_path: &Path,
    vrf_key_path: &Path,
    opcert_path: &Path,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<BlockProducerCredentials, BlockProducerError> {
    let vrf_signing_key = load_vrf_signing_key(vrf_key_path)?;
    let vrf_verification_key = vrf_signing_key.verification_key();
    let kes_signing_key = load_kes_signing_key(kes_key_path)?;
    let operational_cert = load_operational_certificate(opcert_path)?;

    Ok(BlockProducerCredentials {
        vrf_signing_key,
        vrf_verification_key,
        kes_signing_key,
        kes_current_period: 0,
        operational_cert,
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
    let current_kes_period =
        kes_period_of_slot(slot.0, creds.slots_per_kes_period).map_err(|e| {
            BlockProducerError::KesPeriod(e.to_string())
        })?;

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

    u32::try_from(offset).map_err(|_| {
        BlockProducerError::KesPeriod("KES period offset overflows u32".to_owned())
    })
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
pub fn forge_block_header(
    creds: &BlockProducerCredentials,
    election: &LeaderElectionResult,
    slot: SlotNo,
    block_number: BlockNo,
    prev_hash: Option<HeaderHash>,
    block_body_hash: [u8; 32],
    block_body_size: u32,
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
        issuer_vkey: VerificationKey([0u8; 32]), // placeholder — cold VK
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
pub fn evolve_kes_key(
    creds: &mut BlockProducerCredentials,
) -> Result<bool, BlockProducerError> {
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
        let json = make_text_envelope(
            "WrongType",
            "",
            &hex::encode(&cbor),
        );
        let path = write_temp_file(&dir, "vrf_bad.skey", &json);
        let result = load_vrf_signing_key(&path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("unexpected text envelope type"), "{err_msg}");
    }

    #[test]
    fn load_kes_key_from_text_envelope() {
        let dir = tempfile::tempdir().unwrap();
        // Depth-0 KES key = just a 32-byte seed
        let seed = [0xCDu8; 32];
        let cbor = cbor_bstr(&seed);
        let json = make_text_envelope(
            "KesSigningKey_ed25519_kes_2^0",
            "",
            &hex::encode(&cbor),
        );
        let path = write_temp_file(&dir, "kes.skey", &json);
        let sk = load_kes_signing_key(&path).unwrap();
        assert_eq!(sk.depth(), 0);
        assert_eq!(sk.total_periods(), 1);
    }

    #[test]
    fn load_kes_key_depth_6() {
        let dir = tempfile::tempdir().unwrap();
        let seed = [0x42u8; 32];
        let cbor = cbor_bstr(&seed);
        let json = make_text_envelope(
            "KesSigningKey_ed25519_kes_2^6",
            "",
            &hex::encode(&cbor),
        );
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

        let json = make_text_envelope(
            "NodeOperationalCertificate",
            "",
            &hex::encode(&cbor),
        );
        let path = write_temp_file(&dir, "node.cert", &json);
        let opcert = load_operational_certificate(&path).unwrap();

        assert_eq!(opcert.hot_vkey.to_bytes(), hot_vkey);
        assert_eq!(opcert.sequence_number, 5);
        assert_eq!(opcert.kes_period, 10);
        assert_eq!(opcert.sigma.0, sigma);
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
        let cold_vk = cold_sk.verification_key();

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

        let mut creds = BlockProducerCredentials {
            vrf_signing_key: vrf_sk,
            vrf_verification_key: vrf_vk,
            kes_signing_key: kes_sk,
            kes_current_period: 0,
            operational_cert: opcert,
            slots_per_kes_period: 129600,
            max_kes_evolutions: 62,
        };

        let active_slot_coeff = ActiveSlotCoeff::new(1.0).unwrap();
        let nonce = Nonce::Hash([1u8; 32]);
        let slot = SlotNo(42);

        let election = check_slot_leadership(
            &creds,
            slot,
            nonce,
            1,
            1,
            &active_slot_coeff,
        )
        .unwrap()
        .expect("should be leader with f=1.0");

        let mut forged = forge_block_header(
            &creds,
            &election,
            slot,
            BlockNo(1),
            None,
            [0xAA; 32], // body hash
            100,         // body size
            &ForgeBlockConfig {
                protocol_version: (9, 0),
            },
        )
        .unwrap();

        // Patch in the real cold verification key so verify_header can check.
        forged.header_body.issuer_vkey = cold_vk.unwrap();

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
            slots_per_kes_period: 100,
            max_kes_evolutions: 4,
        };

        let evolutions = evolve_kes_key_to_period(&mut creds, 3).unwrap();
        assert_eq!(evolutions, 3);
        assert_eq!(creds.kes_current_period, 3);
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
}
