//! Block header types and KES-based header signature verification.
//!
//! A block header consists of a body (`HeaderBody`) carrying the
//! chain-indexing fields plus an embedded `OpCert`, and a KES signature
//! over the body.  Verification checks the OpCert cold-key signature,
//! the KES period window, and the KES signature itself.
//!
//! Reference: `Cardano.Protocol.TPraos.BHeader` in `cardano-ledger`.

use yggdrasil_crypto::ed25519::VerificationKey;
use yggdrasil_crypto::sum_kes::{SumKesSignature, verify_sum_kes};
use yggdrasil_crypto::vrf::VrfVerificationKey;
use yggdrasil_ledger::{BlockNo, HeaderHash, SlotNo};

use crate::error::ConsensusError;
use crate::opcert::{OpCert, check_kes_period, kes_period_of_slot};

/// The body of a block header, containing chain-indexing fields, VRF
/// outputs, the operational certificate, and protocol version.
///
/// Reference: `BHBody` in `Cardano.Protocol.TPraos.BHeader`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderBody {
    /// Block height.
    pub block_number: BlockNo,
    /// Slot in which this block was issued.
    pub slot: SlotNo,
    /// Hash of the previous block header (`None` for genesis successor).
    pub prev_hash: Option<HeaderHash>,
    /// Cold-key (block issuer) verification key.
    pub issuer_vkey: VerificationKey,
    /// VRF verification key for the block issuer.
    pub vrf_vkey: VrfVerificationKey,
    /// Leader VRF output (hash of the certified VRF evaluation).
    ///
    /// For TPraos (Shelley–Alonzo) this comes from the `leader_vrf` cert;
    /// for Praos (Babbage/Conway) from the single `vrf_result`.
    pub leader_vrf_output: Vec<u8>,
    /// Leader VRF proof (80-byte standard ECVRF proof).
    pub leader_vrf_proof: [u8; 80],
    /// Nonce VRF output (TPraos only — Shelley through Alonzo).
    /// `None` for Praos-era blocks where the single `vrf_result` covers both.
    pub nonce_vrf_output: Option<Vec<u8>>,
    /// Nonce VRF proof (TPraos only).
    pub nonce_vrf_proof: Option<[u8; 80]>,
    /// Size of the block body in bytes.
    pub block_body_size: u32,
    /// Hash of the block body (Blake2b-256).
    pub block_body_hash: [u8; 32],
    /// Operational certificate binding cold key to hot KES key.
    pub operational_cert: OpCert,
    /// Protocol version (major, minor).
    pub protocol_version: (u64, u64),
}

impl HeaderBody {
    /// Produce the canonical serializable bytes for this header body.
    ///
    /// The KES signature is computed over these bytes.  The encoding is a
    /// concatenation of all fields in a fixed, deterministic order that
    /// matches CBOR-level ordering of `BHBody` fields.
    pub fn to_signable_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(256);

        // block_no (u64 BE)
        buf.extend_from_slice(&self.block_number.0.to_be_bytes());
        // slot_no (u64 BE)
        buf.extend_from_slice(&self.slot.0.to_be_bytes());
        // prev_hash: 0x00 for genesis, 0x01 || hash for a block
        match &self.prev_hash {
            None => buf.push(0x00),
            Some(h) => {
                buf.push(0x01);
                buf.extend_from_slice(&h.0);
            }
        }
        // issuer_vk (32 bytes)
        buf.extend_from_slice(&self.issuer_vkey.to_bytes());
        // vrf_vk (32 bytes)
        buf.extend_from_slice(&self.vrf_vkey.to_bytes());
        // body_size (u32 BE)
        buf.extend_from_slice(&self.block_body_size.to_be_bytes());
        // body_hash (32 bytes)
        buf.extend_from_slice(&self.block_body_hash);
        // opcert signable (48 bytes: hot_vk 32 + counter 8 + kes_period 8)
        buf.extend_from_slice(&self.operational_cert.signable_bytes());
        // opcert sigma (64 bytes)
        buf.extend_from_slice(&self.operational_cert.sigma.to_bytes());
        // protocol_version (major u64 BE + minor u64 BE)
        buf.extend_from_slice(&self.protocol_version.0.to_be_bytes());
        buf.extend_from_slice(&self.protocol_version.1.to_be_bytes());

        buf
    }
}

/// A signed block header: the body plus a KES signature over it.
///
/// Reference: `BHeader` = `BHBody` + `SignedKES` in upstream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Header {
    /// The header body that was signed.
    pub body: HeaderBody,
    /// KES signature over `body.to_signable_bytes()`.
    pub kes_signature: SumKesSignature,
}

/// Verify a block header's cryptographic chain.
///
/// This performs three verifications:
///
/// 1. **OpCert** — The cold key (`body.issuer_vk`) signed the operational
///    certificate, binding itself to the hot KES key.
/// 2. **KES period** — The slot's KES period falls within the certificate's
///    validity window.
/// 3. **KES signature** — The hot KES key signed the header body at the
///    correct KES period offset.
///
/// VRF proof verification is intentionally **not** included here; it lives
/// in [`crate::praos`] since it involves stake-distribution context.
pub fn verify_header(
    header: &Header,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<(), ConsensusError> {
    verify_header_with_signed_bytes(header, slots_per_kes_period, max_kes_evolutions, None)
}

/// Variant of [`verify_header`] that accepts the canonical signed bytes
/// for the KES signature, when available.
///
/// On the live network, peers serve headers whose KES signature was
/// computed over the **original CBOR-encoded `BHBody`** preserved by an
/// upstream `Annotator`.  Re-encoding the decoded body cannot reproduce
/// those bytes deterministically when any field encoding choice diverges
/// (definite vs indefinite array, integer minimal-width, etc.).
///
/// When `signed_bytes_override` is `Some(slice)` the slice is used as the
/// KES message, matching upstream
/// `Cardano.Protocol.TPraos.BHeader.verifyHeader` /
/// `Cardano.Protocol.Praos.Header.verifyHeader` exactly.
///
/// When `signed_bytes_override` is `None` the function falls back to the
/// in-memory [`HeaderBody::to_signable_bytes`] layout, which is sufficient
/// for round-trip tests where signing and verification share the same
/// codec.
pub fn verify_header_with_signed_bytes(
    header: &Header,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
    signed_bytes_override: Option<&[u8]>,
) -> Result<(), ConsensusError> {
    // 1. Verify the OpCert cold-key signature.
    header
        .body
        .operational_cert
        .verify(&header.body.issuer_vkey)?;

    // 2. Check the KES period window.
    let current_kes_period = kes_period_of_slot(header.body.slot.0, slots_per_kes_period)?;
    check_kes_period(
        &header.body.operational_cert,
        current_kes_period,
        max_kes_evolutions,
    )?;

    // 3. Verify the KES signature over the header body.
    let synthetic;
    let signable: &[u8] = match signed_bytes_override {
        Some(s) => s,
        None => {
            synthetic = header.body.to_signable_bytes();
            &synthetic
        }
    };
    let kes_offset = current_kes_period
        .checked_sub(header.body.operational_cert.kes_period)
        .ok_or(ConsensusError::KesPeriodTooEarly {
            current: current_kes_period,
            cert_start: header.body.operational_cert.kes_period,
        })?;
    let kes_period_u32 =
        u32::try_from(kes_offset).map_err(|_| ConsensusError::KesPeriodOverflow)?;

    verify_sum_kes(
        &header.body.operational_cert.hot_vkey,
        kes_period_u32,
        signable,
        &header.kes_signature,
    )
    .map_err(|_| ConsensusError::InvalidKesSignature)
}

/// Verify the cold-key signature on an operational certificate and check
/// the KES period, without verifying the KES body signature.
///
/// This is useful when only the OpCert validity needs to be checked (e.g.,
/// during certificate inspection or pre-validation).
pub fn verify_opcert_only(
    opcert: &OpCert,
    cold_vk: &VerificationKey,
    current_slot: SlotNo,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<(), ConsensusError> {
    opcert.verify(cold_vk)?;
    let current_kes_period = kes_period_of_slot(current_slot.0, slots_per_kes_period)?;
    check_kes_period(opcert, current_kes_period, max_kes_evolutions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_crypto::ed25519::SigningKey;
    use yggdrasil_crypto::sum_kes::SumKesVerificationKey;

    fn mk_opcert(kes_period: u64) -> (OpCert, VerificationKey) {
        let cold_sk = SigningKey::from_bytes([0x01; 32]);
        let cold_vk = cold_sk.verification_key().unwrap();
        let hot_vkey = SumKesVerificationKey::from_bytes([0xAA; 32]);
        let mut oc = OpCert {
            hot_vkey,
            sequence_number: 0,
            kes_period,
            sigma: yggdrasil_crypto::ed25519::Signature::from_bytes([0; 64]),
        };
        let signable = oc.signable_bytes();
        oc.sigma = cold_sk.sign(&signable).unwrap();
        (oc, cold_vk)
    }

    fn mk_header_body(opcert: OpCert) -> HeaderBody {
        // Derive the actual cold VK from the same seed used in mk_opcert
        let cold_sk = SigningKey::from_bytes([0x01; 32]);
        let cold_vk = cold_sk.verification_key().unwrap();
        HeaderBody {
            block_number: BlockNo(1),
            slot: SlotNo(100),
            prev_hash: Some(HeaderHash([0xBB; 32])),
            issuer_vkey: cold_vk,
            vrf_vkey: VrfVerificationKey([0xCC; 32]),
            leader_vrf_output: vec![0xDD; 64],
            leader_vrf_proof: [0xEE; 80],
            nonce_vrf_output: None,
            nonce_vrf_proof: None,
            block_body_size: 1024,
            block_body_hash: [0xFF; 32],
            operational_cert: opcert,
            protocol_version: (8, 0),
        }
    }

    // ── HeaderBody::to_signable_bytes ────────────────────────────────

    #[test]
    fn signable_bytes_deterministic() {
        let (oc, _) = mk_opcert(0);
        let hb = mk_header_body(oc);
        let b1 = hb.to_signable_bytes();
        let b2 = hb.to_signable_bytes();
        assert_eq!(b1, b2);
    }

    #[test]
    fn signable_bytes_includes_block_number() {
        let (oc, _) = mk_opcert(0);
        let mut hb = mk_header_body(oc.clone());
        let b1 = hb.to_signable_bytes();
        hb.block_number = BlockNo(999);
        let b2 = hb.to_signable_bytes();
        assert_ne!(b1, b2);
    }

    #[test]
    fn signable_bytes_includes_slot() {
        let (oc, _) = mk_opcert(0);
        let mut hb = mk_header_body(oc.clone());
        let b1 = hb.to_signable_bytes();
        hb.slot = SlotNo(9999);
        let b2 = hb.to_signable_bytes();
        assert_ne!(b1, b2);
    }

    #[test]
    fn signable_bytes_prev_hash_none_vs_some() {
        let (oc, _) = mk_opcert(0);
        let mut hb = mk_header_body(oc);
        hb.prev_hash = None;
        let b_none = hb.to_signable_bytes();
        hb.prev_hash = Some(HeaderHash([0x00; 32]));
        let b_some = hb.to_signable_bytes();
        assert_ne!(b_none, b_some);
        // None encodes as 0x00 (1 byte), Some encodes as 0x01 + 32 bytes
        assert!(b_none.len() < b_some.len());
    }

    #[test]
    fn signable_bytes_includes_body_hash() {
        let (oc, _) = mk_opcert(0);
        let mut hb = mk_header_body(oc.clone());
        let b1 = hb.to_signable_bytes();
        hb.block_body_hash = [0x00; 32];
        let b2 = hb.to_signable_bytes();
        assert_ne!(b1, b2);
    }

    #[test]
    fn signable_bytes_includes_protocol_version() {
        let (oc, _) = mk_opcert(0);
        let mut hb = mk_header_body(oc.clone());
        let b1 = hb.to_signable_bytes();
        hb.protocol_version = (9, 0);
        let b2 = hb.to_signable_bytes();
        assert_ne!(b1, b2);
    }

    #[test]
    fn signable_bytes_includes_opcert_data() {
        let (oc1, _) = mk_opcert(0);
        let (oc2, _) = mk_opcert(10);
        let hb1 = mk_header_body(oc1);
        let hb2 = mk_header_body(oc2);
        assert_ne!(hb1.to_signable_bytes(), hb2.to_signable_bytes());
    }

    #[test]
    fn signable_bytes_includes_body_size() {
        let (oc, _) = mk_opcert(0);
        let mut hb = mk_header_body(oc);
        let b1 = hb.to_signable_bytes();
        hb.block_body_size = 9999;
        let b2 = hb.to_signable_bytes();
        assert_ne!(b1, b2);
    }

    // ── verify_opcert_only ───────────────────────────────────────────

    #[test]
    fn verify_opcert_only_valid() {
        let (oc, cold_vk) = mk_opcert(0);
        let result = verify_opcert_only(&oc, &cold_vk, SlotNo(100), 129_600, 62);
        assert!(result.is_ok());
    }

    #[test]
    fn verify_opcert_only_wrong_cold_key() {
        let (oc, _) = mk_opcert(0);
        let wrong_vk = VerificationKey::from_bytes([0xFF; 32]);
        let result = verify_opcert_only(&oc, &wrong_vk, SlotNo(100), 129_600, 62);
        assert!(result.is_err());
    }

    #[test]
    fn verify_opcert_only_expired_kes_period() {
        let (oc, cold_vk) = mk_opcert(0);
        // Slot far in the future — KES period will be past max evolutions
        let far_slot = SlotNo(129_600 * 100);
        let result = verify_opcert_only(&oc, &cold_vk, far_slot, 129_600, 62);
        assert!(matches!(
            result,
            Err(ConsensusError::KesPeriodExpired { .. })
        ));
    }

    #[test]
    fn verify_opcert_only_zero_slots_per_kes() {
        let (oc, cold_vk) = mk_opcert(0);
        let result = verify_opcert_only(&oc, &cold_vk, SlotNo(100), 0, 62);
        assert_eq!(result, Err(ConsensusError::InvalidSlotsPerKesPeriod));
    }

    // ── verify_header (structural validation) ────────────────────────

    #[test]
    fn verify_header_rejects_bad_opcert_signature() {
        let (mut oc, _) = mk_opcert(0);
        // Corrupt the signature
        oc.sigma = yggdrasil_crypto::ed25519::Signature::from_bytes([0xFF; 64]);
        let hb = mk_header_body(oc);
        let header = Header {
            body: hb,
            kes_signature: SumKesSignature::from_bytes(6, &vec![0u8; 64 + 6 * 64]).unwrap(),
        };
        let result = verify_header(&header, 129_600, 62);
        assert_eq!(result, Err(ConsensusError::InvalidOpCertSignature));
    }

    #[test]
    fn verify_header_rejects_expired_kes() {
        let (oc, _) = mk_opcert(0);
        let mut hb = mk_header_body(oc);
        // Place slot far in the future so KES period expires
        hb.slot = SlotNo(129_600 * 100);
        let header = Header {
            body: hb,
            kes_signature: SumKesSignature::from_bytes(6, &vec![0u8; 64 + 6 * 64]).unwrap(),
        };
        let result = verify_header(&header, 129_600, 62);
        assert!(matches!(
            result,
            Err(ConsensusError::KesPeriodExpired { .. })
        ));
    }
}
