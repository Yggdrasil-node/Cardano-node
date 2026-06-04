//! Result of receiving a KES key bundle.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/RecvResult.hs.
//!
//! Direct type/name mirror of upstream `RecvResult`. Runtime socket
//! handling remains deferred, but these discriminants are the protocol
//! values later codec and driver code must use.

use std::fmt;

/// Result of receiving a key bundle. Mirrors upstream `RecvResult`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
#[repr(u8)]
pub enum RecvResult {
    /// `RecvOK`.
    RecvOK = 0,
    /// `RecvErrorKeyOutdated`.
    RecvErrorKeyOutdated = 1,
    /// `RecvErrorInvalidOpCert`.
    RecvErrorInvalidOpCert = 2,
    /// `RecvErrorNoKey`.
    RecvErrorNoKey = 3,
    /// `RecvErrorUnsupportedOperation`.
    RecvErrorUnsupportedOperation = 4,
    /// `RecvErrorUnknown`.
    RecvErrorUnknown = 5,
}

impl RecvResult {
    /// Discriminants in upstream declaration order.
    pub const ALL: [Self; 6] = [
        Self::RecvOK,
        Self::RecvErrorKeyOutdated,
        Self::RecvErrorInvalidOpCert,
        Self::RecvErrorNoKey,
        Self::RecvErrorUnsupportedOperation,
        Self::RecvErrorUnknown,
    ];

    /// Upstream enum ordinal used by the versioned protocol codecs.
    pub const fn ordinal(self) -> u8 {
        self as u8
    }

    /// Decode an upstream enum ordinal.
    pub const fn from_ordinal(ordinal: u8) -> Option<Self> {
        match ordinal {
            0 => Some(Self::RecvOK),
            1 => Some(Self::RecvErrorKeyOutdated),
            2 => Some(Self::RecvErrorInvalidOpCert),
            3 => Some(Self::RecvErrorNoKey),
            4 => Some(Self::RecvErrorUnsupportedOperation),
            5 => Some(Self::RecvErrorUnknown),
            _ => None,
        }
    }

    /// Mirror of upstream `Pretty RecvResult`.
    pub const fn pretty(self) -> &'static str {
        match self {
            Self::RecvOK => "OK",
            Self::RecvErrorKeyOutdated => "KeyOutdated",
            Self::RecvErrorInvalidOpCert => "InvalidOpCert",
            Self::RecvErrorNoKey => "NoKey",
            Self::RecvErrorUnsupportedOperation => "UnsupportedOperation",
            Self::RecvErrorUnknown => "Unknown",
        }
    }
}

impl fmt::Display for RecvResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.pretty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recv_result_ordinals_match_upstream_declaration_order() {
        for (idx, result) in RecvResult::ALL.iter().copied().enumerate() {
            let ordinal = idx as u8;
            assert_eq!(result.ordinal(), ordinal);
            assert_eq!(RecvResult::from_ordinal(ordinal), Some(result));
        }
        assert_eq!(RecvResult::from_ordinal(6), None);
    }

    #[test]
    fn recv_result_pretty_matches_upstream_instance() {
        assert_eq!(RecvResult::RecvOK.pretty(), "OK");
        assert_eq!(RecvResult::RecvErrorKeyOutdated.pretty(), "KeyOutdated");
        assert_eq!(
            RecvResult::RecvErrorInvalidOpCert.to_string(),
            "InvalidOpCert"
        );
        assert_eq!(
            RecvResult::RecvErrorUnsupportedOperation.to_string(),
            "UnsupportedOperation"
        );
    }
}
