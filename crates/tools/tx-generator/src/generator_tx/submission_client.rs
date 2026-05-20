//! Transaction-submission client state machine for Benchmark mode.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx/SubmissionClient.hs`.
//! Ports the pure request/response bookkeeping from upstream
//! `txSubmissionClient`: acknowledged-window handling, tx-id
//! announcement, requested-tx lookup, and per-thread submission stats.

use std::collections::{BTreeSet, VecDeque};

use thiserror::Error;
use yggdrasil_ledger::{MultiEraSubmittedTx, TxId};
use yggdrasil_network::{
    TxIdAndSize, TxServerRequest, TxSubmissionClient as NetworkTxSubmissionClient,
};

use crate::benchmarking::log_types::{NodeToNodeSubmissionTrace, TraceBenchTxSubmit};
use crate::benchmarking::types::{Ack, Req, Sent, ToAnnce, UnAcked, Unav};
use crate::tx_generator::tx::{GeneratedTx, tx_size_in_bytes};
use crate::types::TxGenError;

/// Mirror of upstream `SubmissionThreadStats`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubmissionThreadStats {
    /// Upstream `stsAcked`.
    pub sts_acked: Ack,
    /// Upstream `stsSent`.
    pub sts_sent: Sent,
    /// Upstream `stsUnavailable`.
    pub sts_unavailable: Unav,
}

/// Runtime counterpart of upstream `SingBlockingStyle`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockingStyle {
    /// Upstream `SingBlocking`.
    Blocking,
    /// Upstream `SingNonBlocking`.
    NonBlocking,
}

impl BlockingStyle {
    fn is_blocking(self) -> bool {
        matches!(self, Self::Blocking)
    }
}

/// Source of transactions for upstream `TxSource`.
pub trait TxSource {
    /// Produce up to `req` transactions under the current blocking style.
    fn produce_next_txs(
        &mut self,
        blocking: BlockingStyle,
        req: Req,
    ) -> Result<Vec<GeneratedTx>, TxGenError>;
}

/// FIFO-backed transaction source used by finite generated streams.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VecTxSource {
    txs: VecDeque<GeneratedTx>,
}

impl VecTxSource {
    /// Build a source from a finite stream of generated transactions.
    pub fn new(txs: impl IntoIterator<Item = GeneratedTx>) -> Self {
        Self {
            txs: txs.into_iter().collect(),
        }
    }

    /// Return true when all transactions have been consumed.
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }
}

impl TxSource for VecTxSource {
    fn produce_next_txs(
        &mut self,
        _blocking: BlockingStyle,
        req: Req,
    ) -> Result<Vec<GeneratedTx>, TxGenError> {
        let count = req.get().min(self.txs.len());
        Ok(self.txs.drain(..count).collect())
    }
}

/// Local state carried by upstream `txSubmissionClient`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionClientState<S> {
    tx_source: S,
    unacked: UnAcked<GeneratedTx>,
    stats: SubmissionThreadStats,
}

impl<S> SubmissionClientState<S> {
    /// Construct the client state with no outstanding transactions.
    pub fn new(tx_source: S) -> Self {
        Self {
            tx_source,
            unacked: UnAcked(Vec::new()),
            stats: SubmissionThreadStats::default(),
        }
    }

    /// Return the accumulated thread stats.
    pub fn stats(&self) -> SubmissionThreadStats {
        self.stats
    }

    /// Return the current unacknowledged transaction ids in upstream list order.
    pub fn unacked_tx_ids(&self) -> Vec<TxId> {
        tx_ids(&self.unacked.0)
    }
}

impl<S: TxSource> SubmissionClientState<S> {
    /// Mirror of upstream `requestTxIds`.
    pub fn request_tx_ids(
        &mut self,
        blocking: BlockingStyle,
        ack: Ack,
        req: Req,
    ) -> Result<TxIdsStep, SubmissionClientError> {
        let mut node_to_node_traces = vec![req_ids_trace(ack, req, blocking)];
        let mut bench_traces = Vec::new();

        self.discard_acknowledged(blocking, ack, &mut bench_traces)?;
        let new_txs = self
            .tx_source
            .produce_next_txs(blocking, req)
            .map_err(SubmissionClientError::TxSource)?;
        self.queue_new_txs(new_txs.clone());
        let outstanding_ids = tx_ids(&self.unacked.0);

        node_to_node_traces.push(id_list_trace(ToAnnce(new_txs.clone()), blocking));
        bench_traces.push(TraceBenchTxSubmit::SubmissionClientReplyTxIds(tx_ids(
            &new_txs,
        )));
        bench_traces.push(TraceBenchTxSubmit::SubmissionClientUnAcked(outstanding_ids));

        let reply = if blocking.is_blocking() && new_txs.is_empty() {
            node_to_node_traces.push(NodeToNodeSubmissionTrace::EndOfProtocol);
            TxIdsReply::Done(self.stats)
        } else {
            TxIdsReply::Reply(tx_to_id_sizes(&new_txs)?)
        };

        Ok(TxIdsStep {
            reply,
            node_to_node_traces,
            bench_traces,
        })
    }

    /// Mirror of upstream `requestTxs`.
    pub fn request_txs(&mut self, requested: &[TxId]) -> TxsStep {
        let mut node_to_node_traces = vec![NodeToNodeSubmissionTrace::ReqTxs(requested.len())];
        let mut bench_traces = Vec::new();
        let requested_set = requested.iter().copied().collect::<BTreeSet<_>>();
        let unacked_ids = tx_ids(&self.unacked.0);
        let to_send = self
            .unacked
            .0
            .iter()
            .filter(|tx| requested_set.contains(&tx.tx_id))
            .cloned()
            .collect::<Vec<_>>();
        let missing = list_difference(requested, &unacked_ids);

        node_to_node_traces.push(NodeToNodeSubmissionTrace::TxList(to_send.len()));
        bench_traces.push(TraceBenchTxSubmit::SubmissionClientUnAcked(unacked_ids));
        bench_traces.push(TraceBenchTxSubmit::TraceBenchTxSubServReq(
            requested.to_vec(),
        ));
        if !missing.is_empty() {
            bench_traces.push(TraceBenchTxSubmit::TraceBenchTxSubServUnav(missing.clone()));
        }

        self.stats.sts_sent += Sent(to_send.len());
        self.stats.sts_unavailable += Unav(missing.len());

        TxsStep {
            txs: to_send.into_iter().map(|tx| tx.tx).collect(),
            unavailable: missing,
            node_to_node_traces,
            bench_traces,
        }
    }

    fn discard_acknowledged(
        &mut self,
        blocking: BlockingStyle,
        ack: Ack,
        bench_traces: &mut Vec<TraceBenchTxSubmit>,
    ) -> Result<(), SubmissionClientError> {
        let ack_count = ack.get();
        let unacked_len = self.unacked.0.len();
        if blocking.is_blocking() && ack_count != unacked_len {
            let message = "decideAnnouncement: SingBlocking, but length unAcked != ack";
            bench_traces.push(TraceBenchTxSubmit::TraceBenchTxSubError(
                message.to_string(),
            ));
            return Err(SubmissionClientError::BlockingAckMismatch {
                ack: ack_count,
                unacked: unacked_len,
            });
        }

        let split_at = unacked_len.saturating_sub(ack_count);
        let acknowledged = if ack_count >= unacked_len {
            std::mem::take(&mut self.unacked.0)
        } else {
            self.unacked.0.split_off(split_at)
        };

        self.stats.sts_acked += ack;
        bench_traces.push(TraceBenchTxSubmit::SubmissionClientDiscardAcknowledged(
            tx_ids(&acknowledged),
        ));
        Ok(())
    }

    fn queue_new_txs(&mut self, new_txs: Vec<GeneratedTx>) {
        if new_txs.is_empty() {
            return;
        }
        let mut next = new_txs;
        next.append(&mut self.unacked.0);
        self.unacked = UnAcked(next);
    }
}

/// Mirror constructor for upstream `txSubmissionClient`'s local state.
pub fn tx_submission_client<S: TxSource>(initial_tx_source: S) -> SubmissionClientState<S> {
    SubmissionClientState::new(initial_tx_source)
}

/// Reply decision for a tx-id request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxIdsReply {
    /// Upstream `SendMsgDone`.
    Done(SubmissionThreadStats),
    /// Upstream `SendMsgReplyTxIds`.
    Reply(Vec<TxIdAndSize>),
}

/// Observable result of upstream `requestTxIds`.
#[derive(Clone, Debug, PartialEq)]
pub struct TxIdsStep {
    /// Reply to send to the peer.
    pub reply: TxIdsReply,
    /// `NodeToNodeSubmissionTrace` values emitted by the step.
    pub node_to_node_traces: Vec<NodeToNodeSubmissionTrace>,
    /// `TraceBenchTxSubmit` values emitted by the step.
    pub bench_traces: Vec<TraceBenchTxSubmit>,
}

/// Observable result of upstream `requestTxs`.
#[derive(Clone, Debug, PartialEq)]
pub struct TxsStep {
    /// Transactions available for the peer request.
    pub txs: Vec<MultiEraSubmittedTx>,
    /// Requested transaction ids that were not in the unacknowledged set.
    pub unavailable: Vec<TxId>,
    /// `NodeToNodeSubmissionTrace` values emitted by the step.
    pub node_to_node_traces: Vec<NodeToNodeSubmissionTrace>,
    /// `TraceBenchTxSubmit` values emitted by the step.
    pub bench_traces: Vec<TraceBenchTxSubmit>,
}

/// Errors from the local submission-client state machine.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SubmissionClientError {
    /// Upstream blocking request invariant failed.
    #[error(
        "decideAnnouncement: SingBlocking, but length unAcked != ack (ack={ack}, unacked={unacked})"
    )]
    BlockingAckMismatch { ack: usize, unacked: usize },
    /// Transaction source returned an error.
    #[error("{0}")]
    TxSource(TxGenError),
    /// Transaction size does not fit the wire `SizeInBytes` representation.
    #[error("txSubmissionClient: transaction size {size} exceeds u32")]
    TxSizeOverflow { size: usize },
    /// Typed network TxSubmission2 driver failed.
    #[error("txSubmissionClient network driver: {0}")]
    Network(String),
}

/// Drive the upstream-shaped local submission state through the typed
/// node-to-node TxSubmission2 wire client.
///
/// This is the Rust equivalent of handing upstream `txSubmissionClient`
/// to the node-to-node mini-protocol runner: the network driver owns the
/// CBOR/mux protocol transitions, while [`SubmissionClientState`] owns
/// the Benchmark-specific request bookkeeping and stats.
pub async fn run_tx_submission_client<S: TxSource>(
    wire_client: &mut NetworkTxSubmissionClient,
    local_state: &mut SubmissionClientState<S>,
) -> Result<SubmissionThreadStats, SubmissionClientError> {
    wire_client.init().await.map_err(network_error)?;

    loop {
        match wire_client.recv_request().await.map_err(network_error)? {
            TxServerRequest::RequestTxIds { blocking, ack, req } => {
                let blocking = if blocking {
                    BlockingStyle::Blocking
                } else {
                    BlockingStyle::NonBlocking
                };
                let step = local_state.request_tx_ids(
                    blocking,
                    Ack(usize::from(ack)),
                    Req(usize::from(req)),
                )?;
                match step.reply {
                    TxIdsReply::Done(stats) => {
                        wire_client.send_done().await.map_err(network_error)?;
                        return Ok(stats);
                    }
                    TxIdsReply::Reply(txids) => {
                        wire_client
                            .reply_tx_ids(txids)
                            .await
                            .map_err(network_error)?;
                    }
                }
            }
            TxServerRequest::RequestTxs { txids } => {
                let step = local_state.request_txs(&txids);
                wire_client
                    .reply_txs_multi_era(step.txs)
                    .await
                    .map_err(network_error)?;
            }
        }
    }
}

fn network_error(error: yggdrasil_network::TxSubmissionClientError) -> SubmissionClientError {
    SubmissionClientError::Network(error.to_string())
}

fn tx_to_id_sizes(txs: &[GeneratedTx]) -> Result<Vec<TxIdAndSize>, SubmissionClientError> {
    txs.iter()
        .map(|tx| {
            let size = tx_size_in_bytes(tx);
            let size =
                u32::try_from(size).map_err(|_| SubmissionClientError::TxSizeOverflow { size })?;
            Ok(TxIdAndSize {
                txid: tx.tx_id,
                size,
            })
        })
        .collect()
}

fn tx_ids(txs: &[GeneratedTx]) -> Vec<TxId> {
    txs.iter().map(|tx| tx.tx_id).collect()
}

fn list_difference(requested: &[TxId], available: &[TxId]) -> Vec<TxId> {
    let mut available = available.to_vec();
    let mut missing = Vec::new();
    for tx_id in requested {
        match available
            .iter()
            .position(|available_id| available_id == tx_id)
        {
            Some(index) => {
                available.remove(index);
            }
            None => missing.push(*tx_id),
        }
    }
    missing
}

fn req_ids_trace(ack: Ack, req: Req, blocking: BlockingStyle) -> NodeToNodeSubmissionTrace {
    match blocking {
        BlockingStyle::Blocking => NodeToNodeSubmissionTrace::ReqIdsBlocking(ack, req),
        BlockingStyle::NonBlocking => NodeToNodeSubmissionTrace::ReqIdsNonBlocking(ack, req),
    }
}

fn id_list_trace(
    to_announce: ToAnnce<GeneratedTx>,
    blocking: BlockingStyle,
) -> NodeToNodeSubmissionTrace {
    match blocking {
        BlockingStyle::Blocking => NodeToNodeSubmissionTrace::IdsListBlocking(to_announce.0.len()),
        BlockingStyle::NonBlocking => {
            NodeToNodeSubmissionTrace::IdsListNonBlocking(to_announce.0.len())
        }
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
    fn request_tx_ids_queues_new_txs_and_reports_ids() {
        let tx1 = tx(1);
        let tx2 = tx(2);
        let mut client = tx_submission_client(VecTxSource::new([tx1.clone(), tx2.clone()]));

        let step = client
            .request_tx_ids(BlockingStyle::NonBlocking, Ack(0), Req(2))
            .expect("tx ids");

        assert_eq!(
            step.node_to_node_traces,
            vec![
                NodeToNodeSubmissionTrace::ReqIdsNonBlocking(Ack(0), Req(2)),
                NodeToNodeSubmissionTrace::IdsListNonBlocking(2),
            ]
        );
        assert_eq!(
            step.reply,
            TxIdsReply::Reply(vec![
                TxIdAndSize {
                    txid: tx1.tx_id,
                    size: tx_size_in_bytes(&tx1) as u32,
                },
                TxIdAndSize {
                    txid: tx2.tx_id,
                    size: tx_size_in_bytes(&tx2) as u32,
                },
            ])
        );
        assert_eq!(client.unacked_tx_ids(), vec![tx1.tx_id, tx2.tx_id]);
    }

    #[test]
    fn request_txs_sends_matching_unacked_and_counts_missing_ids() {
        let tx1 = tx(1);
        let tx2 = tx(2);
        let missing = TxId([9; 32]);
        let mut client = tx_submission_client(VecTxSource::new([tx1.clone(), tx2.clone()]));
        client
            .request_tx_ids(BlockingStyle::NonBlocking, Ack(0), Req(2))
            .expect("tx ids");

        let step = client.request_txs(&[tx2.tx_id, missing]);

        assert_eq!(step.txs, vec![tx2.tx.clone()]);
        assert_eq!(step.unavailable, vec![missing]);
        assert_eq!(client.stats().sts_sent, Sent(1));
        assert_eq!(client.stats().sts_unavailable, Unav(1));
        assert_eq!(
            step.node_to_node_traces,
            vec![
                NodeToNodeSubmissionTrace::ReqTxs(2),
                NodeToNodeSubmissionTrace::TxList(1),
            ]
        );
    }

    #[test]
    fn request_txs_unavailable_ids_match_upstream_list_difference() {
        let tx1 = tx(1);
        let mut client = tx_submission_client(VecTxSource::new([tx1.clone()]));
        client
            .request_tx_ids(BlockingStyle::NonBlocking, Ack(0), Req(1))
            .expect("tx ids");

        let step = client.request_txs(&[tx1.tx_id, tx1.tx_id]);

        assert_eq!(step.txs, vec![tx1.tx.clone()]);
        assert_eq!(step.unavailable, vec![tx1.tx_id]);
        assert_eq!(client.stats().sts_sent, Sent(1));
        assert_eq!(client.stats().sts_unavailable, Unav(1));
    }

    #[test]
    fn blocking_request_done_fires_after_all_unacked_are_acknowledged() {
        let tx1 = tx(1);
        let tx2 = tx(2);
        let mut client = tx_submission_client(VecTxSource::new([tx1, tx2]));
        client
            .request_tx_ids(BlockingStyle::NonBlocking, Ack(0), Req(2))
            .expect("tx ids");

        let step = client
            .request_tx_ids(BlockingStyle::Blocking, Ack(2), Req(4))
            .expect("done");

        assert_eq!(
            step.reply,
            TxIdsReply::Done(SubmissionThreadStats {
                sts_acked: Ack(2),
                sts_sent: Sent(0),
                sts_unavailable: Unav(0),
            })
        );
        assert_eq!(
            step.node_to_node_traces,
            vec![
                NodeToNodeSubmissionTrace::ReqIdsBlocking(Ack(2), Req(4)),
                NodeToNodeSubmissionTrace::IdsListBlocking(0),
                NodeToNodeSubmissionTrace::EndOfProtocol,
            ]
        );
        assert!(client.unacked_tx_ids().is_empty());
    }

    #[test]
    fn blocking_request_rejects_partial_ack_like_upstream_fail() {
        let tx1 = tx(1);
        let tx2 = tx(2);
        let mut client = tx_submission_client(VecTxSource::new([tx1, tx2]));
        client
            .request_tx_ids(BlockingStyle::NonBlocking, Ack(0), Req(2))
            .expect("tx ids");

        let err = client
            .request_tx_ids(BlockingStyle::Blocking, Ack(1), Req(1))
            .expect_err("partial blocking ack should fail");

        assert_eq!(
            err,
            SubmissionClientError::BlockingAckMismatch { ack: 1, unacked: 2 }
        );
        assert_eq!(client.stats().sts_acked, Ack(0));
    }

    #[test]
    fn nonblocking_empty_source_replies_with_empty_id_list() {
        let mut client = tx_submission_client(VecTxSource::default());

        let step = client
            .request_tx_ids(BlockingStyle::NonBlocking, Ack(0), Req(8))
            .expect("empty reply");

        assert_eq!(step.reply, TxIdsReply::Reply(Vec::new()));
        assert_eq!(
            step.node_to_node_traces,
            vec![
                NodeToNodeSubmissionTrace::ReqIdsNonBlocking(Ack(0), Req(8)),
                NodeToNodeSubmissionTrace::IdsListNonBlocking(0),
            ]
        );
    }

    #[test]
    fn network_driver_serves_tx_submission_protocol_until_done() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            use tokio::net::{TcpListener, TcpStream};
            use yggdrasil_network::{
                MiniProtocolDir, MiniProtocolNum, TxIdsReply as ServerTxIdsReply,
                TxSubmissionServer, start_mux,
            };

            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("local addr");
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept");
                let (mut handles, _mux) = start_mux(
                    stream,
                    MiniProtocolDir::Responder,
                    &[MiniProtocolNum::TX_SUBMISSION],
                    4,
                );
                let handle = handles
                    .remove(&MiniProtocolNum::TX_SUBMISSION)
                    .expect("tx submission handle");
                let mut server = TxSubmissionServer::new(handle);
                server.recv_init().await.expect("init");
                let txids = match server
                    .request_tx_ids(true, 0, 2)
                    .await
                    .expect("request ids")
                {
                    ServerTxIdsReply::TxIds(txids) => txids,
                    ServerTxIdsReply::Done => {
                        panic!("first blocking request should advertise txids")
                    }
                };
                assert_eq!(txids.len(), 2);
                let txs = server
                    .request_txs(vec![txids[0].txid])
                    .await
                    .expect("request tx");
                assert_eq!(txs.len(), 1);
                match server
                    .request_tx_ids(true, 2, 2)
                    .await
                    .expect("request done")
                {
                    ServerTxIdsReply::Done => {}
                    ServerTxIdsReply::TxIds(txids) => {
                        panic!("second blocking request should end protocol, got {txids:?}")
                    }
                }
                txs.into_iter().next().expect("tx body")
            });

            let stream = TcpStream::connect(addr).await.expect("connect");
            let (mut handles, _mux) = start_mux(
                stream,
                MiniProtocolDir::Initiator,
                &[MiniProtocolNum::TX_SUBMISSION],
                4,
            );
            let handle = handles
                .remove(&MiniProtocolNum::TX_SUBMISSION)
                .expect("tx submission handle");
            let mut wire_client = NetworkTxSubmissionClient::new(handle);
            let tx1 = tx(1);
            let tx2 = tx(2);
            let expected_body = tx1.tx.raw_cbor();
            let mut local_client = tx_submission_client(VecTxSource::new([tx1, tx2]));

            let stats = run_tx_submission_client(&mut wire_client, &mut local_client)
                .await
                .expect("submission client");
            let submitted_body = server.await.expect("server task");

            assert_eq!(submitted_body, expected_body);
            assert_eq!(
                stats,
                SubmissionThreadStats {
                    sts_acked: Ack(2),
                    sts_sent: Sent(1),
                    sts_unavailable: Unav(0),
                }
            );
        });
    }
}
