//! Upstream-faithful Cardano LocalStateQuery query/result codec.
//!
//! This module implements the wire-format query/result codec used by
//! upstream `cardano-node` + `cardano-cli` over the LocalStateQuery
//! mini-protocol — the layered system documented in
//! [`Ouroboros.Consensus.Ledger.Query`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Ledger/Query.hs)
//! and
//! [`Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/HardFork/Combinator/Serialisation/SerialiseNodeToClient.hs).
//!
//! The on-wire shape is three layered sum types:
//!
//! 1. [`UpstreamQuery`] — the **top-level query envelope** (mirrors
//!    upstream `Query blk`).  Wire tags 0–4 select between
//!    `BlockQuery`, `GetSystemStart`, `GetChainBlockNo`,
//!    `GetChainPoint`, `DebugLedgerConfig`.
//! 2. [`HardForkBlockQuery`] — the **HardForkBlock layer** under
//!    `BlockQuery` (mirrors upstream `SomeBlockQuery (HardForkBlock
//!    xs)`).  Wire tags 0–2 select between `QueryIfCurrent`,
//!    `QueryAnytime`, `QueryHardFork`.
//! 3. [`QueryHardFork`] — the **hard-fork-anytime sub-queries**
//!    (mirrors upstream `QueryHardFork`).  Wire tags 0–1 select
//!    between `GetInterpreter` and `GetCurrentEra`.
//!
//! The `QueryAnytime` sub-layer is also represented by
//! [`QueryAnytimeKind`] (tag 0 = `GetEraStart`).
//!
//! # Captured wire payloads
//!
//! Round 147 (2026-04-27 haskell-parity rehearsal) captured these
//! payloads from `cardano-cli 10.16.0.0 query tip --testnet-magic 1`:
//!
//! ```text
//! 82 00 82 02 81 01    →  BlockQuery (QueryHardFork GetCurrentEra)
//! 82 00 82 02 81 00    →  BlockQuery (QueryHardFork GetInterpreter)
//! ```
//!
//! Both decode through this module's [`UpstreamQuery::decode`].
//!
//! # Reference
//!
//! - Top-level Query: `Ouroboros.Consensus.Ledger.Query` —
//!   `queryEncodeNodeToClient` / `queryDecodeNodeToClient`.
//! - HardForkBlock layer:
//!   `Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`
//!   — `encodeQueryHfc` / `decodeQueryHfc`.
//! - QueryHardFork inner: `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query`
//!   — `encodeQueryHardFork` / `decodeQueryHardFork`.

use yggdrasil_ledger::cbor::{Decoder, Encoder};
use yggdrasil_ledger::{LedgerError, Point};

// ---------------------------------------------------------------------------
// Top-level Query envelope
// ---------------------------------------------------------------------------

/// The top-level query envelope (upstream `Query blk`).
///
/// Wire encoding (per
/// [`queryEncodeNodeToClient`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Ledger/Query.hs)):
///
/// | Tag | Length | Variant            |
/// |-----|--------|--------------------|
/// |  0  |   2    | `BlockQuery(_)`    |
/// |  1  |   1    | `GetSystemStart`   |
/// |  2  |   1    | `GetChainBlockNo`  |
/// |  3  |   3    | `GetChainPoint`    |
/// |  4  |   1    | `DebugLedgerConfig`|
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpstreamQuery {
    /// `[0, <hfc-block-query>]` — query the current chain state.
    BlockQuery(HardForkBlockQuery),
    /// `[1]` — return the genesis system start time.
    GetSystemStart,
    /// `[2]` — return the current chain tip's block number.
    GetChainBlockNo,
    /// `[3]` — return the current chain tip as a `Point`.
    GetChainPoint,
    /// `[4]` — debug query for ledger config (post-V3 NtC only).
    DebugLedgerConfig,
}

impl UpstreamQuery {
    /// Encode this query as upstream-faithful CBOR.
    pub fn encode(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::BlockQuery(inner) => {
                enc.array(2);
                enc.unsigned(0);
                enc.raw(&inner.encode());
            }
            Self::GetSystemStart => {
                enc.array(1);
                enc.unsigned(1);
            }
            Self::GetChainBlockNo => {
                enc.array(1);
                enc.unsigned(2);
            }
            Self::GetChainPoint => {
                enc.array(1);
                enc.unsigned(3);
            }
            Self::DebugLedgerConfig => {
                enc.array(1);
                enc.unsigned(4);
            }
        }
        enc.into_bytes()
    }

    /// Decode an upstream-shaped query payload.  Returns `Err` if the
    /// payload does not match a known wire tag.
    pub fn decode(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match (len, tag) {
            (2, 0) => {
                let inner_start = dec.position();
                dec.skip()?;
                let inner_end = dec.position();
                let inner = HardForkBlockQuery::decode(&bytes[inner_start..inner_end])?;
                Ok(Self::BlockQuery(inner))
            }
            (1, 1) => Ok(Self::GetSystemStart),
            (1, 2) => Ok(Self::GetChainBlockNo),
            (1, 3) => Ok(Self::GetChainPoint),
            (1, 4) => Ok(Self::DebugLedgerConfig),
            _ => Err(LedgerError::CborDecodeError(format!(
                "UpstreamQuery: unrecognised (len={len}, tag={tag})"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// HardForkBlock layer
// ---------------------------------------------------------------------------

/// HardForkBlock query layer (upstream `SomeBlockQuery (HardForkBlock xs)`).
///
/// Wire encoding (per
/// [`encodeQueryHfc`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/HardFork/Combinator/Serialisation/SerialiseNodeToClient.hs)):
///
/// | Tag | Length | Variant                                      |
/// |-----|--------|----------------------------------------------|
/// |  0  |   2    | `QueryIfCurrent(<era-specific block query>)` |
/// |  1  |   3    | `QueryAnytime(kind, era_index)`              |
/// |  2  |   2    | `QueryHardFork(<inner>)`                     |
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HardForkBlockQuery {
    /// `[0, <era-specific-block-query>]` — fail with `MismatchEraInfo`
    /// if the active era doesn't match the inner query's era. The inner
    /// payload is era-specific and intentionally opaque to this codec
    /// layer.
    QueryIfCurrent { inner_cbor: Vec<u8> },
    /// `[1, <some-query>, <era-index>]` — query data from a specific
    /// era's snapshot, regardless of the current era.
    QueryAnytime {
        kind: QueryAnytimeKind,
        era_index: u32,
    },
    /// `[2, <hard-fork-query>]` — query era-history information.
    QueryHardFork(QueryHardFork),
}

impl HardForkBlockQuery {
    pub fn encode(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::QueryIfCurrent { inner_cbor } => {
                enc.array(2);
                enc.unsigned(0);
                enc.raw(inner_cbor);
            }
            Self::QueryAnytime { kind, era_index } => {
                enc.array(3);
                enc.unsigned(1);
                enc.raw(&kind.encode());
                enc.unsigned(*era_index as u64);
            }
            Self::QueryHardFork(inner) => {
                enc.array(2);
                enc.unsigned(2);
                enc.raw(&inner.encode());
            }
        }
        enc.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match (len, tag) {
            (2, 0) => {
                let start = dec.position();
                dec.skip()?;
                let end = dec.position();
                Ok(Self::QueryIfCurrent {
                    inner_cbor: bytes[start..end].to_vec(),
                })
            }
            (3, 1) => {
                let kind_start = dec.position();
                dec.skip()?;
                let kind_end = dec.position();
                let kind = QueryAnytimeKind::decode(&bytes[kind_start..kind_end])?;
                let era_index = dec.unsigned()? as u32;
                Ok(Self::QueryAnytime { kind, era_index })
            }
            (2, 2) => {
                let start = dec.position();
                dec.skip()?;
                let end = dec.position();
                let inner = QueryHardFork::decode(&bytes[start..end])?;
                Ok(Self::QueryHardFork(inner))
            }
            _ => Err(LedgerError::CborDecodeError(format!(
                "HardForkBlockQuery: unrecognised (len={len}, tag={tag})"
            ))),
        }
    }
}

/// Era-specific inner query under [`HardForkBlockQuery::QueryIfCurrent`].
///
/// Each Cardano era exposes its own `BlockQuery era` sum type; the
/// HFC layer wraps this in `[era_index, era_specific_query]` per
/// upstream `Cardano.Consensus.HardFork.Combinator.Ledger.Query`.
///
/// This enum recognises the era_index plus a small, frequently-used
/// subset of era-specific query tags shared across the Shelley
/// family (Shelley/Allegra/Mary/Alonzo/Babbage/Conway).  Other tags
/// remain opaque via [`Self::Unknown`].
///
/// Reference: tag values from
/// `Cardano.Ledger.Shelley.LedgerStateQuery` and successor era
/// modules.  Tags are stable across the Shelley family — newer
/// eras add tags but don't renumber existing ones.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EraSpecificQuery {
    /// `[1]` — `GetEpochNo`.  Returns the current epoch number
    /// (CBOR uint).  Used by `cardano-cli query slot-number /
    /// utxo --epoch`.
    GetEpochNo,
    /// `[3]` — `GetCurrentPParams`.  Returns the active protocol
    /// parameters in the era's native PP shape (a 17-element CBOR
    /// list for Shelley).  Used by every wallet, tx-builder, and
    /// `cardano-cli query protocol-parameters` invocation.
    GetCurrentPParams,
    /// `[6, addresses]` — `GetUTxOByAddress`.  Returns the UTxO
    /// entries for the supplied set of addresses.  Used by
    /// `cardano-cli query utxo --address`.  Carries the raw
    /// CBOR-encoded address-set (a CBOR set/array of address
    /// bytestrings) so the dispatcher can filter without
    /// re-decoding.
    GetUTxOByAddress { address_set_cbor: Vec<u8> },
    /// `[7]` — `GetWholeUTxO`.  Returns the entire UTxO map.
    /// Used by `cardano-cli query utxo --whole-utxo`.
    GetWholeUTxO,
    /// `[15, txin_set]` — `GetUTxOByTxIn`.  Returns the UTxO
    /// entries for the supplied set of TxIns.  Used by
    /// `cardano-cli query utxo --tx-in`.  Captured wire tag 15
    /// from the 2026-04-28 cardano-cli rehearsal.
    GetUTxOByTxIn { txin_set_cbor: Vec<u8> },
    /// `[5]` — `GetStakeDistribution`.  Returns a CBOR map of
    /// `pool_keyhash → relative_stake` (UnitInterval).  Used by
    /// `cardano-cli query stake-distribution` (era-blocked
    /// client-side until Babbage+).
    GetStakeDistribution,
    /// `[10, stake_credential_set]` —
    /// `GetFilteredDelegationsAndRewardAccounts`.  Returns the
    /// delegations and reward balances for the supplied set of
    /// stake credentials.  Used by `cardano-cli query
    /// stake-address-info` (era-blocked client-side until
    /// Babbage+).  Carries the raw CBOR-encoded credential set.
    GetFilteredDelegationsAndRewardAccounts { credential_set_cbor: Vec<u8> },
    /// `[11]` — `GetGenesisConfig`.  Returns the genesis config
    /// for the active era.  Used internally by some cardano-cli
    /// flows (e.g. `query leadership-schedule`).
    GetGenesisConfig,
    /// `[16]` — `GetStakePools` (corrected tag in R179; was 13 in
    /// R163).  Returns a CBOR set of registered pool key hashes.
    /// Used by `cardano-cli query stake-pools` (era-blocked
    /// client-side until Babbage+).
    GetStakePools,
    /// `[17, pool_hash_set]` — `GetStakePoolParams` (Round 171,
    /// corrected tag in R179; was 14 in R171).
    /// Returns a `Map (KeyHash 'StakePool) PoolParams` filtered by
    /// the supplied set of pool key hashes (`tag(258) [* bytes(28)]`
    /// per CIP-21 set tag).  Used by `cardano-cli query pool-state
    /// --stake-pool-id <id>` (era-blocked client-side until Babbage+).
    /// Carries the raw CBOR-encoded pool-hash set.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerStateQuery` —
    /// `GetStakePoolParams`.
    GetStakePoolParams { pool_hash_set_cbor: Vec<u8> },
    /// `[19, maybe_pool_hash_set]` — `GetPoolState` (Round 172,
    /// corrected tag in R179; was 17 in R172).
    /// Returns the full `PState` 4-tuple of maps (current params,
    /// future params, retiring epochs, deposits) optionally filtered
    /// to a subset of pool key hashes.  Used by
    /// `cardano-cli query pool-state --all-stake-pools` and
    /// `query pool-state --stake-pool-id <id>` (era-blocked
    /// client-side until Babbage+).  Carries the raw CBOR-encoded
    /// `Maybe (Set PoolKeyHash)` payload — `Nothing` means "all
    /// pools", `Just <set>` filters to the given subset.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerStateQuery` —
    /// `GetPoolState`; `Cardano.Ledger.Shelley.LedgerState.PState`.
    GetPoolState { maybe_pool_hash_set_cbor: Vec<u8> },
    /// `[20, maybe_pool_hash_set]` — `GetStakeSnapshots` (Round 173,
    /// corrected tag in R179; was 18 in R173).
    /// Returns the per-pool mark/set/go stake amounts plus the three
    /// totals as a 4-element CBOR list, optionally filtered to a
    /// subset of pool key hashes.  Used by
    /// `cardano-cli query stake-snapshot` (era-blocked client-side
    /// until Babbage+).  Carries the raw CBOR-encoded
    /// `Maybe (Set PoolKeyHash)` payload — `Nothing` means "all
    /// pools", `Just <set>` filters to the given subset.
    ///
    /// Reference: `Cardano.Ledger.Shelley.LedgerStateQuery` —
    /// `GetStakeSnapshots`; the wire shape mirrors upstream's
    /// `StakeSnapshots era` record (per-pool map + ssStakeMarkTotal
    /// + ssStakeSetTotal + ssStakeGoTotal).
    GetStakeSnapshots { maybe_pool_hash_set_cbor: Vec<u8> },
    /// `[9, inner_query_cbor]` — `GetCBOR` (Round 179).  Wraps an
    /// inner era-specific query and asks the server to respond with
    /// the inner result encoded as raw CBOR-in-CBOR (`tag(24)
    /// bytes(inner_response)`).  cardano-cli 10.x sends this for
    /// `query pool-state` and `query stake-snapshot`, recursively
    /// nesting tag 19 / 20 inside.
    ///
    /// Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
    /// `encodeShelleyQuery (GetCBOR q')`.
    GetCBOR { inner_query_cbor: Vec<u8> },
    /// `[23]` — `GetConstitution` (Round 180, Conway-only).
    /// Returns the active Conway `Constitution` (`anchor` +
    /// `guardrails_script_hash` option) per upstream
    /// `Cardano.Ledger.Conway.Governance.Constitution`.  Used by
    /// `cardano-cli query constitution`.
    GetConstitution,
    /// `[24]` — `GetGovState` (Round 180, Conway-only).  Returns
    /// the full `ConwayGovState` (proposals, vote tallies,
    /// committee state, etc.) per upstream
    /// `Cardano.Ledger.Conway.Governance`.  Used by
    /// `cardano-cli query gov-state`.
    GetGovState,
    /// `[25, drep_credential_set]` — `GetDRepState` (Round 180,
    /// Conway-only).  Returns a map of registered DReps filtered
    /// by the supplied set of credentials per upstream
    /// `Cardano.Ledger.Conway.LedgerStateQuery.GetDRepState`.
    /// Used by `cardano-cli query drep-state`.  Carries the raw
    /// CBOR-encoded credential set.
    GetDRepState { credential_set_cbor: Vec<u8> },
    /// `[29]` — `GetAccountState` (Round 180, Conway-only).
    /// Returns `[treasury, reserves]` (the consensus-side
    /// `AccountState`) per upstream
    /// `Cardano.Ledger.Shelley.LedgerState.AccountState`.  Used
    /// by `cardano-cli query treasury` / `query reserves` (and
    /// any operator-authored query reading the accounting pots).
    GetAccountState,
    /// `[33]` — `GetFuturePParams` (Round 183, Conway-only).
    /// LSQ-facing result type is `Maybe (PParams era)` (CBOR
    /// `Nothing = 0x80` empty list, `Just pp = [pp]` 1-element
    /// list).  Used by `cardano-cli conway query future-pparams`.
    /// Until yggdrasil tracks pending PPUP enactment as a queued
    /// `PParams` ready for next-epoch adoption, this responds
    /// `Nothing` — cardano-cli renders that as
    /// `"No protocol parameter changes will be enacted at the
    /// next epoch boundary."`.  NB: the LSQ-facing `Maybe (PParams
    /// era)` is distinct from the internal `FuturePParams era`
    /// ADT in `Cardano.Ledger.Core.PParams`; the wire-facing
    /// query result uses `Maybe`.
    GetFuturePParams,
    /// `[12]` — `DebugNewEpochState` (Round 190).  Returns
    /// the full `NewEpochState era` per upstream
    /// `Cardano.Ledger.Shelley.LedgerState.NewEpochState`.
    /// Used by `cardano-cli conway query ledger-state` (a
    /// debug-level query that dumps the raw CBOR; cardano-cli
    /// accepts `null` as a valid response).  Yggdrasil emits
    /// CBOR `null` since constructing a complete NewEpochState
    /// matching upstream's substantial multi-field record is
    /// out of scope for the wire-protocol parity arc.
    DebugNewEpochState,
    /// `[13]` — `DebugChainDepState` (Round 190).  Returns
    /// the protocol's `ChainDepState` (for Praos eras: a
    /// `PraosState` 8-element record).  Used by
    /// `cardano-cli conway query protocol-state`.  Yggdrasil
    /// emits a minimal valid `PraosState` placeholder
    /// `[Origin, empty_map, neutral×6]` until live Praos
    /// chain-state is plumbed into the LSQ snapshot.
    DebugChainDepState,
    /// `[34, peer_kind]` (v15+) or `[34]` (pre-v15) —
    /// `GetLedgerPeerSnapshot'` (Round 189).  Returns the
    /// `LedgerPeerSnapshot` for ledger-derived peer
    /// discovery.  cardano-cli 10.16 sends the v15+ form
    /// with `peer_kind` selecting `BigLedgerPeers (0)` or
    /// `AllLedgerPeers (1)`.  Used by `cardano-cli conway
    /// query ledger-peer-snapshot`.
    ///
    /// Reference:
    /// `Ouroboros.Consensus.Shelley.Ledger.Query.GetLedgerPeerSnapshot'`;
    /// `Ouroboros.Network.PeerSelection.LedgerPeers.Type`
    /// (`encodeLedgerPeerSnapshot`).
    GetLedgerPeerSnapshot {
        /// `Some(0)` for BigLedgerPeers, `Some(1)` for
        /// AllLedgerPeers, `None` for the pre-v15 singleton form.
        peer_kind: Option<u8>,
    },
    /// `[32]` — `GetRatifyState` (Round 187, Conway-only).
    /// Singleton query; returns `RatifyState era` (4-field
    /// record `[EnactState era, Seq GovActionState, Set
    /// GovActionId, Bool]`).  Used by `cardano-cli conway
    /// query ratify-state`.
    ///
    /// Reference:
    /// `Cardano.Ledger.Conway.LedgerStateQuery.GetRatifyState`;
    /// `Cardano.Ledger.Conway.Governance.Internal.RatifyState`.
    GetRatifyState,
    /// `[22, stake_cred_set]` — `GetStakeDelegDeposits`
    /// (Round 186, Conway-only).  Returns
    /// `Map (Credential 'Staking) Coin` (per-credential
    /// delegation deposits) filtered by the supplied set of
    /// stake credentials.  Filter parameter accepted but not
    /// applied — cardano-cli filters client-side.
    ///
    /// Reference:
    /// `Cardano.Ledger.Conway.LedgerStateQuery.GetStakeDelegDeposits`.
    GetStakeDelegDeposits { stake_cred_set_cbor: Vec<u8> },
    /// `[36, maybe_pool_hash_set]` — `GetPoolDistr2` (Round
    /// 186, Conway-only).  Returns `PoolDistr` (2-element
    /// record `[map, NonZero Coin]`) — same shape as
    /// `GetStakeDistribution2` (tag 37, R179) but with an
    /// optional pool-id filter.  Parameter is
    /// `Maybe (Set PoolKeyHash)` (Nothing = all pools).
    /// Filter accepted but not applied — yggdrasil emits the
    /// full distribution.
    ///
    /// Reference:
    /// `Cardano.Ledger.Conway.LedgerStateQuery.GetPoolDistr2`;
    /// `Cardano.Ledger.Core.PoolDistr` (2-tuple of
    /// `[Map PoolKeyHash IndividualPoolStake, NonZero Coin
    /// pdTotalStake]`).
    GetPoolDistr2 { maybe_pool_hash_set_cbor: Vec<u8> },
    /// `[31, gov_action_id_set]` — `GetProposals` (Round 185,
    /// Conway-only).  Returns a `Seq (GovActionState era)`
    /// (CBOR list) of currently-pending governance action
    /// states, optionally filtered to the supplied set of
    /// gov-action IDs.  Used by `cardano-cli conway query
    /// proposals --all-proposals` and the targeted variant
    /// `--governance-action-tx-id ... --governance-action-index N`.
    /// Filter parameter accepted but not applied — cardano-cli
    /// filters client-side.
    ///
    /// Reference:
    /// `Cardano.Ledger.Conway.LedgerStateQuery.GetProposals`.
    GetProposals { gov_action_id_set_cbor: Vec<u8> },
    /// `[35, pool_key_hash]` — `QueryStakePoolDefaultVote`
    /// (Round 185, Conway-only).  Returns the SPO's default
    /// vote choice (`DefaultVote = DefaultNo (0) | DefaultAbstain
    /// (1) | DefaultNoConfidence (2)`, encoded as a single CBOR
    /// uint).  Used by `cardano-cli conway query
    /// stake-pool-default-vote --spo-key-hash <hash>`.  Until
    /// yggdrasil tracks per-pool default-vote registrations,
    /// emit `DefaultNo (0)` as the placeholder.  Pool key hash
    /// parameter carried for protocol compatibility but not
    /// applied (the response is the same for any pool).
    ///
    /// Reference:
    /// `Cardano.Ledger.Conway.LedgerStateQuery.QueryStakePoolDefaultVote`;
    /// `Cardano.Ledger.Conway.Governance.DefaultVote`.
    QueryStakePoolDefaultVote { pool_key_hash_cbor: Vec<u8> },
    /// `[28, stake_cred_set]` — `GetFilteredVoteDelegatees`
    /// (Round 184, Conway-only).  Returns a CBOR map of
    /// `(Credential 'Staking) → DRep` (which DRep each stake
    /// credential delegates its votes to) filtered by the
    /// supplied set of stake credentials.  Used internally by
    /// `cardano-cli conway query spo-stake-distribution` to
    /// resolve SPO vote delegations.  Filter parameter
    /// accepted but not applied.
    ///
    /// Reference:
    /// `Cardano.Ledger.Conway.LedgerStateQuery.GetFilteredVoteDelegatees`
    /// (`type VoteDelegatees = Map (Credential 'Staking) DRep`).
    GetFilteredVoteDelegatees { stake_cred_set_cbor: Vec<u8> },
    /// `[26, drep_set]` — `GetDRepStakeDistr` (Round 184,
    /// Conway-only).  Returns a CBOR map of `DRep → Coin`
    /// (delegated stake per DRep) filtered by the supplied set
    /// of DReps.  Used by `cardano-cli conway query
    /// drep-stake-distribution`.  Filter parameter carried for
    /// protocol compatibility but not applied — yggdrasil
    /// emits the full map and cardano-cli filters client-side.
    ///
    /// Reference:
    /// `Cardano.Ledger.Conway.LedgerStateQuery.GetDRepStakeDistr`
    /// (result type `Map (DRep StandardCrypto) Coin`).
    GetDRepStakeDistr { drep_set_cbor: Vec<u8> },
    /// `[30, spo_set]` — `GetSPOStakeDistr` (Round 184,
    /// Conway-only).  Returns a CBOR map of
    /// `KeyHash 'StakePool → Coin` (Conway-era SPO stake by
    /// active stake distribution).  Used by
    /// `cardano-cli conway query spo-stake-distribution`.
    /// Filter parameter carried for protocol compatibility
    /// but not applied — yggdrasil emits the full map and
    /// cardano-cli filters client-side.
    ///
    /// Reference:
    /// `Cardano.Ledger.Conway.LedgerStateQuery.GetSPOStakeDistr`.
    GetSPOStakeDistr { spo_set_cbor: Vec<u8> },
    /// `[27, cold_creds_set, hot_creds_set, statuses_set]` —
    /// `GetCommitteeMembersState` (Round 182, Conway-only).
    /// Returns the active `CommitteeMembersState`
    /// (3-element record `[committee_map, threshold,
    /// epoch_no]`) optionally filtered by the supplied cold-key,
    /// hot-key, and status sets.  Used by `cardano-cli conway
    /// query committee-state`.  Filter parameters carried but
    /// not applied — yggdrasil's encoder emits the full
    /// committee state and cardano-cli filters client-side.
    ///
    /// Reference:
    /// `Cardano.Ledger.Conway.LedgerStateQuery.GetCommitteeMembersState`.
    GetCommitteeMembersState {
        cold_creds_cbor: Vec<u8>,
        hot_creds_cbor: Vec<u8>,
        statuses_cbor: Vec<u8>,
    },
    /// Any era-specific query whose tag this codec doesn't yet
    /// recognise.  Carries the raw inner CBOR so the dispatcher can
    /// fall through to `null_response()` without losing the bytes.
    Unknown { tag: u64, raw_inner: Vec<u8> },
}

/// Decode the `[era_index, era_specific_query]` inner payload of a
/// [`HardForkBlockQuery::QueryIfCurrent`].  Returns the era_index
/// (0=Byron, 1=Shelley, 2=Allegra, 3=Mary, 4=Alonzo, 5=Babbage,
/// 6=Conway) plus the recognised [`EraSpecificQuery`] variant.
///
/// Reference: `Cardano.Consensus.HardFork.Combinator.Ledger.Query`
/// — `decodeQueryIfCurrent`.
pub fn decode_query_if_current(inner_cbor: &[u8]) -> Result<(u32, EraSpecificQuery), LedgerError> {
    let mut dec = Decoder::new(inner_cbor);
    let outer_len = dec.array()?;
    if outer_len != 2 {
        return Err(LedgerError::CborDecodeError(format!(
            "QueryIfCurrent inner must be a 2-element list \
             [era_index, era_query]; got len={outer_len}"
        )));
    }
    let era_index = dec.unsigned()? as u32;
    // Era-specific query: a singleton (`[tag]`) for tag-only queries
    // like GetCurrentPParams, or a multi-element list for queries
    // with parameters.  We capture the whole sub-list as raw bytes
    // and inspect the leading tag to classify.
    let q_start = dec.position();
    let q_len = dec.array()?;
    let q_tag = dec.unsigned()?;
    let q_end_after_tag = dec.position();
    // Skip remaining elements (if any) so the slice is the full
    // era-specific query CBOR.
    for _ in 1..q_len {
        dec.skip()?;
    }
    let q_end = dec.position();
    let raw_inner = inner_cbor[q_start..q_end].to_vec();
    // Round 179 — corrected tag table to match upstream
    // cardano-node 10.7.x's `Ouroboros.Consensus.Shelley.Ledger.Query
    // .encodeShelleyQuery` (verified against
    // ouroboros-consensus@main).  R163's tag numbers (13/14/17/18 for
    // GetStakePools/GetStakePoolParams/GetPoolState/GetStakeSnapshots)
    // were off by 3 — those slots in upstream are
    // DebugChainDepState/GetRewardProvenance/GetStakePoolParams/
    // GetRewardInfoPools.  Correct upstream tags:
    //
    // | Tag | Query                                    |
    // |-----|------------------------------------------|
    // |  1  | GetEpochNo                               |
    // |  3  | GetCurrentPParams                        |
    // |  5  | GetStakeDistribution (PoolDistr w/ VRF)  |
    // |  6  | GetUTxOByAddress                         |
    // |  7  | GetUTxOWhole                             |
    // | 10  | GetFilteredDelegationsAndRewardAccounts  |
    // | 11  | GetGenesisConfig                         |
    // | 15  | GetUTxOByTxIn                            |
    // | 16  | GetStakePools                            | ← was 13
    // | 17  | GetStakePoolParams                       | ← was 14
    // | 19  | GetPoolState                             | ← was 17
    // | 20  | GetStakeSnapshots                        | ← was 18
    // | 37  | GetStakeDistribution2 (no-VRF PoolDistr) | ← new
    let kind = match (q_len, q_tag) {
        (1, 1) => EraSpecificQuery::GetEpochNo,
        (1, 3) => EraSpecificQuery::GetCurrentPParams,
        (1, 5) => EraSpecificQuery::GetStakeDistribution,
        (1, 7) => EraSpecificQuery::GetWholeUTxO,
        (1, 11) => EraSpecificQuery::GetGenesisConfig,
        (1, 16) => EraSpecificQuery::GetStakePools,
        (1, 37) => EraSpecificQuery::GetStakeDistribution,
        (2, 6) => {
            // `[6, address_set_cbor]` — captured the address-set
            // payload between `q_end_after_tag` and `q_end`.
            EraSpecificQuery::GetUTxOByAddress {
                address_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
            }
        }
        (2, 9) => EraSpecificQuery::GetCBOR {
            inner_query_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 10) => EraSpecificQuery::GetFilteredDelegationsAndRewardAccounts {
            credential_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 17) => EraSpecificQuery::GetStakePoolParams {
            pool_hash_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 15) => EraSpecificQuery::GetUTxOByTxIn {
            txin_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 19) => EraSpecificQuery::GetPoolState {
            maybe_pool_hash_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 20) => EraSpecificQuery::GetStakeSnapshots {
            maybe_pool_hash_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (1, 23) => EraSpecificQuery::GetConstitution,
        (1, 24) => EraSpecificQuery::GetGovState,
        (2, 25) => EraSpecificQuery::GetDRepState {
            credential_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 22) => EraSpecificQuery::GetStakeDelegDeposits {
            stake_cred_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 26) => EraSpecificQuery::GetDRepStakeDistr {
            drep_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 28) => EraSpecificQuery::GetFilteredVoteDelegatees {
            stake_cred_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 30) => EraSpecificQuery::GetSPOStakeDistr {
            spo_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 31) => EraSpecificQuery::GetProposals {
            gov_action_id_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 35) => EraSpecificQuery::QueryStakePoolDefaultVote {
            pool_key_hash_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (2, 36) => EraSpecificQuery::GetPoolDistr2 {
            maybe_pool_hash_set_cbor: inner_cbor[q_end_after_tag..q_end].to_vec(),
        },
        (1, 29) => EraSpecificQuery::GetAccountState,
        (1, 12) => EraSpecificQuery::DebugNewEpochState,
        (1, 13) => EraSpecificQuery::DebugChainDepState,
        (1, 32) => EraSpecificQuery::GetRatifyState,
        (1, 34) => EraSpecificQuery::GetLedgerPeerSnapshot { peer_kind: None },
        (2, 34) => {
            // Re-decode to extract peer_kind byte after the tag.
            let mut sub = Decoder::new(inner_cbor);
            let _outer_len = sub.array()?;
            let _era = sub.unsigned()?;
            let _q_len = sub.array()?;
            let _q_tag = sub.unsigned()?;
            let kind = sub.unsigned()? as u8;
            EraSpecificQuery::GetLedgerPeerSnapshot {
                peer_kind: Some(kind),
            }
        }
        (1, 33) => EraSpecificQuery::GetFuturePParams,
        (4, 27) => {
            // [27, cold_creds, hot_creds, statuses]: re-decode the
            // three CBOR items individually so we can carry each
            // raw payload.  The outer skip-loop above advanced the
            // cursor past all three, so reset and re-decode them.
            let mut sub = Decoder::new(inner_cbor);
            let _outer_len = sub.array()?;
            let _era = sub.unsigned()?;
            let _q_len = sub.array()?;
            let _q_tag = sub.unsigned()?;
            let s1_start = sub.position();
            sub.skip()?;
            let s1_end = sub.position();
            let s2_start = s1_end;
            sub.skip()?;
            let s2_end = sub.position();
            let s3_start = s2_end;
            sub.skip()?;
            let s3_end = sub.position();
            EraSpecificQuery::GetCommitteeMembersState {
                cold_creds_cbor: inner_cbor[s1_start..s1_end].to_vec(),
                hot_creds_cbor: inner_cbor[s2_start..s2_end].to_vec(),
                statuses_cbor: inner_cbor[s3_start..s3_end].to_vec(),
            }
        }
        _ => EraSpecificQuery::Unknown {
            tag: q_tag,
            raw_inner,
        },
    };
    Ok((era_index, kind))
}

// ---------------------------------------------------------------------------
// QueryAnytime
// ---------------------------------------------------------------------------

/// Inner query under [`HardForkBlockQuery::QueryAnytime`] (upstream
/// `QueryAnytime`).
///
/// | Tag | Length | Variant         |
/// |-----|--------|-----------------|
/// |  0  |   1    | `GetEraStart`   |
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryAnytimeKind {
    GetEraStart,
}

impl QueryAnytimeKind {
    pub fn encode(self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::GetEraStart => {
                enc.array(1);
                enc.unsigned(0);
            }
        }
        enc.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match (len, tag) {
            (1, 0) => Ok(Self::GetEraStart),
            _ => Err(LedgerError::CborDecodeError(format!(
                "QueryAnytimeKind: unrecognised (len={len}, tag={tag})"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// QueryHardFork
// ---------------------------------------------------------------------------

/// Inner query under [`HardForkBlockQuery::QueryHardFork`] (upstream
/// `QueryHardFork`).
///
/// | Tag | Length | Variant          | Result type                   |
/// |-----|--------|------------------|-------------------------------|
/// |  0  |   1    | `GetInterpreter` | `Interpreter` (era summary)   |
/// |  1  |   1    | `GetCurrentEra`  | `EraIndex` (active era index) |
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryHardFork {
    GetInterpreter,
    GetCurrentEra,
}

impl QueryHardFork {
    pub fn encode(self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::GetInterpreter => {
                enc.array(1);
                enc.unsigned(0);
            }
            Self::GetCurrentEra => {
                enc.array(1);
                enc.unsigned(1);
            }
        }
        enc.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match (len, tag) {
            (1, 0) => Ok(Self::GetInterpreter),
            (1, 1) => Ok(Self::GetCurrentEra),
            _ => Err(LedgerError::CborDecodeError(format!(
                "QueryHardFork: unrecognised (len={len}, tag={tag})"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Result encoding
// ---------------------------------------------------------------------------

/// Encode the result of [`UpstreamQuery::GetChainPoint`] in upstream
/// `encodePoint` shape.
///
/// Upstream `encodePoint` per `Cardano.Slotting.Block`:
///   - `Origin`         = `[]`             (empty CBOR array)
///   - `BlockPoint(s,h)` = `[slot, hash]`   (length-2 array)
///
/// 2026-04-27 operator capture confirms: upstream `cardano-node 10.7.1`
/// at NtC V_23 sent `82 04 82 1a 00 09 4e f8 58 20 ec 4a ...` —
/// MsgResult `[4, [610040, h'ec4a...']]` for `GetChainPoint`.  No
/// leading constructor tag; the `[slot, hash]` array IS the Point
/// itself.
pub fn encode_chain_point(point: &Point) -> Vec<u8> {
    let mut enc = Encoder::new();
    match point {
        Point::Origin => {
            enc.array(0);
        }
        Point::BlockPoint(slot, hash) => {
            enc.array(2);
            enc.unsigned(slot.0);
            enc.bytes(&hash.0);
        }
    }
    enc.into_bytes()
}

/// Encode a minimal valid `Interpreter` (era-history summary) result
/// for `BlockQuery (QueryHardFork GetInterpreter)`.
///
/// Upstream `Interpreter xs = Interpreter (Summary xs)` encodes the
/// `Summary` as an indefinite-length CBOR array of `EraSummary`
/// records — non-empty (at least one era).  Each EraSummary is
/// `[eraStart :: Bound, eraEnd :: EraEnd, eraParams :: EraParams]`
/// where:
///
///   - `Bound = [relativeTime :: Word64, slot :: Word64, epoch :: Word64]`
///     (3-element array — `relativeTime` is whole + fractional
///     picoseconds packed as a single bignum).
///   - `EraEnd = EraUnbounded | EraEnd Bound` — represented as a
///     1-tuple.  An unbounded era is just `[Bound{...}]` per the
///     2026-04-27 operator capture.
///   - `EraParams = [epochSize, slotLength, safeZone, genesisWindow]`
///     where `slotLength` is encoded as picoseconds and `safeZone`
///     is `[0]` (StandardSafeZone) or `[1, slots]` (UnsafeIndefiniteSafeZone).
///
/// This encoder emits a SINGLE open-ended era anchored at slot 0 with
/// preprod-shape parameters (epochSize=21600 slots, slotLength=1
/// second, safeZone=129600 slots).  cardano-cli's slot-to-time
/// conversion will be wrong for non-Byron slots, but `query tip` only
/// needs the Interpreter to deserialise — the displayed `slot`/`hash`
/// come from `GetChainPoint` directly.  Phase-3 follow-up: derive
/// the real era summaries from the loaded `ShelleyGenesis`/`AlonzoGenesis`/
/// `ConwayGenesis` hard-fork transition epochs threaded through the
/// `LedgerStateSnapshot`.
pub fn encode_interpreter_minimal(_epoch_size: u64, _slot_length_secs: u64) -> Vec<u8> {
    encode_interpreter_preprod()
}

/// Per-network era-history selector.  Distinguishes the live
/// Cardano networks whose vendored `shelley-genesis.json` shapes
/// drive the [`encode_interpreter_for_network`] /
/// [`encode_system_start_for_network`] outputs.  Per-network
/// constants come from
/// [`node/configuration/<network>/shelley-genesis.json`](../../../../node/configuration/).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkKind {
    /// Preprod: `epochLength=432_000` (5-day epochs),
    /// Byron→Shelley at slot 86_400 / epoch 4, system start
    /// 2022-06-01.
    Preprod,
    /// Preview: `epochLength=86_400` (1-day epochs), every hard
    /// fork at epoch 0 (no Byron blocks), system start
    /// 2022-10-25.
    Preview,
    /// Mainnet: `epochLength=432_000`, Byron→Shelley at slot
    /// 4_492_800 / epoch 208, system start 2017-09-23.
    Mainnet,
}

/// Encode the `Interpreter` (era-history summary) tailored to the
/// supplied [`NetworkKind`].  cardano-cli's `query tip` walks the
/// summary list to convert the queried slot to `(epoch,
/// slotInEpoch, slotsToEpochEnd)`; the wrong shape leads to either
/// nonsense values or silent fall-through to genesis-shape display.
pub fn encode_interpreter_for_network(network: NetworkKind) -> Vec<u8> {
    match network {
        NetworkKind::Preprod => encode_interpreter_preprod(),
        NetworkKind::Preview => encode_interpreter_preview(),
        NetworkKind::Mainnet => encode_interpreter_mainnet(),
    }
}

/// Encode `SystemStart` (genesis wall-clock anchor) tailored to
/// the supplied [`NetworkKind`].  cardano-cli's `query tip` uses
/// it together with the `Interpreter` and the queried slot to
/// compute the `syncProgress` percentage.
pub fn encode_system_start_for_network(network: NetworkKind) -> Vec<u8> {
    match network {
        // Preprod: 2022-06-01 = year 2022, day-of-year 152.
        NetworkKind::Preprod => encode_system_start(2022, 152, 0),
        // Preview: 2022-10-25 = year 2022, day-of-year 298.
        NetworkKind::Preview => encode_system_start(2022, 298, 0),
        // Mainnet: 2017-09-23 = year 2017, day-of-year 266.
        NetworkKind::Mainnet => encode_system_start(2017, 266, 0),
    }
}

/// Encode CBOR positive bignum (tag 2) for picosecond values that
/// exceed u64 range.  Round 162 — used by `encode_relative_time`
/// when `picoseconds > u64::MAX` (i.e. for synthetic far-future
/// bounds past slot 1.8e7 at 1s/slot which would overflow
/// u64-as-picoseconds).
fn encode_bignum_u128(enc: &mut Encoder, value: u128) {
    enc.tag(2);
    if value == 0 {
        enc.bytes(&[]);
        return;
    }
    let bytes = value.to_be_bytes();
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(15);
    enc.bytes(&bytes[first_nonzero..]);
}

/// Encode `relativeTime :: NominalDiffTimeMicro` per upstream
/// `Ouroboros.Consensus.HardFork.History.Summary`.  Serialises as
/// a plain CBOR uint when the value fits in u64 (matches captured
/// wire bytes for real era boundaries) and falls through to a
/// CBOR positive-bignum (tag 2) when the value exceeds u64 — used
/// by Round 162's bumped synthetic far-future end.
///
/// Captured wire bytes from `cardano-node 10.7.1` at NtC V_23
/// confirm: Byron eraEnd at 1.728e18 ps encoded as
/// `1b 17fb16d83be00000` (CBOR uint8 prefix + 8-byte big-endian),
/// not `c2 48 17fb16d83be00000` (bignum-wrapped).
fn encode_relative_time(enc: &mut Encoder, picoseconds: u128) {
    if picoseconds <= u64::MAX as u128 {
        enc.unsigned(picoseconds as u64);
    } else {
        encode_bignum_u128(enc, picoseconds);
    }
}

/// Encode an upstream-faithful preprod `Interpreter` with two era
/// summaries — Byron (closed) and Shelley (synthetic far-future end)
/// — so cardano-cli's slot-to-epoch conversion produces the right
/// `epoch` / `slotInEpoch` values for any post-genesis preprod slot.
///
/// Upstream emits one summary per transitioned era (Byron, Shelley,
/// Allegra, Mary, Alonzo, Babbage, Conway), with the latest era's
/// `eraEnd` synthetic-far-future and the rest closed at the
/// successor era's start.  Allegra-onwards share Shelley's
/// `epochSize=21600` and `slotLength=1000ms`, so a single Shelley
/// summary spanning all post-Byron slots gives cardano-cli the right
/// arithmetic for `query tip` purposes.  `current_era` reported via
/// `GetCurrentEra` (separate query) is what produces the displayed
/// `era` field — the interpreter only feeds the slot↔time
/// conversion.
///
/// Phase-3 follow-up: derive era boundaries from the loaded
/// `ShelleyGenesis`/`AlonzoGenesis`/`ConwayGenesis` hard-fork
/// transition epochs threaded through `LedgerStateSnapshot`, and emit
/// all 7 summaries when current era is Conway so the per-era
/// `eraStart`/`eraEnd` align with real preprod boundaries.
fn encode_interpreter_preprod() -> Vec<u8> {
    // Preprod Byron→Shelley boundary captured from `cardano-node 10.7.1`
    // socat -x -v at NtC V_23: epoch 4, slot 86_400, relativeTime
    // 1.728e18 picoseconds = 1.728e6 seconds = 20 days
    // (4 epochs × 21_600 Byron-slots × 20_000ms/slot).
    const BYRON_END_SLOT: u64 = 86_400;
    const BYRON_END_EPOCH: u64 = 4;
    const BYRON_END_PICOS: u64 = 0x17fb_16d8_3be0_0000;

    // Shelley→Allegra boundary captured from same upstream socat:
    // epoch 5, slot 0x7e900 = 518_400.  Allegra inherits Shelley's
    // params shape so we don't need to emit Allegra explicitly until
    // a node progresses past slot 518_400 — Phase-3 follow-up.
    //
    // Synthetic far-future Shelley end at slot=2^36 covers all
    // realistic preprod slots (years past current tip) and keeps
    // relativeTime in u64 range:
    //   2^36 slots × 1e12 ps/slot = 6.87e22 — overflows u64.
    // So cap synthetic end at slot=10_000_000 (≈ 116 days post
    // Byron at 1s/slot, well past current preprod test tip):
    //   relativeTime = 1.728e18 + (10_000_000 - 86400) * 1e12
    //                = 1.0099e19 ps  (fits in u64).
    // Round 162 — bump synthetic far-future end to slot 2^48 to
    // cover all realistic preprod slots indefinitely.  At 1s/slot
    // that's 281 trillion slots ≈ 8.9 million years from genesis;
    // relativeTime in picoseconds = 2^48 * 1e12 ≈ 2.81e26, which
    // overflows u64 and triggers the bignum path in
    // `encode_relative_time`.  This unblocks `query slot-number`
    // and `query era-history` for any timestamp the user could
    // realistically pass (the prior 10M slot end forced
    // `Past horizon` rejections for timestamps past ~116 days
    // post-Byron at 1s/slot).
    const SHELLEY_END_SLOT: u64 = 1u64 << 48;
    const SHELLEY_END_PICOS: u128 = (BYRON_END_PICOS as u128)
        + ((SHELLEY_END_SLOT as u128 - BYRON_END_SLOT as u128) * 1_000_000_000_000_u128);
    // Shelley epochSize = 432_000 slots (5 days × 86_400 s/day).
    const SHELLEY_END_EPOCH: u64 = BYRON_END_EPOCH + (SHELLEY_END_SLOT - BYRON_END_SLOT) / 432_000;

    let mut enc = Encoder::new();
    enc.raw(&[0x9f]);

    // Byron summary
    enc.array(3);
    enc.array(3);
    encode_relative_time(&mut enc, 0);
    enc.unsigned(0);
    enc.unsigned(0);
    enc.array(3);
    encode_relative_time(&mut enc, BYRON_END_PICOS as u128);
    enc.unsigned(BYRON_END_SLOT);
    enc.unsigned(BYRON_END_EPOCH);
    enc.array(4);
    enc.unsigned(21_600); // epochSize
    enc.unsigned(20_000); // slotLength ms
    enc.array(3); // safeZone
    enc.unsigned(0);
    enc.unsigned(4_320);
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(4_320); // genesisWindow

    // Shelley summary (open era — synthetic far-future end)
    enc.array(3);
    enc.array(3);
    encode_relative_time(&mut enc, BYRON_END_PICOS as u128);
    enc.unsigned(BYRON_END_SLOT);
    enc.unsigned(BYRON_END_EPOCH);
    enc.array(3);
    encode_relative_time(&mut enc, SHELLEY_END_PICOS);
    enc.unsigned(SHELLEY_END_SLOT);
    enc.unsigned(SHELLEY_END_EPOCH);
    enc.array(4);
    enc.unsigned(432_000); // epochSize captured from upstream
    enc.unsigned(1_000); // slotLength ms
    enc.array(3); // safeZone
    enc.unsigned(0);
    enc.unsigned(129_600);
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(129_600); // genesisWindow captured from upstream

    enc.raw(&[0xff]);
    enc.into_bytes()
}

/// Encode the preview `Interpreter`.
///
/// Preview's `config.json` sets every `Test*HardForkAtEpoch=0`,
/// meaning all hard forks occurred at epoch 0 and no Byron blocks
/// were ever produced.  The on-disk
/// [`shelley-genesis.json`](../../../../node/configuration/preview/shelley-genesis.json)
/// pins `epochLength=86_400` (1-day epochs at 1s/slot).
///
/// Emits a single open-ended Shelley-shape summary anchored at slot
/// 0 with synthetic far-future end at slot 10_000_000 (well past
/// the current preview tip — 1-day epochs over ~3.6 years gives
/// ~314M slots; the synthetic end caps slot↔epoch math at 10M and
/// is documented as a Phase-3 follow-up to extend coverage).
fn encode_interpreter_preview() -> Vec<u8> {
    const EPOCH_LENGTH: u64 = 86_400;
    // Round 162 — synthetic far-future end at 2^48 covers all
    // realistic preview slots indefinitely; relativeTime overflows
    // u64 and triggers the bignum path.
    const SHELLEY_END_SLOT: u64 = 1u64 << 48;
    const SHELLEY_END_PICOS: u128 = SHELLEY_END_SLOT as u128 * 1_000_000_000_000_u128;
    const SHELLEY_END_EPOCH: u64 = SHELLEY_END_SLOT / EPOCH_LENGTH;

    let mut enc = Encoder::new();
    enc.raw(&[0x9f]);

    enc.array(3);
    enc.array(3);
    encode_relative_time(&mut enc, 0);
    enc.unsigned(0);
    enc.unsigned(0);
    enc.array(3);
    encode_relative_time(&mut enc, SHELLEY_END_PICOS);
    enc.unsigned(SHELLEY_END_SLOT);
    enc.unsigned(SHELLEY_END_EPOCH);
    enc.array(4);
    enc.unsigned(EPOCH_LENGTH); // 86_400
    enc.unsigned(1_000); // slotLength ms
    enc.array(3);
    enc.unsigned(0);
    enc.unsigned(EPOCH_LENGTH * 3); // safeZone slots ≈ 3k/f
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(EPOCH_LENGTH); // genesisWindow

    enc.raw(&[0xff]);
    enc.into_bytes()
}

/// Encode the mainnet `Interpreter`.
///
/// Mainnet Byron→Shelley transitioned at epoch 208 (slot
/// 4_492_800 = 208 epochs × 21_600 Byron-slots).  Byron uses 20s
/// slots; Shelley onwards uses 1s slots at `epochLength=432_000`
/// (5-day epochs).
///
/// Phase-3 follow-up: emit explicit Allegra/Mary/Alonzo/Babbage/
/// Conway summaries when consensus reports the current era past
/// Shelley.  For now a single open Shelley summary with synthetic
/// far-future end at slot 4_492_800 + 10_000_000 keeps
/// `relativeTime` in u64 range and gives correct slot↔epoch math
/// for any slot in the first ~115 days post-Byron.
fn encode_interpreter_mainnet() -> Vec<u8> {
    const BYRON_END_SLOT: u64 = 4_492_800;
    const BYRON_END_EPOCH: u64 = 208;
    // Byron eraEnd relativeTime: 4_492_800 × 20 × 1e12 = 8.9856e19 ps,
    // which exceeds u64 (1.844e19).  Round 162's bignum-aware
    // `encode_relative_time` handles values past u64 via CBOR
    // tag-2 bignum, so we now use the real picosecond value.
    const BYRON_END_PICOS: u128 = BYRON_END_SLOT as u128 * 20_000 * 1_000_000_000_u128;
    // Round 162 — synthetic far-future Shelley end at slot 2^48
    // covers all realistic mainnet slots indefinitely.
    const SHELLEY_END_SLOT: u64 = 1u64 << 48;
    const SHELLEY_END_PICOS: u128 = BYRON_END_PICOS
        + (SHELLEY_END_SLOT as u128 - BYRON_END_SLOT as u128) * 1_000_000_000_000_u128;
    const SHELLEY_END_EPOCH: u64 = BYRON_END_EPOCH + (SHELLEY_END_SLOT - BYRON_END_SLOT) / 432_000;

    let mut enc = Encoder::new();
    enc.raw(&[0x9f]);

    // Byron summary
    enc.array(3);
    enc.array(3);
    encode_relative_time(&mut enc, 0);
    enc.unsigned(0);
    enc.unsigned(0);
    enc.array(3);
    encode_relative_time(&mut enc, BYRON_END_PICOS);
    enc.unsigned(BYRON_END_SLOT);
    enc.unsigned(BYRON_END_EPOCH);
    enc.array(4);
    enc.unsigned(21_600);
    enc.unsigned(20_000);
    enc.array(3);
    enc.unsigned(0);
    enc.unsigned(4_320);
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(4_320);

    // Shelley summary
    enc.array(3);
    enc.array(3);
    encode_relative_time(&mut enc, BYRON_END_PICOS);
    enc.unsigned(BYRON_END_SLOT);
    enc.unsigned(BYRON_END_EPOCH);
    enc.array(3);
    encode_relative_time(&mut enc, SHELLEY_END_PICOS);
    enc.unsigned(SHELLEY_END_SLOT);
    enc.unsigned(SHELLEY_END_EPOCH);
    enc.array(4);
    enc.unsigned(432_000);
    enc.unsigned(1_000);
    enc.array(3);
    enc.unsigned(0);
    enc.unsigned(129_600);
    enc.array(1);
    enc.unsigned(0);
    enc.unsigned(129_600);

    enc.raw(&[0xff]);
    enc.into_bytes()
}

/// Wrap an era-specific `QueryIfCurrent` result in the upstream
/// `Either (MismatchEraInfo xs) r` envelope per
/// `Cardano.Consensus.HardFork.Combinator.Serialisation.Common.encodeEitherMismatch`.
///
/// HFC NodeToClient uses **list-length discrimination** between
/// `Right` and `Left` — there's no leading variant tag:
/// - `Right a` (era matches): `[encoded_a]` — **1-element list**.
/// - `Left mismatch`: `[era1_ns, era2_ns]` — 2-element list of
///   `NS`-encoded era names.
///
/// This helper emits the `Right` (matching) form.  Source:
///
/// ```text
/// (HardForkNodeToClientEnabled{}, Right a) ->
///   mconcat [ Enc.encodeListLen 1, enc a ]
/// ```
pub fn encode_query_if_current_match(result_cbor: &[u8]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1);
    enc.raw(result_cbor);
    enc.into_bytes()
}

/// Encode `MismatchEraInfo` for the `Left` case when the requested
/// era doesn't match the snapshot's active era.
///
/// Per `encodeEitherMismatch`:
///
/// ```text
/// (HardForkNodeToClientEnabled{}, Left (MismatchEraInfo err)) ->
///   mconcat [ Enc.encodeListLen 2
///           , encodeNS (hpure (fn encodeName)) era1
///           , encodeNS (hpure (fn (encodeName . getLedgerEraInfo))) era2
///           ]
/// ```
///
/// `encodeNS` for a non-empty era list emits `[ns_index, payload]`
/// where `ns_index` selects the era and `payload` is the era's
/// `SingleEraInfo`/`LedgerEraInfo` (a text-string era name).
pub fn encode_query_if_current_mismatch(ledger_era_idx: u32, query_era_idx: u32) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2);
    encode_ns_era_name(&mut enc, query_era_idx);
    encode_ns_era_name(&mut enc, ledger_era_idx);
    enc.into_bytes()
}

fn encode_ns_era_name(enc: &mut Encoder, era_idx: u32) {
    // NS-encoded era: `[ns_index, era_name_text]` per
    // `Cardano.Consensus.HardFork.Combinator.Util.SOP.encodeNS`.
    enc.array(2);
    enc.unsigned(era_idx as u64);
    enc.text(era_ordinal_to_upstream_name(era_idx));
}

fn era_ordinal_to_upstream_name(ordinal: u32) -> &'static str {
    match ordinal {
        0 => "Byron",
        1 => "Shelley",
        2 => "Allegra",
        3 => "Mary",
        4 => "Alonzo",
        5 => "Babbage",
        6 => "Conway",
        _ => "Unknown",
    }
}

/// Encode Shelley-era `PParams` in the upstream `GetCurrentPParams`
/// response shape: a 17-element CBOR list (NOT the map-based
/// update-proposal shape).
///
/// Upstream `Cardano.Ledger.Shelley.PParams.encCBOR`:
///
/// ```text
/// encodeListLen 17
///   <> minfeeA <> minfeeB <> maxBBSize <> maxTxSize <> maxBHSize
///   <> keyDeposit <> poolDeposit <> eMax <> nOpt
///   <> a0 <> rho <> tau <> d <> extraEntropy
///   <> protocolVersion <> minUTxOValue <> minPoolCost
/// ```
///
/// Field types:
/// - `a0`: `NonNegativeInterval` = CBOR tag 30 + `[num, den]`.
/// - `rho`/`tau`/`d`: `UnitInterval` = CBOR tag 30 + `[num, den]`.
/// - `extraEntropy`: `Nonce` = `[0]` (Neutral) or `[1, hash]` (Hash).
/// - `protocolVersion`: `[major, minor]`.
///
/// Defaults applied when the snapshot's optional Shelley fields are
/// `None`: `d = 1.0` (fully decentralised), `extraEntropy = Neutral`,
/// `protocolVersion = (2, 0)` (Shelley genesis), `minUTxOValue = 0`.
pub fn encode_shelley_pparams_for_lsq(params: &yggdrasil_ledger::ProtocolParameters) -> Vec<u8> {
    use yggdrasil_ledger::CborEncode;
    let mut enc = Encoder::new();
    enc.array(17);
    enc.unsigned(params.min_fee_a);
    enc.unsigned(params.min_fee_b);
    enc.unsigned(params.max_block_body_size as u64);
    enc.unsigned(params.max_tx_size as u64);
    enc.unsigned(params.max_block_header_size as u64);
    enc.unsigned(params.key_deposit);
    enc.unsigned(params.pool_deposit);
    enc.unsigned(params.e_max);
    enc.unsigned(params.n_opt);
    params.a0.encode_cbor(&mut enc);
    params.rho.encode_cbor(&mut enc);
    params.tau.encode_cbor(&mut enc);
    let d = params.d.unwrap_or(yggdrasil_ledger::types::UnitInterval {
        numerator: 1,
        denominator: 1,
    });
    d.encode_cbor(&mut enc);
    encode_shelley_nonce(&mut enc, params.extra_entropy.as_ref());
    let (pv_major, pv_minor) = params.protocol_version.unwrap_or((2, 0));
    enc.array(2);
    enc.unsigned(pv_major);
    enc.unsigned(pv_minor);
    enc.unsigned(params.min_utxo_value.unwrap_or(0));
    enc.unsigned(params.min_pool_cost);
    enc.into_bytes()
}

/// Encode Alonzo-era `PParams` in the upstream `GetCurrentPParams`
/// response shape: a 24-element CBOR list adding 7 fields beyond
/// Shelley plus replacing `minUTxOValue` with `coinsPerUtxoWord`.
///
/// Upstream `Cardano.Ledger.Alonzo.PParams.encCBOR` order (per
/// `Cardano.Ledger.Alonzo.PParams` source — verified via Round 159
/// operational rehearsal against `cardano-cli query
/// protocol-parameters` on preview at era_index=4):
///
/// 1.  minfeeA              13. d
/// 2.  minfeeB              14. extraEntropy
/// 3.  maxBBSize            15. protocolVersion
/// 4.  maxTxSize            16. minPoolCost
/// 5.  maxBHSize            17. coinsPerUtxoWord
/// 6.  keyDeposit           18. costModels
/// 7.  poolDeposit          19. prices [priceMem, priceSteps]
/// 8.  eMax                 20. maxTxExUnits [mem, steps]
/// 9.  nOpt                 21. maxBlockExUnits [mem, steps]
/// 10. a0                   22. maxValSize
/// 11. rho                  23. collateralPercentage
/// 12. tau                  24. maxCollateralInputs
///
/// Differences from Shelley PP (key 16 `minUTxOValue`): replaced
/// by `coinsPerUtxoWord` at key 17.  Note: yggdrasil's
/// `coins_per_utxo_byte` field stores the Babbage-renamed value
/// (= word-value / 8); this encoder multiplies by 8 when emitting
/// the Alonzo-shape word value.
pub fn encode_alonzo_pparams_for_lsq(params: &yggdrasil_ledger::ProtocolParameters) -> Vec<u8> {
    use yggdrasil_ledger::CborEncode;
    let mut enc = Encoder::new();
    enc.array(24);
    enc.unsigned(params.min_fee_a);
    enc.unsigned(params.min_fee_b);
    enc.unsigned(params.max_block_body_size as u64);
    enc.unsigned(params.max_tx_size as u64);
    enc.unsigned(params.max_block_header_size as u64);
    enc.unsigned(params.key_deposit);
    enc.unsigned(params.pool_deposit);
    enc.unsigned(params.e_max);
    enc.unsigned(params.n_opt);
    params.a0.encode_cbor(&mut enc);
    params.rho.encode_cbor(&mut enc);
    params.tau.encode_cbor(&mut enc);
    let d = params.d.unwrap_or(yggdrasil_ledger::types::UnitInterval {
        numerator: 1,
        denominator: 1,
    });
    d.encode_cbor(&mut enc);
    encode_shelley_nonce(&mut enc, params.extra_entropy.as_ref());
    let (pv_major, pv_minor) = params.protocol_version.unwrap_or((5, 0));
    enc.array(2);
    enc.unsigned(pv_major);
    enc.unsigned(pv_minor);
    enc.unsigned(params.min_pool_cost);
    // Alonzo `coinsPerUtxoWord` = `coinsPerUtxoByte * 8`
    // (Babbage renamed and divided by 8).  Default to mainnet's
    // 34_482 word value when the snapshot doesn't carry it.
    enc.unsigned(params.coins_per_utxo_byte.map(|b| b * 8).unwrap_or(34_482));
    encode_alonzo_cost_models(&mut enc, params.cost_models.as_ref());
    encode_ex_unit_prices(
        &mut enc,
        params.price_mem.as_ref(),
        params.price_step.as_ref(),
    );
    encode_ex_units(&mut enc, params.max_tx_ex_units.as_ref());
    encode_ex_units(&mut enc, params.max_block_ex_units.as_ref());
    enc.unsigned(params.max_val_size.unwrap_or(5000) as u64);
    enc.unsigned(params.collateral_percentage.unwrap_or(150));
    enc.unsigned(params.max_collateral_inputs.unwrap_or(3) as u64);
    enc.into_bytes()
}

/// Encode Babbage-era `PParams` in the upstream `GetCurrentPParams`
/// response shape: a 22-element CBOR list.  Differs from Alonzo:
/// - drops `d` (decentralization, key 13)
/// - drops `extraEntropy` (key 14)
/// - renames `coinsPerUtxoWord` → `coinsPerUtxoByte` at key 17
///   (= word-value / 8 — yggdrasil's `coins_per_utxo_byte` already
///   stores the per-byte value).
///
/// Upstream `Cardano.Ledger.Babbage.PParams.encCBOR` order:
///
/// 1.  minfeeA              12. protocolVersion
/// 2.  minfeeB              13. minPoolCost
/// 3.  maxBBSize            14. coinsPerUtxoByte
/// 4.  maxTxSize            15. costModels
/// 5.  maxBHSize            16. prices [priceMem, priceSteps]
/// 6.  keyDeposit           17. maxTxExUnits [mem, steps]
/// 7.  poolDeposit          18. maxBlockExUnits [mem, steps]
/// 8.  eMax                 19. maxValSize
/// 9.  nOpt                 20. collateralPercentage
/// 10. a0                   21. maxCollateralInputs
/// 11. rho/tau              22. (rho is 11, tau is 12 in the actual list)
///
/// Actually the canonical order is: [minFeeA, minFeeB, maxBBSize,
/// maxTxSize, maxBHSize, keyDeposit, poolDeposit, eMax, nOpt, a0,
/// rho, tau, protocolVersion, minPoolCost, coinsPerUtxoByte,
/// costModels, prices, maxTxExUnits, maxBlockExUnits, maxValSize,
/// collateralPercentage, maxCollateralInputs] — 22 fields.
pub fn encode_babbage_pparams_for_lsq(params: &yggdrasil_ledger::ProtocolParameters) -> Vec<u8> {
    use yggdrasil_ledger::CborEncode;
    let mut enc = Encoder::new();
    enc.array(22);
    enc.unsigned(params.min_fee_a);
    enc.unsigned(params.min_fee_b);
    enc.unsigned(params.max_block_body_size as u64);
    enc.unsigned(params.max_tx_size as u64);
    enc.unsigned(params.max_block_header_size as u64);
    enc.unsigned(params.key_deposit);
    enc.unsigned(params.pool_deposit);
    enc.unsigned(params.e_max);
    enc.unsigned(params.n_opt);
    params.a0.encode_cbor(&mut enc);
    params.rho.encode_cbor(&mut enc);
    params.tau.encode_cbor(&mut enc);
    let (pv_major, pv_minor) = params.protocol_version.unwrap_or((7, 0));
    enc.array(2);
    enc.unsigned(pv_major);
    enc.unsigned(pv_minor);
    enc.unsigned(params.min_pool_cost);
    // Babbage `coinsPerUtxoByte` is yggdrasil's
    // `coins_per_utxo_byte` directly (already in per-byte form,
    // not Alonzo's per-word).  Default to the mainnet value
    // (4310 = 34482/8).
    enc.unsigned(params.coins_per_utxo_byte.unwrap_or(4_310));
    encode_alonzo_cost_models(&mut enc, params.cost_models.as_ref());
    encode_ex_unit_prices(
        &mut enc,
        params.price_mem.as_ref(),
        params.price_step.as_ref(),
    );
    encode_ex_units(&mut enc, params.max_tx_ex_units.as_ref());
    encode_ex_units(&mut enc, params.max_block_ex_units.as_ref());
    enc.unsigned(params.max_val_size.unwrap_or(5000) as u64);
    enc.unsigned(params.collateral_percentage.unwrap_or(150));
    enc.unsigned(params.max_collateral_inputs.unwrap_or(3) as u64);
    enc.into_bytes()
}

fn encode_alonzo_cost_models(
    enc: &mut Encoder,
    cost_models: Option<&yggdrasil_ledger::protocol_params::CostModels>,
) {
    match cost_models {
        Some(map) => {
            enc.map(map.len() as u64);
            for (lang, ops) in map {
                enc.unsigned(*lang as u64);
                enc.array(ops.len() as u64);
                for op in ops {
                    if *op >= 0 {
                        enc.unsigned(*op as u64);
                    } else {
                        enc.negative((-(*op + 1)) as u64);
                    }
                }
            }
        }
        None => {
            enc.map(0);
        }
    }
}

fn encode_ex_unit_prices(
    enc: &mut Encoder,
    price_mem: Option<&yggdrasil_ledger::types::UnitInterval>,
    price_step: Option<&yggdrasil_ledger::types::UnitInterval>,
) {
    use yggdrasil_ledger::CborEncode;
    let default_price = yggdrasil_ledger::types::UnitInterval {
        numerator: 0,
        denominator: 1,
    };
    let pm = price_mem.unwrap_or(&default_price);
    let ps = price_step.unwrap_or(&default_price);
    enc.array(2);
    pm.encode_cbor(enc);
    ps.encode_cbor(enc);
}

fn encode_ex_units(enc: &mut Encoder, ex_units: Option<&yggdrasil_ledger::eras::alonzo::ExUnits>) {
    enc.array(2);
    match ex_units {
        Some(eu) => {
            enc.unsigned(eu.mem);
            enc.unsigned(eu.steps);
        }
        None => {
            enc.unsigned(0);
            enc.unsigned(0);
        }
    }
}

/// Encode Conway-era `PParams` in the upstream `GetCurrentPParams`
/// response shape — a 31-element CBOR list.  Conway extends Babbage
/// with 9 governance fields:
/// - `poolVotingThresholds` (5-element list of UnitInterval)
/// - `drepVotingThresholds` (10-element list of UnitInterval)
/// - `minCommitteeSize` (u64)
/// - `committeeTermLimit` (u64 epoch interval)
/// - `govActionLifetime` (u64 epoch interval)
/// - `govActionDeposit` (u64 lovelace)
/// - `drepDeposit` (u64 lovelace)
/// - `drepActivity` (u64 epoch interval)
/// - `minFeeRefScriptCostPerByte` (UnitInterval, used for the
///   tiered ref-script fee per upstream `tierRefScriptFee`)
///
/// Field order per `Cardano.Ledger.Conway.PParams.encCBOR`: the
/// 22 Babbage fields followed by 9 governance fields.  Defaults
/// for missing optional fields match the Conway-genesis values
/// for mainnet.
pub fn encode_conway_pparams_for_lsq(params: &yggdrasil_ledger::ProtocolParameters) -> Vec<u8> {
    use yggdrasil_ledger::CborEncode;
    let mut enc = Encoder::new();
    enc.array(31);
    // 1-12: Same as Babbage prefix.
    enc.unsigned(params.min_fee_a);
    enc.unsigned(params.min_fee_b);
    enc.unsigned(params.max_block_body_size as u64);
    enc.unsigned(params.max_tx_size as u64);
    enc.unsigned(params.max_block_header_size as u64);
    enc.unsigned(params.key_deposit);
    enc.unsigned(params.pool_deposit);
    enc.unsigned(params.e_max);
    enc.unsigned(params.n_opt);
    params.a0.encode_cbor(&mut enc);
    params.rho.encode_cbor(&mut enc);
    params.tau.encode_cbor(&mut enc);
    // 13: protocolVersion `[major, minor]` — default to (9, 0)
    // (Conway transition) when missing.
    let (pv_major, pv_minor) = params.protocol_version.unwrap_or((9, 0));
    enc.array(2);
    enc.unsigned(pv_major);
    enc.unsigned(pv_minor);
    // 14-16: minPoolCost, coinsPerUtxoByte, costModels.
    enc.unsigned(params.min_pool_cost);
    enc.unsigned(params.coins_per_utxo_byte.unwrap_or(4_310));
    encode_alonzo_cost_models(&mut enc, params.cost_models.as_ref());
    // 17-21: prices, maxTx/Block ExUnits, maxValSize.
    encode_ex_unit_prices(
        &mut enc,
        params.price_mem.as_ref(),
        params.price_step.as_ref(),
    );
    encode_ex_units(&mut enc, params.max_tx_ex_units.as_ref());
    encode_ex_units(&mut enc, params.max_block_ex_units.as_ref());
    enc.unsigned(params.max_val_size.unwrap_or(5000) as u64);
    // 22-23: collateralPercentage, maxCollateralInputs.
    enc.unsigned(params.collateral_percentage.unwrap_or(150));
    enc.unsigned(params.max_collateral_inputs.unwrap_or(3) as u64);
    // 24-25: governance voting thresholds.
    let pool_thresh = params.pool_voting_thresholds.clone().unwrap_or_default();
    pool_thresh.encode_cbor(&mut enc);
    let drep_thresh = params.drep_voting_thresholds.clone().unwrap_or_default();
    drep_thresh.encode_cbor(&mut enc);
    // 26-30: committee + governance-action params (Conway-genesis defaults).
    enc.unsigned(params.min_committee_size.unwrap_or(7));
    enc.unsigned(params.committee_term_limit.unwrap_or(146));
    enc.unsigned(params.gov_action_lifetime.unwrap_or(6));
    enc.unsigned(params.gov_action_deposit.unwrap_or(100_000_000_000));
    enc.unsigned(params.drep_deposit.unwrap_or(500_000_000));
    // 31: drepActivity, minFeeRefScriptCostPerByte.
    enc.unsigned(params.drep_activity.unwrap_or(20));
    let ref_script_default = yggdrasil_ledger::types::UnitInterval {
        numerator: 15,
        denominator: 1,
    };
    let ref_script_cost = params
        .min_fee_ref_script_cost_per_byte
        .as_ref()
        .unwrap_or(&ref_script_default);
    ref_script_cost.encode_cbor(&mut enc);
    enc.into_bytes()
}

/// Encode upstream's `Nonce` per
/// `Cardano.Ledger.BaseTypes.Nonce.encCBOR`:
/// - `NeutralNonce` → `[0]` (1-element list with value 0)
/// - `Nonce h` → `[1, h]` (2-element list)
fn encode_shelley_nonce(enc: &mut Encoder, nonce: Option<&yggdrasil_ledger::types::Nonce>) {
    use yggdrasil_ledger::types::Nonce;
    match nonce {
        Some(Nonce::Hash(h)) => {
            enc.array(2);
            enc.unsigned(1);
            enc.bytes(h);
        }
        _ => {
            enc.array(1);
            enc.unsigned(0);
        }
    }
}

/// Encode the result of [`UpstreamQuery::GetChainBlockNo`].
///
/// Upstream `BlockNo` is `WithOrigin BlockNo` encoded as either
/// `[0]` (Origin) or `[1, n]` (At n).
pub fn encode_chain_block_no(block_no: Option<u64>) -> Vec<u8> {
    let mut enc = Encoder::new();
    match block_no {
        None => {
            enc.array(1);
            enc.unsigned(0);
        }
        Some(n) => {
            enc.array(2);
            enc.unsigned(1);
            enc.unsigned(n);
        }
    }
    enc.into_bytes()
}

/// Encode the result of [`UpstreamQuery::GetSystemStart`] as a
/// `SystemStart` (UTCTime) per upstream `Cardano.Slotting.Time`.
///
/// Upstream's Serialise instance for UTCTime is a 3-element CBOR
/// list `[year, dayOfYear, picosecondsOfDay]` per the
/// 2026-04-27 cardano-cli decoder error message
/// `Size mismatch when decoding UTCTime. Expected 3, but found 2.`:
///   - year: integer Gregorian year (e.g. 2022)
///   - dayOfYear: integer day of year `[1, 366]`
///   - picosecondsOfDay: integer in `[0, 86400*10^12)`
pub fn encode_system_start(year: u64, day_of_year: u64, picoseconds_of_day: u64) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    enc.unsigned(year);
    enc.unsigned(day_of_year);
    enc.unsigned(picoseconds_of_day);
    enc.into_bytes()
}

/// Encode the result of `BlockQuery (QueryHardFork GetCurrentEra)`.
///
/// Upstream NtC V_23 emits `EraIndex` as a bare CBOR unsigned per the
/// 2026-04-27 operator capture (`socat -x -v` proxy on
/// `cardano-node 10.7.1`'s NtC socket): `MsgResult` for
/// `BlockQuery (QueryHardFork GetCurrentEra)` is `82 04 02` —
/// `[4, 2]` with `2` (Allegra ordinal) as a bare uint.  Round 149
/// extends yggdrasil to advertise NtC V_17..V_23 so the negotiated
/// version against upstream `cardano-cli 10.16.0.0` is V_23, matching
/// the canonical V_23 wire format.
pub fn encode_era_index(index: u32) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.unsigned(index as u64);
    enc.into_bytes()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Round 152 — pin the preprod Interpreter Byron prefix
    /// byte-for-byte against upstream `cardano-node 10.7.1`'s
    /// captured wire bytes.  When this regresses, cardano-cli's
    /// `query tip` against yggdrasil silently falls back to
    /// displaying origin (`slot=0/epoch=0/syncProgress=0.00`) and
    /// operator visibility into chain progress is lost.  Capture
    /// source: `/tmp/ygg-runbook/haskell-traffic.bin` (socat -x -v
    /// proxy of `cardano-cli 10.16.0.0 query tip --testnet-magic 1`
    /// against upstream Haskell preprod).
    #[test]
    fn preprod_interpreter_byron_prefix_matches_upstream_capture() {
        let bytes = encode_interpreter_minimal(21_600, 1);
        // Wire shape (37 bytes for Byron summary):
        //   9f                                 — indef-array opener
        //   83                                 — Byron summary [eraStart, eraEnd, params]
        //     83 00 00 00                      — eraStart [relativeTime=0, slot=0, epoch=0]
        //     83 1b 17fb16d83be00000 1a 00015180 04
        //                                      — eraEnd [1.728e18 ps, 86400, 4]
        //     84 195460 194e20 83 00 1910e0 81 00 1910e0
        //                                      — params [21600, 20000, [0,4320,[0]], 4320]
        let expected_byron_prefix: [u8; 39] = [
            0x9f, // indef-array opener
            0x83, // Byron summary header
            0x83, 0x00, 0x00, 0x00, // eraStart [0,0,0]
            0x83, // eraEnd opener
            0x1b, 0x17, 0xfb, 0x16, 0xd8, 0x3b, 0xe0, 0x00,
            0x00, // relativeTime u64 (NOT bignum)
            0x1a, 0x00, 0x01, 0x51, 0x80, // slot 86400
            0x04, // epoch 4
            0x84, // eraParams opener (4 fields)
            0x19, 0x54, 0x60, // epochSize=21600
            0x19, 0x4e, 0x20, // slotLength=20000ms
            0x83, 0x00, 0x19, 0x10, 0xe0, 0x81, 0x00, // safeZone=[0,4320,[0]]
            0x19, 0x10, 0xe0, // genesisWindow=4320
        ];
        assert!(
            bytes.starts_with(&expected_byron_prefix),
            "Byron summary prefix must match upstream capture verbatim — \
             relativeTime is CBOR uint (0x1b prefix), NOT bignum (0xc2 0x48 …); \
             when this drifts, cardano-cli silently falls back to origin tip",
        );
    }

    /// Round 152 — pin the Shelley summary's `epochSize=432000` and
    /// `slotLength=1000ms` against the socat capture.  Earlier
    /// drafts used Shelley `epochSize=21600` (Byron-shape) which
    /// caused cardano-cli to compute the wrong epoch boundaries
    /// (and ultimately fall back to origin display because the
    /// Shelley summary failed downstream validation).
    #[test]
    fn preprod_interpreter_shelley_uses_captured_epoch_size_and_genesis_window() {
        let bytes = encode_interpreter_minimal(21_600, 1);
        // Shelley summary's params block: locate by walking past
        // the Byron summary (38 bytes) then past the Shelley
        // start+end Bound headers.  The Shelley `eraParams` starts
        // with `84 1a 00069780 1903e8 …` — the `0x69780` (432000)
        // is the load-bearing value.
        let shelley_params_marker = [0x84u8, 0x1a, 0x00, 0x06, 0x97, 0x80, 0x19, 0x03, 0xe8];
        assert!(
            bytes
                .windows(shelley_params_marker.len())
                .any(|w| w == shelley_params_marker),
            "Shelley eraParams must start with `84 1a 00069780 1903e8` \
             (epochSize=432000, slotLength=1000ms) — captured from \
             upstream `cardano-node 10.7.1`; using Byron-shape values \
             (21600/20000) here breaks cardano-cli's slot↔epoch \
             conversion",
        );
        // Shelley genesisWindow=129600 (0x1fa40) and
        // safeZone=[0, 129600, [0]] both reuse the same 4-byte literal.
        let shelley_genesis_window = [0x1au8, 0x00, 0x01, 0xfa, 0x40];
        let occurrences = bytes
            .windows(shelley_genesis_window.len())
            .filter(|w| *w == shelley_genesis_window)
            .count();
        assert!(
            occurrences >= 2,
            "Shelley summary must encode 0x1fa40 (=129600) for both \
             safeZone-slots and genesisWindow",
        );
    }

    /// Round 153 — preview testnet's vendored `shelley-genesis.json`
    /// pins `epochLength=86_400` (1-day epochs at 1s/slot) and
    /// `config.json` sets every `Test*HardForkAtEpoch=0` (no Byron
    /// blocks).  This test pins the resulting wire shape so a future
    /// drift in either constant fails CI rather than silently
    /// regressing operator-visible cardano-cli output.
    #[test]
    fn preview_interpreter_emits_single_shelley_summary_with_1day_epochs() {
        let bytes = encode_interpreter_for_network(NetworkKind::Preview);
        // Indef-array opener
        assert_eq!(bytes[0], 0x9f, "Summary indef-array opener");
        assert_eq!(bytes[1], 0x83, "Single EraSummary header (array len 3)");
        // eraStart [0, 0, 0]
        assert_eq!(
            &bytes[2..6],
            &[0x83, 0x00, 0x00, 0x00],
            "Preview eraStart=[0,0,0]"
        );
        // Critical: Preview eraParams use epochSize=86_400 (NOT
        // 432_000 as preprod), encoded as `1a 00 01 51 80`.
        let expected_preview_params_marker =
            [0x84u8, 0x1a, 0x00, 0x01, 0x51, 0x80, 0x19, 0x03, 0xe8];
        assert!(
            bytes
                .windows(expected_preview_params_marker.len())
                .any(|w| w == expected_preview_params_marker),
            "Preview eraParams must start with `84 1a 00015180 1903e8` \
             (epochSize=86_400, slotLength=1000ms) — preprod's `0x69780` \
             (=432_000) must NOT appear in preview output",
        );
        // Confirm preprod's signature `0x69780` (=432_000) is NOT in
        // the preview output — guards against accidentally falling
        // through to the preprod encoder.
        let preprod_marker = [0x1au8, 0x00, 0x06, 0x97, 0x80];
        assert!(
            !bytes
                .windows(preprod_marker.len())
                .any(|w| w == preprod_marker),
            "Preview must NOT emit preprod's epochSize=432_000",
        );
    }

    /// Round 153 — preview's `systemStart` is 2022-10-25 (day-of-year
    /// 298).  Pin the encoding so a regression in the date constant
    /// fails CI cleanly.
    #[test]
    fn preview_system_start_is_2022_day_298() {
        let bytes = encode_system_start_for_network(NetworkKind::Preview);
        // [year=2022, dayOfYear=298, picosecondsOfDay=0]
        // 2022 = uint16 0x07e6, 298 = uint16 0x012a.
        assert_eq!(bytes, [0x83, 0x19, 0x07, 0xe6, 0x19, 0x01, 0x2a, 0x00]);
    }

    /// Round 153 — preprod `systemStart` baseline pinned alongside
    /// the per-network selector to guard against accidental swap.
    #[test]
    fn preprod_system_start_is_2022_day_152() {
        let bytes = encode_system_start_for_network(NetworkKind::Preprod);
        // 2022 = 0x07e6, 152 = uint8 0x18 0x98.
        assert_eq!(bytes, [0x83, 0x19, 0x07, 0xe6, 0x18, 0x98, 0x00]);
    }

    /// Round 156 — captured upstream `cardano-cli 10.16.0.0 query
    /// protocol-parameters --testnet-magic 1` payload:
    /// `82 03 82 00 82 00 82 01 81 03` =
    /// `MsgQuery [BlockQuery [QueryIfCurrent [era_index=1, [GetCurrentPParams=3]]]]`.
    /// Pin the decoder so a future drift in any layer fails CI cleanly.
    #[test]
    fn decode_real_cardano_cli_get_current_pparams_payload() {
        // The full MsgQuery wraps the UpstreamQuery; extract the
        // UpstreamQuery payload (skip the leading `82 03` MsgQuery
        // wrapper which is the LSQ codec's responsibility).
        let upstream_query_bytes = [0x82, 0x00, 0x82, 0x00, 0x82, 0x01, 0x81, 0x03];
        let q = UpstreamQuery::decode(&upstream_query_bytes).expect("must decode");
        let inner = match q {
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryIfCurrent { inner_cbor }) => {
                inner_cbor
            }
            other => panic!("expected QueryIfCurrent, got {other:?}"),
        };
        let (era_idx, era_query) =
            decode_query_if_current(&inner).expect("inner decode must succeed");
        assert_eq!(era_idx, 1, "era_index must be Shelley=1");
        assert!(matches!(era_query, EraSpecificQuery::GetCurrentPParams));
    }

    /// Round 157 — pin the captured `query utxo --whole-utxo`
    /// payload `82 00 82 00 82 01 81 07` so a future drift in
    /// QueryIfCurrent or `GetWholeUTxO` (era-specific tag 7)
    /// fails CI cleanly.
    #[test]
    fn decode_real_cardano_cli_get_whole_utxo_payload() {
        let bytes = [0x82, 0x00, 0x82, 0x00, 0x82, 0x01, 0x81, 0x07];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        let inner = match q {
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryIfCurrent { inner_cbor }) => {
                inner_cbor
            }
            other => panic!("expected QueryIfCurrent, got {other:?}"),
        };
        let (era_idx, era_query) = decode_query_if_current(&inner).expect("decode");
        assert_eq!(era_idx, 1);
        assert!(matches!(era_query, EraSpecificQuery::GetWholeUTxO));
    }

    /// Round 157 — pin the captured `query utxo --tx-in` payload
    /// shape: `[era_idx=1, [15, txin_set]]`.  Tag **15** (NOT 14) is
    /// the load-bearing fact captured from the 2026-04-28
    /// cardano-cli rehearsal.
    #[test]
    fn decode_real_cardano_cli_get_utxo_by_tx_in_payload() {
        // Inner: `82 01 82 0f 81 82 58 20 <32 bytes txid> 00`.
        let mut inner = vec![0x82, 0x01, 0x82, 0x0f, 0x81, 0x82, 0x58, 0x20];
        inner.extend_from_slice(&[0xa0u8; 32]);
        inner.push(0x00); // index 0
        let (era_idx, era_query) = decode_query_if_current(&inner).expect("decode");
        assert_eq!(era_idx, 1);
        match era_query {
            EraSpecificQuery::GetUTxOByTxIn { txin_set_cbor } => {
                // First byte must be the array length-1 marker (0x81).
                assert_eq!(txin_set_cbor[0], 0x81, "txin_set is array len 1");
            }
            other => panic!("expected GetUTxOByTxIn, got {other:?}"),
        }
    }

    /// Round 157 — `GetUTxOByAddress` is era-specific tag 6.  Pin
    /// the decoder so a future drift in tag assignment fails CI.
    #[test]
    fn decode_get_utxo_by_address_recognises_tag_6() {
        // Inner: `[1, [6, [<addr_bytes>]]]`.
        let mut inner = vec![0x82, 0x01, 0x82, 0x06, 0x81, 0x58, 0x1d];
        inner.extend_from_slice(&[0xab; 29]); // 29-byte addr
        let (era_idx, era_query) = decode_query_if_current(&inner).expect("decode");
        assert_eq!(era_idx, 1);
        assert!(matches!(
            era_query,
            EraSpecificQuery::GetUTxOByAddress { .. }
        ));
    }

    /// Round 173 (corrected R179) — pin the era-specific tag table
    /// addition for `GetStakeSnapshots` (tag 20 per upstream).
    /// Wire form mirrors `GetPoolState` (Maybe payload) but with
    /// tag 20.
    #[test]
    fn decode_recognises_stake_snapshots_tag_with_just_filter() {
        // [1, [20, [1, tag(258)[bytes(28)]]]] = era 1, GetStakeSnapshots
        // (Just {single_pool_keyhash})
        let mut payload = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x14, // [tag=20, maybe_payload]
            0x82, 0x01, // [discriminator=1 (Just), set]
            0xd9, 0x01, 0x02, // tag 258
            0x81, // 1-element array
            0x58, 0x1c, // bytes(28)
        ];
        payload.extend_from_slice(&[0x77; 28]);
        let (era_idx, q) = decode_query_if_current(&payload).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetStakeSnapshots { .. }));
    }

    /// Round 173 (corrected R179) — pin the `Nothing` shape for
    /// `GetStakeSnapshots` at tag 20.
    #[test]
    fn decode_recognises_stake_snapshots_tag_with_nothing_filter() {
        // [1, [20, [0]]] = era 1, GetStakeSnapshots Nothing
        let payload = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x14, // [tag=20, maybe_payload]
            0x81, 0x00, // [0] = Nothing
        ];
        let (era_idx, q) = decode_query_if_current(&payload).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetStakeSnapshots { .. }));
    }

    /// Round 172 (corrected R179) — pin the era-specific tag table
    /// addition for `GetPoolState` (tag 19 per upstream) with
    /// `Just <set>` payload.
    #[test]
    fn decode_recognises_pool_state_tag_with_just_filter() {
        // [1, [19, [1, tag(258)[bytes(28)]]]] = era 1, GetPoolState
        // (Just {single_pool_keyhash})
        // Wire form: 82 01 82 13 82 01 d9 0102 81 581c <28 bytes>
        let mut payload = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x13, // [tag=19, maybe_payload]
            0x82, 0x01, // [discriminator=1 (Just), set]
            0xd9, 0x01, 0x02, // tag 258
            0x81, // 1-element array
            0x58, 0x1c, // bytes(28)
        ];
        payload.extend_from_slice(&[0x55; 28]);
        let (era_idx, q) = decode_query_if_current(&payload).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetPoolState { .. }));
    }

    /// Round 172 (corrected R179) — pin the `Nothing` shape for
    /// `GetPoolState` at tag 19.
    #[test]
    fn decode_recognises_pool_state_tag_with_nothing_filter() {
        // [1, [19, [0]]] = era 1, GetPoolState Nothing
        // Wire form: 82 01 82 13 81 00
        let payload = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x13, // [tag=19, maybe_payload]
            0x81, 0x00, // [0] = Nothing
        ];
        let (era_idx, q) = decode_query_if_current(&payload).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetPoolState { .. }));
    }

    /// Round 171 (corrected R179) — pin the era-specific tag table
    /// addition for `GetStakePoolParams` (tag 17 per upstream).
    #[test]
    fn decode_recognises_stake_pool_params_tag() {
        // [1, [17, tag(258)[bytes(28)]]] = era 1, GetStakePoolParams
        // with a single 28-byte pool keyhash in a CIP-21 tagged set.
        // Wire form: 82 01 82 11 d9 0102 81 581c <28 bytes>
        let mut payload = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x11, // [tag=17, set]
            0xd9, 0x01, 0x02, // tag 258
            0x81, // 1-element array
            0x58, 0x1c, // bytes(28)
        ];
        payload.extend_from_slice(&[0xcd; 28]);
        let (era_idx, q) = decode_query_if_current(&payload).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetStakePoolParams { .. }));
    }

    /// Round 179 — pin upstream tag 37 (`GetStakeDistribution2`,
    /// post-Conway no-VRF variant) decoded as `GetStakeDistribution`.
    /// `cardano-cli query stake-distribution` sends tag 37 since
    /// cardano-node 10.x.
    #[test]
    fn decode_recognises_stake_distribution2_tag_37() {
        let payload = vec![0x82, 0x01, 0x81, 0x18, 0x25]; // [1, [37]]
        let (era_idx, q) = decode_query_if_current(&payload).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetStakeDistribution));
    }

    /// Round 189 — pin upstream tag 34 `GetLedgerPeerSnapshot'`.
    /// cardano-cli 10.16 sends the v15+ 2-element form with a
    /// `peer_kind` byte (`0` = BigLedgerPeers, `1` =
    /// AllLedgerPeers).  Older clients may send the 1-element
    /// singleton form.
    #[test]
    fn decode_recognises_ledger_peer_snapshot_tag_34() {
        // [1, [34, 1]] = era 1, GetLedgerPeerSnapshot AllLedgerPeers
        let payload_v15 = vec![0x82, 0x01, 0x82, 0x18, 0x22, 0x01];
        let (era_idx, q) = decode_query_if_current(&payload_v15).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(
            q,
            EraSpecificQuery::GetLedgerPeerSnapshot { peer_kind: Some(1) }
        ));

        // [1, [34]] = era 1, GetLedgerPeerSnapshot pre-v15
        let payload_legacy = vec![0x82, 0x01, 0x81, 0x18, 0x22];
        let (era_idx, q) = decode_query_if_current(&payload_legacy).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(
            q,
            EraSpecificQuery::GetLedgerPeerSnapshot { peer_kind: None }
        ));
    }

    /// Round 187 — pin upstream tag 32 `GetRatifyState`
    /// (singleton query — no parameters).
    #[test]
    fn decode_recognises_ratify_state_tag_32() {
        // [1, [32]] = era 1, GetRatifyState
        let payload = vec![0x82, 0x01, 0x81, 0x18, 0x20];
        let (era_idx, q) = decode_query_if_current(&payload).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetRatifyState));
    }

    /// Round 183 — pin upstream tag 33 `GetFuturePParams`
    /// (singleton, no parameters).
    #[test]
    fn decode_recognises_future_pparams_tag_33() {
        // [1, [33]] = era 1, GetFuturePParams
        let payload = vec![0x82, 0x01, 0x81, 0x18, 0x21];
        let (era_idx, q) = decode_query_if_current(&payload).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetFuturePParams));
    }

    /// Round 184 — pin upstream tags 26 `GetDRepStakeDistr`,
    /// 28 `GetFilteredVoteDelegatees`, and 30 `GetSPOStakeDistr`
    /// (each 2-element query carrying a filter set).
    #[test]
    fn decode_recognises_drep_and_spo_stake_distr_tags() {
        // [1, [26, tag(258) [empty]]] = GetDRepStakeDistr
        let drep_stake = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x18, 0x1a, // 2-elem list, tag 26
            0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
        ];
        let (era_idx, q) = decode_query_if_current(&drep_stake).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetDRepStakeDistr { .. }));

        // [1, [28, tag(258) [empty]]] = GetFilteredVoteDelegatees
        let vote_delegatees = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x18, 0x1c, // 2-elem list, tag 28
            0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
        ];
        let (era_idx, q) = decode_query_if_current(&vote_delegatees).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(
            q,
            EraSpecificQuery::GetFilteredVoteDelegatees { .. }
        ));

        // [1, [30, tag(258) [empty]]] = GetSPOStakeDistr
        let spo_stake = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x18, 0x1e, // 2-elem list, tag 30
            0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
        ];
        let (era_idx, q) = decode_query_if_current(&spo_stake).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetSPOStakeDistr { .. }));
    }

    /// Round 186 — pin upstream tags 22 `GetStakeDelegDeposits`
    /// (Map Credential Coin) and 36 `GetPoolDistr2` (PoolDistr
    /// with optional pool-id filter).
    #[test]
    fn decode_recognises_stake_deleg_deposits_and_pool_distr2_tags() {
        // [1, [22, tag(258) [empty]]] = GetStakeDelegDeposits
        let stake_deleg_deposits = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x16, // 2-elem list, tag 22
            0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
        ];
        let (era_idx, q) = decode_query_if_current(&stake_deleg_deposits).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetStakeDelegDeposits { .. }));

        // [1, [36, []]] = GetPoolDistr2 with `Nothing` filter
        let pool_distr2 = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x18, 0x24, // 2-elem list, tag 36
            0x80, // empty list (Maybe Nothing)
        ];
        let (era_idx, q) = decode_query_if_current(&pool_distr2).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetPoolDistr2 { .. }));
    }

    /// Round 185 — pin upstream tags 31 `GetProposals` (Seq
    /// of GovActionState filtered by gov-action-id set) and
    /// 35 `QueryStakePoolDefaultVote` (per-pool default-vote
    /// query).
    #[test]
    fn decode_recognises_proposals_and_default_vote_tags() {
        // [1, [31, tag(258) [empty]]] = GetProposals
        let proposals = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x18, 0x1f, // 2-elem list, tag 31
            0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
        ];
        let (era_idx, q) = decode_query_if_current(&proposals).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(q, EraSpecificQuery::GetProposals { .. }));

        // [1, [35, bytes(28)]] = QueryStakePoolDefaultVote
        let mut default_vote = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x18, 0x23, // 2-elem list, tag 35
            0x58, 0x1c, // bytes(28)
        ];
        default_vote.extend_from_slice(&[0u8; 28]);
        let (era_idx, q) = decode_query_if_current(&default_vote).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(
            q,
            EraSpecificQuery::QueryStakePoolDefaultVote { .. }
        ));
    }

    /// Round 182 — pin upstream tag 27 `GetCommitteeMembersState`
    /// (4-element query with cold creds + hot creds + statuses
    /// filter sets).
    #[test]
    fn decode_recognises_committee_members_state_tag_27() {
        // [1, [27, set_cold, set_hot, set_status]]
        // = [1, [4-elem [27, tag(258)[empty], tag(258)[empty], tag(258)[empty]]]]
        let payload = vec![
            0x82, 0x01, // [era=1, ...]
            0x84, 0x18, 0x1b, // 4-elem list, tag 27
            0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array (cold)
            0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array (hot)
            0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array (statuses)
        ];
        let (era_idx, q) = decode_query_if_current(&payload).unwrap();
        assert_eq!(era_idx, 1);
        assert!(matches!(
            q,
            EraSpecificQuery::GetCommitteeMembersState { .. }
        ));
    }

    /// Round 180 — pin upstream Conway-only governance query tags:
    /// 23 GetConstitution, 24 GetGovState, 25 GetDRepState (with
    /// credential-set parameter), 29 GetAccountState.
    #[test]
    fn decode_recognises_conway_governance_tags() {
        // [1, [23]] = GetConstitution
        let constitution = vec![0x82, 0x01, 0x81, 0x17];
        let (_, q) = decode_query_if_current(&constitution).unwrap();
        assert!(matches!(q, EraSpecificQuery::GetConstitution));

        // [1, [24]] = GetGovState
        let gov_state = vec![0x82, 0x01, 0x81, 0x18, 0x18];
        let (_, q) = decode_query_if_current(&gov_state).unwrap();
        assert!(matches!(q, EraSpecificQuery::GetGovState));

        // [1, [25, <credential_set>]] = GetDRepState
        let drep_state = vec![
            0x82, 0x01, // [era=1, ...]
            0x82, 0x18, 0x19, // [tag=25, set]
            0xd9, 0x01, 0x02, 0x80, // tag 258 + empty array
        ];
        let (_, q) = decode_query_if_current(&drep_state).unwrap();
        assert!(matches!(q, EraSpecificQuery::GetDRepState { .. }));

        // [1, [29]] = GetAccountState
        let account_state = vec![0x82, 0x01, 0x81, 0x18, 0x1d];
        let (_, q) = decode_query_if_current(&account_state).unwrap();
        assert!(matches!(q, EraSpecificQuery::GetAccountState));
    }

    /// Round 163 (corrected R179) — pin the era-specific tag table
    /// for `GetStakeDistribution` (5), `GetFilteredDelegationsAndRewardAccounts`
    /// (10), `GetGenesisConfig` (11), `GetStakePools` (tag 16 per
    /// upstream — was 13 in R163, off by 3).
    #[test]
    fn decode_recognises_stake_pool_distribution_genesis_tags() {
        // [1, [5]] = era 1, GetStakeDistribution
        let stake_dist = vec![0x82, 0x01, 0x81, 0x05];
        let (_, q) = decode_query_if_current(&stake_dist).unwrap();
        assert!(matches!(q, EraSpecificQuery::GetStakeDistribution));

        // [1, [11]] = era 1, GetGenesisConfig
        let gen_cfg = vec![0x82, 0x01, 0x81, 0x0b];
        let (_, q) = decode_query_if_current(&gen_cfg).unwrap();
        assert!(matches!(q, EraSpecificQuery::GetGenesisConfig));

        // [1, [16]] = era 1, GetStakePools
        let stake_pools = vec![0x82, 0x01, 0x81, 0x10];
        let (_, q) = decode_query_if_current(&stake_pools).unwrap();
        assert!(matches!(q, EraSpecificQuery::GetStakePools));

        // [1, [10, [<credentials>]]] = GetFilteredDelegationsAndRewardAccounts
        let mut delegs = vec![0x82, 0x01, 0x82, 0x0a, 0x81, 0x82, 0x00, 0x58, 0x1c];
        delegs.extend_from_slice(&[0xab; 28]);
        let (_, q) = decode_query_if_current(&delegs).unwrap();
        assert!(matches!(
            q,
            EraSpecificQuery::GetFilteredDelegationsAndRewardAccounts { .. }
        ));
    }

    /// Round 156 — encode_query_if_current_match must produce a
    /// **1-element** CBOR list (not 2-element with tag) per upstream
    /// `encodeEitherMismatch`.  This is the load-bearing wire-shape
    /// fact: cardano-cli's decoder uses list-len discrimination
    /// between Right (len=1) and Left (len=2) — there is NO leading
    /// variant tag for Right.
    #[test]
    fn encode_query_if_current_match_is_one_element_list_no_tag() {
        let result_payload = [0x91u8, 0x01]; // sentinel inner result
        let envelope = encode_query_if_current_match(&result_payload);
        // 0x81 = array(1), then the inner result bytes verbatim.
        assert_eq!(envelope, [0x81, 0x91, 0x01]);
        assert_ne!(
            envelope[0], 0x82,
            "must NOT be 2-element list — that's the Left/mismatch shape, \
             not Right/match",
        );
    }

    /// Round 156 — encode_query_if_current_mismatch must produce a
    /// 2-element CBOR list of NS-encoded era names per upstream
    /// `encodeEitherMismatch` `Left` case.  The order matches
    /// upstream: `era1` (the query's requested era) first, then
    /// `era2` (the ledger's actual era).
    #[test]
    fn encode_query_if_current_mismatch_is_two_element_ns_list() {
        // ledger=Shelley(1), query=Babbage(5)
        let bytes = encode_query_if_current_mismatch(1, 5);
        // 0x82 array(2), then `[5, "Babbage"]`, then `[1, "Shelley"]`.
        assert_eq!(bytes[0], 0x82, "outer list len 2");
        assert_eq!(bytes[1], 0x82, "first NS-era is a 2-element list");
        assert_eq!(bytes[2], 0x05, "first NS-era index = 5 (Babbage)");
    }

    /// Round 156 — encode_shelley_pparams_for_lsq emits the upstream
    /// 17-element PParams list with preprod-genesis-shape values.
    #[test]
    fn shelley_pparams_emit_17_element_list_with_preprod_values() {
        use yggdrasil_ledger::ProtocolParameters;
        let params = ProtocolParameters {
            min_fee_a: 44,
            min_fee_b: 155381,
            max_block_body_size: 65536,
            max_tx_size: 16384,
            max_block_header_size: 1100,
            key_deposit: 2_000_000,
            pool_deposit: 500_000_000,
            e_max: 18,
            n_opt: 150,
            min_utxo_value: Some(1_000_000),
            min_pool_cost: 340_000_000,
            protocol_version: Some((2, 0)),
            ..ProtocolParameters::default()
        };
        let bytes = encode_shelley_pparams_for_lsq(&params);
        // 0x91 = array(17).
        assert_eq!(bytes[0], 0x91, "must be 17-element list");
        // First element: minFeeA = 44 = 0x18 0x2c.
        assert_eq!(&bytes[1..3], &[0x18, 0x2c]);
        // Second: minFeeB = 155381 = 0x1a 0x00 0x02 0x5e 0xf5.
        assert_eq!(&bytes[3..8], &[0x1a, 0x00, 0x02, 0x5e, 0xf5]);
    }

    /// Round 159 — pin `encode_alonzo_pparams_for_lsq` produces a
    /// 24-element CBOR list (Alonzo's `[minfeeA, minfeeB, maxBBSize,
    /// maxTxSize, maxBHSize, keyDeposit, poolDeposit, eMax, nOpt,
    /// a0, rho, tau, d, extraEntropy, protocolVersion, minPoolCost,
    /// coinsPerUtxoWord, costModels, prices, maxTxExUnits,
    /// maxBlockExUnits, maxValSize, collateralPercentage,
    /// maxCollateralInputs]`).  This is what
    /// `cardano-cli 10.16.0.0 query protocol-parameters` against
    /// preview's Alonzo era expects, captured during the Round 159
    /// operational rehearsal.
    #[test]
    fn alonzo_pparams_emit_24_element_list() {
        use yggdrasil_ledger::ProtocolParameters;
        let params = ProtocolParameters {
            min_fee_a: 44,
            min_fee_b: 155381,
            max_block_body_size: 65536,
            max_tx_size: 16384,
            max_block_header_size: 1100,
            key_deposit: 2_000_000,
            pool_deposit: 500_000_000,
            e_max: 18,
            n_opt: 150,
            min_utxo_value: None,
            min_pool_cost: 340_000_000,
            coins_per_utxo_byte: Some(34_482 / 8),
            collateral_percentage: Some(150),
            max_collateral_inputs: Some(3),
            max_val_size: Some(5000),
            protocol_version: Some((6, 0)),
            ..ProtocolParameters::default()
        };
        let bytes = encode_alonzo_pparams_for_lsq(&params);
        // 0x98 = uint8-prefix array length follows for len ≥ 24.
        assert_eq!(bytes[0], 0x98, "must be array(N) with N≥24 prefix");
        assert_eq!(bytes[1], 24, "Alonzo PP has 24 fields");
        // Fields 1+2: minFeeA=44, minFeeB=155381 — same as Shelley prefix.
        assert_eq!(&bytes[2..4], &[0x18, 0x2c]);
        assert_eq!(&bytes[4..9], &[0x1a, 0x00, 0x02, 0x5e, 0xf5]);
    }

    /// Round 160 — pin Babbage PP shape: 22-element list (drops
    /// `d` and `extraEntropy` from Alonzo, renames
    /// `coinsPerUtxoWord` → `coinsPerUtxoByte`).
    #[test]
    fn babbage_pparams_emit_22_element_list() {
        use yggdrasil_ledger::ProtocolParameters;
        let params = ProtocolParameters {
            min_fee_a: 44,
            min_fee_b: 155381,
            max_block_body_size: 90112,
            max_tx_size: 16384,
            max_block_header_size: 1100,
            key_deposit: 2_000_000,
            pool_deposit: 500_000_000,
            e_max: 18,
            n_opt: 500,
            min_pool_cost: 340_000_000,
            coins_per_utxo_byte: Some(4_310),
            collateral_percentage: Some(150),
            max_collateral_inputs: Some(3),
            max_val_size: Some(5000),
            protocol_version: Some((8, 0)),
            ..ProtocolParameters::default()
        };
        let bytes = encode_babbage_pparams_for_lsq(&params);
        // 0x96 = array(22) (uint5-inlined since 22 < 24).
        assert_eq!(bytes[0], 0x96, "Babbage PP has 22 fields");
        assert_eq!(&bytes[1..3], &[0x18, 0x2c], "minFeeA=44");
        assert_eq!(
            &bytes[3..8],
            &[0x1a, 0x00, 0x02, 0x5e, 0xf5],
            "minFeeB=155381",
        );
    }

    /// Round 161 — pin Conway PP shape: 31-element list adding
    /// governance fields (pool/DRep voting thresholds, committee
    /// params, gov-action lifetime/deposit, DRep deposit/activity,
    /// minFeeRefScriptCostPerByte).
    #[test]
    fn conway_pparams_emit_31_element_list() {
        use yggdrasil_ledger::ProtocolParameters;
        let params = ProtocolParameters {
            min_fee_a: 44,
            min_fee_b: 155381,
            max_block_body_size: 90112,
            max_tx_size: 16384,
            max_block_header_size: 1100,
            key_deposit: 2_000_000,
            pool_deposit: 500_000_000,
            e_max: 18,
            n_opt: 500,
            min_pool_cost: 340_000_000,
            coins_per_utxo_byte: Some(4_310),
            collateral_percentage: Some(150),
            max_collateral_inputs: Some(3),
            max_val_size: Some(5000),
            protocol_version: Some((10, 0)),
            ..ProtocolParameters::default()
        };
        let bytes = encode_conway_pparams_for_lsq(&params);
        // 0x98 = uint8-len-prefix array, 0x1f = 31.
        assert_eq!(bytes[0], 0x98, "must be array(N) with N≥24 prefix");
        assert_eq!(bytes[1], 0x1f, "Conway PP has 31 fields");
        // Field 1: minFeeA=44 = 0x18 0x2c.
        assert_eq!(&bytes[2..4], &[0x18, 0x2c]);
        // Field 2: minFeeB=155381.
        assert_eq!(&bytes[4..9], &[0x1a, 0x00, 0x02, 0x5e, 0xf5]);
    }

    /// Captured upstream `cardano-cli 10.16.0.0 query tip --testnet-magic 1`
    /// payload — `BlockQuery (QueryHardFork GetCurrentEra)`.  Operator
    /// rehearsal record in
    /// `docs/operational-runs/2026-04-27-runbook-pass.md`.
    #[test]
    fn decode_real_cardano_cli_get_current_era_payload() {
        let bytes = [0x82, 0x00, 0x82, 0x02, 0x81, 0x01];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(
            q,
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryHardFork(
                QueryHardFork::GetCurrentEra
            ))
        );
        // Round-trip
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_real_cardano_cli_get_interpreter_payload() {
        let bytes = [0x82, 0x00, 0x82, 0x02, 0x81, 0x00];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(
            q,
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryHardFork(
                QueryHardFork::GetInterpreter
            ))
        );
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_get_chain_point_top_level() {
        let bytes = [0x81, 0x03];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(q, UpstreamQuery::GetChainPoint);
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_get_chain_block_no_top_level() {
        let bytes = [0x81, 0x02];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(q, UpstreamQuery::GetChainBlockNo);
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_get_system_start_top_level() {
        let bytes = [0x81, 0x01];
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(q, UpstreamQuery::GetSystemStart);
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn decode_query_anytime_get_era_start() {
        // [0, [1, [0], 3]] — BlockQuery (QueryAnytime GetEraStart era=3)
        let mut enc = Encoder::new();
        enc.array(2);
        enc.unsigned(0);
        enc.array(3);
        enc.unsigned(1);
        enc.array(1);
        enc.unsigned(0);
        enc.unsigned(3);
        let bytes = enc.into_bytes();
        let q = UpstreamQuery::decode(&bytes).expect("must decode");
        assert_eq!(
            q,
            UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryAnytime {
                kind: QueryAnytimeKind::GetEraStart,
                era_index: 3,
            })
        );
        assert_eq!(q.encode(), bytes);
    }

    #[test]
    fn unrecognised_top_level_tag_is_rejected_cleanly() {
        // `[42]` — invalid top-level tag.
        let bytes = [0x81, 0x18, 0x2a];
        UpstreamQuery::decode(&bytes).expect_err("must reject");
    }

    #[test]
    fn unrecognised_hfc_block_query_tag_rejected() {
        // [0, [99, [0]]]
        let bytes = [0x82, 0x00, 0x82, 0x18, 0x63, 0x81, 0x00];
        UpstreamQuery::decode(&bytes).expect_err("must reject");
    }

    #[test]
    fn query_hardfork_round_trip() {
        for q in [QueryHardFork::GetInterpreter, QueryHardFork::GetCurrentEra] {
            let bytes = q.encode();
            let decoded = QueryHardFork::decode(&bytes).expect("round-trip");
            assert_eq!(decoded, q);
        }
    }

    #[test]
    fn encode_chain_point_origin_is_empty_array() {
        let bytes = encode_chain_point(&Point::Origin);
        assert_eq!(bytes, vec![0x80]);
    }

    #[test]
    fn encode_chain_point_block_point_is_slot_hash_pair() {
        use yggdrasil_ledger::{HeaderHash, SlotNo};
        let p = Point::BlockPoint(SlotNo(42), HeaderHash([0xab; 32]));
        let bytes = encode_chain_point(&p);
        // [42, h'<32 bytes>'] — length 2, no constructor tag.
        assert_eq!(bytes[0], 0x82); // array len 2
        assert_eq!(bytes[1], 0x18); // CBOR uint8 escape
        assert_eq!(bytes[2], 0x2a); // 42
        assert_eq!(bytes[3], 0x58); // bytes uint8 length follows
        assert_eq!(bytes[4], 0x20); // length 32
        // Remaining 32 bytes are the hash payload.
    }

    /// Operator capture from `cardano-node 10.7.1` at NtC V_23 — Allegra era,
    /// slot 610040, hash `ec4a816d...12`.  Inner MsgResult after the [4, ...]
    /// wrapper is `82 1a 00 09 4e f8 58 20 ec 4a 81 6d ...` =
    /// `[610040, h'<32-byte hash>']`.
    #[test]
    fn encode_chain_point_matches_real_haskell_capture_block_point() {
        use yggdrasil_ledger::{HeaderHash, SlotNo};
        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(
            &hex::decode("ec4a816d939b1999386ffcda5d0df3d96a535c282c59edefdec20a9cd841cf12")
                .expect("valid hex"),
        );
        let p = Point::BlockPoint(SlotNo(610040), HeaderHash(hash_bytes));
        let bytes = encode_chain_point(&p);
        let expected = hex::decode(
            "821a00094ef85820\
             ec4a816d939b1999386ffcda5d0df3d96a535c282c59edefdec20a9cd841cf12",
        )
        .expect("valid hex");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn encode_chain_block_no_origin_and_at() {
        assert_eq!(encode_chain_block_no(None), vec![0x81, 0x00]);
        let at = encode_chain_block_no(Some(100));
        assert_eq!(at[0], 0x82); // array len 2
        assert_eq!(at[1], 0x01); // tag 1 (At)
        assert_eq!(at[2], 0x18); // uint8
        assert_eq!(at[3], 0x64); // 100
    }

    #[test]
    fn encode_era_index_bare_unsigned_v23_shape() {
        // NtC V_23 (negotiated against modern upstream cardano-cli)
        // emits EraIndex as bare CBOR uint, per the 2026-04-27
        // socat-proxy capture from `cardano-node 10.7.1`.
        assert_eq!(encode_era_index(7), vec![0x07]);
        assert_eq!(encode_era_index(0), vec![0x00]);
        assert_eq!(encode_era_index(23), vec![0x17]); // boundary: still 1 byte
        assert_eq!(encode_era_index(24), vec![0x18, 0x18]); // CBOR uint8 escape
    }

    /// The exact bytes captured from `cardano-node 10.7.1` at NtC V_23 in
    /// response to `BlockQuery (QueryHardFork GetCurrentEra)` while in
    /// Allegra era — `MsgResult [4, 2]` = `82 04 02`.  `encode_era_index(2)`
    /// must match the inner result `02`.
    #[test]
    fn encode_era_index_matches_real_haskell_capture() {
        assert_eq!(encode_era_index(2), vec![0x02]);
    }
}
