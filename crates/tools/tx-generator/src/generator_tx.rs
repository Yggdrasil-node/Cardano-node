//! Transaction-stream generator runtime surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx.hs`.
//! Hosts the Rust modules that mirror `Cardano.Benchmarking.GeneratorTx.*`.
//! The current concrete leaves are `SizedMetadata.hs` and the
//! `SubmissionClient.hs` / `Submission.hs` Benchmark submission core.
//! This file owns the upstream `walletBenchmark` orchestration around
//! those leaves: target resolution, TxSubmission2 worker spawning, TPS
//! feeder spawning, shutdown, and summary collection.

use std::net::SocketAddr;
use std::time::SystemTime;

use thiserror::Error;
use tokio::task::JoinHandle;
use yggdrasil_network::{
    HandshakeVersion, MiniProtocolNum, NodeToNodeVersionData,
    TxSubmissionClient as NetworkTxSubmissionClient, peer_connect,
};

use crate::benchmarking::log_types::SubmissionSummary;
use crate::benchmarking::tps_throttle::{TpsThrottle, new_tps_throttle};
use crate::benchmarking::types::SubmissionErrorPolicy;
use crate::generator_tx::submission::{
    ReportRef, SharedTxStream, mk_submission_summary, submit_submission_thread_stats,
    submit_thread_report, tx_stream_source,
};
use crate::generator_tx::submission_client::{
    SubmissionClientError, run_tx_submission_client, tx_submission_client,
};
use crate::setup::nix_service::NodeDescription;
use crate::tx_generator::tx::GeneratedTx;
use crate::types::{NumberOfTxs, TpsRate};

pub mod sized_metadata;
pub mod submission;
pub mod submission_client;

/// Target node after upstream `lookupNodeAddress`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedNodeDescription {
    /// Upstream `ndName`.
    pub name: String,
    /// Original target host string.
    pub addr: String,
    /// Resolved IPv4 socket address.
    pub socket_addr: SocketAddr,
}

/// Errors from `walletBenchmark` orchestration.
#[derive(Debug, Error)]
pub enum WalletBenchmarkError {
    /// Upstream `NonEmpty NodeDescription` invariant was violated.
    #[error("walletBenchmark: target node list is empty")]
    EmptyTargets,
    /// DNS / address lookup failed.
    #[error("lookupNodeAddress {target}: {message}")]
    Resolve {
        /// Target label.
        target: String,
        /// Lookup error text.
        message: String,
    },
    /// Upstream uses AF_INET hints, so absence of an IPv4 address is a
    /// resolution failure for this benchmark path.
    #[error("lookupNodeAddress {target}: no IPv4 address resolved")]
    NoIpv4Address {
        /// Target label.
        target: String,
    },
    /// Node-to-node handshake/connect failed.
    #[error("benchmarkConnectTxSubmit {target}: {message}")]
    Connect {
        /// Target label.
        target: String,
        /// Connect error text.
        message: String,
    },
    /// The negotiated NtN connection did not expose TxSubmission2.
    #[error("benchmarkConnectTxSubmit {target}: missing TxSubmission2 mini-protocol handle")]
    MissingTxSubmission {
        /// Target label.
        target: String,
    },
    /// The TxSubmission2 client failed after connection.
    #[error("txSubmissionClient {target}: {source}")]
    Submission {
        /// Target label.
        target: String,
        /// Submission error.
        source: SubmissionClientError,
    },
    /// Tokio worker task failed to join.
    #[error("walletBenchmark worker join failed: {0}")]
    Join(String),
}

/// Runtime control returned by upstream `walletBenchmark`.
pub struct WalletBenchmarkControl {
    feeder: JoinHandle<()>,
    workers: Vec<JoinHandle<Result<(), WalletBenchmarkError>>>,
    report_refs: Vec<ReportRef>,
    start_time: SystemTime,
    tps_throttle: TpsThrottle,
}

impl WalletBenchmarkControl {
    /// Upstream `abcSummary` after `waitBenchmark` has waited for all
    /// feeder/worker threads.
    pub async fn wait_summary(mut self) -> Result<SubmissionSummary, WalletBenchmarkError> {
        self.feeder
            .await
            .map_err(|err| WalletBenchmarkError::Join(err.to_string()))?;
        for worker in self.workers.drain(..) {
            let _ = worker
                .await
                .map_err(|err| WalletBenchmarkError::Join(err.to_string()))?;
        }

        let start_time = self.start_time;
        let report_refs = self.report_refs.clone();
        tokio::task::spawn_blocking(move || mk_submission_summary(start_time, &report_refs))
            .await
            .map_err(|err| WalletBenchmarkError::Join(err.to_string()))
    }

    /// Upstream `abcShutdown`: stop the TPS feeder.
    pub fn shutdown(&self) {
        self.tps_throttle.send_stop();
    }
}

/// Mirror of upstream `lookupNodeAddress`.
pub async fn lookup_node_address(
    node: &NodeDescription,
) -> Result<ResolvedNodeDescription, WalletBenchmarkError> {
    let target = format!("{}:{}", node.addr, node.port);
    let mut resolved = tokio::net::lookup_host((node.addr.as_str(), node.port))
        .await
        .map_err(|err| WalletBenchmarkError::Resolve {
            target: target.clone(),
            message: err.to_string(),
        })?;
    let socket_addr =
        resolved
            .find(SocketAddr::is_ipv4)
            .ok_or_else(|| WalletBenchmarkError::NoIpv4Address {
                target: target.clone(),
            })?;
    Ok(ResolvedNodeDescription {
        name: node.name.clone(),
        addr: node.addr.clone(),
        socket_addr,
    })
}

/// Node-to-node version proposal used by upstream
/// `benchmarkConnectTxSubmit`.
pub fn wallet_benchmark_n2n_versions(
    network_magic: u32,
) -> Vec<(HandshakeVersion, NodeToNodeVersionData)> {
    vec![(
        HandshakeVersion::V14,
        NodeToNodeVersionData {
            network_magic,
            initiator_only_diffusion_mode: true,
            peer_sharing: 0,
            query: false,
        },
    )]
}

/// Mirror of upstream `walletBenchmark`.
///
/// This starts one TxSubmission2 worker per target node and one TPS
/// feeder. The returned control owns the same conceptual fields as
/// upstream `AsyncBenchmarkControl`: feeder, workers, summary action,
/// and shutdown action.
pub async fn wallet_benchmark(
    targets: Vec<NodeDescription>,
    tps_rate: TpsRate,
    error_policy: SubmissionErrorPolicy,
    network_magic: u32,
    count: NumberOfTxs,
    txs: Vec<GeneratedTx>,
) -> Result<WalletBenchmarkControl, WalletBenchmarkError> {
    if targets.is_empty() {
        return Err(WalletBenchmarkError::EmptyTargets);
    }

    let mut resolved_targets = Vec::with_capacity(targets.len());
    for target in &targets {
        resolved_targets.push(lookup_node_address(target).await?);
    }

    let start_time = SystemTime::now();
    let tps_throttle = new_tps_throttle(32, count, tps_rate);
    let tx_stream_ref = SharedTxStream::new(txs);
    let report_refs = (0..resolved_targets.len())
        .map(|_| ReportRef::new())
        .collect::<Vec<_>>();

    let workers = report_refs
        .iter()
        .cloned()
        .zip(resolved_targets)
        .map(|(report_ref, target)| {
            let source = tx_stream_source(tx_stream_ref.clone(), tps_throttle.clone());
            tokio::spawn(run_wallet_benchmark_worker(
                report_ref,
                target,
                network_magic,
                error_policy,
                source,
            ))
        })
        .collect::<Vec<_>>();

    let feeder_throttle = tps_throttle.clone();
    let feeder = tokio::task::spawn_blocking(move || {
        feeder_throttle.start_sending();
        feeder_throttle.send_stop();
    });

    Ok(WalletBenchmarkControl {
        feeder,
        workers,
        report_refs,
        start_time,
        tps_throttle,
    })
}

async fn run_wallet_benchmark_worker(
    report_ref: ReportRef,
    target: ResolvedNodeDescription,
    network_magic: u32,
    error_policy: SubmissionErrorPolicy,
    tx_source: submission::ThrottledTxSource,
) -> Result<(), WalletBenchmarkError> {
    let result = async {
        let mut connection = peer_connect(
            target.socket_addr,
            wallet_benchmark_n2n_versions(network_magic),
        )
        .await
        .map_err(|err| WalletBenchmarkError::Connect {
            target: target.name.clone(),
            message: err.to_string(),
        })?;
        let handle = connection
            .protocols
            .remove(&MiniProtocolNum::TX_SUBMISSION)
            .ok_or_else(|| WalletBenchmarkError::MissingTxSubmission {
                target: target.name.clone(),
            })?;
        let mut wire_client = NetworkTxSubmissionClient::new(handle);
        let mut local_state = tx_submission_client(tx_source);
        run_tx_submission_client(&mut wire_client, &mut local_state)
            .await
            .map_err(|err| WalletBenchmarkError::Submission {
                target: target.name.clone(),
                source: err,
            })
    }
    .await;

    match result {
        Ok(stats) => {
            submit_submission_thread_stats(&report_ref, stats);
            Ok(())
        }
        Err(err) => {
            let desc = worker_error_description(&target, &err);
            submit_thread_report(&report_ref, Err(desc));
            match error_policy {
                SubmissionErrorPolicy::FailOnError => Err(err),
                SubmissionErrorPolicy::LogErrors => Ok(()),
            }
        }
    }
}

fn worker_error_description(
    target: &ResolvedNodeDescription,
    err: &WalletBenchmarkError,
) -> String {
    format!(
        "Exception while talking to peer {} ({}): {}",
        target.name, target.socket_addr, err
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use yggdrasil_ledger::{
        AllegraTxBody, MultiEraSubmittedTx, ShelleyCompatibleSubmittedTx, ShelleyTxIn,
        ShelleyWitnessSet,
    };
    use yggdrasil_network::{TxIdsReply as ServerTxIdsReply, TxSubmissionServer, peer_accept};

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
    fn n2n_version_proposal_matches_upstream_v14_initiator_only() {
        assert_eq!(
            wallet_benchmark_n2n_versions(42),
            vec![(
                HandshakeVersion::V14,
                NodeToNodeVersionData {
                    network_magic: 42,
                    initiator_only_diffusion_mode: true,
                    peer_sharing: 0,
                    query: false,
                },
            )]
        );
    }

    #[test]
    fn lookup_node_address_resolves_ipv4_loopback_and_preserves_name() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let node = NodeDescription {
                addr: "127.0.0.1".to_string(),
                port: 3001,
                name: "node-1".to_string(),
            };

            let resolved = lookup_node_address(&node).await.expect("resolve");

            assert_eq!(resolved.name, "node-1");
            assert_eq!(resolved.socket_addr.ip().to_string(), "127.0.0.1");
            assert_eq!(resolved.socket_addr.port(), 3001);
        });
    }

    #[test]
    fn wallet_benchmark_submits_to_negotiated_txsubmission_server() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let network_magic = 42;
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("local addr");
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut connection = peer_accept(stream, network_magic, &[HandshakeVersion::V14])
                    .await
                    .expect("peer accept");
                let handle = connection
                    .protocols
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
                    ServerTxIdsReply::Done => panic!("first request should advertise txids"),
                };
                assert_eq!(txids.len(), 2);
                let txs = server
                    .request_txs(txids.iter().map(|tx| tx.txid).collect())
                    .await
                    .expect("request txs");
                match server
                    .request_tx_ids(true, 2, 2)
                    .await
                    .expect("request done")
                {
                    ServerTxIdsReply::Done => {}
                    ServerTxIdsReply::TxIds(txids) => {
                        panic!("final blocking request should end protocol, got {txids:?}")
                    }
                }
                txs
            });

            let tx1 = tx(1);
            let tx2 = tx(2);
            let expected = vec![tx1.tx.raw_cbor(), tx2.tx.raw_cbor()];
            let target = NodeDescription {
                addr: "127.0.0.1".to_string(),
                port: addr.port(),
                name: "loopback".to_string(),
            };

            let control = wallet_benchmark(
                vec![target],
                100_000.0,
                SubmissionErrorPolicy::LogErrors,
                network_magic,
                2,
                vec![tx1, tx2],
            )
            .await
            .expect("wallet benchmark");
            let summary = control.wait_summary().await.expect("summary");
            let submitted = server.await.expect("server task");
            assert_eq!(submitted, expected);
            assert_eq!(summary.ss_tx_sent.get(), 2);
            assert_eq!(summary.ss_tx_unavailable.get(), 0);
            assert!(summary.ss_failures.is_empty());
            assert_eq!(summary.ss_threadwise_tps.len(), 1);
        });
    }
}
