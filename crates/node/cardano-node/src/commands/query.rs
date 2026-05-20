//! `query` subcommand: drive the running node's NtC `LocalStateQuery`
//! mini-protocol with a CBOR-encoded query and pretty-print the
//! response.
//!
//! Mirrors upstream `Cardano.CLI.Shelley.Run.Query.runQueryCmd` (and
//! its modern Conway-era successor in `Cardano.CLI.Run.Query`). The
//! Yggdrasil surface is intentionally a subset — only the queries that
//! Yggdrasil's `LocalStateQuery` server-side dispatcher implements are
//! exposed.
//!
//! Wire format reference:
//!   - `Ouroboros.Consensus.Ledger.Query.queryEncodeNodeToClient`
//!   - `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query`
//!   - `Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`
//!
//! Unix-only because the transport is a Unix domain socket.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-cli/blob/master/cardano-cli/src/Cardano/CLI/Shelley/Run/Query.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `query` subcommand
//! dispatcher (LSQ-side: tip, protocol-parameters, utxo,
//! epoch-state, etc.). Mirrors the dispatch half of upstream
//! `Cardano.CLI.Compatible.Run::runQueryCmds`. Yggdrasil's
//! binary covers a subset of the upstream cardano-cli query
//! surface; Phase F (R289-R295) expands the coverage.

#![cfg_attr(not(unix), allow(dead_code))]

use std::path::PathBuf;

use clap::Subcommand;
use eyre::Result;
#[cfg(unix)]
use eyre::WrapErr;
use yggdrasil_cardano_cli::lsq::{self, NtcQuery};

pub(crate) use yggdrasil_cardano_cli::lsq::decode_optional_prefixed_hex;
#[cfg(test)]
pub(crate) use yggdrasil_cardano_cli::lsq::format_utc_time;

/// LocalStateQuery query sub-commands.
#[derive(Subcommand, Debug)]
pub enum QueryCommand {
    /// Query the current era.
    CurrentEra,
    /// Query the chain tip.
    Tip,
    /// Query the chain block number (height) at the current tip.
    ChainBlockNo,
    /// Query the network's system-start time.  Mirrors upstream
    /// `cardano-cli query system-start` and the `GetSystemStart`
    /// hard-fork query (`Ouroboros.Consensus.HardFork.Combinator.Ledger.Query`).
    SystemStart,
    /// Query the era-summary interpreter — a sequence of past+future
    /// hard-fork era boundaries used by clients to convert between
    /// slots, epochs, and wall-clock times across era transitions.
    /// Mirrors upstream `cardano-cli query era-history` and the
    /// `BlockQuery (QueryHardFork GetInterpreter)` query.
    EraHistory,
    /// Query the current epoch number.
    CurrentEpoch,
    /// Query the current protocol parameters.
    ProtocolParams,
    /// Query the UTxO set for a given address (hex-encoded).
    UtxoByAddress {
        /// Hex-encoded address bytes (with or without `0x` prefix).
        #[arg(long)]
        address: String,
    },
    /// Query the stake distribution.
    StakeDistribution,
    /// Query the reward balance for a reward account (hex-encoded).
    RewardBalance {
        /// Hex-encoded reward account bytes (with or without `0x` prefix).
        #[arg(long)]
        account: String,
    },
    /// Query the treasury and reserves.
    TreasuryAndReserves,
    /// Query UTxO entries for specific transaction inputs (hex-encoded CBOR array of TxIn).
    UtxoByTxIn {
        /// Hex-encoded transaction ID, 32 bytes (with or without `0x` prefix).
        #[arg(long)]
        tx_id: String,
        /// Output index within the transaction.
        #[arg(long)]
        index: u16,
    },
    /// Query the set of all registered stake pool IDs.
    StakePools,
    /// Query delegations and reward accounts for a stake credential (hex-encoded 28-byte hash).
    DelegationsAndRewards {
        /// Hex-encoded credential hash, 28 bytes (with or without `0x` prefix).
        #[arg(long)]
        credential: String,
        /// Whether the credential is a key hash (true, default) or script hash (false).
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        is_key_hash: bool,
    },
    /// Query the DRep stake distribution.
    DrepStakeDistr,
    /// Query the enacted Conway constitution (anchor + guardrails script hash).
    Constitution,
    /// Query pending Conway governance-action state
    /// (all currently-submitted proposals + recorded votes).
    GovState,
    /// Query all registered DReps and their registration metadata.
    DrepState,
    /// Query known Conway constitutional committee members
    /// (cold credential, hot-key status, expiration epoch).
    CommitteeMembersState,
    /// Query the registered stake-pool parameters for a specific pool.
    StakePoolParams {
        /// Hex-encoded pool key hash, 28 bytes (with or without `0x` prefix).
        #[arg(long)]
        pool_hash: String,
    },
    /// Query the ledger accounting state
    /// (treasury, reserves, and total deposit obligation).
    AccountState,
    /// Query the Shelley genesis-delegations map
    /// (genesis key hash → (cold delegate, vrf key) pair).
    GenesisDelegations,
    /// Query the derived `3k/f` chain stability window in slots
    /// (null when not configured).
    StabilityWindow,
    /// Query the consecutive dormant-epoch counter
    /// (Conway-only governance bookkeeping; `0` until a dormant epoch fires).
    NumDormantEpochs,
    /// Query the configured expected reward-account network id
    /// (mainnet = 1, test networks = 0; null when not configured).
    ExpectedNetworkId,
    /// Query the four Conway-era deposit-pot buckets
    /// (key / pool / DRep / proposal deposits, in lovelace).
    DepositPot,
    /// Query aggregate cardinality counters for the major ledger-state
    /// buckets (stake credentials / pools / DReps / committee members /
    /// governance actions / genesis delegates). Cheap monitoring query.
    LedgerCounts,
}

impl QueryCommand {
    /// Return the shared `cardano-cli` LSQ query for variants that have
    /// completed migration into `yggdrasil-cardano-cli`.
    fn shared_lsq_query(&self) -> NtcQuery {
        match self {
            QueryCommand::CurrentEra => NtcQuery::CurrentEra,
            QueryCommand::Tip => NtcQuery::Tip,
            QueryCommand::ChainBlockNo => NtcQuery::ChainBlockNo,
            QueryCommand::SystemStart => NtcQuery::SystemStart,
            QueryCommand::EraHistory => NtcQuery::EraHistory,
            QueryCommand::CurrentEpoch => NtcQuery::CurrentEpoch,
            QueryCommand::ProtocolParams => NtcQuery::ProtocolParameters,
            QueryCommand::UtxoByAddress { address } => NtcQuery::UtxoByAddress {
                address: decode_optional_prefixed_hex(address),
            },
            QueryCommand::StakeDistribution => NtcQuery::StakeDistribution,
            QueryCommand::RewardBalance { account } => NtcQuery::RewardBalance {
                account: decode_optional_prefixed_hex(account),
            },
            QueryCommand::TreasuryAndReserves => NtcQuery::TreasuryAndReserves,
            QueryCommand::UtxoByTxIn { tx_id, index } => NtcQuery::UtxoByTxIn {
                tx_id: decode_optional_prefixed_hex(tx_id),
                index: *index,
            },
            QueryCommand::StakePools => NtcQuery::StakePools,
            QueryCommand::DelegationsAndRewards {
                credential,
                is_key_hash,
            } => NtcQuery::DelegationsAndRewards {
                credential: decode_optional_prefixed_hex(credential),
                is_key_hash: *is_key_hash,
            },
            QueryCommand::DrepStakeDistr => NtcQuery::DrepStakeDistribution,
            QueryCommand::Constitution => NtcQuery::Constitution,
            QueryCommand::GovState => NtcQuery::GovState,
            QueryCommand::DrepState => NtcQuery::DrepState,
            QueryCommand::CommitteeMembersState => NtcQuery::CommitteeState,
            QueryCommand::StakePoolParams { pool_hash } => NtcQuery::StakePoolParams {
                pool_hash: decode_optional_prefixed_hex(pool_hash),
            },
            QueryCommand::AccountState => NtcQuery::AccountState,
            QueryCommand::GenesisDelegations => NtcQuery::GenesisDelegations,
            QueryCommand::StabilityWindow => NtcQuery::StabilityWindow,
            QueryCommand::NumDormantEpochs => NtcQuery::NumDormantEpochs,
            QueryCommand::ExpectedNetworkId => NtcQuery::ExpectedNetworkId,
            QueryCommand::DepositPot => NtcQuery::DepositPot,
            QueryCommand::LedgerCounts => NtcQuery::LedgerCounts,
        }
    }
}

/// Connect to the running node's NtC Unix socket and execute a
/// LocalStateQuery request, printing the result as JSON.
///
/// Reference: `cardano-cli query` commands against
/// `ouroboros-network/ouroboros-network/protocols/lib` LocalStateQuery.
pub async fn run_query(
    socket_path: PathBuf,
    network_magic: u32,
    query: QueryCommand,
) -> Result<()> {
    #[cfg(not(unix))]
    {
        let _ = (socket_path, network_magic, query);
        eyre::bail!(
            "query subcommands require a Unix domain node-to-client socket; \
             not supported on this platform"
        )
    }

    #[cfg(unix)]
    {
        use yggdrasil_network::{
            AcquireTarget, LocalStateQueryClient, MiniProtocolNum, ntc_connect,
        };

        let mut conn = ntc_connect(&socket_path, network_magic, true)
            .await
            .wrap_err_with(|| {
                format!("failed to connect to NtC socket {}", socket_path.display())
            })?;

        let sq_handle = conn
            .protocols
            .remove(&MiniProtocolNum::NTC_LOCAL_STATE_QUERY)
            .expect("NTC_LOCAL_STATE_QUERY handle missing");
        let mut client = LocalStateQueryClient::new(sq_handle);

        // Acquire at the volatile tip — always available on a running node.
        client
            .acquire(AcquireTarget::VolatileTip)
            .await
            .wrap_err("LocalStateQuery acquire failed")?;

        // Encode the query as CBOR [tag] or [tag, param].
        let query_bytes = encode_ntc_query(&query);

        let result = client
            .query(query_bytes)
            .await
            .wrap_err("LocalStateQuery query failed")?;

        // Decode the result according to the known response format.
        let json_val = decode_ntc_result(&query, &result)?;
        println!("{}", serde_json::to_string_pretty(&json_val)?);

        let _ = client.release().await;
        let _ = client.done().await;
        Ok(())
    }
}

/// Encode a [`QueryCommand`] as a CBOR `[tag, ...]` byte vector matching
/// the format expected by `BasicLocalQueryDispatcher`.
pub(crate) fn encode_ntc_query(query: &QueryCommand) -> Vec<u8> {
    lsq::encode_query(query.shared_lsq_query())
}
/// Decode a raw CBOR result from the node into a `serde_json::Value` suitable
/// for pretty-printing.
pub(crate) fn decode_ntc_result(query: &QueryCommand, result: &[u8]) -> Result<serde_json::Value> {
    Ok(lsq::decode_query_result(query.shared_lsq_query(), result))
}
