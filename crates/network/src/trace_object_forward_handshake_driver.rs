//! Trace-forwarder handshake state-machine driver — runs the
//! ProposeVersions / AcceptVersion / Refuse exchange on a mux'd
//! HANDSHAKE channel for both responder (cardano-tracer-server)
//! and initiator (cardano-tracer-client) roles.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side specialization. Mirror
//! of upstream's `Server.with` + `connectToNode` handshake-loop
//! semantics for the trace-forwarder pipe (called from
//! `Cardano.Tracer.Acceptors.{Server, Client}`).
//!
//! Upstream wraps the handshake in
//! `HandshakeArguments`/`Handshake.simpleSingletonVersions` +
//! `connectToNode` (initiator) / `Server.with` (responder).
//! Yggdrasil's port collapses that machinery into a pair of
//! plain `async fn` drivers operating on a [`crate::mux::ProtocolHandle`]
//! — matching the precedent in `crates/network/src/peer.rs::accept` /
//! `peer::connect` for the NtN handshake.
//!
//! Mapping summary:
//!
//! | Upstream                                                 | Yggdrasil                              |
//! |----------------------------------------------------------|----------------------------------------|
//! | `connectToNode snocket bearer args _ versions _ address` | [`run_handshake_initiator`]            |
//! | `Server.with snocket _ _ bearer _ address args versions` | [`run_handshake_responder`]            |
//! | `Handshake.simpleSingletonVersions ForwardingV_1 d _`    | [`crate::protocols::simple_singleton_versions`] (R433) |
//! | `Handshake.acceptableVersion local remote`               | [`crate::protocols::ForwardingVersionData::accept`] (R432) |
//! | `Handshake.codecHandshake forwardingVersionCodec`        | [`crate::protocols::TraceForwardHandshakeMessage::to_cbor`] / `::from_cbor` (R433/R434) |
//! | upstream's automatic refuse-on-mismatch                  | [`HandshakeError::NoCompatibleVersion`] / `MagicMismatch` |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`HandshakeArguments` record**: upstream's
//!   `HandshakeArguments` carries 6 fields (handshake-tracer,
//!   bearer-tracer, codec, version-data-codec, accept-version,
//!   query-version, time-limits). Yggdrasil collapses these to
//!   plain function args + module-level constants — the codec is
//!   pinned by R433/R434, the version-data accept logic by R432,
//!   tracers are deferred.
//! - **`Handshake.timeLimitsHandshake` / `noTimeLimitsHandshake`**:
//!   upstream toggles a per-state timeout for RemoteSocket vs
//!   LocalPipe paths. Yggdrasil applies a single 5-second
//!   end-to-end deadline on both sides via [`HANDSHAKE_DEADLINE`].
//! - **`HandshakeException` / `Refuse` exception path**: upstream
//!   throws a Haskell exception to abort the responder's
//!   `Server.with` continuation. Yggdrasil returns a `Result` —
//!   callers decide whether to drop the connection or retry.

use std::time::Duration;

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    ForwardingVersion, ForwardingVersionData, TraceForwardHandshakeMessage,
    TraceForwardRefuseReason,
};

/// End-to-end deadline for a single handshake exchange. Mirrors
/// the operationally-canonical 5-second budget upstream uses for
/// the NtN handshake (Yggdrasil applies the same to trace-
/// forwarder for symmetry).
pub const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the trace-forwarder handshake driver.
#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed before the handshake completed.
    #[error("connection closed before handshake completion")]
    ConnectionClosed,

    /// CBOR decode failure on the wire-level message.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Received a message in the wrong sub-state (e.g. responder
    /// got `AcceptVersion` instead of `ProposeVersions`).
    #[error("unexpected message: {0}")]
    Unexpected(String),

    /// Remote refused the handshake — surfaces upstream's
    /// `RefuseReason` payload.
    #[error("handshake refused: {reason:?}")]
    Refused {
        /// The refuse reason as decoded from the wire.
        reason: TraceForwardRefuseReason,
    },

    /// No proposed version overlapped with our supported set.
    #[error("no compatible trace-forwarder version")]
    NoCompatibleVersion,

    /// A compatible version existed but its `network_magic` did
    /// not match ours. Mirror of upstream's `Refuse $ "ForwardingVersionData mismatch: ..."`.
    #[error("network magic mismatch: local={local}, remote={remote}")]
    MagicMismatch {
        /// Our local network magic.
        local: u32,
        /// The remote-supplied network magic for the agreed version.
        remote: u32,
    },

    /// End-to-end deadline exceeded.
    #[error("handshake deadline exceeded ({0:?})")]
    Timeout(Duration),
}

// ---------------------------------------------------------------------------
// Negotiated outcome
// ---------------------------------------------------------------------------

/// Outcome of a successful handshake exchange.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct HandshakeOutcome {
    /// The version the two parties agreed on.
    pub version: ForwardingVersion,
    /// The version-data both parties have agreed on (mirrors what
    /// the responder accepted; the initiator's matched version-
    /// data exchanged).
    pub version_data: ForwardingVersionData,
}

// ---------------------------------------------------------------------------
// Responder
// ---------------------------------------------------------------------------

/// Run the responder side of the trace-forwarder handshake.
/// Receives `ProposeVersions` from the remote, picks the highest
/// version we support whose network-magic matches ours, sends
/// `AcceptVersion` (or `Refuse`), and returns the negotiated
/// outcome.
///
/// `local_versions` is our supported version set, in the order we
/// prefer (the driver iterates highest-to-lowest by tag).
/// `our_magic` is the cardano-network magic we expect from the
/// remote.
pub async fn run_handshake_responder(
    handle: ProtocolHandle,
    local_versions: &[ForwardingVersion],
    our_magic: u32,
) -> Result<HandshakeOutcome, HandshakeError> {
    tokio::time::timeout(
        HANDSHAKE_DEADLINE,
        run_responder_inner(handle, local_versions, our_magic),
    )
    .await
    .map_err(|_| HandshakeError::Timeout(HANDSHAKE_DEADLINE))?
}

async fn run_responder_inner(
    handle: ProtocolHandle,
    local_versions: &[ForwardingVersion],
    our_magic: u32,
) -> Result<HandshakeOutcome, HandshakeError> {
    let mut hs = MessageChannel::new(handle);

    // Step 1: receive ProposeVersions.
    let propose_bytes = hs.recv().await.ok_or(HandshakeError::ConnectionClosed)?;
    let propose = TraceForwardHandshakeMessage::from_cbor(&propose_bytes)
        .map_err(|e| HandshakeError::Decode(e.to_string()))?;
    let proposed = match propose {
        TraceForwardHandshakeMessage::ProposeVersions(versions) => versions,
        other => {
            return Err(HandshakeError::Unexpected(format!(
                "expected ProposeVersions, got {other:?}"
            )));
        }
    };

    // Step 2: pick the highest local version that the remote also
    // proposed (sorted highest-to-lowest by tag).
    let mut sorted_local: Vec<ForwardingVersion> = local_versions.to_vec();
    sorted_local.sort_unstable_by_key(|v| std::cmp::Reverse(v.tag()));

    for our_ver in &sorted_local {
        if let Some((_, remote_data)) = proposed.iter().find(|(v, _)| v == our_ver) {
            // Found a matching version — verify the network-magic
            // via R432's accept logic.
            let our_data = ForwardingVersionData {
                network_magic: our_magic,
            };
            if remote_data.network_magic == our_magic {
                let accept = TraceForwardHandshakeMessage::AcceptVersion(*our_ver, our_data);
                hs.send(accept.to_cbor()).await?;
                return Ok(HandshakeOutcome {
                    version: *our_ver,
                    version_data: our_data,
                });
            } else {
                // Magic mismatch — refuse with the upstream-faithful
                // message text.
                let refuse =
                    TraceForwardHandshakeMessage::Refuse(TraceForwardRefuseReason::Refused(
                        *our_ver,
                        format!(
                            "ForwardingVersionData mismatch: local={our_magic}, remote={}",
                            remote_data.network_magic
                        ),
                    ));
                hs.send(refuse.to_cbor()).await?;
                return Err(HandshakeError::MagicMismatch {
                    local: our_magic,
                    remote: remote_data.network_magic,
                });
            }
        }
    }

    // Step 3: no compatible version — refuse.
    let refuse = TraceForwardHandshakeMessage::Refuse(TraceForwardRefuseReason::VersionMismatch(
        proposed.iter().map(|(v, _)| *v).collect(),
    ));
    hs.send(refuse.to_cbor()).await?;
    Err(HandshakeError::NoCompatibleVersion)
}

// ---------------------------------------------------------------------------
// Initiator
// ---------------------------------------------------------------------------

/// Run the initiator side of the trace-forwarder handshake.
/// Sends `ProposeVersions` carrying the supplied
/// `(version, data)` table, awaits the remote's response, and
/// returns the agreed `HandshakeOutcome` (or surfaces the
/// remote's refuse-reason as `HandshakeError::Refused`).
pub async fn run_handshake_initiator(
    handle: ProtocolHandle,
    proposals: Vec<(ForwardingVersion, ForwardingVersionData)>,
) -> Result<HandshakeOutcome, HandshakeError> {
    tokio::time::timeout(HANDSHAKE_DEADLINE, run_initiator_inner(handle, proposals))
        .await
        .map_err(|_| HandshakeError::Timeout(HANDSHAKE_DEADLINE))?
}

async fn run_initiator_inner(
    handle: ProtocolHandle,
    proposals: Vec<(ForwardingVersion, ForwardingVersionData)>,
) -> Result<HandshakeOutcome, HandshakeError> {
    let mut hs = MessageChannel::new(handle);

    // Step 1: send ProposeVersions.
    let propose = TraceForwardHandshakeMessage::ProposeVersions(proposals);
    hs.send(propose.to_cbor()).await?;

    // Step 2: await the remote's response.
    let response_bytes = hs.recv().await.ok_or(HandshakeError::ConnectionClosed)?;
    let response = TraceForwardHandshakeMessage::from_cbor(&response_bytes)
        .map_err(|e| HandshakeError::Decode(e.to_string()))?;

    match response {
        TraceForwardHandshakeMessage::AcceptVersion(version, version_data) => {
            Ok(HandshakeOutcome {
                version,
                version_data,
            })
        }
        TraceForwardHandshakeMessage::Refuse(reason) => Err(HandshakeError::Refused { reason }),
        other => Err(HandshakeError::Unexpected(format!(
            "expected AcceptVersion or Refuse, got {other:?}"
        ))),
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::mux::{MiniProtocolDir, MiniProtocolNum, MuxHandle, start_unix};
    use tokio::net::UnixStream;

    /// Mux protocol number reserved for the handshake. Mirrors
    /// `MiniProtocolNum::HANDSHAKE = Self(0)` upstream.
    const HANDSHAKE_NUM: MiniProtocolNum = MiniProtocolNum::HANDSHAKE;

    fn handle_pair() -> (ProtocolHandle, ProtocolHandle, MuxHandle, MuxHandle) {
        let (a_stream, f_stream) = UnixStream::pair().expect("unix stream pair");
        let (mut a_handles, a_mux) =
            start_unix(a_stream, MiniProtocolDir::Initiator, &[HANDSHAKE_NUM], 1);
        let (mut f_handles, f_mux) =
            start_unix(f_stream, MiniProtocolDir::Responder, &[HANDSHAKE_NUM], 1);
        let a = a_handles.remove(&HANDSHAKE_NUM).expect("a handle");
        let f = f_handles.remove(&HANDSHAKE_NUM).expect("f handle");
        (a, f, a_mux, f_mux)
    }

    #[test]
    fn handshake_deadline_matches_upstream_5_seconds() {
        assert_eq!(HANDSHAKE_DEADLINE, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn responder_accepts_matching_version_and_magic() {
        let (a_handle, f_handle, _a_mux, _f_mux) = handle_pair();
        let our_magic = 764824073;

        // Initiator sends ProposeVersions(V1, magic=764824073).
        let initiator_task = tokio::spawn(async move {
            run_handshake_initiator(
                a_handle,
                vec![(
                    ForwardingVersion::V1,
                    ForwardingVersionData {
                        network_magic: our_magic,
                    },
                )],
            )
            .await
        });

        let outcome = run_handshake_responder(f_handle, &[ForwardingVersion::V1], our_magic)
            .await
            .expect("responder accepts");
        assert_eq!(outcome.version, ForwardingVersion::V1);
        assert_eq!(outcome.version_data.network_magic, our_magic);

        let initiator_outcome = initiator_task
            .await
            .expect("initiator task")
            .expect("initiator accepts");
        assert_eq!(initiator_outcome.version, ForwardingVersion::V1);
        assert_eq!(initiator_outcome.version_data.network_magic, our_magic);
    }

    #[tokio::test]
    async fn responder_picks_highest_overlapping_version() {
        let (a_handle, f_handle, _a_mux, _f_mux) = handle_pair();
        let our_magic = 1;

        let initiator_task = tokio::spawn(async move {
            run_handshake_initiator(
                a_handle,
                vec![
                    (
                        ForwardingVersion::V1,
                        ForwardingVersionData { network_magic: 1 },
                    ),
                    (
                        ForwardingVersion::V2,
                        ForwardingVersionData { network_magic: 1 },
                    ),
                ],
            )
            .await
        });

        let outcome = run_handshake_responder(
            f_handle,
            &[ForwardingVersion::V1, ForwardingVersion::V2],
            our_magic,
        )
        .await
        .expect("responder accepts");
        // V2 has the higher tag (2 > 1), so the responder should
        // pick it.
        assert_eq!(outcome.version, ForwardingVersion::V2);

        let initiator_outcome = initiator_task.await.expect("task").expect("accepts");
        assert_eq!(initiator_outcome.version, ForwardingVersion::V2);
    }

    #[tokio::test]
    async fn responder_refuses_on_no_overlap() {
        let (a_handle, f_handle, _a_mux, _f_mux) = handle_pair();

        // Initiator proposes V1; responder only supports V2. No overlap.
        let initiator_task = tokio::spawn(async move {
            run_handshake_initiator(
                a_handle,
                vec![(
                    ForwardingVersion::V1,
                    ForwardingVersionData { network_magic: 1 },
                )],
            )
            .await
        });

        let result = run_handshake_responder(f_handle, &[ForwardingVersion::V2], 1).await;
        assert!(matches!(result, Err(HandshakeError::NoCompatibleVersion)));

        // Initiator should observe the responder's Refuse.
        let initiator_outcome = initiator_task.await.expect("task");
        match initiator_outcome {
            Err(HandshakeError::Refused {
                reason: TraceForwardRefuseReason::VersionMismatch(vs),
            }) => {
                assert_eq!(vs, vec![ForwardingVersion::V1]);
            }
            other => panic!("expected initiator Refused, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn responder_refuses_on_magic_mismatch() {
        let (a_handle, f_handle, _a_mux, _f_mux) = handle_pair();

        // Both parties support V1 but the magics differ.
        let initiator_task = tokio::spawn(async move {
            run_handshake_initiator(
                a_handle,
                vec![(
                    ForwardingVersion::V1,
                    ForwardingVersionData { network_magic: 999 },
                )],
            )
            .await
        });

        let result = run_handshake_responder(f_handle, &[ForwardingVersion::V1], 1).await;
        match result {
            Err(HandshakeError::MagicMismatch { local, remote }) => {
                assert_eq!(local, 1);
                assert_eq!(remote, 999);
            }
            other => panic!("expected MagicMismatch, got {other:?}"),
        }

        let initiator_outcome = initiator_task.await.expect("task");
        match initiator_outcome {
            Err(HandshakeError::Refused {
                reason: TraceForwardRefuseReason::Refused(ver, msg),
            }) => {
                assert_eq!(ver, ForwardingVersion::V1);
                assert!(msg.contains("ForwardingVersionData mismatch"));
                assert!(msg.contains("local=1"));
                assert!(msg.contains("remote=999"));
            }
            other => panic!("expected initiator Refused-magic, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn initiator_errors_on_unexpected_message() {
        let (a_handle, f_handle, _a_mux, _f_mux) = handle_pair();

        // Forwarder sends a QueryReply (unexpected response shape
        // for the initiator who proposed but didn't query).
        let forwarder_task = tokio::spawn(async move {
            let mut hs = MessageChannel::new(f_handle);
            // Receive (and discard) the initiator's Propose.
            let _ = hs.recv().await;
            // Send a malformed-for-state QueryReply.
            let qr = TraceForwardHandshakeMessage::QueryReply(vec![]);
            hs.send(qr.to_cbor()).await.expect("send");
        });

        let result = run_handshake_initiator(
            a_handle,
            vec![(
                ForwardingVersion::V1,
                ForwardingVersionData { network_magic: 1 },
            )],
        )
        .await;
        assert!(
            matches!(result, Err(HandshakeError::Unexpected(_))),
            "expected Unexpected, got {result:?}"
        );
        forwarder_task.await.expect("task");
    }

    #[tokio::test]
    async fn responder_errors_on_unexpected_first_message() {
        let (a_handle, f_handle, _a_mux, _f_mux) = handle_pair();

        // Initiator sends an AcceptVersion as the first message
        // (illegal — the initiator should send ProposeVersions).
        let initiator_task = tokio::spawn(async move {
            let hs = MessageChannel::new(a_handle);
            let bad = TraceForwardHandshakeMessage::AcceptVersion(
                ForwardingVersion::V1,
                ForwardingVersionData { network_magic: 1 },
            );
            hs.send(bad.to_cbor()).await.expect("send");
        });

        let result = run_handshake_responder(f_handle, &[ForwardingVersion::V1], 1).await;
        assert!(
            matches!(result, Err(HandshakeError::Unexpected(_))),
            "expected Unexpected, got {result:?}"
        );
        initiator_task.await.expect("task");
    }
}
