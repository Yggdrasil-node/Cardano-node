//! Benchmark submission reports and throttled transaction source.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx/Submission.hs`.
//! Ports `SubmissionParams`, `ReportRef`, `SubmissionThreadReport`,
//! `mkSubmissionSummary`, and `txStreamSource` for the Benchmark
//! submission path.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use crate::benchmarking::log_types::SubmissionSummary;
use crate::benchmarking::tps_throttle::{
    Step, TpsThrottle, consume_txs_blocking, consume_txs_non_blocking,
};
use crate::benchmarking::types::{Req, Sent, Unav};
use crate::generator_tx::submission_client::{BlockingStyle, SubmissionThreadStats};
use crate::tx_generator::tx::GeneratedTx;
use crate::types::{TpsRate, TxGenError};

pub use crate::generator_tx::submission_client::TxSource;

/// Mirror of upstream `SubmissionParams`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubmissionParams {
    /// Upstream `spTps`.
    pub sp_tps: TpsRate,
    /// Upstream `spTargets`.
    pub sp_targets: usize,
}

/// Mirror of upstream `SubmissionThreadReport`.
#[derive(Clone, Debug, PartialEq)]
pub struct SubmissionThreadReport {
    /// Upstream `strStats`.
    pub str_stats: SubmissionThreadStats,
    /// Upstream `strEndOfProtocol`.
    pub str_end_of_protocol: SystemTime,
}

type ReportSlot = Option<Result<SubmissionThreadReport, String>>;
type ReportShared = Arc<(Mutex<ReportSlot>, Condvar)>;

/// Rust analogue of upstream `ReportRef = TMVar (Either String SubmissionThreadReport)`.
#[derive(Clone, Debug)]
pub struct ReportRef {
    shared: ReportShared,
}

impl Default for ReportRef {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportRef {
    /// Construct an empty report reference.
    pub fn new() -> Self {
        Self {
            shared: Arc::new((Mutex::new(None), Condvar::new())),
        }
    }

    /// Blocking read matching upstream `readTMVar`.
    pub fn read(&self) -> Result<SubmissionThreadReport, String> {
        let (lock, cv) = &*self.shared;
        let mut slot = lock_report_slot(lock);
        while slot.is_none() {
            slot = wait_report_slot(cv, slot);
        }
        match slot.as_ref() {
            Some(report) => report.clone(),
            None => unreachable!("report slot was checked above"),
        }
    }
}

/// Mirror of upstream `submitThreadReport`.
pub fn submit_thread_report(
    report_ref: &ReportRef,
    report: Result<SubmissionThreadReport, String>,
) {
    let (lock, cv) = &*report_ref.shared;
    let mut slot = lock_report_slot(lock);
    while slot.is_some() {
        slot = wait_report_slot(cv, slot);
    }
    *slot = Some(report);
    cv.notify_all();
}

/// Mirror of upstream `submitSubmissionThreadStats`.
pub fn submit_submission_thread_stats(report_ref: &ReportRef, str_stats: SubmissionThreadStats) {
    let report = SubmissionThreadReport {
        str_stats,
        str_end_of_protocol: SystemTime::now(),
    };
    submit_thread_report(report_ref, Ok(report));
}

/// Mirror of upstream `mkSubmissionSummary`.
pub fn mk_submission_summary(
    start_time: SystemTime,
    report_refs: &[ReportRef],
) -> SubmissionSummary {
    mk_submission_summary_at(start_time, report_refs, SystemTime::now())
}

fn mk_submission_summary_at(
    start_time: SystemTime,
    report_refs: &[ReportRef],
    now: SystemTime,
) -> SubmissionSummary {
    let results = report_refs
        .iter()
        .map(ReportRef::read)
        .collect::<Vec<Result<SubmissionThreadReport, String>>>();
    let mut failures = Vec::new();
    let mut reports = Vec::new();
    for result in results {
        match result {
            Ok(report) => reports.push(report),
            Err(failure) => failures.push(failure),
        }
    }

    let elapsed = duration_from(start_time, now);
    let mut sent = Sent(0);
    let mut unavailable = Unav(0);
    for report in &reports {
        sent += report.str_stats.sts_sent;
        unavailable += report.str_stats.sts_unavailable;
    }
    let threadwise_tps = reports
        .iter()
        .map(|report| {
            tx_diff_time_tps(
                report.str_stats.sts_acked.get(),
                duration_from(start_time, report.str_end_of_protocol),
            )
        })
        .collect::<Vec<_>>();

    SubmissionSummary {
        ss_tx_sent: sent,
        ss_tx_unavailable: unavailable,
        ss_elapsed: elapsed.as_secs_f64(),
        ss_effective_tps: tx_diff_time_tps(sent.get(), elapsed),
        ss_threadwise_tps: threadwise_tps,
        ss_failures: failures,
    }
}

fn tx_diff_time_tps(tx_count: usize, elapsed: Duration) -> TpsRate {
    tx_count as TpsRate / elapsed.as_secs_f64()
}

fn duration_from(start: SystemTime, end: SystemTime) -> Duration {
    end.duration_since(start).unwrap_or(Duration::ZERO)
}

fn lock_report_slot(lock: &Mutex<ReportSlot>) -> MutexGuard<'_, ReportSlot> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_report_slot<'a>(
    cv: &Condvar,
    guard: MutexGuard<'a, ReportSlot>,
) -> MutexGuard<'a, ReportSlot> {
    match cv.wait(guard) {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Mirror of upstream `StreamState`.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamState<T> {
    /// Upstream `StreamEmpty`.
    StreamEmpty,
    /// Upstream `StreamError`.
    StreamError(TxGenError),
    /// Upstream `StreamActive`.
    StreamActive(T),
}

/// Rust analogue of upstream `MVar (StreamState (TxStream IO era))`.
#[derive(Clone, Debug)]
pub struct SharedTxStream {
    state: Arc<Mutex<StreamState<VecDeque<GeneratedTx>>>>,
}

impl SharedTxStream {
    /// Construct an active finite stream.
    pub fn new(txs: impl IntoIterator<Item = GeneratedTx>) -> Self {
        Self::from_state(StreamState::StreamActive(txs.into_iter().collect()))
    }

    /// Construct a stream reference from an explicit state.
    pub fn from_state(state: StreamState<VecDeque<GeneratedTx>>) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    fn un_fold(&self, count: usize) -> Result<Vec<GeneratedTx>, TxGenError> {
        let mut txs = Vec::new();
        for _ in 0..count {
            match self.next_on_mvar()? {
                StreamState::StreamActive(tx) => txs.push(tx),
                StreamState::StreamEmpty => break,
                StreamState::StreamError(err) => return Err(err),
            }
        }
        Ok(txs)
    }

    fn next_on_mvar(&self) -> Result<StreamState<GeneratedTx>, TxGenError> {
        let mut state = match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match &mut *state {
            StreamState::StreamEmpty => Ok(StreamState::StreamEmpty),
            StreamState::StreamError(err) => Ok(StreamState::StreamError(err.clone())),
            StreamState::StreamActive(stream) => match stream.pop_front() {
                Some(tx) => {
                    if stream.is_empty() {
                        *state = StreamState::StreamEmpty;
                    }
                    Ok(StreamState::StreamActive(tx))
                }
                None => {
                    *state = StreamState::StreamEmpty;
                    Ok(StreamState::StreamEmpty)
                }
            },
        }
    }
}

/// Transaction source returned by upstream `txStreamSource`.
#[derive(Clone, Debug)]
pub struct ThrottledTxSource {
    stream_ref: SharedTxStream,
    tps_throttle: TpsThrottle,
    exhausted: bool,
}

/// Mirror of upstream `txStreamSource`.
pub fn tx_stream_source(
    stream_ref: SharedTxStream,
    tps_throttle: TpsThrottle,
) -> ThrottledTxSource {
    ThrottledTxSource {
        stream_ref,
        tps_throttle,
        exhausted: false,
    }
}

impl TxSource for ThrottledTxSource {
    fn produce_next_txs(
        &mut self,
        blocking: BlockingStyle,
        req: Req,
    ) -> Result<Vec<GeneratedTx>, TxGenError> {
        if self.exhausted {
            return Ok(Vec::new());
        }

        let (step, tx_count) = match blocking {
            BlockingStyle::Blocking => consume_txs_blocking(&self.tps_throttle, req),
            BlockingStyle::NonBlocking => consume_txs_non_blocking(&self.tps_throttle, req),
        };
        let txs = self.stream_ref.un_fold(tx_count)?;
        if matches!(step, Step::Stop) {
            self.exhausted = true;
        }
        Ok(txs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_ledger::{
        AllegraTxBody, MultiEraSubmittedTx, ShelleyCompatibleSubmittedTx, ShelleyTxIn,
        ShelleyWitnessSet,
    };

    fn tx(byte: u8) -> GeneratedTx {
        let body = AllegraTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [byte; 32],
                index: u16::from(byte),
            }],
            outputs: Vec::new(),
            fee: u64::from(byte),
            ttl: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
        };
        let witness_set = ShelleyWitnessSet {
            vkey_witnesses: Vec::new(),
            native_scripts: Vec::new(),
            bootstrap_witnesses: Vec::new(),
            plutus_v1_scripts: Vec::new(),
            plutus_data: Vec::new(),
            redeemers: Vec::new(),
            plutus_v2_scripts: Vec::new(),
            plutus_v3_scripts: Vec::new(),
        };
        GeneratedTx::new(MultiEraSubmittedTx::Allegra(
            ShelleyCompatibleSubmittedTx::new(body, witness_set, None),
        ))
    }

    #[test]
    fn submission_summary_aggregates_reports_like_upstream() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        let report_a = ReportRef::new();
        let report_b = ReportRef::new();
        let failure = ReportRef::new();

        submit_thread_report(
            &report_a,
            Ok(SubmissionThreadReport {
                str_stats: SubmissionThreadStats {
                    sts_acked: crate::benchmarking::types::Ack(4),
                    sts_sent: Sent(5),
                    sts_unavailable: Unav(1),
                },
                str_end_of_protocol: start + Duration::from_secs(2),
            }),
        );
        submit_thread_report(
            &report_b,
            Ok(SubmissionThreadReport {
                str_stats: SubmissionThreadStats {
                    sts_acked: crate::benchmarking::types::Ack(6),
                    sts_sent: Sent(7),
                    sts_unavailable: Unav(3),
                },
                str_end_of_protocol: start + Duration::from_secs(4),
            }),
        );
        submit_thread_report(&failure, Err("peer failed".to_string()));

        let summary = mk_submission_summary_at(
            start,
            &[report_a, report_b, failure],
            start + Duration::from_secs(6),
        );

        assert_eq!(summary.ss_tx_sent, Sent(12));
        assert_eq!(summary.ss_tx_unavailable, Unav(4));
        assert_eq!(summary.ss_elapsed, 6.0);
        assert_eq!(summary.ss_effective_tps, 2.0);
        assert_eq!(summary.ss_threadwise_tps, vec![2.0, 1.5]);
        assert_eq!(summary.ss_failures, vec!["peer failed"]);
    }

    #[test]
    fn tx_stream_source_respects_nonblocking_throttle_empty() {
        let mut source =
            tx_stream_source(SharedTxStream::new([tx(1)]), TpsThrottle::new(32, 0, 1.0));

        let txs = source
            .produce_next_txs(BlockingStyle::NonBlocking, Req(4))
            .expect("txs");

        assert!(txs.is_empty());
    }

    #[test]
    fn tx_stream_source_unfolds_allowed_transactions() {
        let throttle = TpsThrottle::new(32, 2, 100_000.0);
        throttle.start_sending();
        let tx1 = tx(1);
        let tx2 = tx(2);
        let mut source =
            tx_stream_source(SharedTxStream::new([tx1.clone(), tx2.clone()]), throttle);

        let txs = source
            .produce_next_txs(BlockingStyle::Blocking, Req(2))
            .expect("txs");

        assert_eq!(txs, vec![tx1, tx2]);
    }

    #[test]
    fn tx_stream_source_propagates_stream_errors_as_typed_errors() {
        let throttle = TpsThrottle::new(32, 1, 100_000.0);
        throttle.start_sending();
        let mut source = tx_stream_source(
            SharedTxStream::from_state(StreamState::StreamError(TxGenError::ApiError(
                "stream failed".to_string(),
            ))),
            throttle,
        );

        let err = source
            .produce_next_txs(BlockingStyle::Blocking, Req(1))
            .expect_err("stream error");

        assert_eq!(err, TxGenError::ApiError("stream failed".to_string()));
    }
}
