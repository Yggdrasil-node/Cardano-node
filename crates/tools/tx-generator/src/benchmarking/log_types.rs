//! Benchmark submission trace and summary types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/LogTypes.hs`.
//! Ports the data carried by `TraceBenchTxSubmit`,
//! `NodeToNodeSubmissionTrace`, and `SubmissionSummary` for the
//! tx-generator Benchmark submission path.

use serde::Serialize;
use yggdrasil_ledger::TxId;

use crate::benchmarking::types::{Ack, Req, Sent, Unav};
use crate::types::TpsRate;

/// Mirror of upstream `TraceBenchTxSubmit txid`.
#[derive(Clone, Debug, PartialEq)]
pub enum TraceBenchTxSubmit {
    /// Upstream `TraceTxGeneratorVersion`.
    TraceTxGeneratorVersion(String),
    /// Upstream `TraceBenchTxSubRecv`.
    TraceBenchTxSubRecv(Vec<TxId>),
    /// Upstream `TraceBenchTxSubStart`.
    TraceBenchTxSubStart(Vec<TxId>),
    /// Upstream `SubmissionClientReplyTxIds`.
    SubmissionClientReplyTxIds(Vec<TxId>),
    /// Upstream `TraceBenchTxSubServReq`.
    TraceBenchTxSubServReq(Vec<TxId>),
    /// Upstream `SubmissionClientDiscardAcknowledged`.
    SubmissionClientDiscardAcknowledged(Vec<TxId>),
    /// Upstream `TraceBenchTxSubServDrop`.
    TraceBenchTxSubServDrop(Vec<TxId>),
    /// Upstream `SubmissionClientUnAcked`.
    SubmissionClientUnAcked(Vec<TxId>),
    /// Upstream `TraceBenchTxSubServUnav`.
    TraceBenchTxSubServUnav(Vec<TxId>),
    /// Upstream `TraceBenchTxSubServFed`.
    TraceBenchTxSubServFed(Vec<TxId>, usize),
    /// Upstream `TraceBenchTxSubServCons`.
    TraceBenchTxSubServCons(Vec<TxId>),
    /// Upstream `TraceBenchTxSubIdle`.
    TraceBenchTxSubIdle,
    /// Upstream `TraceBenchTxSubRateLimit`.
    TraceBenchTxSubRateLimit(f64),
    /// Upstream `TraceBenchTxSubSummary`.
    TraceBenchTxSubSummary(SubmissionSummary),
    /// Upstream `TraceBenchTxSubDebug`.
    TraceBenchTxSubDebug(String),
    /// Upstream `TraceBenchTxSubError`.
    TraceBenchTxSubError(String),
    /// Upstream `TraceBenchPlutusBudgetSummary`.
    TraceBenchPlutusBudgetSummary(serde_json::Value),
    /// Upstream `TraceBenchForwardingInterrupted`.
    TraceBenchForwardingInterrupted(String, String),
}

/// Mirror of upstream `SubmissionSummary`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SubmissionSummary {
    /// Upstream `ssTxSent`.
    #[serde(rename = "ssTxSent")]
    pub ss_tx_sent: Sent,
    /// Upstream `ssTxUnavailable`.
    #[serde(rename = "ssTxUnavailable")]
    pub ss_tx_unavailable: Unav,
    /// Upstream `ssElapsed`, represented as elapsed seconds.
    #[serde(rename = "ssElapsed")]
    pub ss_elapsed: f64,
    /// Upstream `ssEffectiveTps`.
    #[serde(rename = "ssEffectiveTps")]
    pub ss_effective_tps: TpsRate,
    /// Upstream `ssThreadwiseTps`.
    #[serde(rename = "ssThreadwiseTps")]
    pub ss_threadwise_tps: Vec<TpsRate>,
    /// Upstream `ssFailures`.
    #[serde(rename = "ssFailures")]
    pub ss_failures: Vec<String>,
}

/// Mirror of upstream `NodeToNodeSubmissionTrace`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeToNodeSubmissionTrace {
    /// Upstream `ReqIdsBlocking`.
    ReqIdsBlocking(Ack, Req),
    /// Upstream `IdsListBlocking`.
    IdsListBlocking(usize),
    /// Upstream `ReqIdsNonBlocking`.
    ReqIdsNonBlocking(Ack, Req),
    /// Upstream `IdsListNonBlocking`.
    IdsListNonBlocking(usize),
    /// Upstream `ReqTxs`.
    ReqTxs(usize),
    /// Upstream `TxList`.
    TxList(usize),
    /// Upstream `EndOfProtocol`.
    EndOfProtocol,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_summary_serializes_with_upstream_field_names() {
        let summary = SubmissionSummary {
            ss_tx_sent: Sent(7),
            ss_tx_unavailable: Unav(2),
            ss_elapsed: 3.5,
            ss_effective_tps: 2.0,
            ss_threadwise_tps: vec![1.0, 3.0],
            ss_failures: vec!["peer failed".to_string()],
        };

        let value = match serde_json::to_value(summary) {
            Ok(value) => value,
            Err(err) => panic!("summary should serialize: {err}"),
        };

        assert_eq!(value["ssTxSent"], 7);
        assert_eq!(value["ssTxUnavailable"], 2);
        assert_eq!(value["ssElapsed"], 3.5);
        assert_eq!(value["ssEffectiveTps"], 2.0);
        assert_eq!(value["ssThreadwiseTps"], serde_json::json!([1.0, 3.0]));
        assert_eq!(value["ssFailures"], serde_json::json!(["peer failed"]));
    }
}
