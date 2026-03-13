//! Core protocol-level types shared across ledger, storage, and consensus.
//!
//! These newtypes match upstream Cardano naming from `cardano-slotting` and
//! `ouroboros-network` so that cross-referencing against the official node
//! remains straightforward.

use std::fmt;

// ---------------------------------------------------------------------------
// Slot and block numbering
// ---------------------------------------------------------------------------

/// Absolute slot number on the blockchain.
///
/// Reference: `Cardano.Slotting.Slot` — `SlotNo`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SlotNo(pub u64);

/// Absolute block number (height of the chain).
///
/// Reference: `Cardano.Slotting.Block` — `BlockNo`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlockNo(pub u64);

/// Epoch number.
///
/// Reference: `Cardano.Slotting.Slot` — `EpochNo`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EpochNo(pub u64);

// ---------------------------------------------------------------------------
// Hash-based identifiers
// ---------------------------------------------------------------------------

/// Blake2b-256 hash of a block header, used as the primary block identifier.
///
/// Reference: `Ouroboros.Consensus.Block.Abstract` — `HeaderHash`.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Abbreviated hex for display (first 8 bytes).
fn hex_short(bytes: &[u8; 32]) -> String {
    bytes[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        + "…"
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
    /// Reference: upstream `(⭒)` operator on `Nonce`.
    ///
    /// Rules:
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
                let type_nibble = if e.payment.is_key_hash() {
                    0x60
                } else {
                    0x70
                };
                out.push(type_nibble | (e.network & 0x0f));
                out.extend_from_slice(e.payment.hash());
                out
            }
            Self::Pointer(p) => {
                let mut out = Vec::with_capacity(32);
                let type_nibble = if p.payment.is_key_hash() {
                    0x40
                } else {
                    0x50
                };
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
///   [0, stake_credential]                          ; stake_registration
/// / [1, stake_credential]                          ; stake_deregistration
/// / [2, stake_credential, pool_keyhash]            ; stake_delegation
/// / [3, pool_params]                               ; pool_registration
/// / [4, pool_keyhash, epoch]                       ; pool_retirement
/// / [5, genesishash, genesis_delegate_hash, vrf_keyhash]  ; genesis_key_delegation
/// ```
///
/// CDDL (Conway extensions):
/// ```text
/// / [7, stake_credential, coin]                    ; reg_cert
/// / [8, stake_credential, coin]                    ; unreg_cert
/// / [9, stake_credential, drep]                    ; vote_deleg_cert
/// / [10, stake_credential, pool_keyhash]           ; stake_vote_deleg_cert
/// / [11, stake_credential, pool_keyhash, drep]     ; stake_reg_deleg_cert (combined)
/// / [12, stake_credential, drep, coin]             ; vote_reg_deleg_cert
/// / [13, stake_credential, pool_keyhash, drep, coin] ; stake_vote_reg_deleg_cert
/// / [14, committee_cold_credential, committee_hot_credential] ; auth_committee_hot_cert
/// / [15, committee_cold_credential, anchor / null] ; resign_committee_cold_cert
/// / [16, drep_credential, coin, anchor / null]     ; reg_drep_cert
/// / [17, drep_credential]                          ; unreg_drep_cert
/// / [18, drep_credential, anchor / null]           ; update_drep_cert
/// ```
///
/// Reference: `Cardano.Ledger.Shelley.TxBody` and
/// `Cardano.Ledger.Conway.TxCert`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DCert {
    // -- Shelley (tags 0–5) --------------------------------------------------

    /// Tag 0: Stake key registration.
    StakeRegistration(StakeCredential),
    /// Tag 1: Stake key deregistration.
    StakeDeregistration(StakeCredential),
    /// Tag 2: Delegate stake to a pool.
    StakeDelegation(StakeCredential, PoolKeyHash),
    /// Tag 3: Register a stake pool.
    PoolRegistration(PoolParams),
    /// Tag 4: Retire a stake pool at the given epoch.
    PoolRetirement(PoolKeyHash, EpochNo),
    /// Tag 5: Genesis key delegation.
    GenesisKeyDelegation(GenesisHash, GenesisDelegateHash, VrfKeyHash),

    // -- Conway (tags 7–18) --------------------------------------------------

    /// Tag 7: Registration certificate with deposit.
    RegCert(StakeCredential, u64),
    /// Tag 8: Unregistration certificate with deposit refund.
    UnregCert(StakeCredential, u64),
    /// Tag 9: Vote delegation certificate.
    VoteDelegCert(StakeCredential, DRep),
    /// Tag 10: Stake-and-vote delegation certificate.
    StakeVoteDelegCert(StakeCredential, PoolKeyHash, DRep),
    /// Tag 11: Stake registration + delegation combined.
    StakeRegDelegCert(StakeCredential, PoolKeyHash, u64),
    /// Tag 12: Vote registration + delegation combined.
    VoteRegDelegCert(StakeCredential, DRep, u64),
    /// Tag 13: Stake + vote registration + delegation combined.
    StakeVoteRegDelegCert(StakeCredential, PoolKeyHash, DRep, u64),
    /// Tag 14: Authorize committee hot credential.
    AuthCommitteeHotCert(StakeCredential, StakeCredential),
    /// Tag 15: Resign committee cold credential.
    ResignCommitteeColdCert(StakeCredential, Option<Anchor>),
    /// Tag 16: Register DRep with deposit and optional anchor.
    RegDRepCert(StakeCredential, u64, Option<Anchor>),
    /// Tag 17: Unregister DRep with deposit refund.
    UnregDRepCert(StakeCredential, u64),
    /// Tag 18: Update DRep certificate with optional anchor.
    UpdateDRepCert(StakeCredential, Option<Anchor>),
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
