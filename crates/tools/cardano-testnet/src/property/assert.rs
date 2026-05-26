//! Pure and injectable helpers from upstream `Testnet.Property.Assert`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Property/Assert.hs.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::time::SystemTime;

use crate::process::run::{self, ExecConfig, ProcessRunError};

const NEWLINE_BYTE: u8 = b'\n';

/// Error returned by pure Property/Assert helpers.
#[derive(Debug, thiserror::Error)]
pub enum PropertyAssertError {
    /// The deadline loop reached its deadline before the predicate succeeded.
    #[error("Condition not met by deadline: {0}")]
    ConditionNotMetByDeadline(String),
    /// Reading a JSON-lines file failed.
    #[error("property Assert IO failed: {0}")]
    Io(#[from] std::io::Error),
    /// The stake-pools query output could not be decoded.
    #[error("Failed to decode stake pools from ledger state: {0}")]
    StakePoolsDecode(String),
    /// The `cardano-cli` stake-pools query failed.
    #[error(transparent)]
    Cli(#[from] ProcessRunError),
    /// The decoded stake-pool set did not match the expected pool count.
    #[error(
        "Expected number of stake pools not found in ledger state\nExpected: \n{expected}\nActual: \n{actual}\n"
    )]
    ExpectedSposInLedgerState {
        /// Expected stake-pool count.
        expected: usize,
        /// Actual decoded stake-pool count.
        actual: usize,
    },
    /// The received era did not match the expected era.
    #[error("Eras mismatch! expected: {expected}, received era: {received}")]
    ErasMismatch {
        /// Expected era display value.
        expected: String,
        /// Received era display value.
        received: String,
    },
}

/// Slots extracted from cardano-node trace JSON lines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelevantSlots {
    /// Slots where the node reports `TraceNodeIsLeader`.
    pub leader_slots: Vec<i64>,
    /// Slots where the node reports `TraceNodeNotLeader`.
    pub not_leader_slots: Vec<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TraceNode {
    is_leader: bool,
    slot: i64,
}

/// Mirror upstream `readJsonLines`, decoding only valid JSON lines.
pub fn read_json_lines(path: impl AsRef<Path>) -> Result<Vec<Value>, PropertyAssertError> {
    Ok(read_json_lines_from_slice(&std::fs::read(path)?))
}

/// Decode the JSON lines accepted by upstream `readJsonLines`.
pub fn read_json_lines_from_slice(bytes: &[u8]) -> Vec<Value> {
    bytes
        .split(|byte| *byte == NEWLINE_BYTE)
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .collect()
}

/// Mirror upstream `assertByDeadlineIOCustom` with an injectable predicate.
pub fn assert_by_deadline_custom<F>(
    description: &str,
    deadline: SystemTime,
    mut condition: F,
) -> Result<(), PropertyAssertError>
where
    F: FnMut() -> Result<bool, PropertyAssertError>,
{
    loop {
        if condition()? {
            return Ok(());
        }
        if SystemTime::now() >= deadline {
            return Err(PropertyAssertError::ConditionNotMetByDeadline(
                description.to_string(),
            ));
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// Read a node stdout JSON-lines file and extract relevant leader slots.
pub fn get_relevant_slots(
    pool_node_stdout_file: impl AsRef<Path>,
    slot_lower_bound: i64,
) -> Result<RelevantSlots, PropertyAssertError> {
    let values = read_json_lines(pool_node_stdout_file)?;
    Ok(get_relevant_slots_from_values(&values, slot_lower_bound))
}

/// Extract the leader and non-leader slots from decoded node trace values.
pub fn get_relevant_slots_from_values(values: &[Value], slot_lower_bound: i64) -> RelevantSlots {
    let slots = values
        .iter()
        .filter_map(trace_node_from_log_entry)
        .collect::<Vec<_>>();

    RelevantSlots {
        leader_slots: slots
            .iter()
            .filter(|node| node.is_leader && node.slot >= slot_lower_bound)
            .map(|node| node.slot)
            .collect(),
        not_leader_slots: slots
            .iter()
            .filter(|node| !node.is_leader && node.slot >= slot_lower_bound)
            .map(|node| node.slot)
            .collect(),
    }
}

fn trace_node_from_log_entry(value: &Value) -> Option<TraceNode> {
    let val = value.get("data")?.get("val")?;
    let kind = val.get("kind")?.as_str()?;
    let slot = val.get("slot")?.as_i64()?;
    match kind {
        "TraceNodeIsLeader" => Some(TraceNode {
            is_leader: true,
            slot,
        }),
        "TraceNodeNotLeader" => Some(TraceNode {
            is_leader: false,
            slot,
        }),
        _ => None,
    }
}

/// Pure assertion core for upstream `assertExpectedSposInLedgerState`.
pub fn assert_expected_spos_in_ledger_state_value(
    stake_pools: &Value,
    expected_pools: usize,
) -> Result<(), PropertyAssertError> {
    let actual = stake_pool_count(stake_pools)?;
    if actual == expected_pools {
        Ok(())
    } else {
        Err(PropertyAssertError::ExpectedSposInLedgerState {
            expected: expected_pools,
            actual,
        })
    }
}

/// Build the upstream `cardano-cli latest query stake-pools` arguments.
pub fn stake_pools_query_args(output: impl AsRef<Path>) -> Vec<String> {
    vec![
        "latest".to_string(),
        "query".to_string(),
        "stake-pools".to_string(),
        "--out-file".to_string(),
        output.as_ref().to_string_lossy().into_owned(),
    ]
}

/// Mirror upstream `assertExpectedSposInLedgerState` with injectable CLI execution.
pub fn assert_expected_spos_in_ledger_state_with_executor<F>(
    output: impl AsRef<Path>,
    expected_pools: usize,
    exec_config: &ExecConfig,
    mut exec_cli: F,
) -> Result<(), PropertyAssertError>
where
    F: FnMut(&ExecConfig, &[String]) -> Result<String, ProcessRunError>,
{
    let output = output.as_ref();
    let args = stake_pools_query_args(output);
    exec_cli(exec_config, &args)?;

    let stake_pools = serde_json::from_slice::<Value>(&std::fs::read(output)?)
        .map_err(|err| PropertyAssertError::StakePoolsDecode(err.to_string()))?;
    assert_expected_spos_in_ledger_state_value(&stake_pools, expected_pools)
}

/// Mirror upstream `assertExpectedSposInLedgerState` using `cardano-cli`.
pub fn assert_expected_spos_in_ledger_state(
    output: impl AsRef<Path>,
    expected_pools: usize,
    exec_config: &ExecConfig,
) -> Result<(), PropertyAssertError> {
    assert_expected_spos_in_ledger_state_with_executor(
        output,
        expected_pools,
        exec_config,
        |config, args| run::exec_cli(config, args),
    )
}

fn stake_pool_count(value: &Value) -> Result<usize, PropertyAssertError> {
    let pools = value.as_array().ok_or_else(|| {
        PropertyAssertError::StakePoolsDecode("expected JSON array of pool id strings".to_string())
    })?;
    let mut unique = BTreeSet::new();
    for pool in pools {
        let pool_id = pool.as_str().ok_or_else(|| {
            PropertyAssertError::StakePoolsDecode(
                "expected JSON array of pool id strings".to_string(),
            )
        })?;
        unique.insert(pool_id);
    }
    Ok(unique.len())
}

/// Mirror upstream `assertErasEqual` for Rust display/equality values.
pub fn assert_eras_equal<T>(expected_era: T, received_era: T) -> Result<(), PropertyAssertError>
where
    T: Eq + fmt::Display,
{
    if expected_era == received_era {
        Ok(())
    } else {
        Err(PropertyAssertError::ErasMismatch {
            expected: expected_era.to_string(),
            received: received_era.to_string(),
        })
    }
}
