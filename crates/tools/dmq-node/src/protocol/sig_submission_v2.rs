//! DMQ `SigSubmissionV2` mini-protocol — the object-diffusion-based
//! signature diffusion protocol.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Collapses the upstream
//! `DMQ/Protocol/SigSubmissionV2/{Type,Codec,Inbound,Outbound}.hs`
//! files into one Rust file, mirroring the
//! `crates/network/src/protocols/` one-file-per-mini-protocol
//! pattern. `SigSubmissionV2` is based on upstream's
//! `Ouroboros.Network.Protocol.ObjectDiffusion` mini-protocol
//! (originally designed for Peras) — a pull-based protocol where the
//! inbound side requests signature identifiers and then signatures.
//!
//! This slice ports the `Type.hs` count newtypes and the protocol
//! state machine; the message enum, transitions, and codec land in
//! subsequent dmq-node-arc rounds.

/// Number of outstanding signature identifiers being acknowledged.
///
/// Upstream `newtype NumIdsAck = NumIdsAck Word16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumIdsAck(pub u16);

/// Number of signature identifiers being requested.
///
/// Upstream `newtype NumIdsReq = NumIdsReq Word16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumIdsReq(pub u16);

/// Number of signatures being requested.
///
/// Upstream `newtype NumReq = NumReq Word16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumReq(pub u16);

/// Number of unacknowledged signature identifiers.
///
/// Upstream `newtype NumUnacknowledged = NumUnacknowledged Word16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumUnacknowledged(pub u16);

/// States of the `SigSubmissionV2` mini-protocol state machine.
///
/// Upstream `data SigSubmissionV2 sigId sig where StIdle / StSigIds
/// StBlockingStyle / StSigs / StDone`. The inbound ("client") side
/// receives signatures; the outbound ("server") side sends them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SigSubmissionV2State {
    /// Client agency — request identifiers or signatures, or terminate.
    StIdle,
    /// Server agency — reply with a list of signature identifiers.
    StSigIds {
        /// Whether the request was blocking.
        blocking: bool,
    },
    /// Server agency — reply with the requested signatures.
    StSigs,
    /// Terminal state — nobody has agency.
    StDone,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_newtypes_wrap_word16() {
        assert_eq!(NumIdsAck(3).0, 3);
        assert_eq!(NumIdsReq::default(), NumIdsReq(0));
        assert!(NumReq(5) > NumReq(2));
        assert_ne!(NumUnacknowledged(1), NumUnacknowledged(2));
    }

    #[test]
    fn state_variants_compare() {
        assert_eq!(
            SigSubmissionV2State::StSigIds { blocking: true },
            SigSubmissionV2State::StSigIds { blocking: true }
        );
        assert_ne!(
            SigSubmissionV2State::StSigIds { blocking: true },
            SigSubmissionV2State::StSigIds { blocking: false }
        );
        assert_ne!(SigSubmissionV2State::StIdle, SigSubmissionV2State::StDone);
    }
}
