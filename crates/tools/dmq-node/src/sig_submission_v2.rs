//! DMQ `SigSubmissionV2` higher-level protocol surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side module for the upstream
//! `DMQ/SigSubmissionV2/` directory. This slice ports
//! `SigSubmissionV2/Types.hs` — the `SigSubmissionProtocolError`
//! peer-misbehaviour enum. The `Inbound.hs` / `Outbound.hs` driver
//! halves, and the `Protocol/SigSubmissionV2/` mini-protocol itself,
//! land in subsequent dmq-node-arc rounds.

/// A `SigSubmissionV2` peer-protocol violation.
///
/// Mirror of upstream `data SigSubmissionProtocolError`
/// (`SigSubmissionV2/Types.hs`). The `displayException` strings are
/// reproduced as the `thiserror` messages. The count fields of
/// `RequestedTooManySigIds` are `u16` — upstream's `NumIdsReq` /
/// `NumIdsAck` are `Word16` newtypes (introduced with the
/// `SigSubmissionV2` protocol-type slice).
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SigSubmissionProtocolError {
    /// `ProtocolErrorAckedTooManySigIds`.
    #[error("The peer tried to acknowledged more sigIds than are available to do so.")]
    AckedTooManySigIds,
    /// `ProtocolErrorRequestedNothing`.
    #[error("The peer requested zero sigIds.")]
    RequestedNothing,
    /// `ProtocolErrorRequestedTooManySigIds NumIdsReq Word16 NumIdsAck`.
    #[error(
        "The peer requested {requested} sigIds which would put the total in flight over the \
         limit of {max_unacked}. Number of unacked sigIds {unacked}"
    )]
    RequestedTooManySigIds {
        /// Number of signature ids requested.
        requested: u16,
        /// Number of currently-unacknowledged signature ids.
        unacked: u16,
        /// The in-flight limit on unacknowledged signature ids.
        max_unacked: u16,
    },
    /// `ProtocolErrorRequestBlocking`.
    #[error(
        "The peer made a blocking request for more sigIds when there are still unacknowledged \
         sigIds. It should have used a non-blocking request."
    )]
    RequestBlocking,
    /// `ProtocolErrorRequestNonBlocking`.
    #[error(
        "The peer made a non-blocking request for more sigIds when there are no unacknowledged \
         sigIds. It should have used a blocking request."
    )]
    RequestNonBlocking,
    /// `ProtocolErrorRequestedUnavailableSig`.
    #[error(
        "The peer requested a signature which is not available, either because it was never \
         available or because it was previously requested."
    )]
    RequestedUnavailableSig,
    /// `ProtocolErrorSigIdsNotRequested`.
    #[error("The peer replied with more txids than we asked for.")]
    SigIdsNotRequested,
    /// `ProtocolErrorSigNotRequested`.
    #[error("The peer replied with a transaction we did not ask for.")]
    SigNotRequested,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_less_variants_render_upstream_messages() {
        assert_eq!(
            SigSubmissionProtocolError::RequestedNothing.to_string(),
            "The peer requested zero sigIds."
        );
        assert_eq!(
            SigSubmissionProtocolError::SigNotRequested.to_string(),
            "The peer replied with a transaction we did not ask for."
        );
    }

    #[test]
    fn requested_too_many_renders_its_counts() {
        let err = SigSubmissionProtocolError::RequestedTooManySigIds {
            requested: 40,
            unacked: 100,
            max_unacked: 132,
        };
        let rendered = err.to_string();
        assert!(rendered.contains("requested 40 sigIds"), "got: {rendered}");
        assert!(rendered.contains("limit of 132"), "got: {rendered}");
        assert!(rendered.contains("unacked sigIds 100"), "got: {rendered}");
    }
}
