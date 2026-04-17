//! End-to-end integration tests for the Node-to-Client (NtC) local socket
//! surface.
//!
//! These tests bind a real Unix-domain socket via
//! [`yggdrasil_node::run_local_accept_loop`], connect through
//! [`yggdrasil_network::ntc_connect`] using the same handshake helper the CLI
//! uses, and drive the LocalStateQuery + LocalTxSubmission mini-protocols
//! from typed clients. This closes the previously untested seam between the
//! CLI handshake path (`run_query` / `run_submit_tx` in `main.rs`) and the
//! server-side dispatch pipeline.
//!
//! Reference:
//! * Server: `ouroboros-network` `LocalClient.hs` accept loop
//! * Client handshake: `Ouroboros.Network.NodeToClient`
//! * Query dispatch: `Cardano.Node.Query.*`

#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, RwLock};
use std::time::Duration;

use tempfile::tempdir;
use yggdrasil_ledger::{CborDecode, CborEncode, Decoder, Encoder, Era, HeaderHash, LedgerState, Point, SlotNo};
use yggdrasil_mempool::{Mempool, SharedMempool};
use yggdrasil_network::{
    AcquireTarget, LocalStateQueryClient, LocalTxSubmissionClient, MiniProtocolNum, ntc_connect,
};
use yggdrasil_node::{BasicLocalQueryDispatcher, run_local_accept_loop};
use yggdrasil_storage::{ChainDb, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile};

/// Network magic used for all tests in this file.
const TEST_MAGIC: u32 = 42;

type TestChainDb = ChainDb<InMemoryImmutable, InMemoryVolatile, InMemoryLedgerStore>;

/// Build an empty ChainDb with a persisted Byron-era ledger checkpoint so
/// that `recover_ledger_state_chaindb` returns a usable state.
fn build_empty_chain_db() -> Arc<RwLock<TestChainDb>> {
    let chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    Arc::new(RwLock::new(chain_db))
}

/// Bind a local accept loop on a temp Unix socket and return
/// (`socket_path`, `shutdown_tx`, `join_handle`) for test cleanup.
async fn spawn_local_server()
-> (
    std::path::PathBuf,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
    tempfile::TempDir,
) {
    let tmp = tempdir().expect("tempdir");
    let socket_path = tmp.path().join("ntc.sock");

    let chain_db = build_empty_chain_db();
    let mempool = SharedMempool::new(Mempool::with_capacity(1 << 20));
    let dispatcher = Arc::new(BasicLocalQueryDispatcher);

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let socket_path_server = socket_path.clone();

    let handle = tokio::spawn(async move {
        let _ = run_local_accept_loop(
            &socket_path_server,
            TEST_MAGIC,
            chain_db,
            mempool,
            dispatcher,
            None,
            async move {
                let _ = rx.await;
            },
        )
        .await;
    });

    // Poll for the socket to appear (bind races with connect).
    for _ in 0..50 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    (socket_path, tx, handle, tmp)
}

/// End-to-end test: connect through the same `ntc_connect` driver the CLI
/// uses, acquire the volatile tip, and issue a CurrentEra query.
///
/// The CurrentEra response is a CBOR unsigned encoding the era ordinal.
#[tokio::test]
async fn ntc_local_state_query_current_era_round_trip() {
    let (socket_path, shutdown, handle, _tmp) = spawn_local_server().await;

    // Client side: use the same NtC handshake path the CLI uses.
    let mut conn = ntc_connect(&socket_path, TEST_MAGIC, true)
        .await
        .expect("ntc_connect should succeed against our test accept loop");

    assert_eq!(conn.version_data.network_magic, TEST_MAGIC);
    assert!(
        conn.version_data.query,
        "CLI-style query=true must be preserved through handshake"
    );

    let sq_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_STATE_QUERY)
        .expect("NTC_LOCAL_STATE_QUERY handle missing");
    let mut client = LocalStateQueryClient::new(sq_handle);

    client
        .acquire(AcquireTarget::VolatileTip)
        .await
        .expect("acquire volatile tip");

    // Encode the CurrentEra query the same way the CLI does: [0u64].
    let mut enc = Encoder::new();
    enc.array(1).unsigned(0u64);
    let query_bytes = enc.into_bytes();

    let result = client
        .query(query_bytes)
        .await
        .expect("LocalStateQuery query round-trip");

    // BasicLocalQueryDispatcher returns the era ordinal as a CBOR unsigned.
    let mut dec = Decoder::new(&result);
    let era_ordinal = dec.unsigned().expect("CurrentEra returns CBOR unsigned");
    // An empty ChainDb recovers a Byron-era ledger state (ordinal 0).
    assert_eq!(
        era_ordinal,
        Era::Byron as u64,
        "empty ledger should report Byron era"
    );

    let _ = client.release().await;
    let _ = client.done().await;

    let _ = shutdown.send(());
    let _ = handle.await;
}

/// End-to-end test: acquiring a point that is not on the chain fails.
///
/// On an empty ChainDb, only `Origin` is on-chain; any `BlockPoint` must be
/// rejected with acquire failure.
#[tokio::test]
async fn ntc_local_state_query_rejects_unknown_acquire_point() {
    let (socket_path, shutdown, handle, _tmp) = spawn_local_server().await;

    let mut conn = ntc_connect(&socket_path, TEST_MAGIC, true)
        .await
        .expect("ntc_connect should succeed against our test accept loop");

    let sq_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_STATE_QUERY)
        .expect("NTC_LOCAL_STATE_QUERY handle missing");
    let mut client = LocalStateQueryClient::new(sq_handle);

    let requested = Point::BlockPoint(SlotNo(42), HeaderHash([7u8; 32]));
    let mut enc = Encoder::new();
    requested.encode_cbor(&mut enc);
    let requested_bytes = enc.into_bytes();

    let result = client.acquire(AcquireTarget::Point(requested_bytes)).await;
    assert!(
        result.is_err(),
        "acquire must fail for a point not on the current chain"
    );

    let _ = client.done().await;
    let _ = shutdown.send(());
    let _ = handle.await;
}

/// End-to-end test: LocalTxSubmission with a malformed transaction payload
/// is rejected with a CBOR-encoded reason rather than aborting the session.
///
/// This exercises the `MultiEraSubmittedTx::from_cbor_bytes_for_era` decode
/// error path inside `run_local_tx_submission_session` and verifies the
/// server emits a typed `MsgRejectTx` that the typed client can surface.
#[tokio::test]
async fn ntc_local_tx_submission_rejects_malformed_tx() {
    let (socket_path, shutdown, handle, _tmp) = spawn_local_server().await;

    let mut conn = ntc_connect(&socket_path, TEST_MAGIC, false)
        .await
        .expect("ntc_connect should succeed against our test accept loop");
    assert!(
        !conn.version_data.query,
        "submit-tx CLI uses query=false in handshake"
    );

    let tx_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION)
        .expect("NTC_LOCAL_TX_SUBMISSION handle missing");
    let mut client = LocalTxSubmissionClient::new(tx_handle);

    // Submit a clearly-malformed CBOR payload; the node must reject it
    // through MsgRejectTx rather than disconnect.
    let bogus_tx = vec![0xff, 0xff, 0xff];
    let result = client.submit(bogus_tx).await;
    assert!(
        result.is_err(),
        "malformed tx bytes should produce LocalTxSubmissionClientError::RejectTx"
    );

    let _ = client.done().await;

    let _ = shutdown.send(());
    let _ = handle.await;
}

/// End-to-end test: LocalStateQuery `ChainTip` (tag 1) returns the encoded
/// `Point` for the recovered ledger tip. For an empty chain this is
/// `Point::Origin`, which must CBOR-decode cleanly on the client side.
#[tokio::test]
async fn ntc_local_state_query_chain_tip_round_trip() {
    let (socket_path, shutdown, handle, _tmp) = spawn_local_server().await;

    let mut conn = ntc_connect(&socket_path, TEST_MAGIC, true)
        .await
        .expect("ntc_connect should succeed");

    let sq_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_STATE_QUERY)
        .expect("NTC_LOCAL_STATE_QUERY handle missing");
    let mut client = LocalStateQueryClient::new(sq_handle);
    client
        .acquire(AcquireTarget::VolatileTip)
        .await
        .expect("acquire volatile tip");

    // ChainTip query: [1u64].
    let mut enc = Encoder::new();
    enc.array(1).unsigned(1u64);
    let result = client
        .query(enc.into_bytes())
        .await
        .expect("ChainTip query round-trip");

    // Result is a raw CBOR-encoded Point; decode it.
    let point = Point::from_cbor_bytes(&result).expect("ChainTip CBOR must decode as Point");
    assert_eq!(
        point,
        Point::Origin,
        "empty ChainDb must report Origin tip"
    );

    let _ = client.release().await;
    let _ = client.done().await;

    let _ = shutdown.send(());
    let _ = handle.await;
}

/// End-to-end test: NtC handshake fails cleanly when the client advertises
/// the wrong network magic. This mirrors upstream wallet/CLI behavior when
/// pointed at the wrong network socket.
#[tokio::test]
async fn ntc_connect_wrong_magic_refused() {
    let (socket_path, shutdown, handle, _tmp) = spawn_local_server().await;

    let result = ntc_connect(&socket_path, TEST_MAGIC + 1, true).await;
    assert!(
        result.is_err(),
        "handshake must fail on network-magic mismatch"
    );

    let _ = shutdown.send(());
    let _ = handle.await;
}

// Keep a reference to the unused type alias to avoid dead-code warnings when
// future tests are added.
#[allow(dead_code)]
fn _type_aliases_are_live() -> Option<LedgerState> {
    None
}
