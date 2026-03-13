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
    // 1. Verify the OpCert cold-key signature.
    header.body.operational_cert.verify(&header.body.issuer_vkey)?;

    // 2. Check the KES period window.
    let current_kes_period = kes_period_of_slot(header.body.slot.0, slots_per_kes_period)?;
    check_kes_period(&header.body.operational_cert, current_kes_period, max_kes_evolutions)?;

    // 3. Verify the KES signature over the header body.
    let signable = header.body.to_signable_bytes();
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
        &signable,
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
