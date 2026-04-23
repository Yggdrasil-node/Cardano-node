//! Core protocol-level types shared across ledger, storage, and consensus.
//!
//! These newtypes match upstream Cardano naming from `cardano-slotting` and
//! `ouroboros-network` so that cross-referencing against the official node
//! remains straightforward.

use std::fmt;

use crate::cbor::Decoder;
use crate::error::LedgerError;

// ---------------------------------------------------------------------------
// Slot and block numbering
// ---------------------------------------------------------------------------

/// Absolute slot number on the blockchain.
///
/// Reference: `Cardano.Slotting.Slot` — `SlotNo`.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct SlotNo(pub u64);

/// Absolute block number (height of the chain).
///
/// Reference: `Cardano.Slotting.Block` — `BlockNo`.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct BlockNo(pub u64);

/// Epoch number.
///
/// Reference: `Cardano.Slotting.Slot` — `EpochNo`.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct EpochNo(pub u64);

// ---------------------------------------------------------------------------
// Hash-based identifiers
// ---------------------------------------------------------------------------

/// Blake2b-256 hash of a block header, used as the primary block identifier.
///
/// Reference: `Ouroboros.Consensus.Block.Abstract` — `HeaderHash`.
#[derive(
    Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct HeaderHash(pub [u8; 32]);

impl fmt::Debug for HeaderHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HeaderHash({})", hex_short(&self.0))
    }
}

impl fmt::Display for HeaderHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex_short(&self.0))
    }
}

/// Blake2b-256 hash of a serialized transaction body.
///
/// Reference: `Cardano.Ledger.TxIn` — `TxId`.
#[derive(
    Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct TxId(pub [u8; 32]);

impl fmt::Debug for TxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxId({})", hex_short(&self.0))
    }
}

impl fmt::Display for TxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex_short(&self.0))
    }
}

// ---------------------------------------------------------------------------
// Chain point
// ---------------------------------------------------------------------------

/// A point on the chain, identifying a specific block by its slot and hash.
///
/// `Origin` represents the genesis pseudo-block that precedes slot 0.
///
/// Reference: `Ouroboros.Network.Block` — `Point` (with `GenesisPoint` and
/// `BlockPoint` patterns).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Point {
    /// The genesis pseudo-block (before any real block).
    Origin,
    /// A specific block identified by slot and header hash.
    BlockPoint(SlotNo, HeaderHash),
}

impl Point {
    /// Returns the slot number, or `None` for `Origin`.
    pub fn slot(&self) -> Option<SlotNo> {
        match self {
            Self::Origin => None,
            Self::BlockPoint(s, _) => Some(*s),
        }
    }

    /// Returns the header hash, or `None` for `Origin`.
    pub fn hash(&self) -> Option<HeaderHash> {
        match self {
            Self::Origin => None,
            Self::BlockPoint(_, h) => Some(*h),
        }
    }
}

/// A chain tip: either the genesis tip or a specific point with a block
/// number, matching the upstream `Tip` type in
/// `Ouroboros.Network.Block`.
///
/// Wire encoding:
/// - `[]` — genesis tip
/// - `[slot, hash, blockNo]` — specific tip
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Tip {
    /// The genesis tip (no blocks yet).
    TipGenesis,
    /// A specific tip at the given point with the given block number.
    Tip(Point, BlockNo),
}

impl Tip {
    /// Returns the `Point` component of this tip.
    pub fn point(&self) -> Point {
        match self {
            Self::TipGenesis => Point::Origin,
            Self::Tip(p, _) => *p,
        }
    }

    /// Returns the block number, or `None` for genesis.
    pub fn block_no(&self) -> Option<BlockNo> {
        match self {
            Self::TipGenesis => None,
            Self::Tip(_, bn) => Some(*bn),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Abbreviated hex for display (first 8 bytes).
fn hex_short(bytes: &[u8; 32]) -> String {
    bytes[..8].iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    }) + "…"
}

// ---------------------------------------------------------------------------
// Nonce
// ---------------------------------------------------------------------------

/// A nonce used in the Praos leader election lottery.
///
/// The neutral nonce is an identity element for nonce combination (XOR):
/// combining any nonce with `Neutral` yields that nonce unchanged.
///
/// Reference: `Cardano.Ledger.BaseTypes` — `Nonce` (`NeutralNonce` |
/// `Nonce Hash`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Nonce {
    /// Identity element — does not contribute entropy.
    Neutral,
    /// A 32-byte hash carrying entropy.
    Hash([u8; 32]),
}

impl Nonce {
    /// Combines two nonces by XOR-ing their bytes.
    ///
    /// **⚠ Known upstream-parity gap.** Upstream's `(⭒)` operator is
    /// defined as `Nonce(Blake2b-256(bytesOf(a) ‖ bytesOf(b)))` — a
    /// hash-concatenation, NOT a byte-wise XOR. The canonical reference
    /// is the `Semigroup Nonce` instance in
    /// `Cardano.Ledger.BaseTypes` (cardano-ledger), reused by
    /// `Cardano.Protocol.TPraos.BHeader` as the nonce combinator across
    /// UPDN and TICKN.
    ///
    /// The XOR implementation here is a historical simplification; many
    /// downstream tests (e.g. `nonce_combine_is_xor` in
    /// `crates/consensus/tests/integration.rs`) pin the XOR semantics
    /// directly and would need to be rewritten alongside any switch to
    /// the upstream form. Real-network VRF verification depends on the
    /// epoch-nonce evolution being bit-identical to upstream's, so this
    /// gap blocks mainnet-replay parity and is tracked for a dedicated
    /// follow-up slice rather than squeezed into incremental work.
    ///
    /// Current rules (XOR — local):
    /// * `Neutral ⊕ n = n`
    /// * `n ⊕ Neutral = n`
    /// * `Hash(a) ⊕ Hash(b) = Hash(a XOR b)`
    pub fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Neutral, n) | (n, Self::Neutral) => n,
            (Self::Hash(a), Self::Hash(b)) => {
                let mut out = [0u8; 32];
                for i in 0..32 {
                    out[i] = a[i] ^ b[i];
                }
                Self::Hash(out)
            }
        }
    }

    /// Creates a nonce from a 32-byte header hash.
    ///
    /// Reference: `hashHeaderToNonce` in `BHeader.hs`.
    pub fn from_header_hash(hash: HeaderHash) -> Self {
        Self::Hash(hash.0)
    }
}

// ---------------------------------------------------------------------------
// Credential types
// ---------------------------------------------------------------------------

/// A 28-byte Blake2b-224 hash used as an address key hash.
///
/// CDDL: `addr_keyhash = hash28`
///
/// Reference: `Cardano.Ledger.Keys` — `KeyHash`.
pub type AddrKeyHash = [u8; 28];

/// A 28-byte Blake2b-224 hash used as a script hash.
///
/// CDDL: `scripthash = hash28`
///
/// Reference: `Cardano.Ledger.Hashes` — `ScriptHash`.
pub type ScriptHash = [u8; 28];

/// A 28-byte Blake2b-224 hash used as a pool key hash.
///
/// CDDL: `pool_keyhash = hash28`
pub type PoolKeyHash = [u8; 28];

/// A 28-byte Blake2b-224 hash used as a genesis key hash.
///
/// CDDL: `genesis_hash = hash28`
pub type GenesisHash = [u8; 28];

/// A 28-byte Blake2b-224 hash used as a genesis delegate key hash.
///
/// CDDL: `genesis_delegate_hash = hash28`
pub type GenesisDelegateHash = [u8; 28];

/// A 28-byte Blake2b-224 hash used as a VRF key hash.
///
/// CDDL: `vrf_keyhash = hash32`
pub type VrfKeyHash = [u8; 32];

/// A stake credential identifying a staking participant.
///
/// CDDL: `credential = [0, addr_keyhash] / [1, scripthash]`
///
/// Reference: `Cardano.Ledger.Credential` — `Credential`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StakeCredential {
    /// Key-hash based credential (tag 0).
    AddrKeyHash(AddrKeyHash),
    /// Script-hash based credential (tag 1).
    ScriptHash(ScriptHash),
}

impl StakeCredential {
    /// Returns the raw 28-byte hash regardless of variant.
    pub fn hash(&self) -> &[u8; 28] {
        match self {
            Self::AddrKeyHash(h) | Self::ScriptHash(h) => h,
        }
    }

    /// Returns `true` for a key-hash credential.
    pub fn is_key_hash(&self) -> bool {
        matches!(self, Self::AddrKeyHash(_))
    }

    /// Returns `true` for a script-hash credential.
    pub fn is_script_hash(&self) -> bool {
        matches!(self, Self::ScriptHash(_))
    }
}

// ---------------------------------------------------------------------------
// Move-instantaneous-reward (MIR) types
// ---------------------------------------------------------------------------

/// Which pot to draw from (or transfer to).
///
/// CDDL: `0 / 1`  — 0 = reserves, 1 = treasury.
///
/// Reference: `Cardano.Ledger.Shelley.TxCert` — `MIRPot`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MirPot {
    Reserves = 0,
    Treasury = 1,
}

/// The target of a MIR certificate.
///
/// CDDL: `{ * stake_credential => delta_coin } / coin`
///
/// Reference: `Cardano.Ledger.Shelley.TxCert` — `MIRTarget`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirTarget {
    /// Per-credential reward delta map.  `delta_coin = int` (signed).
    StakeCredentials(std::collections::BTreeMap<StakeCredential, i64>),
    /// Transfer a fixed amount to the opposite pot.
    SendToOppositePot(u64),
}

// ---------------------------------------------------------------------------
// Reward account
// ---------------------------------------------------------------------------

/// A reward account (stake address) used for delegation rewards.
///
/// Encoded as 29 bytes: a 1-byte header (network_id in lower 4 bits,
/// credential type indicator in upper 4 bits) followed by the 28-byte
/// credential hash.
///
/// CDDL: `reward_account = bytes .size 29`
///
/// The header byte for a reward account is `0xe0 | network_id` (key hash)
/// or `0xf0 | network_id` (script hash), following the Shelley address
/// scheme type 14 (key) and type 15 (script).
///
/// Reference: `Cardano.Ledger.Address` — `RewardAccount`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RewardAccount {
    /// Network identifier (0 = testnet, 1 = mainnet).
    pub network: u8,
    /// The staking credential.
    pub credential: StakeCredential,
}

impl RewardAccount {
    /// Serializes the reward account to its canonical 29-byte form.
    pub fn to_bytes(&self) -> [u8; 29] {
        let mut out = [0u8; 29];
        let type_nibble = if self.credential.is_key_hash() {
            0xe0
        } else {
            0xf0
        };
        out[0] = type_nibble | (self.network & 0x0f);
        out[1..29].copy_from_slice(self.credential.hash());
        out
    }

    /// Parses a reward account from a 29-byte slice.
    ///
    /// Returns `None` if the slice length is wrong or the header byte
    /// does not correspond to a valid reward account type (14 or 15).
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 29 {
            return None;
        }
        let header = bytes[0];
        let network = header & 0x0f;
        if !is_valid_network_id(network) {
            return None;
        }
        let addr_type = header >> 4;
        let hash: [u8; 28] = bytes[1..29].try_into().ok()?;
        let credential = match addr_type {
            0x0e => StakeCredential::AddrKeyHash(hash),
            0x0f => StakeCredential::ScriptHash(hash),
            _ => return None,
        };
        Some(Self {
            network,
            credential,
        })
    }

    /// Validates the reward-account network id.
    pub fn validate(&self) -> Result<(), LedgerError> {
        if !is_valid_network_id(self.network) {
            return Err(LedgerError::InvalidAddressNetworkId(self.network));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Address
// ---------------------------------------------------------------------------

/// A Shelley-era address covering the address types defined in the
/// Shelley specification.
///
/// Layout: 1 header byte (`address_type << 4 | network_id`) followed by
/// type-dependent payload bytes.
///
/// Reference: `Cardano.Ledger.Address` — `Addr`, `BootstrapAddress`.
///
/// The header byte upper nibble encodes the address type:
/// * 0x0/0x1 — base address (payment key/script + staking key/script)
/// * 0x2/0x3 — pointer address (payment key/script + stake pointer)
/// * 0x4/0x5 — enterprise address (payment key/script, no staking)
/// * 0x6/0x7 — bootstrap (Byron) address
/// * 0x8 — Byron CBOR-encoded address (handled via raw bytes)
/// * 0xe/0xf — reward account (staking key/script only)
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Address {
    /// Base address: payment credential + staking credential (57 bytes).
    Base(BaseAddress),
    /// Enterprise address: payment credential only (29 bytes).
    Enterprise(EnterpriseAddress),
    /// Pointer address: payment credential + chain pointer (variable length).
    Pointer(PointerAddress),
    /// Reward address / stake address (29 bytes, same as `RewardAccount`).
    Reward(RewardAccount),
    /// Byron-era bootstrap address (variable length, opaque).
    Byron(Vec<u8>),
}

/// A base address carrying both a payment and a staking credential.
///
/// Header type nibbles: 0x0 (key/key), 0x1 (script/key), 0x2 (key/script), 0x3 (script/script).
/// Encoded as 57 bytes: header + 28 payment hash + 28 staking hash.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BaseAddress {
    /// Network identifier (0 = testnet, 1 = mainnet).
    pub network: u8,
    /// Payment credential.
    pub payment: StakeCredential,
    /// Staking credential (delegation).
    pub staking: StakeCredential,
}

/// An enterprise address with a payment credential and no staking part.
///
/// Header type nibbles: 0x6 (key), 0x7 (script).
/// Encoded as 29 bytes: header + 28 payment hash.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EnterpriseAddress {
    /// Network identifier (0 = testnet, 1 = mainnet).
    pub network: u8,
    /// Payment credential.
    pub payment: StakeCredential,
}

/// A pointer address: payment credential + variable-length chain pointer.
///
/// Header type nibbles: 0x4 (key), 0x5 (script).
/// The pointer uses variable-length natural encoding per the Shelley spec.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PointerAddress {
    /// Network identifier (0 = testnet, 1 = mainnet).
    pub network: u8,
    /// Payment credential.
    pub payment: StakeCredential,
    /// Slot in which the stake key was registered.
    pub slot: u64,
    /// Transaction index within that slot's block.
    pub tx_index: u64,
    /// Certificate index within that transaction.
    pub cert_index: u64,
}

impl Address {
    /// Attempts to parse a Shelley-era address from raw bytes.
    ///
    /// Returns `None` if the bytes are too short or the header type nibble
    /// does not map to a known address type.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        let header = bytes[0];
        let addr_type = header >> 4;
        let network = header & 0x0f;

        match addr_type {
            // Base addresses: 0x0 = key/key, 0x1 = script/key,
            //                 0x2 = key/script, 0x3 = script/script
            0x0..=0x3 => {
                if !is_valid_network_id(network) {
                    return None;
                }
                if bytes.len() != 57 {
                    return None;
                }
                let pay_hash: [u8; 28] = bytes[1..29].try_into().ok()?;
                let stk_hash: [u8; 28] = bytes[29..57].try_into().ok()?;
                let payment = if addr_type & 0x01 == 0 {
                    StakeCredential::AddrKeyHash(pay_hash)
                } else {
                    StakeCredential::ScriptHash(pay_hash)
                };
                let staking = if addr_type & 0x02 == 0 {
                    StakeCredential::AddrKeyHash(stk_hash)
                } else {
                    StakeCredential::ScriptHash(stk_hash)
                };
                Some(Self::Base(BaseAddress {
                    network,
                    payment,
                    staking,
                }))
            }
            // Pointer addresses: 0x4 = key, 0x5 = script
            0x4..=0x5 => {
                if !is_valid_network_id(network) {
                    return None;
                }
                if bytes.len() < 30 {
                    return None;
                }
                let pay_hash: [u8; 28] = bytes[1..29].try_into().ok()?;
                let payment = if addr_type == 0x4 {
                    StakeCredential::AddrKeyHash(pay_hash)
                } else {
                    StakeCredential::ScriptHash(pay_hash)
                };
                let mut pos = 29;
                let slot = decode_variable_nat(bytes, &mut pos)?;
                let tx_index = decode_variable_nat(bytes, &mut pos)?;
                let cert_index = decode_variable_nat(bytes, &mut pos)?;
                if pos != bytes.len() {
                    return None;
                }
                Some(Self::Pointer(PointerAddress {
                    network,
                    payment,
                    slot,
                    tx_index,
                    cert_index,
                }))
            }
            // Enterprise addresses: 0x6 = key, 0x7 = script
            0x6..=0x7 => {
                if !is_valid_network_id(network) {
                    return None;
                }
                if bytes.len() != 29 {
                    return None;
                }
                let pay_hash: [u8; 28] = bytes[1..29].try_into().ok()?;
                let payment = if addr_type == 0x6 {
                    StakeCredential::AddrKeyHash(pay_hash)
                } else {
                    StakeCredential::ScriptHash(pay_hash)
                };
                Some(Self::Enterprise(EnterpriseAddress { network, payment }))
            }
            // Byron addresses: 0x8 (legacy CBOR-in-CBOR)
            0x8 => Some(Self::Byron(bytes.to_vec())),
            // Reward accounts: 0xe = key, 0xf = script
            0xe..=0xf => {
                let ra = RewardAccount::from_bytes(bytes)?;
                Some(Self::Reward(ra))
            }
            _ => None,
        }
    }

    /// Parses and deeply validates an address from raw bytes.
    pub fn validate_bytes(bytes: &[u8]) -> Result<Self, LedgerError> {
        let address = Self::from_bytes(bytes)
            .ok_or_else(|| LedgerError::InvalidAddressBytes(bytes.to_vec()))?;
        address.validate()?;
        Ok(address)
    }

    /// Runs additional validation checks that go beyond structural decoding.
    pub fn validate(&self) -> Result<(), LedgerError> {
        match self {
            Self::Base(b) => validate_network_id(b.network),
            Self::Enterprise(e) => validate_network_id(e.network),
            Self::Pointer(p) => validate_network_id(p.network),
            Self::Reward(r) => r.validate(),
            Self::Byron(raw) => validate_byron_address_bytes(raw),
        }
    }

    /// Returns the payment credential for address types that carry one.
    ///
    /// Base, Enterprise, and Pointer addresses carry a payment credential.
    /// Returns `true` if this address is VKey-locked.
    ///
    /// An address is VKey-locked if its payment credential is a key hash
    /// (not a script hash).  Byron bootstrap addresses are also considered
    /// VKey-locked (they are always signed by a key).
    ///
    /// Reference: `Cardano.Ledger.Address` — `isKeyHashAddr` / `vKeyLocked`.
    pub fn is_vkey_locked(&self) -> bool {
        match self {
            Self::Base(b) => b.payment.is_key_hash(),
            Self::Enterprise(e) => e.payment.is_key_hash(),
            Self::Pointer(p) => p.payment.is_key_hash(),
            // Reward addresses are staking-only; treating as non-VKey per
            // upstream (they are not used as payment addresses).
            Self::Reward(_) => false,
            // Byron bootstrap addresses are always key-signed.
            Self::Byron(_) => true,
        }
    }

    /// Reward addresses carry only a staking credential (returned here).
    /// Byron addresses have no extractable credential.
    pub fn payment_credential(&self) -> Option<&StakeCredential> {
        match self {
            Self::Base(b) => Some(&b.payment),
            Self::Enterprise(e) => Some(&e.payment),
            Self::Pointer(p) => Some(&p.payment),
            Self::Reward(r) => Some(&r.credential),
            Self::Byron(_) => None,
        }
    }

    /// Serializes the address to its canonical byte representation.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Base(b) => {
                let mut out = Vec::with_capacity(57);
                let type_nibble = match (&b.payment, &b.staking) {
                    (StakeCredential::AddrKeyHash(_), StakeCredential::AddrKeyHash(_)) => 0x00,
                    (StakeCredential::ScriptHash(_), StakeCredential::AddrKeyHash(_)) => 0x10,
                    (StakeCredential::AddrKeyHash(_), StakeCredential::ScriptHash(_)) => 0x20,
                    (StakeCredential::ScriptHash(_), StakeCredential::ScriptHash(_)) => 0x30,
                };
                out.push(type_nibble | (b.network & 0x0f));
                out.extend_from_slice(b.payment.hash());
                out.extend_from_slice(b.staking.hash());
                out
            }
            Self::Enterprise(e) => {
                let mut out = Vec::with_capacity(29);
                let type_nibble = if e.payment.is_key_hash() { 0x60 } else { 0x70 };
                out.push(type_nibble | (e.network & 0x0f));
                out.extend_from_slice(e.payment.hash());
                out
            }
            Self::Pointer(p) => {
                let mut out = Vec::with_capacity(32);
                let type_nibble = if p.payment.is_key_hash() { 0x40 } else { 0x50 };
                out.push(type_nibble | (p.network & 0x0f));
                out.extend_from_slice(p.payment.hash());
                encode_variable_nat(p.slot, &mut out);
                encode_variable_nat(p.tx_index, &mut out);
                encode_variable_nat(p.cert_index, &mut out);
                out
            }
            Self::Reward(ra) => ra.to_bytes().to_vec(),
            Self::Byron(raw) => raw.clone(),
        }
    }

    /// Returns the network identifier, if determinable.
    pub fn network(&self) -> Option<u8> {
        match self {
            Self::Base(b) => Some(b.network),
            Self::Enterprise(e) => Some(e.network),
            Self::Pointer(p) => Some(p.network),
            Self::Reward(ra) => Some(ra.network),
            Self::Byron(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// UnitInterval (rational number)
// ---------------------------------------------------------------------------

/// A rational number in [0, 1] encoded as tag 30 wrapping a 2-element array.
///
/// CDDL: `unit_interval = #6.30([1, 2])`
///        `nonnegative_interval = #6.30([uint, positive_int])`
///
/// Upstream stores numerator/denominator as a tagged rational.
///
/// Reference: `Cardano.Ledger.BaseTypes` — `UnitInterval`, `BoundedRational`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UnitInterval {
    /// Numerator of the rational.
    pub numerator: u64,
    /// Denominator of the rational (must be > 0).
    pub denominator: u64,
}

// ---------------------------------------------------------------------------
// Relay
// ---------------------------------------------------------------------------

/// A relay entry for a stake pool registration certificate.
///
/// CDDL:
/// ```text
/// relay =
///   [  0, port / null, ipv4 / null, ipv6 / null  ]
/// / [  1, port / null, dns_name  ]
/// / [  2, dns_name  ]
/// ```
///
/// Reference: `Cardano.Ledger.Shelley.TxBody` — `StakePoolRelay`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Relay {
    /// Tag 0: optional port + optional IPv4 + optional IPv6.
    SingleHostAddr(Option<u16>, Option<[u8; 4]>, Option<[u8; 16]>),
    /// Tag 1: optional port + DNS name.
    SingleHostName(Option<u16>, String),
    /// Tag 2: multi-host DNS name.
    MultiHostName(String),
}

// ---------------------------------------------------------------------------
// PoolMetadata
// ---------------------------------------------------------------------------

/// Off-chain pool metadata: URL + hash of the content at that URL.
///
/// CDDL: `pool_metadata = [url, pool_metadata_hash]`
///
/// Reference: `Cardano.Ledger.Shelley.TxBody` — `PoolMetadata`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolMetadata {
    /// URL pointing to pool metadata JSON (max 64 bytes).
    pub url: String,
    /// Blake2b-256 hash of the metadata content.
    pub metadata_hash: [u8; 32],
}

// ---------------------------------------------------------------------------
// PoolParams
// ---------------------------------------------------------------------------

/// Parameters for a stake pool registration certificate.
///
/// CDDL:
/// ```text
/// pool_params = ( operator:       pool_keyhash
///               , vrf_keyhash:    vrf_keyhash
///               , pledge:         coin
///               , cost:           coin
///               , margin:         unit_interval
///               , reward_account: reward_account
///               , pool_owners:    set<addr_keyhash>
///               , relays:         [* relay]
///               , pool_metadata:  pool_metadata / null
///               )
/// ```
///
/// Reference: `Cardano.Ledger.Shelley.TxBody` — `PoolParams`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolParams {
    /// Pool operator key hash.
    pub operator: PoolKeyHash,
    /// VRF verification key hash.
    pub vrf_keyhash: VrfKeyHash,
    /// Pledge (in lovelace).
    pub pledge: u64,
    /// Fixed cost per epoch (in lovelace).
    pub cost: u64,
    /// Pool margin (rational in [0, 1]).
    pub margin: UnitInterval,
    /// Reward account for pool rewards.
    pub reward_account: RewardAccount,
    /// Set of pool-owner key hashes.
    pub pool_owners: Vec<AddrKeyHash>,
    /// Relay entries for the pool.
    pub relays: Vec<Relay>,
    /// Optional off-chain metadata.
    pub pool_metadata: Option<PoolMetadata>,
}

// ---------------------------------------------------------------------------
// Anchor
// ---------------------------------------------------------------------------

/// Off-chain metadata anchor: a URL plus a hash of the data at that URL.
///
/// CDDL: `anchor = [anchor_url : url, anchor_data_hash : $hash32]`
///
/// Reference: `Cardano.Ledger.BaseTypes.Anchor`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Anchor {
    /// URL pointing to off-chain metadata (UTF-8 text).
    pub url: String,
    /// Blake2b-256 hash of the data at the URL.
    pub data_hash: [u8; 32],
}

// ---------------------------------------------------------------------------
// DRep (Conway)
// ---------------------------------------------------------------------------

/// A delegated representative for governance voting (CIP-1694).
///
/// CDDL:
/// ```text
/// drep =
///   [0, addr_keyhash]
/// / [1, scripthash]
/// / [2]  ; always_abstain
/// / [3]  ; always_no_confidence
/// ```
///
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures` — `DRep`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DRep {
    /// DRep identified by key hash (tag 0).
    KeyHash(AddrKeyHash),
    /// DRep identified by script hash (tag 1).
    ScriptHash(ScriptHash),
    /// Always abstain sentinel (tag 2).
    AlwaysAbstain,
    /// Always no-confidence sentinel (tag 3).
    AlwaysNoConfidence,
}

// ---------------------------------------------------------------------------
// DCert (all eras)
// ---------------------------------------------------------------------------

/// Delegation and pool certificate covering Shelley through Conway eras.
///
/// Shelley certificates use tags 0–5; Conway adds tags 7–18 with
/// extended delegation and governance certificate variants.
///
/// CDDL (Shelley):
/// ```text
/// certificate =
///   [0, stake_credential]                          ; account_registration_cert
/// / [1, stake_credential]                          ; account_unregistration_cert
/// / [2, stake_credential, pool_keyhash]            ; delegation_to_stake_pool_cert
/// / [3, pool_params]                               ; pool_registration_cert
/// / [4, pool_keyhash, epoch]                       ; pool_retirement_cert
/// / [5, genesishash, genesis_delegate_hash, vrf_keyhash]  ; genesis_delegation_cert
/// ```
///
/// CDDL (Conway extensions):
/// ```text
/// / [7, stake_credential, coin]                    ; account_registration_deposit_cert
/// / [8, stake_credential, coin]                    ; account_unregistration_deposit_cert
/// / [9, stake_credential, drep]                    ; delegation_to_drep_cert
/// / [10, stake_credential, pool_keyhash, drep]     ; delegation_to_stake_pool_and_drep_cert
/// / [11, stake_credential, pool_keyhash, coin]     ; account_registration_delegation_to_stake_pool_cert
/// / [12, stake_credential, drep, coin]             ; account_registration_delegation_to_drep_cert
/// / [13, stake_credential, pool_keyhash, drep, coin] ; account_registration_delegation_to_stake_pool_and_drep_cert
/// / [14, committee_cold_credential, committee_hot_credential] ; committee_authorization_cert
/// / [15, committee_cold_credential, anchor / null] ; committee_resignation_cert
/// / [16, drep_credential, coin, anchor / null]     ; drep_registration_cert
/// / [17, drep_credential, coin]                    ; drep_unregistration_cert
/// / [18, drep_credential, anchor / null]           ; drep_update_cert
/// ```
///
/// Reference: `Cardano.Ledger.Shelley.TxBody` and
/// `Cardano.Ledger.Conway.TxCert`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DCert {
    // -- Shelley (tags 0–5) --------------------------------------------------
    /// Tag 0: Account registration (`account_registration_cert`).
    AccountRegistration(StakeCredential),
    /// Tag 1: Account unregistration (`account_unregistration_cert`).
    AccountUnregistration(StakeCredential),
    /// Tag 2: Delegation to stake pool (`delegation_to_stake_pool_cert`).
    DelegationToStakePool(StakeCredential, PoolKeyHash),
    /// Tag 3: Pool registration (`pool_registration_cert`).
    PoolRegistration(PoolParams),
    /// Tag 4: Pool retirement (`pool_retirement_cert`).
    PoolRetirement(PoolKeyHash, EpochNo),
    /// Tag 5: Genesis delegation (`genesis_delegation_cert`).
    GenesisDelegation(GenesisHash, GenesisDelegateHash, VrfKeyHash),
    /// Tag 6: Move instantaneous rewards (`move_instantaneous_rewards_cert`).
    ///
    /// Transfers ada between reserves/treasury and reward accounts or between
    /// pots.  Used in Shelley through Babbage; not supported in Conway.
    ///
    /// Reference: `Cardano.Ledger.Shelley.TxCert` — `MIRCert`.
    MoveInstantaneousReward(MirPot, MirTarget),

    // -- Conway (tags 7–18) --------------------------------------------------
    /// Tag 7: Account registration with deposit (`account_registration_deposit_cert`).
    AccountRegistrationDeposit(StakeCredential, u64),
    /// Tag 8: Account unregistration with deposit refund (`account_unregistration_deposit_cert`).
    AccountUnregistrationDeposit(StakeCredential, u64),
    /// Tag 9: Delegation to DRep (`delegation_to_drep_cert`).
    DelegationToDrep(StakeCredential, DRep),
    /// Tag 10: Delegation to stake pool and DRep (`delegation_to_stake_pool_and_drep_cert`).
    DelegationToStakePoolAndDrep(StakeCredential, PoolKeyHash, DRep),
    /// Tag 11: Account registration + delegation to stake pool (`account_registration_delegation_to_stake_pool_cert`).
    AccountRegistrationDelegationToStakePool(StakeCredential, PoolKeyHash, u64),
    /// Tag 12: Account registration + delegation to DRep (`account_registration_delegation_to_drep_cert`).
    AccountRegistrationDelegationToDrep(StakeCredential, DRep, u64),
    /// Tag 13: Account registration + delegation to stake pool and DRep (`account_registration_delegation_to_stake_pool_and_drep_cert`).
    AccountRegistrationDelegationToStakePoolAndDrep(StakeCredential, PoolKeyHash, DRep, u64),
    /// Tag 14: Committee authorization (`committee_authorization_cert`).
    CommitteeAuthorization(StakeCredential, StakeCredential),
    /// Tag 15: Committee resignation (`committee_resignation_cert`).
    CommitteeResignation(StakeCredential, Option<Anchor>),
    /// Tag 16: DRep registration with deposit (`drep_registration_cert`).
    DrepRegistration(StakeCredential, u64, Option<Anchor>),
    /// Tag 17: DRep unregistration with deposit refund (`drep_unregistration_cert`).
    DrepUnregistration(StakeCredential, u64),
    /// Tag 18: DRep update (`drep_update_cert`).
    DrepUpdate(StakeCredential, Option<Anchor>),
}

// ---------------------------------------------------------------------------
// Variable-length natural number encoding (Shelley address pointer)
// ---------------------------------------------------------------------------

/// Decodes a variable-length natural number from `bytes` at position `pos`.
///
/// Each byte contributes 7 bits; the high bit indicates continuation.
/// This matches the Shelley pointer address encoding.
fn decode_variable_nat(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result: u64 = 0;
    loop {
        if *pos >= bytes.len() {
            return None;
        }
        let byte = bytes[*pos];
        *pos += 1;
        result = result.checked_shl(7)? | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Some(result);
        }
    }
}

fn is_valid_network_id(network: u8) -> bool {
    matches!(network, 0 | 1)
}

fn validate_network_id(network: u8) -> Result<(), LedgerError> {
    if is_valid_network_id(network) {
        Ok(())
    } else {
        Err(LedgerError::InvalidAddressNetworkId(network))
    }
}

fn validate_byron_address_bytes(raw: &[u8]) -> Result<(), LedgerError> {
    let mut dec = Decoder::new(raw);
    let len = dec
        .array()
        .map_err(|_| LedgerError::InvalidByronAddressStructure(raw.to_vec()))?;
    if len != 2 {
        return Err(LedgerError::InvalidByronAddressStructure(raw.to_vec()));
    }
    let tag = dec
        .tag()
        .map_err(|_| LedgerError::InvalidByronAddressStructure(raw.to_vec()))?;
    if tag != 24 {
        return Err(LedgerError::InvalidByronAddressStructure(raw.to_vec()));
    }
    let payload = dec
        .bytes()
        .map_err(|_| LedgerError::InvalidByronAddressStructure(raw.to_vec()))?;
    let checksum =
        dec.unsigned()
            .map_err(|_| LedgerError::InvalidByronAddressStructure(raw.to_vec()))? as u32;
    if dec.position() != raw.len() {
        return Err(LedgerError::InvalidByronAddressStructure(raw.to_vec()));
    }
    if crc32_ieee(payload) != checksum {
        return Err(LedgerError::InvalidByronAddressChecksum);
    }
    Ok(())
}

/// Extracts the 28-byte address root from raw Byron address bytes.
///
/// Byron address CBOR: `[tag 24 CBOR([address_root, attributes, type]), CRC32]`
/// The `address_root` is the first element of the inner CBOR array and is
/// a 28-byte Blake2b-224(SHA3-256(...)) hash.
///
/// Reference: `Cardano.Ledger.Address` — `bootstrapKeyHash` extracts
/// `Byron.addrRoot byronAddress` and reinterprets the 28 raw bytes as a
/// `KeyHash`.
pub fn byron_address_root(raw: &[u8]) -> Option<[u8; 28]> {
    let mut dec = Decoder::new(raw);
    // Outer array of length 2: [tag-24-payload, checksum]
    if dec.array().ok()? != 2 {
        return None;
    }
    if dec.tag().ok()? != 24 {
        return None;
    }
    let payload = dec.bytes().ok()?;
    // Inner array of length 3: [address_root, attributes, type]
    let mut inner = Decoder::new(payload);
    if inner.array().ok()? != 3 {
        return None;
    }
    let root = inner.bytes().ok()?;
    if root.len() != 28 {
        return None;
    }
    let mut result = [0u8; 28];
    result.copy_from_slice(root);
    Some(result)
}

fn crc32_ieee(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg() & 0xedb8_8320;
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Encodes a natural number using variable-length encoding into `out`.
fn encode_variable_nat(mut value: u64, out: &mut Vec<u8>) {
    if value == 0 {
        out.push(0);
        return;
    }
    // Collect 7-bit groups, MSB first.
    let mut groups = Vec::new();
    while value > 0 {
        groups.push((value & 0x7f) as u8);
        value >>= 7;
    }
    groups.reverse();
    let last = groups.len() - 1;
    for (i, g) in groups.into_iter().enumerate() {
        if i < last {
            out.push(g | 0x80);
        } else {
            out.push(g);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    // ── SlotNo / BlockNo / EpochNo ─────────────────────────────────────

    #[test]
    fn slot_no_ord() {
        assert!(SlotNo(1) < SlotNo(2));
        assert_eq!(SlotNo(0), SlotNo::default());
    }

    #[test]
    fn block_no_ord() {
        assert!(BlockNo(5) > BlockNo(3));
    }

    #[test]
    fn epoch_no_default_is_zero() {
        assert_eq!(EpochNo::default(), EpochNo(0));
    }

    // ── HeaderHash / TxId display ──────────────────────────────────────

    #[test]
    fn header_hash_debug_and_display() {
        let hh = HeaderHash([0xab; 32]);
        let dbg = format!("{hh:?}");
        assert!(dbg.contains("HeaderHash("));
        let disp = format!("{hh}");
        // Short hex: first 8 bytes = "abababababababab…"
        assert!(disp.starts_with("abab"));
        assert!(disp.ends_with('…'));
    }

    #[test]
    fn tx_id_debug_and_display() {
        let tid = TxId([0x01; 32]);
        let dbg = format!("{tid:?}");
        assert!(dbg.contains("TxId("));
        let disp = format!("{tid}");
        assert!(disp.starts_with("0101"));
    }

    // ── Point ──────────────────────────────────────────────────────────

    #[test]
    fn point_origin_accessors() {
        let p = Point::Origin;
        assert_eq!(p.slot(), None);
        assert_eq!(p.hash(), None);
    }

    #[test]
    fn point_block_point_accessors() {
        let hh = HeaderHash([0xff; 32]);
        let p = Point::BlockPoint(SlotNo(42), hh);
        assert_eq!(p.slot(), Some(SlotNo(42)));
        assert_eq!(p.hash(), Some(hh));
    }

    // ── Nonce ──────────────────────────────────────────────────────────

    #[test]
    fn nonce_neutral_combine_identity() {
        let n = Nonce::Hash([0xaa; 32]);
        assert_eq!(n.combine(Nonce::Neutral), n);
        assert_eq!(Nonce::Neutral.combine(n), n);
    }

    #[test]
    fn nonce_combine_xor() {
        let a = Nonce::Hash([0xff; 32]);
        let b = Nonce::Hash([0x0f; 32]);
        let c = a.combine(b);
        if let Nonce::Hash(h) = c {
            assert!(h.iter().all(|&byte| byte == 0xf0));
        } else {
            panic!("Expected Hash");
        }
    }

    #[test]
    fn nonce_neutral_combine_neutral_is_neutral() {
        assert_eq!(Nonce::Neutral.combine(Nonce::Neutral), Nonce::Neutral);
    }

    #[test]
    fn nonce_from_header_hash() {
        let hh = HeaderHash([0x42; 32]);
        let n = Nonce::from_header_hash(hh);
        assert_eq!(n, Nonce::Hash([0x42; 32]));
    }

    // ── StakeCredential ────────────────────────────────────────────────

    #[test]
    fn stake_credential_hash() {
        let h = [0x01; 28];
        let sc = StakeCredential::AddrKeyHash(h);
        assert_eq!(sc.hash(), &h);
        assert!(sc.is_key_hash());
        assert!(!sc.is_script_hash());
    }

    #[test]
    fn stake_credential_script_variant() {
        let h = [0x02; 28];
        let sc = StakeCredential::ScriptHash(h);
        assert_eq!(sc.hash(), &h);
        assert!(!sc.is_key_hash());
        assert!(sc.is_script_hash());
    }

    // ── RewardAccount ──────────────────────────────────────────────────

    #[test]
    fn reward_account_key_hash_round_trip() {
        let ra = RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xaa; 28]),
        };
        let bytes = ra.to_bytes();
        assert_eq!(bytes.len(), 29);
        // Header: 0xe0 | 1 = 0xe1
        assert_eq!(bytes[0], 0xe1);
        let decoded = RewardAccount::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, ra);
    }

    #[test]
    fn reward_account_script_hash_round_trip() {
        let ra = RewardAccount {
            network: 0,
            credential: StakeCredential::ScriptHash([0xbb; 28]),
        };
        let bytes = ra.to_bytes();
        // Header: 0xf0 | 0 = 0xf0
        assert_eq!(bytes[0], 0xf0);
        let decoded = RewardAccount::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, ra);
    }

    #[test]
    fn reward_account_from_bytes_wrong_length() {
        assert!(RewardAccount::from_bytes(&[0xe1; 10]).is_none());
    }

    #[test]
    fn reward_account_from_bytes_bad_header() {
        // Type nibble 0x0d is not a valid reward account type
        let mut bytes = [0u8; 29];
        bytes[0] = 0xd1;
        assert!(RewardAccount::from_bytes(&bytes).is_none());
    }

    #[test]
    fn reward_account_from_bytes_bad_network() {
        // network 5 is invalid (only 0 and 1 valid)
        let mut bytes = [0u8; 29];
        bytes[0] = 0xe5;
        assert!(RewardAccount::from_bytes(&bytes).is_none());
    }

    #[test]
    fn reward_account_validate_valid() {
        let ra = RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0x01; 28]),
        };
        assert!(ra.validate().is_ok());
    }

    #[test]
    fn reward_account_validate_bad_network() {
        let ra = RewardAccount {
            network: 3,
            credential: StakeCredential::AddrKeyHash([0x01; 28]),
        };
        assert!(ra.validate().is_err());
    }

    // ── Address: Base ──────────────────────────────────────────────────

    #[test]
    fn base_address_key_key_round_trip() {
        let addr = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0x11; 28]),
            staking: StakeCredential::AddrKeyHash([0x22; 28]),
        });
        let bytes = addr.to_bytes();
        assert_eq!(bytes.len(), 57);
        assert_eq!(bytes[0], 0x01); // type 0, network 1
        let decoded = Address::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, addr);
    }

    #[test]
    fn base_address_script_script_round_trip() {
        let addr = Address::Base(BaseAddress {
            network: 0,
            payment: StakeCredential::ScriptHash([0x33; 28]),
            staking: StakeCredential::ScriptHash([0x44; 28]),
        });
        let bytes = addr.to_bytes();
        assert_eq!(bytes[0], 0x30); // type 3, network 0
        let decoded = Address::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, addr);
    }

    #[test]
    fn base_address_key_script_round_trip() {
        let addr = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0x55; 28]),
            staking: StakeCredential::ScriptHash([0x66; 28]),
        });
        let bytes = addr.to_bytes();
        assert_eq!(bytes[0], 0x21); // type 2, network 1
        let decoded = Address::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, addr);
    }

    #[test]
    fn base_address_script_key_round_trip() {
        let addr = Address::Base(BaseAddress {
            network: 0,
            payment: StakeCredential::ScriptHash([0x77; 28]),
            staking: StakeCredential::AddrKeyHash([0x88; 28]),
        });
        let bytes = addr.to_bytes();
        assert_eq!(bytes[0], 0x10); // type 1, network 0
        let decoded = Address::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, addr);
    }

    // ── Address: Enterprise ────────────────────────────────────────────

    #[test]
    fn enterprise_address_key_round_trip() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0xab; 28]),
        });
        let bytes = addr.to_bytes();
        assert_eq!(bytes.len(), 29);
        assert_eq!(bytes[0], 0x61); // type 6, network 1
        let decoded = Address::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, addr);
    }

    #[test]
    fn enterprise_address_script_round_trip() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 0,
            payment: StakeCredential::ScriptHash([0xcd; 28]),
        });
        let bytes = addr.to_bytes();
        assert_eq!(bytes[0], 0x70); // type 7, network 0
        let decoded = Address::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, addr);
    }

    // ── Address: Pointer ───────────────────────────────────────────────

    #[test]
    fn pointer_address_round_trip() {
        let addr = Address::Pointer(PointerAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0x01; 28]),
            slot: 100,
            tx_index: 2,
            cert_index: 0,
        });
        let bytes = addr.to_bytes();
        assert_eq!(bytes[0], 0x41); // type 4, network 1
        let decoded = Address::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, addr);
    }

    #[test]
    fn pointer_address_script_round_trip() {
        let addr = Address::Pointer(PointerAddress {
            network: 0,
            payment: StakeCredential::ScriptHash([0x02; 28]),
            slot: 0,
            tx_index: 0,
            cert_index: 0,
        });
        let bytes = addr.to_bytes();
        assert_eq!(bytes[0], 0x50); // type 5, network 0
        let decoded = Address::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, addr);
    }

    // ── Address: Reward ────────────────────────────────────────────────

    #[test]
    fn reward_address_round_trip() {
        let addr = Address::Reward(RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xfe; 28]),
        });
        let bytes = addr.to_bytes();
        assert_eq!(bytes[0], 0xe1);
        let decoded = Address::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, addr);
    }

    // ── Address: Byron ─────────────────────────────────────────────────

    #[test]
    fn byron_address_parses_with_type_8() {
        // Construct a minimal type-8 header byte + dummy content
        let mut raw = vec![0x80]; // type 8, network 0
        raw.extend_from_slice(&[0x00; 28]);
        let decoded = Address::from_bytes(&raw);
        match decoded {
            Some(Address::Byron(b)) => assert_eq!(b, raw),
            _ => panic!("Expected Byron address"),
        }
    }

    // ── Address: error cases ───────────────────────────────────────────

    #[test]
    fn address_from_bytes_empty() {
        assert!(Address::from_bytes(&[]).is_none());
    }

    #[test]
    fn address_from_bytes_unknown_type() {
        // Type nibble 0x9 is not defined
        let mut bytes = [0u8; 29];
        bytes[0] = 0x91;
        assert!(Address::from_bytes(&bytes).is_none());
    }

    #[test]
    fn address_from_bytes_base_wrong_length() {
        // Base address must be 57 bytes
        let bytes = vec![0x01; 30];
        assert!(Address::from_bytes(&bytes).is_none());
    }

    #[test]
    fn address_from_bytes_enterprise_wrong_length() {
        let bytes = vec![0x61; 10];
        assert!(Address::from_bytes(&bytes).is_none());
    }

    #[test]
    fn address_from_bytes_bad_network() {
        // Base address with network=5 (invalid)
        let mut bytes = vec![0u8; 57];
        bytes[0] = 0x05; // type 0, network 5
        assert!(Address::from_bytes(&bytes).is_none());
    }

    // ── Address: payment_credential ────────────────────────────────────

    #[test]
    fn payment_credential_base() {
        let cred = StakeCredential::AddrKeyHash([0x01; 28]);
        let addr = Address::Base(BaseAddress {
            network: 1,
            payment: cred,
            staking: StakeCredential::AddrKeyHash([0x02; 28]),
        });
        assert_eq!(addr.payment_credential(), Some(&cred));
    }

    #[test]
    fn payment_credential_enterprise() {
        let cred = StakeCredential::ScriptHash([0x03; 28]);
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 0,
            payment: cred,
        });
        assert_eq!(addr.payment_credential(), Some(&cred));
    }

    #[test]
    fn payment_credential_byron_is_none() {
        let addr = Address::Byron(vec![0x80, 0x00]);
        assert!(addr.payment_credential().is_none());
    }

    // ── Address: is_vkey_locked ────────────────────────────────────────

    #[test]
    fn vkey_locked_base_key_key() {
        let addr = Address::Base(BaseAddress {
            network: 0,
            payment: StakeCredential::AddrKeyHash([0; 28]),
            staking: StakeCredential::AddrKeyHash([1; 28]),
        });
        assert!(addr.is_vkey_locked());
    }

    #[test]
    fn vkey_locked_base_script_key_is_not() {
        let addr = Address::Base(BaseAddress {
            network: 0,
            payment: StakeCredential::ScriptHash([0; 28]),
            staking: StakeCredential::AddrKeyHash([1; 28]),
        });
        assert!(!addr.is_vkey_locked());
    }

    #[test]
    fn vkey_locked_enterprise_key() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 0,
            payment: StakeCredential::AddrKeyHash([0; 28]),
        });
        assert!(addr.is_vkey_locked());
    }

    #[test]
    fn vkey_locked_enterprise_script_is_not() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 0,
            payment: StakeCredential::ScriptHash([0; 28]),
        });
        assert!(!addr.is_vkey_locked());
    }

    #[test]
    fn vkey_locked_byron_is_true() {
        let addr = Address::Byron(vec![0x82, 0x00]);
        assert!(addr.is_vkey_locked());
    }

    #[test]
    fn vkey_locked_reward_is_false() {
        let addr = Address::Reward(RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0; 28]),
        });
        assert!(!addr.is_vkey_locked());
    }

    // ── Address: network ───────────────────────────────────────────────

    #[test]
    fn address_network() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0; 28]),
        });
        assert_eq!(addr.network(), Some(1));
    }

    #[test]
    fn byron_address_network_is_none() {
        let addr = Address::Byron(vec![0x80]);
        assert_eq!(addr.network(), None);
    }

    // ── Address: validate_bytes ────────────────────────────────────────

    #[test]
    fn validate_bytes_valid_enterprise() {
        let addr = Address::Enterprise(EnterpriseAddress {
            network: 0,
            payment: StakeCredential::AddrKeyHash([0x01; 28]),
        });
        let bytes = addr.to_bytes();
        assert!(Address::validate_bytes(&bytes).is_ok());
    }

    #[test]
    fn validate_bytes_invalid_returns_error() {
        assert!(Address::validate_bytes(&[]).is_err());
    }

    // ── variable-length nat encoding ───────────────────────────────────

    #[test]
    fn variable_nat_zero() {
        let mut out = Vec::new();
        encode_variable_nat(0, &mut out);
        assert_eq!(out, [0x00]);
        let mut pos = 0;
        assert_eq!(decode_variable_nat(&out, &mut pos), Some(0));
        assert_eq!(pos, 1);
    }

    #[test]
    fn variable_nat_small_value() {
        let mut out = Vec::new();
        encode_variable_nat(127, &mut out);
        let mut pos = 0;
        assert_eq!(decode_variable_nat(&out, &mut pos), Some(127));
    }

    #[test]
    fn variable_nat_multi_byte() {
        let mut out = Vec::new();
        encode_variable_nat(128, &mut out);
        assert!(out.len() > 1);
        let mut pos = 0;
        assert_eq!(decode_variable_nat(&out, &mut pos), Some(128));
    }

    #[test]
    fn variable_nat_large_value_round_trip() {
        let mut out = Vec::new();
        let val = 100_000_000u64;
        encode_variable_nat(val, &mut out);
        let mut pos = 0;
        assert_eq!(decode_variable_nat(&out, &mut pos), Some(val));
        assert_eq!(pos, out.len());
    }

    #[test]
    fn decode_variable_nat_empty_returns_none() {
        let mut pos = 0;
        assert_eq!(decode_variable_nat(&[], &mut pos), None);
    }

    // ── hex_short ──────────────────────────────────────────────────────

    #[test]
    fn hex_short_format() {
        let bytes = [0u8; 32];
        let s = hex_short(&bytes);
        assert_eq!(s, "0000000000000000…");
    }

    // ── crc32_ieee ─────────────────────────────────────────────────────

    #[test]
    fn crc32_ieee_empty() {
        assert_eq!(crc32_ieee(&[]), 0x0000_0000);
    }

    #[test]
    fn crc32_ieee_known_value() {
        // CRC-32 of "123456789" = 0xCBF43926
        let val = crc32_ieee(b"123456789");
        assert_eq!(val, 0xCBF4_3926);
    }

    // ── MirPot / MirTarget ─────────────────────────────────────────────

    #[test]
    fn mir_pot_values() {
        assert_eq!(MirPot::Reserves as u8, 0);
        assert_eq!(MirPot::Treasury as u8, 1);
    }

    #[test]
    fn mir_target_send_to_opposite_pot() {
        let target = MirTarget::SendToOppositePot(1_000_000);
        if let MirTarget::SendToOppositePot(amt) = target {
            assert_eq!(amt, 1_000_000);
        }
    }

    // ── DRep ───────────────────────────────────────────────────────────

    #[test]
    fn drep_variants() {
        let d1 = DRep::KeyHash([0x01; 28]);
        let d2 = DRep::ScriptHash([0x02; 28]);
        let d3 = DRep::AlwaysAbstain;
        let d4 = DRep::AlwaysNoConfidence;
        assert_ne!(d1, d2);
        assert_ne!(d3, d4);
    }

    #[test]
    fn drep_ord() {
        // AlwaysAbstain < AlwaysNoConfidence by derive ordering
        assert!(DRep::AlwaysAbstain < DRep::AlwaysNoConfidence);
    }

    // ── DCert variant construction ─────────────────────────────────────

    #[test]
    fn dcert_account_registration() {
        let cred = StakeCredential::AddrKeyHash([0x10; 28]);
        let cert = DCert::AccountRegistration(cred);
        if let DCert::AccountRegistration(c) = cert {
            assert_eq!(c, cred);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn dcert_pool_retirement() {
        let cert = DCert::PoolRetirement([0xaa; 28], EpochNo(100));
        if let DCert::PoolRetirement(pool, epoch) = cert {
            assert_eq!(pool, [0xaa; 28]);
            assert_eq!(epoch, EpochNo(100));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn dcert_drep_registration_with_anchor() {
        let anchor = Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0x01; 32],
        };
        let cert = DCert::DrepRegistration(
            StakeCredential::AddrKeyHash([0x01; 28]),
            2_000_000,
            Some(anchor.clone()),
        );
        if let DCert::DrepRegistration(_, deposit, anc) = cert {
            assert_eq!(deposit, 2_000_000);
            assert_eq!(anc.unwrap().url, anchor.url);
        }
    }

    // ── UnitInterval ───────────────────────────────────────────────────

    #[test]
    fn unit_interval_fields() {
        let ui = UnitInterval {
            numerator: 1,
            denominator: 2,
        };
        assert_eq!(ui.numerator, 1);
        assert_eq!(ui.denominator, 2);
    }

    // ── Anchor ─────────────────────────────────────────────────────────

    #[test]
    fn anchor_fields() {
        let a = Anchor {
            url: "https://metadata.example".to_string(),
            data_hash: [0xff; 32],
        };
        assert_eq!(a.url.len(), 24);
        assert_eq!(a.data_hash, [0xff; 32]);
    }

    // ── PoolParams ─────────────────────────────────────────────────────

    #[test]
    fn pool_params_construction() {
        let pp = PoolParams {
            operator: [0x01; 28],
            vrf_keyhash: [0x02; 32],
            pledge: 500_000_000,
            cost: 340_000_000,
            margin: UnitInterval {
                numerator: 1,
                denominator: 100,
            },
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0x03; 28]),
            },
            pool_owners: vec![[0x04; 28]],
            relays: vec![Relay::MultiHostName("relay.example.com".to_string())],
            pool_metadata: Some(PoolMetadata {
                url: "https://pool.example".to_string(),
                metadata_hash: [0x05; 32],
            }),
        };
        assert_eq!(pp.pledge, 500_000_000);
        assert_eq!(pp.pool_owners.len(), 1);
        assert_eq!(pp.relays.len(), 1);
        assert!(pp.pool_metadata.is_some());
    }

    // ── Relay ──────────────────────────────────────────────────────────

    #[test]
    fn relay_variants() {
        let r1 = Relay::SingleHostAddr(Some(3001), Some([127, 0, 0, 1]), None);
        let r2 = Relay::SingleHostName(Some(3001), "relay.example.com".to_string());
        let r3 = Relay::MultiHostName("pool.example.com".to_string());
        assert_ne!(r1, r2);
        assert_ne!(r2, r3);
    }

    // ── PoolMetadata ───────────────────────────────────────────────────

    #[test]
    fn pool_metadata_fields() {
        let pm = PoolMetadata {
            url: "https://meta.pool".to_string(),
            metadata_hash: [0xab; 32],
        };
        assert_eq!(pm.url, "https://meta.pool");
    }
}
