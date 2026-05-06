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

#![cfg(unix)]

use std::path::PathBuf;

use clap::Subcommand;
use eyre::{Result, WrapErr};
use serde_json::json;

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
        #[arg(long, default_value = "true")]
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

/// Connect to the running node's NtC Unix socket and execute a
/// LocalStateQuery request, printing the result as JSON.
///
/// Reference: `cardano-cli query` commands against
/// `ouroboros-network-protocols` LocalStateQuery.
pub async fn run_query(
    socket_path: PathBuf,
    network_magic: u32,
    query: QueryCommand,
) -> Result<()> {
    use yggdrasil_network::{AcquireTarget, LocalStateQueryClient, MiniProtocolNum, ntc_connect};

    let mut conn = ntc_connect(&socket_path, network_magic, true)
        .await
        .wrap_err_with(|| format!("failed to connect to NtC socket {}", socket_path.display()))?;

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

/// Encode a [`QueryCommand`] as a CBOR `[tag, ...]` byte vector matching
/// the format expected by `BasicLocalQueryDispatcher`.
pub(crate) fn encode_ntc_query(query: &QueryCommand) -> Vec<u8> {
    use yggdrasil_ledger::Encoder;
    let mut enc = Encoder::new();
    match query {
        QueryCommand::CurrentEra => {
            // Upstream-shaped: `BlockQuery (QueryHardFork GetCurrentEra)`
            // = `[0, [2, [1]]]` per upstream
            // `Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`.
            // Round 148 unifies yggdrasil's CLI with upstream
            // cardano-cli on the canonical Cardano ABI.
            enc.array(2).unsigned(0u64);
            enc.array(2).unsigned(2u64);
            enc.array(1).unsigned(1u64);
        }
        QueryCommand::Tip => {
            // Upstream-shaped: `GetChainPoint = [3]` per upstream
            // `Ouroboros.Consensus.Ledger.Query.queryEncodeNodeToClient`.
            enc.array(1).unsigned(3u64);
        }
        QueryCommand::ChainBlockNo => {
            // Upstream-shaped: `GetChainBlockNo = [2]` per upstream
            // `Ouroboros.Consensus.Ledger.Query.queryEncodeNodeToClient`.
            enc.array(1).unsigned(2u64);
        }
        QueryCommand::SystemStart => {
            // Upstream-shaped: `GetSystemStart = [1]` per upstream
            // `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query`.
            enc.array(1).unsigned(1u64);
        }
        QueryCommand::EraHistory => {
            // Upstream-shaped: `BlockQuery (QueryHardFork GetInterpreter)`
            // = `[0, [2, [0]]]` per upstream
            // `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query`.
            // Same envelope as `CurrentEra` but with the inner
            // `QueryHardFork` payload tag `0` (GetInterpreter) instead
            // of `1` (GetCurrentEra).
            enc.array(2).unsigned(0u64);
            enc.array(2).unsigned(2u64);
            enc.array(1).unsigned(0u64);
        }
        QueryCommand::CurrentEpoch => {
            // Yggdrasil-extension tag — upstream `[2]` is
            // `GetChainBlockNo`, so yggdrasil's `CurrentEpoch` query
            // moves to `[101]` to avoid collision under Round 148.
            enc.array(1).unsigned(101u64);
        }
        QueryCommand::ProtocolParams => {
            // Yggdrasil-extension tag — upstream `[3]` is
            // `GetChainPoint`, so yggdrasil's `ProtocolParams` query
            // moves to `[102]` to avoid collision under Round 148.
            enc.array(1).unsigned(102u64);
        }
        QueryCommand::UtxoByAddress { address } => {
            let addr_bytes = decode_optional_prefixed_hex(address);
            enc.array(2).unsigned(4u64).bytes(&addr_bytes);
        }
        QueryCommand::StakeDistribution => {
            enc.array(1).unsigned(5u64);
        }
        QueryCommand::RewardBalance { account } => {
            let acct_bytes = decode_optional_prefixed_hex(account);
            enc.array(2).unsigned(6u64).bytes(&acct_bytes);
        }
        QueryCommand::TreasuryAndReserves => {
            enc.array(1).unsigned(7u64);
        }
        QueryCommand::UtxoByTxIn { tx_id, index } => {
            let tx_id_bytes = decode_optional_prefixed_hex(tx_id);
            enc.array(2).unsigned(14u64);
            // Encode as a single-element array of TxIn: [[tx_id_bytes, index]]
            enc.array(1);
            enc.array(2).bytes(&tx_id_bytes).unsigned(*index as u64);
        }
        QueryCommand::StakePools => {
            enc.array(1).unsigned(15u64);
        }
        QueryCommand::DelegationsAndRewards {
            credential,
            is_key_hash,
        } => {
            let cred_bytes = decode_optional_prefixed_hex(credential);
            enc.array(2).unsigned(16u64);
            // Encode as a single-element credential array: [[tag, hash]]
            enc.array(1);
            enc.array(2);
            if *is_key_hash {
                enc.unsigned(0u64);
            } else {
                enc.unsigned(1u64);
            }
            enc.bytes(&cred_bytes);
        }
        QueryCommand::DrepStakeDistr => {
            enc.array(1).unsigned(17u64);
        }
        QueryCommand::Constitution => {
            enc.array(1).unsigned(8u64);
        }
        QueryCommand::GovState => {
            enc.array(1).unsigned(9u64);
        }
        QueryCommand::DrepState => {
            enc.array(1).unsigned(10u64);
        }
        QueryCommand::CommitteeMembersState => {
            enc.array(1).unsigned(11u64);
        }
        QueryCommand::StakePoolParams { pool_hash } => {
            let pool_bytes = decode_optional_prefixed_hex(pool_hash);
            enc.array(2).unsigned(12u64).bytes(&pool_bytes);
        }
        QueryCommand::AccountState => {
            enc.array(1).unsigned(13u64);
        }
        QueryCommand::GenesisDelegations => {
            enc.array(1).unsigned(18u64);
        }
        QueryCommand::StabilityWindow => {
            enc.array(1).unsigned(19u64);
        }
        QueryCommand::NumDormantEpochs => {
            enc.array(1).unsigned(20u64);
        }
        QueryCommand::ExpectedNetworkId => {
            enc.array(1).unsigned(21u64);
        }
        QueryCommand::DepositPot => {
            enc.array(1).unsigned(22u64);
        }
        QueryCommand::LedgerCounts => {
            enc.array(1).unsigned(23u64);
        }
    }
    enc.into_bytes()
}

/// Decode a raw CBOR result from the node into a `serde_json::Value` suitable
/// for pretty-printing.
pub(crate) fn decode_ntc_result(query: &QueryCommand, result: &[u8]) -> Result<serde_json::Value> {
    use yggdrasil_ledger::Decoder;
    let val = match query {
        QueryCommand::CurrentEra => {
            // Round 148 — upstream `EraIndex` shape `[era_index]`.
            let mut dec = Decoder::new(result);
            let era = match dec.array() {
                Ok(1) => dec.unsigned().unwrap_or(0),
                _ => 0,
            };
            json!({"era": era})
        }
        QueryCommand::Tip => {
            // Upstream `Ouroboros.Network.Block.encodePoint`:
            //   Origin     = `encodeListLen 0`           — `[]`
            //   BlockPoint = `encodeListLen 2 <> slot <> hash` — `[slot, hash]`
            //
            // Captured against `cardano-node 10.7.1` socat proxy in the
            // server-side regression test
            // `upstream_get_chain_point_returns_encoded_tip_point`
            // (`local_server.rs`).  No constructor tag; the historical
            // `[1, slot, hash]` shape this decoder used to expect never
            // existed in upstream and silently fell back to a raw-hex
            // dump on every real response.
            let mut dec = Decoder::new(result);
            match dec.array() {
                Ok(0) => json!({"tip": {"origin": true}}),
                Ok(2) => {
                    let slot = dec.unsigned().unwrap_or(0);
                    let hash = dec.bytes().unwrap_or_default();
                    json!({"tip": {"origin": false, "slot": slot, "hash": hex::encode(hash)}})
                }
                _ => json!({"tip_cbor": hex::encode(result)}),
            }
        }
        QueryCommand::ChainBlockNo => {
            // Upstream `Cardano.Slotting.Block.encodeChainBlockNo`:
            //   `Origin = [0]`, `At b = [1, b]`.
            // Mirrors `Ouroboros.Network.Block.Tip.tipBlockNo`.
            let mut dec = Decoder::new(result);
            match dec.array() {
                Ok(1) => json!({"chain_block_no": null}),
                Ok(2) => {
                    let _tag = dec.unsigned().unwrap_or(1);
                    let block_no = dec.unsigned().unwrap_or(0);
                    json!({"chain_block_no": block_no})
                }
                _ => json!({"chain_block_no_cbor": hex::encode(result)}),
            }
        }
        QueryCommand::SystemStart => {
            // Upstream `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query`
            // — `GetSystemStart` reply is a 3-element CBOR array
            // `[year, dayOfYear, picosecondsOfDay]` (UTCTime as
            // `Cardano.Slotting.Time.SystemStart`).  Operators expect
            // an ISO 8601 string; we surface both the raw structured
            // fields (matching `cardano-cli query system-start --output
            // cbor`) and a derived `time` field for human use.
            let mut dec = Decoder::new(result);
            match dec.array() {
                Ok(3) => {
                    let year = dec.unsigned().unwrap_or(0);
                    let day_of_year = dec.unsigned().unwrap_or(1);
                    let picoseconds_of_day = dec.unsigned().unwrap_or(0);
                    json!({
                        "system_start": {
                            "year": year,
                            "dayOfYear": day_of_year,
                            "picosecondsOfDay": picoseconds_of_day,
                            "time": format_utc_time(year, day_of_year, picoseconds_of_day),
                        }
                    })
                }
                _ => json!({"system_start_cbor": hex::encode(result)}),
            }
        }
        QueryCommand::EraHistory => {
            // The interpreter response is a deeply-nested CBOR record
            // (`Ouroboros.Consensus.HardFork.History.Summary.Interpreter`)
            // whose top-level shape is a list of era summaries.
            // Operators consume it as opaque bytes (passed back into
            // ledger libraries for slot↔time conversion); surface it as
            // hex so the result is round-trip-safe and matches what
            // `cardano-cli query era-history --output cbor` emits.
            json!({
                "era_history_cbor": hex::encode(result),
            })
        }
        QueryCommand::CurrentEpoch => {
            let mut dec = Decoder::new(result);
            let epoch = dec.unsigned().unwrap_or(0);
            json!({"epoch": epoch})
        }
        QueryCommand::ProtocolParams => {
            // CBOR null (0xf6) means no parameters available yet.
            if result == [0xf6] {
                json!({"protocol_parameters": null})
            } else {
                json!({"protocol_parameters": hex::encode(result)})
            }
        }
        QueryCommand::UtxoByAddress { .. } => {
            json!({"utxo_cbor": hex::encode(result)})
        }
        QueryCommand::StakeDistribution => {
            json!({"stake_distribution_cbor": hex::encode(result)})
        }
        QueryCommand::RewardBalance { .. } => {
            let mut dec = Decoder::new(result);
            let balance = dec.unsigned().unwrap_or(0);
            json!({"reward_balance_lovelace": balance})
        }
        QueryCommand::TreasuryAndReserves => {
            let mut dec = Decoder::new(result);
            if dec.array().ok() == Some(2) {
                let treasury = dec.unsigned().unwrap_or(0);
                let reserves = dec.unsigned().unwrap_or(0);
                json!({"treasury_lovelace": treasury, "reserves_lovelace": reserves})
            } else {
                json!({"result_cbor": hex::encode(result)})
            }
        }
        QueryCommand::UtxoByTxIn { .. } => {
            json!({"utxo_cbor": hex::encode(result)})
        }
        QueryCommand::StakePools => {
            // Decode CBOR array of pool hashes, convert to hex strings.
            let mut dec = Decoder::new(result);
            let mut pools = Vec::new();
            if let Ok(n) = dec.array() {
                for _ in 0..n {
                    if let Ok(hash_bytes) = dec.bytes() {
                        pools.push(hex::encode(hash_bytes));
                    }
                }
            }
            json!({"stake_pools": pools, "count": pools.len()})
        }
        QueryCommand::DelegationsAndRewards { .. } => {
            json!({"delegations_and_rewards_cbor": hex::encode(result)})
        }
        QueryCommand::DrepStakeDistr => {
            json!({"drep_stake_distribution_cbor": hex::encode(result)})
        }
        QueryCommand::Constitution => {
            // Complex Conway type — surface raw CBOR so clients can decode.
            json!({"constitution_cbor": hex::encode(result)})
        }
        QueryCommand::GovState => {
            json!({"governance_actions_cbor": hex::encode(result)})
        }
        QueryCommand::DrepState => {
            json!({"drep_state_cbor": hex::encode(result)})
        }
        QueryCommand::CommitteeMembersState => {
            json!({"committee_state_cbor": hex::encode(result)})
        }
        QueryCommand::StakePoolParams { .. } => {
            // Server returns CBOR-encoded RegisteredPool or CBOR null when
            // the pool is not registered.
            if result == [0xf6] {
                json!({"pool": null})
            } else {
                json!({"pool_cbor": hex::encode(result)})
            }
        }
        QueryCommand::AccountState => {
            // Server returns `[treasury, reserves, total_deposits]`.
            let mut dec = Decoder::new(result);
            if dec.array().ok() == Some(3) {
                let treasury = dec.unsigned().unwrap_or(0);
                let reserves = dec.unsigned().unwrap_or(0);
                let deposits = dec.unsigned().unwrap_or(0);
                json!({
                    "treasury_lovelace": treasury,
                    "reserves_lovelace": reserves,
                    "total_deposits_lovelace": deposits,
                })
            } else {
                json!({"account_state_cbor": hex::encode(result)})
            }
        }
        QueryCommand::GenesisDelegations => {
            // CBOR map keyed by genesis hash; surface raw for now since
            // the value side carries multiple sub-entries per key.
            json!({"genesis_delegations_cbor": hex::encode(result)})
        }
        QueryCommand::StabilityWindow => {
            // CBOR null (0xf6) when unset; otherwise plain unsigned u64.
            if result == [0xf6] {
                json!({"stability_window": null})
            } else {
                let mut dec = Decoder::new(result);
                let w = dec.unsigned().unwrap_or(0);
                json!({"stability_window_slots": w})
            }
        }
        QueryCommand::NumDormantEpochs => {
            let mut dec = Decoder::new(result);
            let n = dec.unsigned().unwrap_or(0);
            json!({"num_dormant_epochs": n})
        }
        QueryCommand::ExpectedNetworkId => {
            // CBOR null (0xf6) means no expectation is configured;
            // otherwise the server returns a plain CBOR unsigned (u8 range).
            if result == [0xf6] {
                json!({"expected_network_id": null})
            } else {
                let mut dec = Decoder::new(result);
                let id = dec.unsigned().unwrap_or(0);
                json!({"expected_network_id": id})
            }
        }
        QueryCommand::DepositPot => {
            // 4-element CBOR array [key, pool, drep, proposal].
            let mut dec = Decoder::new(result);
            if dec.array().ok() == Some(4) {
                let key_deposits = dec.unsigned().unwrap_or(0);
                let pool_deposits = dec.unsigned().unwrap_or(0);
                let drep_deposits = dec.unsigned().unwrap_or(0);
                let proposal_deposits = dec.unsigned().unwrap_or(0);
                json!({
                    "key_deposits_lovelace": key_deposits,
                    "pool_deposits_lovelace": pool_deposits,
                    "drep_deposits_lovelace": drep_deposits,
                    "proposal_deposits_lovelace": proposal_deposits,
                    "total_lovelace":
                        key_deposits + pool_deposits + drep_deposits + proposal_deposits,
                })
            } else {
                json!({"deposit_pot_cbor": hex::encode(result)})
            }
        }
        QueryCommand::LedgerCounts => {
            // 6-element CBOR array of cardinality counters.
            let mut dec = Decoder::new(result);
            if dec.array().ok() == Some(6) {
                let stake_credentials = dec.unsigned().unwrap_or(0);
                let pools = dec.unsigned().unwrap_or(0);
                let dreps = dec.unsigned().unwrap_or(0);
                let committee_members = dec.unsigned().unwrap_or(0);
                let governance_actions = dec.unsigned().unwrap_or(0);
                let gen_delegs = dec.unsigned().unwrap_or(0);
                json!({
                    "stake_credentials": stake_credentials,
                    "pools": pools,
                    "dreps": dreps,
                    "committee_members": committee_members,
                    "governance_actions": governance_actions,
                    "gen_delegs": gen_delegs,
                })
            } else {
                json!({"ledger_counts_cbor": hex::encode(result)})
            }
        }
    };
    Ok(val)
}

/// Lenient hex decoder used by the query-argument encoders. Accepts the
/// same surface as the `submit-tx` `--tx-hex` decoder (whitespace trim,
/// optional `0x` prefix) but returns an empty `Vec<u8>` on parse failure
/// instead of a typed error, matching the prior call-site behavior where
/// invalid hex produced a query with empty parameter bytes.
///
/// Centralising this behavior in one place means operator-facing
/// `--address` / `--account` / `--tx-id` / `--credential` / `--pool-hash`
/// CLI arguments all accept the same shapes consistently.
pub(crate) fn decode_optional_prefixed_hex(raw: &str) -> Vec<u8> {
    let stripped = raw.trim();
    let stripped = stripped.strip_prefix("0x").unwrap_or(stripped);
    hex::decode(stripped).unwrap_or_default()
}

/// Format a Cardano `SystemStart` triple `(year, dayOfYear, picosecondsOfDay)`
/// as an ISO 8601 UTC timestamp, matching `cardano-cli query system-start`'s
/// human-facing rendering.
///
/// Inputs are taken straight from upstream `Cardano.Slotting.Time.SystemStart`
/// — `year` is a Gregorian year, `dayOfYear` is `[1, 366]`, and
/// `picosecondsOfDay` is `[0, 86_400 * 10^12)`.  The conversion uses the
/// proleptic Gregorian calendar (matching the `time` Haskell library
/// `fromOrdinalDate` semantics) and floors picoseconds → seconds for the
/// timestamp's HH:MM:SS portion.
pub(crate) fn format_utc_time(year: u64, day_of_year: u64, picoseconds_of_day: u64) -> String {
    let is_leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let month_days: [u64; 12] = [
        31,
        if is_leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut remaining = day_of_year.saturating_sub(1);
    let mut month: u32 = 1;
    let mut day_of_month: u32 = 1;
    for (idx, &md) in month_days.iter().enumerate() {
        if remaining < md {
            month = (idx as u32) + 1;
            day_of_month = (remaining as u32) + 1;
            break;
        }
        remaining -= md;
    }
    let total_seconds = picoseconds_of_day / 1_000_000_000_000;
    let hour = (total_seconds / 3600) % 24;
    let minute = (total_seconds / 60) % 60;
    let second = total_seconds % 60;
    format!("{year:04}-{month:02}-{day_of_month:02}T{hour:02}:{minute:02}:{second:02}Z")
}
