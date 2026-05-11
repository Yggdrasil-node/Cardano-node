//! End-to-end integration test: DataPoint acceptor + forwarder
//! interoperating over a single mux'd Unix pipe.
//!
//! R474: cheap-insurance follow-on to the R452-R459 acceptor-side
//! arc + R471-R473 forwarder-side arc. Verifies that Yggdrasil's
//! own acceptor + forwarder round-trip the DataPointForward wire
//! format correctly — neither side talking to a synthetic test
//! peer, both being the real driver.
//!
//! Why this exists: unit tests at R454 (acceptor) and R471
//! (forwarder) each verify their side against a synthetic peer
//! implemented in the test harness. This test verifies the two
//! Yggdrasil drivers together, which is the actual operational
//! shape (when wired into cardano-tracer + a future node-binary
//! trace-source).

#![allow(clippy::unwrap_used)]
#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use tokio::net::UnixStream;
use yggdrasil_network::data_point_forwarder::DataPointForwarderEvent;
use yggdrasil_network::data_point_run_forwarder::forward_data_points_resp;
use yggdrasil_network::mux::{MiniProtocolDir, MiniProtocolNum, start_unix};
use yggdrasil_network::protocols::{
    DataPointAcceptorConfiguration, DataPointForwarderConfiguration, DataPointName,
    DataPointRequestor, DataPointValue, init_data_point_store, write_to_store,
};
use yggdrasil_network::{DataPointAcceptor, DataPointForwarder};

const DATA_POINTS_NUM: MiniProtocolNum = MiniProtocolNum(3);

/// End-to-end: acceptor.request(names) ↔ forwarder.send_reply(values)
/// across a single mux'd Unix pipe. Both drivers are Yggdrasil's
/// real implementations.
#[tokio::test]
async fn acceptor_and_forwarder_round_trip_single_request() {
    let (acc_stream, fwd_stream) = UnixStream::pair().expect("unix stream pair");
    let (mut acc_handles, _acc_mux) = start_unix(
        acc_stream,
        MiniProtocolDir::Initiator,
        &[DATA_POINTS_NUM],
        1,
    );
    let (mut fwd_handles, _fwd_mux) = start_unix(
        fwd_stream,
        MiniProtocolDir::Responder,
        &[DATA_POINTS_NUM],
        1,
    );
    let acc_handle = acc_handles.remove(&DATA_POINTS_NUM).expect("acc handle");
    let fwd_handle = fwd_handles.remove(&DATA_POINTS_NUM).expect("fwd handle");

    // Forwarder side: spawn the real `DataPointForwarder` driver
    // looping against a populated DataPointStore.
    let store = init_data_point_store();
    write_to_store(
        &store,
        DataPointName::new("node-info"),
        DataPointValue::new(b"{\"version\":\"11.0.1\"}".to_vec()),
    )
    .await;
    write_to_store(
        &store,
        DataPointName::new("tip"),
        DataPointValue::new(b"42".to_vec()),
    )
    .await;
    let fwd_config = DataPointForwarderConfiguration::new();
    let forwarder_task =
        tokio::spawn(async move { forward_data_points_resp(fwd_config, fwd_handle, store).await });

    // Acceptor side: real `DataPointAcceptor` driver makes a
    // request, awaits the forwarder's reply, asserts the values
    // round-tripped correctly, then sends Done.
    let mut acceptor = DataPointAcceptor::new(acc_handle);
    let names = vec![
        DataPointName::new("node-info"),
        DataPointName::new("tip"),
        DataPointName::new("unknown"),
    ];
    let values = acceptor.request(names).await.expect("acceptor request");
    assert_eq!(values.len(), 3, "3 input names → 3 output entries");

    // node-info: Some(JSON bytes)
    assert_eq!(values[0].0, DataPointName::new("node-info"));
    assert_eq!(
        values[0].1.as_ref().expect("Just").as_slice(),
        b"{\"version\":\"11.0.1\"}"
    );
    // tip: Some(42)
    assert_eq!(values[1].0, DataPointName::new("tip"));
    assert_eq!(values[1].1.as_ref().expect("Just").as_slice(), b"42");
    // unknown: None
    assert_eq!(values[2].0, DataPointName::new("unknown"));
    assert!(values[2].1.is_none());

    // Terminate the protocol from the acceptor side.
    acceptor.done().await.expect("acceptor done");

    // The forwarder loop should exit cleanly on MsgDone.
    let result = tokio::time::timeout(Duration::from_secs(2), forwarder_task)
        .await
        .expect("forwarder did not exit")
        .expect("forwarder task panicked");
    assert!(result.is_ok(), "forwarder result: {result:?}");
}

/// End-to-end: multiple sequential acceptor.request → forwarder.reply
/// round-trips across a single mux'd Unix pipe. Verifies the state
/// machine correctly cycles through StIdle → StBusy → StIdle for
/// each round on both sides.
#[tokio::test]
async fn acceptor_and_forwarder_round_trip_multiple_requests() {
    let (acc_stream, fwd_stream) = UnixStream::pair().expect("unix stream pair");
    let (mut acc_handles, _acc_mux) = start_unix(
        acc_stream,
        MiniProtocolDir::Initiator,
        &[DATA_POINTS_NUM],
        1,
    );
    let (mut fwd_handles, _fwd_mux) = start_unix(
        fwd_stream,
        MiniProtocolDir::Responder,
        &[DATA_POINTS_NUM],
        1,
    );
    let acc_handle = acc_handles.remove(&DATA_POINTS_NUM).expect("acc handle");
    let fwd_handle = fwd_handles.remove(&DATA_POINTS_NUM).expect("fwd handle");

    let store = init_data_point_store();
    write_to_store(
        &store,
        DataPointName::new("counter"),
        DataPointValue::new(b"7".to_vec()),
    )
    .await;
    let fwd_config = DataPointForwarderConfiguration::new();
    let forwarder_task =
        tokio::spawn(async move { forward_data_points_resp(fwd_config, fwd_handle, store).await });

    let mut acceptor = DataPointAcceptor::new(acc_handle);
    for _round in 0..5u32 {
        let values = acceptor
            .request(vec![DataPointName::new("counter")])
            .await
            .expect("acceptor request");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].1.as_ref().expect("Just").as_slice(), b"7");
    }
    acceptor.done().await.expect("acceptor done");

    let result = tokio::time::timeout(Duration::from_secs(2), forwarder_task)
        .await
        .expect("forwarder did not exit")
        .expect("forwarder task panicked");
    assert!(result.is_ok());
}

/// End-to-end via `DataPointRequestor` external-context API:
/// simulates the cardano-tracer-side production shape where a
/// query-router task calls `requestor.ask_for_data_points(names)`
/// rather than calling the acceptor driver directly.
#[tokio::test]
async fn requestor_and_forwarder_round_trip_via_run_acceptor() {
    use yggdrasil_network::data_point_run_acceptor::accept_data_points_resp as accept_resp;
    let (acc_stream, fwd_stream) = UnixStream::pair().expect("unix stream pair");
    let (mut acc_handles, _acc_mux) = start_unix(
        acc_stream,
        MiniProtocolDir::Initiator,
        &[DATA_POINTS_NUM],
        1,
    );
    let (mut fwd_handles, _fwd_mux) = start_unix(
        fwd_stream,
        MiniProtocolDir::Responder,
        &[DATA_POINTS_NUM],
        1,
    );
    let acc_handle = acc_handles.remove(&DATA_POINTS_NUM).expect("acc handle");
    let fwd_handle = fwd_handles.remove(&DATA_POINTS_NUM).expect("fwd handle");

    // Forwarder side: real R473 runtime aggregator + populated store.
    let store = init_data_point_store();
    write_to_store(
        &store,
        DataPointName::new("node-info"),
        DataPointValue::new(b"{\"niName\":\"alice-pool\"}".to_vec()),
    )
    .await;
    let fwd_config = DataPointForwarderConfiguration::new();
    let forwarder_task =
        tokio::spawn(async move { forward_data_points_resp(fwd_config, fwd_handle, store).await });

    // Acceptor side: real R457 runtime aggregator + a
    // DataPointRequestor that an external context (e.g. the
    // ask_node_name helper at R469) will drive.
    let acc_config = DataPointAcceptorConfiguration::new();
    let acc_brake = acc_config.should_we_stop.clone();
    let requestor = DataPointRequestor::new();
    let requestor_for_acceptor = requestor.clone();
    let acceptor_task = tokio::spawn(async move {
        accept_resp(
            acc_config,
            acc_handle,
            move || requestor_for_acceptor,
            |_e| {},
        )
        .await
    });

    // External context: ask for node-info. The R457 acceptor loop
    // picks it up + drives the protocol; the R473 forwarder loop
    // looks up the name in the store and replies.
    let values = requestor
        .ask_for_data_points(vec![DataPointName::new("node-info")])
        .await;
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].0, DataPointName::new("node-info"));
    assert_eq!(
        values[0].1.as_ref().expect("Just").as_slice(),
        b"{\"niName\":\"alice-pool\"}"
    );

    // Trip the acceptor brake → it'll send MsgDone → forwarder
    // returns cleanly.
    *acc_brake.write().await = true;

    let acceptor_result = tokio::time::timeout(Duration::from_secs(5), acceptor_task)
        .await
        .expect("acceptor did not exit")
        .expect("acceptor task panicked");
    assert!(
        acceptor_result.is_ok(),
        "acceptor result: {acceptor_result:?}"
    );
    let forwarder_result = tokio::time::timeout(Duration::from_secs(5), forwarder_task)
        .await
        .expect("forwarder did not exit")
        .expect("forwarder task panicked");
    assert!(
        forwarder_result.is_ok(),
        "forwarder result: {forwarder_result:?}"
    );
}

/// Lightweight sanity: the bare driver pair completes the
/// MsgDone exchange even when the acceptor never sends any
/// requests. Mirror of upstream's empty-session shape.
#[tokio::test]
async fn acceptor_and_forwarder_immediate_done() {
    let (acc_stream, fwd_stream) = UnixStream::pair().expect("unix stream pair");
    let (mut acc_handles, _acc_mux) = start_unix(
        acc_stream,
        MiniProtocolDir::Initiator,
        &[DATA_POINTS_NUM],
        1,
    );
    let (mut fwd_handles, _fwd_mux) = start_unix(
        fwd_stream,
        MiniProtocolDir::Responder,
        &[DATA_POINTS_NUM],
        1,
    );
    let acc_handle = acc_handles.remove(&DATA_POINTS_NUM).expect("acc handle");
    let fwd_handle = fwd_handles.remove(&DATA_POINTS_NUM).expect("fwd handle");

    let mut forwarder = DataPointForwarder::new(fwd_handle);
    let forwarder_task = tokio::spawn(async move {
        match forwarder.wait_for_request().await? {
            DataPointForwarderEvent::Request(_) => panic!("expected Done, got Request"),
            DataPointForwarderEvent::Done => Ok::<(), Box<dyn std::error::Error + Send + Sync>>(()),
        }
    });

    let acceptor = DataPointAcceptor::new(acc_handle);
    acceptor.done().await.expect("acceptor done");

    let result = tokio::time::timeout(Duration::from_secs(2), forwarder_task)
        .await
        .expect("forwarder timeout")
        .expect("forwarder panicked");
    assert!(result.is_ok());
}

// Suppress unused-import warning for `Arc` — exists for future
// follow-on tests that share state across acceptor/forwarder tasks.
#[allow(dead_code)]
fn _arc_anchor() -> Option<Arc<()>> {
    None
}
